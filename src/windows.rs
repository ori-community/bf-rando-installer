use color_eyre::eyre::{Context, bail};
use std::ffi::{CStr, OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::copy_nonoverlapping;
use std::time::Duration;
use std::{ptr, thread};
use tracing::{error, info, instrument};
use windows_sys::Win32::Foundation::{BOOL, HWND, POINT, TRUE, WPARAM};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_ZERO_MEMORY, HeapAlloc};
use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC_EX, MapVirtualKeyW, SendInput, VK_MENU,
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHChangeNotify};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FindWindowA, GW_OWNER, GetForegroundWindow, GetWindow, IsHungAppWindow,
    IsIconic, PostMessageA, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_DROPFILES,
};
use winreg::RegKey;

#[derive(Debug, Copy, Clone)]
pub struct WindowRef {
    hwnd: HWND,
}

impl WindowRef {
    fn from_hwnd(hwnd: HWND) -> Option<Self> {
        if hwnd.is_null() {
            None
        } else {
            Some(Self { hwnd })
        }
    }
}

pub fn find_window(window_class: Option<&CStr>, window_title: Option<&CStr>) -> Option<WindowRef> {
    let window_class = window_class.map_or(ptr::null(), |wc| wc.as_ptr().cast());
    let window_title = window_title.map_or(ptr::null(), |wt| wt.as_ptr().cast());

    let hwnd = unsafe { FindWindowA(window_class, window_title) };

    WindowRef::from_hwnd(hwnd)
}

#[instrument]
pub fn drop_file(window: WindowRef, file_path: &Path) -> color_eyre::Result<()> {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct _DROPFILES {
        pFiles: u32,
        pt: POINT,
        fNC: BOOL,
        fWide: BOOL,
    }

    let path: Vec<_> = file_path.as_os_str().encode_wide().collect();

    // Message payload consists of a `_DROPFILES` struct and a string table right after.
    // Each string is null terminated, as well as the table as a whole, resulting in two null terminators.
    let size = size_of::<_DROPFILES>() + path.len() * 2 + 4;
    // Use HeapAlloc as we need to use the windows allocator. Using a custom allocator leads to failure.
    let heap = unsafe { GetProcessHeap() };
    let pointer = unsafe { HeapAlloc(heap, HEAP_ZERO_MEMORY, size) };

    let string_offset = size_of::<_DROPFILES>();

    let df = pointer.cast::<_DROPFILES>();
    #[allow(clippy::cast_possible_truncation)]
    unsafe {
        (&raw mut (*df).pFiles).write(string_offset as u32);
        (&raw mut (*df).fWide).write(1);

        copy_nonoverlapping(path.as_ptr(), pointer.add(string_offset).cast(), path.len());
    };

    let result = unsafe { PostMessageA(window.hwnd, WM_DROPFILES, pointer as WPARAM, 0) };

    if result == 0 {
        bail!("Failed to send WM_DROPFILES message");
    }

    Ok(())

    // I think windows takes ownership of the pointer, as the program crashes with STATUS_HEAP_CORRUPTION if this is left in
    // (I couldn't find any documentation on that though)
    // unsafe { HeapFree(heap, 0, pointer) };
}

// Credit to AutoHotkey, method copied from there (https://github.com/AutoHotkey/AutoHotkey/blob/a34bc07d357b7299ca229757162cef8a91e37f52/source/window.cpp)
pub fn activate_window(window: WindowRef) {
    unsafe {
        let target_hwnd = window.hwnd;

        if IsHungAppWindow(target_hwnd) == TRUE {
            return;
        }

        let foreground_hwnd = GetForegroundWindow();

        if IsIconic(target_hwnd) != 0 {
            ShowWindow(target_hwnd, SW_RESTORE);
        }

        if foreground_hwnd == target_hwnd {
            return;
        }

        if try_set_foreground(target_hwnd, foreground_hwnd) {
            return;
        }

        send_alt_up();

        if try_set_foreground(target_hwnd, foreground_hwnd) {
            return;
        }

        BringWindowToTop(target_hwnd);
    }
}

fn try_set_foreground(target_hwnd: HWND, current_foreground_hwnd: HWND) -> bool {
    unsafe {
        SetForegroundWindow(target_hwnd);

        thread::sleep(Duration::from_millis(10));

        let new_foreground_hwnd = GetForegroundWindow();

        if new_foreground_hwnd == target_hwnd {
            true
        } else {
            new_foreground_hwnd != current_foreground_hwnd
                && GetWindow(new_foreground_hwnd, GW_OWNER) == target_hwnd
        }
    }
}

fn send_alt_up() {
    let vk = VK_MENU;
    let esc = unsafe { MapVirtualKeyW(vk.into(), MAPVK_VK_TO_VSC_EX) };
    #[expect(clippy::cast_possible_truncation)]
    let sc = esc as u8;
    let extended_flag = if esc & 0xff00 != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: sc.into(),
                dwFlags: KEYEVENTF_KEYUP | extended_flag,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        SendInput(1, &raw const input, size_of_val(&input).try_into().unwrap());
    }
}

#[derive(Debug, Copy, Clone)]
pub enum AssociationKind {
    Url,
    File,
}

#[instrument]
pub fn is_association_set(kind: AssociationKind) -> color_eyre::Result<bool> {
    let self_path = std::env::current_exe().wrap_err("Getting self exe path")?;
    let self_command = create_handler_command(self_path.as_os_str());

    let saved_command: OsString = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(
            r"Software\Classes\{}\shell\open\command",
            class_key(kind)
        ))
        .wrap_err("Opening command key")?
        .get_value("")
        .wrap_err("Reading command key")?;

    Ok(saved_command == self_command)
}

#[instrument]
pub fn remove_association(kind: AssociationKind) -> color_eyre::Result<()> {
    info!("Removing URL handler");

    RegKey::predef(HKEY_CURRENT_USER)
        .delete_subkey_all(format!(r"Software\Classes\{}", class_key(kind)))
        .wrap_err("Deleting Association")?;

    update_associations();

    Ok(())
}

#[instrument]
pub fn ensure_association_exists(kind: AssociationKind) -> color_eyre::Result<()> {
    info!("Setting Association");

    let self_path = std::env::current_exe().wrap_err("Getting self exe path")?;
    let self_path = self_path.as_os_str();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let (proto_key, _) = hkcu
        .create_subkey(format!(r"Software\Classes\{}", class_key(kind)))
        .wrap_err("Opening")?;

    let (name, is_url) = match kind {
        AssociationKind::Url => ("URL:Ori and the Blind Forest Randomizer", true),
        AssociationKind::File => ("Ori and the Blind Forest Randomizer seed", false),
    };
    proto_key.set_value("", &name).wrap_err("Set proto value")?;

    if is_url {
        proto_key
            .set_value("URL Protocol", &"")
            .wrap_err("Setting as URL handler")?;
    }

    if let Err(err) = set_default_icon(&proto_key, self_path) {
        error!(?err, "Could not net association icon");
    }

    let command_value = create_handler_command(self_path);

    let (command_key, _) = proto_key
        .create_subkey(r"shell\open\command")
        .wrap_err("Creating command key")?;
    command_key
        .set_value("", &command_value)
        .wrap_err("Setting command")?;

    update_associations();

    Ok(())
}

fn class_key(association_kind: AssociationKind) -> &'static str {
    match association_kind {
        AssociationKind::Url => "bfr",
        AssociationKind::File => ".bfr",
    }
}

fn create_handler_command(self_path: &OsStr) -> OsString {
    let mut command_value = OsString::new();
    command_value.push(r#"""#);
    command_value.push(self_path);
    command_value.push(r#"" -- "%1""#);
    command_value
}

fn set_default_icon(assoc_key: &RegKey, self_path: &OsStr) -> color_eyre::Result<()> {
    let (icon_key, _) = assoc_key
        .create_subkey("DefaultIcon")
        .wrap_err("Opening DefaultIcon")?;
    icon_key
        .set_value("", &self_path)
        .wrap_err("Setting DefaultIcon")?;

    Ok(())
}

fn update_associations() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED.cast_signed(),
            0,
            ptr::null(),
            ptr::null(),
        )
    };
}

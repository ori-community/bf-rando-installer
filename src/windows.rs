use color_eyre::eyre::bail;
use std::ffi::CStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::copy_nonoverlapping;
use std::time::Duration;
use std::{ptr, thread};
use tracing::instrument;
use windows_sys::Win32::Foundation::{BOOL, HWND, POINT, TRUE, WPARAM};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_ZERO_MEMORY, HeapAlloc};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FindWindowA, GW_OWNER, GetForegroundWindow, GetWindow,
    GetWindowThreadProcessId, IsHungAppWindow, IsIconic, PostMessageA, SW_RESTORE,
    SetForegroundWindow, ShowWindow, WM_DROPFILES,
};

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

        let target_thread = GetWindowThreadProcessId(target_hwnd, ptr::null_mut());
        if target_thread == 0 {
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
        } else if new_foreground_hwnd != current_foreground_hwnd
            && GetWindow(new_foreground_hwnd, GW_OWNER) == target_hwnd
        {
            true
        } else {
            false
        }
    }
}

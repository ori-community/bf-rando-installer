use dll_classifier::classify_file;
use std::env::args;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::ptr::copy_nonoverlapping;
use windows_sys::Win32::Foundation::{BOOL, POINT, WPARAM};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_ZERO_MEMORY, HeapAlloc};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowA, PostMessageA, WM_DROPFILES};

mod dll_classifier;
mod dll_parser;

fn main() {
    match main_impl() {
        Ok(results) => {
            for msg in results {
                println!("{msg}");
            }
        }
        Err(msg) => eprintln!("{msg}"),
    }

    // try_drop();
}

fn main_impl() -> Result<Vec<String>, &'static str> {
    let dir_path = args().skip(1).next().ok_or("Missing dir path argument")?;

    let dir = std::fs::read_dir(dir_path).map_err(|_| "Couldn't read dir")?;

    let mut results = Vec::new();

    for file in dir {
        let file = file.map_err(|_| "Couldn't step file")?;

        if !file
            .file_type()
            .map_err(|_| "Couldn't file type file")?
            .is_file()
        {
            continue;
        }

        let file_name = file.file_name();

        let file_data = std::fs::read(file.path()).map_err(|_| "Couldn't read file")?;

        let result = classify_file(&file_data);
        results.push(format!("{}: {:?}", file_name.to_string_lossy(), result));
    }

    Ok(results)
}

#[allow(dead_code)]
fn try_drop() {
    let hwnd = unsafe { FindWindowA(c"IrfanView".as_ptr() as *const _, ptr::null()) };

    let path = PathBuf::from(r"D:\documents\bild.jpg");
    let path: Vec<_> = path.as_os_str().encode_wide().collect();

    #[repr(C)]
    #[allow(non_snake_case)]
    struct _DROPFILES {
        pFiles: u32,
        pt: POINT,
        fNC: BOOL,
        fWide: BOOL,
    }

    // Message payload consists of a `_DROPFILES` struct and a string table right after.
    // Each string is null terminated, as well as the table as a whole, resulting in two null terminators.
    let size = size_of::<_DROPFILES>() + path.len() * 2 + 4;
    // Use HeapAlloc as we need to use the windows allocator. Using a custom allocator leads to failure.
    let heap = unsafe { GetProcessHeap() };
    let pointer = unsafe { HeapAlloc(heap, HEAP_ZERO_MEMORY, size) };

    let string_offset = size_of::<_DROPFILES>();

    let df = pointer as *mut _DROPFILES;
    unsafe {
        (&raw mut (*df).pFiles).write(string_offset as u32);
        (&raw mut (*df).fWide).write(1);

        copy_nonoverlapping(
            path.as_ptr(),
            pointer.offset(string_offset as isize) as *mut _,
            path.len(),
        )
    };

    let result = unsafe { PostMessageA(hwnd, WM_DROPFILES, pointer as WPARAM, 0) };

    let _result = result != 0;
    println!("{result}");

    // I think windows takes ownership of the pointer, as the program crashes with STATUS_HEAP_CORRUPTION if this is left in
    // (I couldn't find any documentation on that though)
    // unsafe { HeapFree(heap, 0, pointer) };
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![warn(clippy::pedantic)]

use crate::game::{search_for_game_dir, verify_game_dir};
use crate::gui::run_gui;
use crate::self_update::self_update;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::bail;
use std::any::Any;
use std::default::Default;
use std::env::temp_dir;
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::copy_nonoverlapping;
use std::sync::OnceLock;
use std::{io, ptr};
use tracing::{debug, error, info, info_span, instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};
use windows_sys::Win32::Foundation::{BOOL, POINT, WPARAM};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HEAP_ZERO_MEMORY, HeapAlloc};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowA, PostMessageA, WM_DROPFILES};

mod dll_classifier;
mod dll_management;
mod dll_parser;
mod game;
mod gui;
mod orirando;
mod self_update;
mod settings;
mod steam;

static LOGFILE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Default)]
struct Args {
    no_self_update_check: bool,
}

fn main() {
    let _logger_guard = setup();

    let _span = info_span!("main").entered();

    let args = match parse_args() {
        Ok(args) => {
            info!(?args, "Parsed CLI args");
            args
        }
        Err(err) => {
            error!(?err, "Error parsing CLI args");
            Args::default()
        }
    };

    let mut settings = Settings::load();

    if settings.game_dir.install.as_os_str().is_empty() || !verify_game_dir(&settings.game_dir) {
        settings.game_dir = search_for_game_dir().unwrap_or_default();
        settings.save_async();
    }

    if settings.self_update && !args.no_self_update_check {
        match self_update() {
            Ok(true) => {
                info!("Updated app, closing this instance");
                return;
            }
            Ok(false) => info!("Performed update check, no new version"),
            Err(err) => {
                error!(?err, "Could not perform self-update");
            }
        }
    }

    if let Err(e) = run_gui(settings) {
        error!(?e, "Error running gui");
    }

    // try_drop();
}

fn setup() -> impl Any {
    let colors = ansi_term::enable_ansi_support().is_ok();

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("debug"))
        .unwrap();

    let (stdout_writer, stdout_guard) = tracing_appender::non_blocking(io::stdout());
    let stdout_logger = fmt::layer()
        .with_target(false)
        .with_ansi(colors)
        .with_writer(stdout_writer);

    let (file_logger, file_guard) = match create_log_file() {
        Err(e) => {
            eprintln!("Can't open log file: {e:?}");
            (None, None)
        }
        Ok(file) => {
            let (writer, guard) = tracing_appender::non_blocking(file);

            let file_logger = fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(writer);

            (Some(file_logger), Some(guard))
        }
    };

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(file_logger)
        .with(stdout_logger)
        .with(ErrorLayer::default())
        .init();

    if let Err(e) = color_eyre::config::HookBuilder::new()
        .theme(color_eyre::config::Theme::new())
        .install()
    {
        eprintln!("Error installing color_eyre hook: {e:?}");
    }

    (stdout_guard, file_guard)
}

fn create_log_file() -> io::Result<File> {
    let path = temp_dir().join("ori-de-randomizer.log");
    let result = File::create(&path);

    if result.is_ok() {
        _ = LOGFILE.set(path);
    }

    result
}

#[instrument]
fn parse_args() -> Result<Args> {
    debug!(args_os=?std::env::args_os().collect::<Vec<_>>(), "Parsing CLI args");

    let mut args = Args::default();

    // Skip argv[0]
    for arg in std::env::args_os().skip(1) {
        if arg == "--no-self-update-check" {
            args.no_self_update_check = true;
        } else {
            bail!("Unexpected argument {arg:?}");
        }
    }

    Ok(args)
}

#[allow(dead_code, clippy::pedantic)]
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
            pointer.add(string_offset) as *mut _,
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

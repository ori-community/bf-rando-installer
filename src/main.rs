use std::env::args;
use std::sync::LazyLock;

use memchr::memmem;
use regex::bytes::Regex;

use crate::dll_parser::parse_dll;

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

        results.push(format!("{}: {}", file_name.to_string_lossy(), result));
    }

    Ok(results)
}

fn classify_file(file_data: &[u8]) -> String {
    let heaps = match parse_dll(&file_data) {
        Ok(heaps) => heaps,
        Err(msg) => return format!("Invalid DLL: {msg}"),
    };

    if !memmem::find(heaps.strings, b"SpiritGrenadeDamageDealer\0").is_some() {
        return "Unknown DLL".into();
    }

    if !memmem::find(heaps.strings, b"Randomizer\0").is_some() {
        return "Vanilla Ori DE DLL".into();
    }

    static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        // Regex to find a version string literal embedded into the dll, e.g. "1.2.34"
        // Format for a version string: <length prefix><version string><nul byte>
        // Format for <version string>: UTF-16 encoded [\d+ '.' \d+ '.' \d+]
        Regex::new(r#"(?-u).((?:\d\x00)+)\.\x00((?:\d\x00)+)\.\x00((?:\d\x00)+)\x00"#).unwrap()
    });

    let version = VERSION_REGEX
        .captures_iter(heaps.us)
        .filter_map(|m| {
            let (full, [major, minor, patch]) = m.extract();

            let length_prefix = full[0];
            if length_prefix as usize != full.len() - 1 {
                None
            } else {
                Some((
                    parse_utf16_number(major)?,
                    parse_utf16_number(minor)?,
                    parse_utf16_number(patch)?,
                ))
            }
        })
        .max();

    match version {
        Some((major, minor, patch)) => {
            format!("Ori DE rando DLL (version {major}.{minor}.{patch})")
        }
        None => "Ori DE rando DLL (unknown version)".into(),
    }
}

fn parse_utf16_number(bytes: &[u8]) -> Option<u64> {
    let mut number = 0u64;

    for i in (0..bytes.len()).step_by(2) {
        let digit = (bytes[i] - b'0') as u64;
        number = number.checked_mul(10)?.checked_add(digit)?;
    }

    Some(number)
}

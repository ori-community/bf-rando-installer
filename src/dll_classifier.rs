use crate::dll_parser::parse_dll;
use memchr::memmem;
use regex::bytes::Regex;
use std::sync::LazyLock;
use tracing::{info, instrument};

#[derive(Debug)]
pub enum DllClassification {
    Invalid,
    NonDe,
    Vanilla,
    Rando(Option<RandoVersion>),
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct RandoVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

#[instrument(skip(file_data))]
pub fn classify_file(file_data: &[u8]) -> DllClassification {
    let heaps = match parse_dll(file_data) {
        Ok(heaps) => heaps,
        Err(e) => {
            info!(?e, "Invalid Dll");
            return DllClassification::Invalid;
        }
    };

    if !memmem::find(heaps.strings, b"SpiritGrenadeDamageDealer\0").is_some() {
        return if memmem::find(heaps.strings, b"HoldingNightberryCondition\0").is_some() {
            DllClassification::NonDe
        } else {
            DllClassification::Invalid
        };
    }

    if !memmem::find(heaps.strings, b"Randomizer\0").is_some() {
        return DllClassification::Vanilla;
    }

    return DllClassification::Rando(extract_rando_version(heaps.us));
}

fn extract_rando_version(us_heap: &[u8]) -> Option<RandoVersion> {
    static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        // Regex to find a version string literal embedded into the dll, e.g. "1.2.34"
        // Format for a version string: <length prefix><version string><nul byte>
        // Format for <version string>: UTF-16 encoded [\d+ '.' \d+ '.' \d+]
        Regex::new(r"(?-u).((?:\d\x00)+)\.\x00((?:\d\x00)+)\.\x00((?:\d\x00)+)\x00").unwrap()
    });

    VERSION_REGEX
        .captures_iter(us_heap)
        .filter_map(|m| {
            let (full, [major, minor, patch]) = m.extract();

            let length_prefix = full[0];
            if length_prefix as usize != full.len() - 1 {
                None
            } else {
                Some(RandoVersion {
                    major: parse_trusted_utf16_number(major)?,
                    minor: parse_trusted_utf16_number(minor)?,
                    patch: parse_trusted_utf16_number(patch)?,
                })
            }
        })
        .max()
}

fn parse_trusted_utf16_number(bytes: &[u8]) -> Option<u64> {
    let mut number = 0u64;

    for i in (0..bytes.len()).step_by(2) {
        let digit = (bytes[i] - b'0') as u64;
        number = number.checked_mul(10)?.checked_add(digit)?;
    }

    Some(number)
}

use crate::dll_parser::parse_dll;
use memchr::memmem;
use regex::bytes::Regex;
use std::fmt::{Display, Formatter};
use std::hash::{DefaultHasher, Hasher};
use std::io;
use std::path::Path;
use std::sync::LazyLock;
use tracing::{debug, info_span, instrument};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum DllClassification {
    Invalid,
    NonDe,
    Vanilla,
    Rando(RandoVersion),
    UnknownRando(u64),
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct RandoVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Display for RandoVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let &Self {
            major,
            minor,
            patch,
        } = self;
        f.write_fmt(format_args!("{major}.{minor}.{patch}"))
    }
}

#[instrument]
pub fn classify_dll_file(path: &Path) -> io::Result<DllClassification> {
    let file = info_span!("open_file").in_scope(|| std::fs::File::open(path))?;
    let data = info_span!("mmap_file").in_scope(|| unsafe { memmap2::Mmap::map(&file) })?;
    Ok(classify_dll(&data))
}

#[instrument(skip(file_data))]
pub fn classify_dll(file_data: &[u8]) -> DllClassification {
    let heaps = match parse_dll(file_data) {
        Ok(heaps) => heaps,
        Err(e) => {
            debug!(?e, "Invalid Dll");
            return DllClassification::Invalid;
        }
    };

    if memmem::find(heaps.strings, b"SpiritGrenadeDamageDealer\0").is_none() {
        return if memmem::find(heaps.strings, b"HoldingNightberryCondition\0").is_some() {
            DllClassification::NonDe
        } else {
            DllClassification::Invalid
        };
    }

    if memmem::find(heaps.strings, b"Randomizer\0").is_none() {
        return DllClassification::Vanilla;
    }

    if let Some(v) = extract_rando_version(heaps.us) {
        DllClassification::Rando(v)
    } else {
        DllClassification::UnknownRando(compute_hash(file_data))
    }
}

fn compute_hash(value: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(value);
    hasher.finish()
}

#[instrument(skip_all)]
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

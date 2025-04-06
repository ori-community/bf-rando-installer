use crate::dll_classifier::RandoVersion;
use color_eyre::Result;
use color_eyre::eyre::{OptionExt, WrapErr, bail};
use regex::Regex;
use std::sync::LazyLock;
use tracing::instrument;

static VERSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<title>Ori DE Randomizer (\d+)\.(\d+)\.(\d+)</title>").unwrap());

#[instrument]
pub fn check_version() -> Result<RandoVersion> {
    let resp =
        reqwest::blocking::get("https://orirando.com/").wrap_err("Error accessing orirando.com")?;

    if !resp.status().is_success() {
        bail!("orirando.com did not return success: {}", resp.status());
    }

    let html = resp.text().wrap_err("Error getting text of orirando.com")?;

    let captures = VERSION_REGEX
        .captures(&html)
        .ok_or_eyre("Failed to extract version from title")?;
    let (_full, [major, minor, patch]) = captures.extract();

    Ok(RandoVersion {
        major: parse_version_number_part(major)?,
        minor: parse_version_number_part(minor)?,
        patch: parse_version_number_part(patch)?,
    })
}

fn parse_version_number_part(num: &str) -> Result<u32> {
    num.parse().wrap_err("Failed to parse version number part")
}

#[instrument]
pub fn download_dll() -> Result<Vec<u8>> {
    let resp = reqwest::blocking::get("https://orirando.com/dll")
        .wrap_err("Error accessing orirando.com")?;

    if !resp.status().is_success() {
        bail!("orirando.com did not return success: {}", resp.status());
    }

    let bytes = resp.bytes().wrap_err("Error downloading dll")?;

    Ok(bytes.to_vec())
}

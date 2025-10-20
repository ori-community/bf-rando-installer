use crate::dll_classifier::RandoVersion;
use crate::settings::NetworkSettings;
use color_eyre::Result;
use color_eyre::eyre::{OptionExt, WrapErr, bail};
use regex::Regex;
use reqwest::Url;
use std::str::FromStr;
use std::sync::LazyLock;
use tracing::{info, instrument};

static VERSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<title>Ori DE Randomizer (\d+)\.(\d+)\.(\d+)</title>").unwrap());

#[instrument(skip_all)]
pub fn check_version(network: &NetworkSettings) -> Result<RandoVersion> {
    network.check_offline_mode()?;

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

#[instrument(skip_all)]
pub fn download_dll(network: &NetworkSettings) -> Result<Vec<u8>> {
    network.check_offline_mode()?;

    let url = Url::from_str("https://orirando.com/dll").expect("static URL is valid");
    info!(%url, "Downloading dll");

    let resp = reqwest::blocking::get(url).wrap_err("Error accessing orirando.com")?;

    if !resp.status().is_success() {
        bail!("orirando.com did not return success: {}", resp.status());
    }

    let bytes = resp.bytes().wrap_err("Error downloading dll")?;
    info!("Downloaded dll ({} bytes)", bytes.len());

    Ok(bytes.to_vec())
}

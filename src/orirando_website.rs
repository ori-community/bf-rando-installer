use crate::dll_classifier::RandoVersion;
use crate::settings::{NetworkSettings, Settings};
use color_eyre::Result;
use color_eyre::eyre::{OptionExt, WrapErr, bail, eyre};
use regex::Regex;
use reqwest::Url;
use std::str::FromStr;
use std::sync::LazyLock;
use tracing::{error, info, instrument};

static VERSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\.(\d+)\.(\d+)$").unwrap());

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Endpoint {
    Stable,
    Beta,
    Dev,
}

impl Endpoint {
    pub fn base_url(self) -> Url {
        match self {
            Endpoint::Stable => {
                Url::from_str("https://bf.orirando.com").expect("static URL is valid")
            }
            Endpoint::Beta => {
                Url::from_str("https://bfbeta.eiko.blue").expect("static URL is valid")
            }
            Endpoint::Dev => Url::from_str("https://bfdev.eiko.blue").expect("static URL is valid"),
        }
    }
}

#[instrument(skip_all)]
pub fn check_website_version(settings: &Settings) -> Result<(RandoVersion, Endpoint)> {
    let mut stable_version = Err(eyre!("Stable version check somehow didn't run"));
    let mut beta_version = None;
    rayon::scope(|s| {
        s.spawn(|_| {
            stable_version = match check_version(&settings.network, Endpoint::Stable) {
                Ok(v) => Ok(v),
                Err(err) => {
                    error!(?err, "Checking latest stable version available");
                    Err(err)
                }
            }
        });

        if settings.show_beta {
            s.spawn(|_| match check_version(&settings.network, Endpoint::Beta) {
                Ok(v) => beta_version = Some(v),
                Err(err) => error!(?err, "Checking latest beta version available"),
            });
        }
    });

    match (stable_version, beta_version) {
        (Err(_), Some(beta)) => Ok((beta, Endpoint::Beta)),
        (Ok(stable), Some(beta)) if beta > stable => Ok((beta, Endpoint::Beta)),
        (stable, _) => stable.map(|v| (v, Endpoint::Stable)),
    }
}

#[instrument(skip_all, fields(endpoint))]
pub fn check_version(network: &NetworkSettings, endpoint: Endpoint) -> Result<RandoVersion> {
    network.check_offline_mode()?;

    let url = {
        let mut url = endpoint.base_url();
        url.path_segments_mut()
            .unwrap()
            .extend(["version", "latest"]);
        url
    };

    let resp = reqwest::blocking::get(url).wrap_err("Error accessing endpoint")?;

    if !resp.status().is_success() {
        bail!("bf.orirando.com did not return success: {}", resp.status());
    }

    let html = resp.text().wrap_err("Error getting text of endpoint")?;

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

#[instrument(skip_all, fields(endpoint))]
pub fn download_dll(network: &NetworkSettings, endpoint: Endpoint) -> Result<Vec<u8>> {
    network.check_offline_mode()?;

    let url = {
        let mut url = endpoint.base_url();
        url.path_segments_mut().unwrap().push("dll");
        url
    };
    info!(%url, "Downloading dll");

    let resp = reqwest::blocking::get(url).wrap_err("Error accessing endpoint")?;

    if !resp.status().is_success() {
        bail!("bf.orirando.com did not return success: {}", resp.status());
    }

    let bytes = resp.bytes().wrap_err("Error downloading dll")?;
    info!("Downloaded dll ({} bytes)", bytes.len());

    Ok(bytes.to_vec())
}

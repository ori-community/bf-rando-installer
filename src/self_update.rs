use color_eyre::eyre::{OptionExt, WrapErr, bail};
use color_eyre::{Result, Section, SectionExt};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tracing::{debug, info, instrument};

#[derive(Debug, Serialize, Deserialize)]
struct LatestReleaseResponse {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[instrument]
pub fn self_update() -> Result<bool> {
    let Some(url) = new_version_url().wrap_err("Error fetching new version")? else {
        return Ok(false);
    };

    info!(?url, "Installing new app version");

    let new_version = download_new_version(url).wrap_err("Error downloading new version")?;

    let current_file = prepare_target_file().wrap_err("Error preparing target file")?;

    std::fs::write(&current_file, new_version).wrap_err("Failed to write new version")?;

    info!(?current_file, "New version written, spawning replacement");

    Command::new(current_file)
        .arg("--no-self-update-check")
        .args(std::env::args_os().skip(1)) // skip argv[0]
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("Failed to spawn replacement process")?;

    Ok(true)
}

#[instrument]
fn new_version_url() -> Result<Option<String>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("ori-de-randomizer")
        .build()
        .wrap_err("Cannot create client")?;

    let resp = client
        .get("https://api.github.com/repos/ori-community/bf-rando-installer/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .wrap_err("Could not query github API")?;

    if !resp.status().is_success() {
        bail!("Non success status code {}", resp.status());
    }

    let payload = resp.text().wrap_err("Could not get response text")?;
    let payload: LatestReleaseResponse =
        serde_json::from_str(&payload).wrap_err("Invalid response json")?;

    let version_string = payload
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&payload.tag_name);

    let current_version: Vec<u32> = parse_version_string(env!("CARGO_PKG_VERSION"))
        .wrap_err("Failed to parse current version")?;
    let new_version =
        parse_version_string(version_string).wrap_err("Failed to parse new version string")?;

    debug!(?current_version, ?new_version, "Fetched app versions");

    if current_version >= new_version {
        return Ok(None);
    }

    for asset in payload.assets {
        if std::path::Path::new(&asset.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Ok(Some(asset.browser_download_url));
        }
    }

    bail!("No exe asset in release")
}

#[instrument]
fn parse_version_string(version_string: &str) -> Result<Vec<u32>> {
    version_string
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .wrap_err("Failed to parse version string")
}

#[instrument]
fn download_new_version(url: String) -> Result<impl AsRef<[u8]>> {
    let resp = reqwest::blocking::get(url).wrap_err("Could not fetch new version")?;

    if !resp.status().is_success() {
        bail!("Non-success status code {}", resp.status());
    }

    resp.bytes().wrap_err("Could not download new version")
}

#[instrument]
fn prepare_target_file() -> Result<PathBuf> {
    let current_file = std::env::current_exe().wrap_err("Failed to get current exe path")?;

    let mut file_name = current_file
        .file_stem()
        .or(current_file.file_name())
        .ok_or_eyre("No file name on current file")?
        .to_owned();
    file_name.push(".old");
    if let Some(ext) = current_file.extension() {
        file_name.push(".");
        file_name.push(ext);
    }
    let old_file = current_file.with_file_name(file_name);

    match std::fs::remove_file(&old_file) {
        Ok(()) => (),
        Err(err) if err.kind() == ErrorKind::NotFound => (),
        Err(err) => {
            return Err(err)
                .wrap_err("Failed to delete old version")
                .section(format!("{old_file:?}").header("File path"));
        }
    }

    std::fs::rename(&current_file, &old_file)
        .wrap_err("Failed to move current version")
        .with_section(|| format!("{current_file:?}").header("Current file"))
        .with_section(|| format!("{old_file:?}").header("Target file"))?;

    Ok(current_file)
}

use color_eyre::eyre::{OptionExt, WrapErr, bail, eyre};
use std::path::PathBuf;
use std::sync::LazyLock;
use winreg::RegKey;
use winreg::enums::HKEY_CLASSES_ROOT;

use color_eyre::{Result, Section, SectionExt};
use regex::Regex;
use tracing::{info, instrument};

#[instrument]
pub fn get_game_dir(app_id: &str) -> Result<PathBuf> {
    let steam_dir = get_steam_dir().wrap_err("Getting steam dir")?;
    let library_dir = get_library_for(steam_dir, app_id).wrap_err("Getting game library")?;
    let game_dir =
        get_game_install_dir(library_dir, app_id).wrap_err("Getting game install dir")?;
    Ok(game_dir)
}

#[instrument]
fn get_steam_dir() -> Result<PathBuf> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let steam_key = hkcr.open_subkey("steam").wrap_err("Opening HKCR\\steam")?;

    steam_key
        .get_raw_value("URL Protocol")
        .wrap_err("Opening HKCR\\steam\\URL Protocol")?;

    let command: String = steam_key
        .open_subkey(r"Shell\Open\Command")
        .wrap_err("Opening HKCR\\steam\\Shell\\Open\\Command")?
        .get_value("")
        .wrap_err("Reading HKCR\\steam\\Shell\\Open\\Command")?;
    let command = command.trim();

    let steam_exe_path = if command.starts_with('"') {
        let end = command[1..].find('"').ok_or_else(|| {
            eyre!("Invalid command string").section(command.to_owned().header("Steam Command"))
        })?;
        &command[1..=end]
    } else if let Some(end) = command.find(' ') {
        &command[..=end]
    } else {
        command
    };

    info!(?steam_exe_path, "Retrieved steam exe path");

    let steam_dir = PathBuf::from(steam_exe_path)
        .parent()
        .ok_or_else(|| eyre!("Invalid steam path"))?
        .to_owned();

    Ok(steam_dir)
}

static LIBRARY_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*"path"\s*"([^"]+)"$"#).unwrap());

static LIBRARY_APP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*"([0-9]+)"\s*"[0-9]+"$"#).unwrap());

#[instrument]
fn get_library_for(steam_dir: PathBuf, app_id: &str) -> Result<PathBuf> {
    let mut vdf_path = steam_dir;
    vdf_path.extend(["steamapps", "libraryfolders.vdf"]);
    let vdf = std::fs::read_to_string(vdf_path).wrap_err("reading libraryfolders.vdf")?;

    let mut current_library = None;
    for line in vdf.lines() {
        if let Some(captures) = LIBRARY_PATH.captures(line) {
            let (_full, [path]) = captures.extract();
            current_library = Some(path.replace(r"\\", r"\"));
        } else if let Some(captures) = LIBRARY_APP.captures(line) {
            let (_full, [app_str]) = captures.extract();
            if app_str == app_id {
                let library = current_library
                    .ok_or_eyre("Found app before the first library in libraryfolders.vdf")?;
                return Ok(library.into());
            }
        }
    }

    bail!("App not found in libraryfolders.vdf");
}

static INSTALL_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*"installdir"\s*"([^"]+)"$"#).unwrap());

#[instrument]
fn get_game_install_dir(library_dir: PathBuf, app_id: &str) -> Result<PathBuf> {
    let mut manifest_path = library_dir.clone();
    manifest_path.push("steamapps");
    manifest_path.push(format!("appmanifest_{app_id}.acf"));
    let manifest = std::fs::read_to_string(manifest_path).wrap_err("reading app manifest")?;

    if let Some(captures) = INSTALL_DIR.captures(&manifest) {
        let (_full, [path]) = captures.extract();
        let mut game_dir = library_dir;
        game_dir.extend(["steamapps", "common"]);
        game_dir.push(path);
        return Ok(game_dir);
    }

    bail!("installdir not found in app manifest")
}

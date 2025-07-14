use crate::rando_files::play_rando_url;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::{OptionExt, WrapErr, bail};
use reqwest::Url;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::str::FromStr;
use tracing::{error, info, instrument};
use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
use winreg::RegKey;

#[instrument(skip_all, fields(%url))]
pub fn handle_bfr_url(settings: &Settings, url: Url) -> Result<()> {
    if url.scheme() != "bfr" {
        bail!("Wrong url scheme: expected 'bfr', got '{}'", url.scheme());
    }

    let params = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let mut path_segments = url.path_segments().ok_or_eyre("No path in url")?;

    match path_segments.next() {
        Some("play") => play_seed(settings, path_segments, params).wrap_err("Playing URL seed")?,
        _ => bail!("Invalid bfr URL"),
    }

    Ok(())
}

#[instrument(skip_all)]
fn play_seed<'a>(
    settings: &Settings,
    mut path_segments: impl Iterator<Item = &'a str>,
    query_params: HashMap<String, String>,
) -> Result<()> {
    match path_segments.next() {
        Some("params") => {
            let seed_params = path_segments
                .next()
                .ok_or_eyre("Missing params path segment")?;

            if path_segments.next().is_some() {
                bail!("Path too long");
            }

            let url = build_seed_url(seed_params, &query_params)?;

            play_rando_url(settings, url).wrap_err("Playing seed")?;
        }
        _ => bail!("Invalid bfr play URL"),
    }

    Ok(())
}

fn build_seed_url(seed_params: &str, query_params: &HashMap<String, String>) -> Result<Url> {
    let mut url = Url::from_str(&format!(
        "https://orirando.com/generator/seed/{seed_params}"
    ))
    .wrap_err("Generated URL should be valid")?;

    let mut url_params = url.query_pairs_mut();

    if let Some(game_id) = query_params.get("game_id") {
        url_params.append_pair("game_id", game_id);
    }

    if let Some(player_id) = query_params.get("player_id") {
        url_params.append_pair("player_id", player_id);
    }

    drop(url_params);

    Ok(url)
}

#[instrument]
pub fn is_url_handler_set() -> Result<bool> {
    let self_path = std::env::current_exe().wrap_err("Getting self exe path")?;
    let self_command = create_handler_command(self_path.as_os_str());

    let saved_command: OsString = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Classes\bfr\shell\open\command")
        .wrap_err("Opening command key")?
        .get_value("")
        .wrap_err("Reading command key")?;

    Ok(saved_command == self_command)
}

#[instrument]
pub fn remove_url_handler() -> Result<()> {
    info!("Removing URL handler");

    RegKey::predef(HKEY_CURRENT_USER)
        .delete_subkey_all(r"Software\Classes\bfr")
        .wrap_err("Deleting URL handler")
}

#[instrument]
pub fn ensure_url_handler_exists() -> Result<()> {
    info!("Setting URL handler");

    let self_path = std::env::current_exe().wrap_err("Getting self exe path")?;
    let self_path = self_path.as_os_str();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let (proto_key, _) = hkcu
        .create_subkey(r"Software\Classes\bfr")
        .wrap_err("Opening")?;

    proto_key
        .set_value("", &"URL:Ori and the Blind Forest Randomizer")
        .wrap_err("Set proto value")?;

    proto_key
        .set_value("URL Protocol", &"")
        .wrap_err("Setting as URL handler")?;

    if let Err(err) = set_default_icon(&proto_key, self_path) {
        error!(?err, "Could not net URL Handler icon");
    }

    let command_value = create_handler_command(self_path);

    let (command_key, _) = proto_key
        .create_subkey(r"shell\open\command")
        .wrap_err("Creating command key")?;
    command_key
        .set_value("", &command_value)
        .wrap_err("Setting command")?;

    Ok(())
}

fn create_handler_command(self_path: &OsStr) -> OsString {
    let mut command_value = OsString::new();
    command_value.push(r#"""#);
    command_value.push(self_path);
    command_value.push(r#"" -- "%1""#);
    command_value
}

fn set_default_icon(assoc_key: &RegKey, self_path: &OsStr) -> Result<()> {
    let (icon_key, _) = assoc_key
        .create_subkey("DefaultIcon")
        .wrap_err("Opening DefaultIcon")?;
    icon_key
        .set_value("", &self_path)
        .wrap_err("Setting DefaultIcon")?;

    Ok(())
}

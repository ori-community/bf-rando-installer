use crate::rando_files::play_rando_url;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::{OptionExt, WrapErr, bail};
use reqwest::Url;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::instrument;

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
        "https://bf.orirando.com/generator/seed/{seed_params}"
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

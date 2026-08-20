use crate::files::{make_valid_filename, move_file};
use crate::game::GameDir;
use crate::settings::{MoveSeedMode, NetworkSettings, Settings};
use color_eyre::Result;
use color_eyre::eyre::{Context, OptionExt, bail};
use rand::distr::{Alphanumeric, SampleString};
use regex::Regex;
use reqwest::Url;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tracing::Level;
use tracing::{debug, error, info, instrument};

#[instrument(skip(settings))]
pub fn play_rando_file(settings: &Settings, file_path: PathBuf) -> Result<()> {
    let seed_path = install_rando_file(settings.move_seed_mode, &settings.game_dir, file_path)
        .wrap_err("Moving seed file")?;

    settings
        .game_dir
        .try_play_seed(&seed_path, settings.launch_type)
        .wrap_err("Launching game")
}

#[instrument(skip(settings), fields(%url))]
pub fn play_rando_url(settings: &Settings, url: Url) -> Result<()> {
    let seed = download_seed(&settings.network, url).wrap_err("Downloading seed")?;
    let seed_path =
        install_new_rando_file(&settings.game_dir, &seed).wrap_err("Installing seed")?;

    settings
        .game_dir
        .try_play_seed(&seed_path, settings.launch_type)
        .wrap_err("Launching game")
}

#[instrument(skip(network), fields(%url))]
fn download_seed(network: &NetworkSettings, url: Url) -> Result<Vec<u8>> {
    network.check_offline_mode()?;

    info!("Downloading seed");

    let response = reqwest::blocking::get(url).wrap_err("Sending request")?;
    if !response.status().is_success() {
        bail!("Received non-success status code: {}", response.status());
    }

    let bytes = response.bytes().wrap_err("Downloading seed")?;
    info!("Downloaded seed ({} bytes)", bytes.len());

    Ok(bytes.to_vec())
}

#[instrument(skip_all)]
fn install_rando_file(
    mode: MoveSeedMode,
    game_dir: &GameDir,
    file_path: PathBuf,
) -> Result<PathBuf> {
    let ext = match file_path.extension() {
        Some(ext) if ext == "dat" || ext == "bfr" => ext.to_str().unwrap(),
        None => bail!(
            "Refusing to install seed file \"{file_path:?}\": File must have .bfr or .dat extension, but has none"
        ),
        Some(ext) => bail!(
            "Refusing to install seed file \"{file_path:?}\": File must have .bfr or .dat extension, but has {ext:?}"
        ),
    };

    let destination_path = game_dir.install.join(format!("randomizer.{ext}"));

    if file_path == destination_path {
        return Ok(destination_path);
    }

    backup_previous_rando_file(game_dir).wrap_err("Backing up existing seed file")?;

    info!(?file_path, ?destination_path, "Installing rando file");

    if should_move_rando_file(mode, &file_path) {
        move_file(&file_path, &destination_path).wrap_err("Moving seed file")?;
    } else {
        std::fs::copy(&file_path, &destination_path).wrap_err("Copying seed file")?;
    }

    Ok(destination_path)
}

#[instrument(ret(level=Level::DEBUG))]
fn should_move_rando_file(mode: MoveSeedMode, file_path: &Path) -> bool {
    match mode {
        MoveSeedMode::Always => true,
        MoveSeedMode::Never => false,
        MoveSeedMode::Auto => {
            static NAME_REGEX: LazyLock<Regex> =
                LazyLock::new(|| Regex::new(r"^randomizer \(\d+\)\.(?:bfr|dat)$").unwrap());

            let Some(file_name) = file_path.file_name() else {
                return false;
            };

            let file_name = file_name.to_string_lossy();

            file_name == "randomizer.dat"
                || file_name == "randomizer.bfr"
                || NAME_REGEX.is_match(&file_name)
        }
    }
}

#[instrument(skip_all)]
fn install_new_rando_file(game_dir: &GameDir, seed: &[u8]) -> Result<PathBuf> {
    let destination_path = game_dir.install.join("randomizer.bfr");

    backup_previous_rando_file(game_dir).wrap_err("Backing up existing seed file")?;

    info!(?destination_path, "Installing rando file");
    std::fs::write(&destination_path, seed).wrap_err("Writing randomizer.bfr")?;

    Ok(destination_path)
}

#[instrument(skip_all)]
fn backup_previous_rando_file(game_dir: &GameDir) -> Result<()> {
    let bfr_seed = game_dir.install.join("randomizer.bfr");
    if std::fs::exists(&bfr_seed).wrap_err("Checking if randomizer.bfr exists")? {
        backup_rando_file(game_dir, "randomizer.bfr", "bfr")?;
    }

    let dat_seed = game_dir.install.join("randomizer.dat");
    if std::fs::exists(&dat_seed).wrap_err("Checking if randomizer.dat exists")? {
        backup_rando_file(game_dir, "randomizer.dat", "dat")?;
    }

    Ok(())
}

#[instrument(skip_all, fields(file_name))]
fn backup_rando_file(game_dir: &GameDir, file_name: &str, extension: &str) -> Result<()> {
    let source_path = game_dir.install.join(file_name);
    let header_line = BufReader::new(File::open(&source_path).wrap_err("Reading seed file")?)
        .lines()
        .next()
        .ok_or_eyre("seed file was empty")?
        .wrap_err("Reading seed file")?;

    let target_dir = game_dir.install.join("seeds");
    std::fs::create_dir_all(&target_dir).wrap_err("Creating seeds directory")?;

    let seed_name = seed_name_for(&header_line);
    let (target_seed_path, target_stats_path) =
        get_seed_file_paths(&seed_name, extension, &target_dir);

    info!(?source_path, ?target_seed_path, "Backup up rando file");
    move_file(&source_path, &target_seed_path).wrap_err("Moving seed file")?;

    // Also move stats.txt
    let stats_path = game_dir.install.join("stats.txt");
    debug!(?stats_path, ?target_stats_path, "Moving stats.txt");
    match std::fs::rename(stats_path, target_stats_path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            debug!("Didn't move stats.txt, because it was not found");
        }
        Err(err) => error!(?err, "Could not move stats.txt"),
    }

    Ok(())
}

fn get_seed_file_paths(seed_name: &str, extension: &str, target_dir: &Path) -> (PathBuf, PathBuf) {
    let (seed_path, stats_path) = create_paths(seed_name, None, extension, target_dir);

    if check_paths(&seed_path, &stats_path) {
        return (seed_path, stats_path);
    }

    let random_suffix = Alphanumeric.sample_string(&mut rand::rng(), 10);
    create_paths(seed_name, Some(&random_suffix), extension, target_dir)
}

fn create_paths(
    seed_name: &str,
    suffix: Option<&str>,
    extension: &str,
    target_dir: &Path,
) -> (PathBuf, PathBuf) {
    let mut middle = String::from(seed_name);
    if let Some(suffix) = suffix {
        middle += "-";
        middle += suffix;
    }

    let seed_path = target_dir.join(format!("ori-{middle}.{extension}"));
    let stats_path = target_dir.join(format!("stats-{middle}.txt"));
    (seed_path, stats_path)
}

#[instrument(skip_all)]
fn check_paths(seed_path: &Path, stats_path: &Path) -> bool {
    let seed_exists = std::fs::exists(seed_path);
    let stats_exists = std::fs::exists(stats_path);

    matches!((seed_exists, stats_exists), (Ok(false), Ok(false)))
}

fn seed_name_for(header: &str) -> String {
    let Some((flags, seed_name)) = header.split_once('|') else {
        return "unknown".to_owned();
    };

    let flags = flags.to_lowercase();
    let flags: Vec<_> = flags.split(',').collect();

    let difficulty = get_difficulty(&flags);
    let key_mode = get_key_mode(&flags);
    let goal_mode = get_goal_mode(&flags);

    make_valid_filename(&format!("{difficulty}-{key_mode}-{goal_mode}-{seed_name}"))
}

fn get_difficulty(flags: &[&str]) -> &'static str {
    if flags.contains(&"casual") {
        "Casual"
    } else if flags.contains(&"standard") {
        "Standard"
    } else if flags.contains(&"expert") {
        "Expert"
    } else if flags.contains(&"master") {
        "Master"
    } else if flags.contains(&"glitched") {
        "Glitched"
    } else {
        "Custom"
    }
}

#[allow(clippy::if_same_then_else)]
fn get_key_mode(flags: &[&str]) -> &'static str {
    if flags.contains(&"free") {
        "Free"
    } else if flags.contains(&"shards") {
        "Shards"
    } else if flags.contains(&"clues") {
        "Clues"
    } else if flags.contains(&"limitkeys") {
        "Limitkeys"
    } else {
        // Implicit or explicitly specified with "Default" flag
        "None"
    }
}

fn get_goal_mode(flags: &[&str]) -> String {
    let mut modes = Vec::new();

    if flags.iter().any(|&f| f.starts_with("frags/")) {
        modes.push("Frags");
    }

    if flags.iter().any(|&f| f.starts_with("worldtour=")) {
        modes.push("WorldTour");
    }

    if flags.contains(&"forcemaps") {
        modes.push("ForceMaps");
    }

    if flags.contains(&"forcetrees") {
        modes.push("ForceTrees");
    }

    if flags.contains(&"bingo") {
        modes.push("Bingo");
    }

    if modes.is_empty() {
        "None".to_owned()
    } else {
        modes.join(",")
    }
}

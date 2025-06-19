use crate::files::{make_valid_filename, move_file};
use crate::game::GameDir;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::{Context, OptionExt};
use rand::distr::{Alphanumeric, SampleString};
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, instrument};

#[instrument]
pub fn play_rando_file(settings: Settings, file_path: PathBuf) -> Result<()> {
    install_rando_file(&settings.game_dir, file_path).wrap_err("Moving seed file")?;

    settings
        .game_dir
        .try_launch_game(settings.launch_type)
        .wrap_err("Launching game")
}

#[instrument(skip_all)]
fn install_rando_file(game_dir: &GameDir, file_path: PathBuf) -> Result<()> {
    let destination_path = game_dir.install.join("randomizer.dat");

    if std::fs::exists(&destination_path).wrap_err("Checking if randomizer.dat already exists")? {
        backup_rando_file(game_dir).wrap_err("Backing up existing randomizer.dat")?;
    }

    info!(?file_path, ?destination_path, "Installing rando file");
    move_file(&file_path, &destination_path).wrap_err("Moving randomizer.dat")
}

#[instrument(skip_all)]
fn backup_rando_file(game_dir: &GameDir) -> Result<()> {
    let source_path = game_dir.install.join("randomizer.dat");
    let header_line = BufReader::new(File::open(&source_path).wrap_err("Reading randomizer.dat")?)
        .lines()
        .next()
        .ok_or_eyre("randomizer.dat was empty")?
        .wrap_err("Reading randomizer.dat")?;

    let target_dir = game_dir.install.join("seeds");
    std::fs::create_dir_all(&target_dir).wrap_err("Creating seeds directory")?;

    let seed_name = seed_name_for(header_line);
    let (target_seed_path, target_stats_path) = get_seed_file_paths(&seed_name, &target_dir);

    info!(?source_path, ?target_seed_path, "Backup up rando file");
    move_file(&source_path, &target_seed_path).wrap_err("Moving randomizer.dat")?;

    // Also move stats.txt
    let stats_path = game_dir.install.join("stats.txt");
    debug!(?stats_path, ?target_stats_path, "Moving stats.txt");
    match std::fs::rename(stats_path, target_stats_path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            debug!("Didn't move stats.txt, because it was not found")
        }
        Err(err) => error!(?err, "Could not move stats.txt"),
    }

    Ok(())
}

fn get_seed_file_paths(seed_name: &str, target_dir: &Path) -> (PathBuf, PathBuf) {
    let (seed_path, stats_path) = create_paths(seed_name, None, target_dir);

    if check_paths(&seed_path, &stats_path) {
        return (seed_path, stats_path);
    }

    let random_suffix = Alphanumeric.sample_string(&mut rand::rng(), 10);
    create_paths(seed_name, Some(&random_suffix), target_dir)
}

fn create_paths(seed_name: &str, suffix: Option<&str>, target_dir: &Path) -> (PathBuf, PathBuf) {
    let mut middle = String::from(seed_name);
    if let Some(suffix) = suffix {
        middle += "-";
        middle += suffix;
    }

    let seed_path = target_dir.join(format!("ori-{middle}.dat"));
    let stats_path = target_dir.join(format!("stats-{middle}.txt"));
    (seed_path, stats_path)
}

#[instrument(skip_all)]
fn check_paths(seed_path: &Path, stats_path: &Path) -> bool {
    let seed_exists = std::fs::exists(seed_path);
    let stats_exists = std::fs::exists(stats_path);

    matches!((seed_exists, stats_exists), (Ok(false), Ok(false)))
}

fn seed_name_for(header: String) -> String {
    let Some((flags, seed_name)) = header.split_once("|") else {
        return "unknown".to_owned();
    };

    let mut flags = flags.to_owned();
    flags.insert(0, ',');
    flags.push(',');

    let difficulty = get_difficulty(&flags);
    let key_mode = get_key_mode(&flags);
    let goal_mode = get_goal_mode(&flags);

    make_valid_filename(&format!("{difficulty}-{key_mode}-{goal_mode}-{seed_name}"))
}

fn get_difficulty(header: &str) -> &str {
    if header.contains(",Casual,") {
        "Casual"
    } else if header.contains(",Standard,") {
        "Standard"
    } else if header.contains(",Expert,") {
        "Expert"
    } else if header.contains(",Master,") {
        "Master"
    } else if header.contains(",Glitched,") {
        "Glitched"
    } else {
        "Unknown"
    }
}

fn get_key_mode(header: &str) -> &str {
    if header.contains(",Free,") {
        "Free"
    } else if header.contains(",Shards,") {
        "Shards"
    } else if header.contains(",Clues,") {
        "Clues"
    } else if header.contains(",Limitkeys,") {
        "Limitkeys"
    } else if header.contains(",Default,") {
        "None"
    } else {
        "Unknown"
    }
}

fn get_goal_mode(header: &str) -> String {
    let mut modes = Vec::new();

    if header.contains(",Frags/") {
        modes.push("Frags");
    }

    if header.contains(",WorldTour=") {
        modes.push("WorldTour");
    }

    if header.contains(",ForceMaps,") {
        modes.push("ForceMaps");
    }

    if header.contains(",ForceTrees,") {
        modes.push("ForceTrees");
    }

    if header.contains(",Bingo,") {
        modes.push("Bingo");
    }

    if modes.is_empty() {
        "None".to_owned()
    } else {
        modes.join(",")
    }
}

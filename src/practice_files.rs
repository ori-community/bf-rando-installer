use crate::files::{move_file, same_path};
use crate::game::GameDir;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use rand::distr::{Alphanumeric, SampleString};
use std::io::ErrorKind;
use std::io::ErrorKind::NotFound;
use std::path::{Path, PathBuf};
use std::{fs, path};
use tracing::instrument;

#[instrument(skip(settings))]
pub fn play_practice_file(settings: &Settings, practice_file: &Path) -> Result<()> {
    let practice_dir =
        get_practice_dir(&settings.game_dir).wrap_err("Reading practice directory setting")?;

    let target_path = if practice_file
        .parent()
        .is_some_and(|parent| same_path(parent, &practice_dir))
    {
        practice_file.to_owned()
    } else {
        match fs::create_dir_all(&practice_dir) {
            Ok(()) => (),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => (),
            Err(err) => return Err(err).wrap_err("Creating practice directory"),
        }

        let target_path = target_path(&practice_dir, practice_file)
            .wrap_err("Determining target practice file name")?;

        move_file(practice_file, &target_path).wrap_err("Moving practice file")?;

        target_path
    };

    let target_path = path::absolute(&target_path).unwrap_or(target_path);

    settings
        .game_dir
        .try_play_practice_file(&target_path, settings)
        .wrap_err("Launching game")
}

fn target_path(practice_dir: &Path, practice_file: &Path) -> Result<PathBuf> {
    let file_name = practice_file
        .file_name()
        .unwrap_or("practice.bfrp".as_ref());

    let target_path = practice_dir.join(file_name);

    if !target_path
        .try_exists()
        .wrap_err("Determining whether target practice file name exists")?
    {
        return Ok(target_path);
    }

    let file_stem = Path::new(file_name)
        .file_stem()
        .expect("Guaranteed to exist via unwrap_or above")
        .display();

    let ext = practice_file
        .extension()
        .unwrap_or("bfrp".as_ref())
        .to_string_lossy();

    for i in 2..=10 {
        let target_path = practice_dir.join(format!("{file_stem} ({i}).{ext}"));

        if !target_path.try_exists().wrap_err_with(|| {
            format!("Determining whether target practice file name with suffix {i} exists")
        })? {
            return Ok(target_path);
        }
    }

    let random_suffix = Alphanumeric.sample_string(&mut rand::rng(), 10);
    Ok(practice_dir.join(format!("{}-{}.{ext}", file_stem, random_suffix)))
}

const PRACTICE_DIR_SETTING: &str = "Practice Folder";

#[instrument]
fn get_practice_dir(game_dir: &GameDir) -> Result<PathBuf> {
    let rando_settings = match fs::read_to_string(game_dir.install.join("RandomizerSettings.txt")) {
        Ok(settings) => settings,
        Err(err) if err.kind() == NotFound => return Ok(default_practice_dir(game_dir)),
        Err(err) => return Err(err).wrap_err("Reading RandomizerSettings.txt"),
    };

    for line in rando_settings.lines() {
        if !line.contains(PRACTICE_DIR_SETTING) {
            continue;
        }

        let line = line.split_once("//").map(|(line, _)| line).unwrap_or(line);
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };

        if key.trim() != PRACTICE_DIR_SETTING {
            continue;
        }

        return Ok(game_dir.install.join(value.trim()));
    }

    Ok(default_practice_dir(game_dir))
}

fn default_practice_dir(game_dir: &GameDir) -> PathBuf {
    game_dir.install.join("practice")
}

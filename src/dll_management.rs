use crate::dll_classifier::{DllClassification, RandoVersion, classify_dll_file};
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use rand::distr::{Alphanumeric, SampleString};
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::fs::read_dir;
use std::io::ErrorKind;
use std::mem;
use std::path::{Path, PathBuf};
use tracing::{Span, debug, error, info, instrument};

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct GameDir {
    managed: PathBuf,
}

impl GameDir {
    pub fn new(mut game_dir: PathBuf) -> Self {
        game_dir.extend(["oriDE_Data", "Managed"]);
        Self { managed: game_dir }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OriDll {
    pub kind: OriDllKind,
    pub path: PathBuf,
    pub display_name: String,
}

impl OriDll {
    fn new(path: PathBuf, classification: DllClassification) -> Option<Self> {
        let kind = match classification {
            DllClassification::Invalid => return None,
            DllClassification::NonDe => return None,
            DllClassification::Vanilla => OriDllKind::Vanilla,
            DllClassification::Rando(v) => OriDllKind::Rando(v),
            DllClassification::UnknownRando(hash) => OriDllKind::UnknownRando(hash),
        };

        let display_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        Some(Self {
            display_name,
            path,
            kind,
        })
    }
}

impl Display for OriDll {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            OriDllKind::Vanilla => f.write_str("Vanilla"),
            OriDllKind::UnknownRando(_) => {
                f.write_fmt(format_args!("Rando (unknown) [{}]", self.display_name))
            }
            OriDllKind::Rando(version) => f.write_fmt(format_args!("Rando ({version})")),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum OriDllKind {
    Vanilla,
    Rando(RandoVersion),
    UnknownRando(u64),
}

#[instrument(skip(all_dlls, to_install), fields(to_install.path=?to_install.path))]
pub fn install_dll(game_dir: &GameDir, to_install: &OriDll, all_dlls: &[OriDll]) -> Result<()> {
    let target = prepare_target(game_dir, all_dlls)?;

    info!(?target, "Copying/Installing dll");
    std::fs::copy(&to_install.path, target).wrap_err("Error copying dll")?;

    Ok(())
}

#[instrument(skip(dll, all_dlls))]
pub fn install_new_dll(game_dir: &GameDir, dll: &[u8], all_dlls: &[OriDll]) -> Result<()> {
    let target = prepare_target(game_dir, all_dlls)?;

    info!(?target, "Installing dll");
    std::fs::write(target, dll).wrap_err("Error writing dll")?;

    Ok(())
}

#[instrument(skip(game_dir, all_dlls))]
fn prepare_target(game_dir: &GameDir, all_dlls: &[OriDll]) -> Result<PathBuf> {
    let target = game_dir.managed.join("Assembly-CSharp.dll");

    let target_classification = match classify_dll_file(&target) {
        Ok(classification) => classification,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(target),
        Err(err) => return Err(err).wrap_err("Failed to classify target"),
    };

    if should_backup_target(&target, target_classification, all_dlls) {
        let new_name = unique_name_for_dll(&game_dir.managed, target_classification);
        info!(install_target=?target, ?new_name, "Renaming dll as backup");
        std::fs::rename(&target, new_name).wrap_err("Error creating backup")?;
    }

    Ok(target)
}

#[instrument(skip(all_dlls), ret)]
fn should_backup_target(
    target: &Path,
    target_classification: DllClassification,
    all_dlls: &[OriDll],
) -> bool {
    let copy_exists = all_dlls
        .iter()
        .filter(|&dll| dll.path != target)
        .any(|dll| match (dll.kind, target_classification) {
            (OriDllKind::Vanilla, DllClassification::Vanilla) => true,
            (OriDllKind::Rando(a), DllClassification::Rando(b)) => a == b,
            (OriDllKind::UnknownRando(a), DllClassification::UnknownRando(b)) => a == b,
            _ => false,
        });

    !copy_exists
}

#[instrument]
fn unique_name_for_dll(target_dir: &Path, dll: DllClassification) -> PathBuf {
    let target_name = match dll {
        DllClassification::Vanilla => Cow::Borrowed("Assembly-CSharp.vanilla"),
        DllClassification::UnknownRando(_) => "Assembly-CSharp.rando".into(),
        DllClassification::Rando(v) => format!("Assembly-CSharp.rando.{v}").into(),
        _ => "Assembly-CSharp.unknown".into(),
    };

    let target_path = target_dir.join(format!("{target_name}.dll"));

    if let Ok(false) = std::fs::exists(&target_path) {
        return target_path;
    }

    let random_suffix = Alphanumeric.sample_string(&mut rand::rng(), 10);

    target_dir.join(format!("{target_name}.{random_suffix}.dll"))
}

#[instrument]
pub fn search_game_dir(game_dir: &GameDir) -> Result<(Option<OriDll>, Vec<OriDll>)> {
    let current_span = Span::current();

    let mut all_dlls = read_dir(&game_dir.managed)
        .wrap_err("Couldn't read ori dll dir")?
        .par_bridge()
        .filter_map(|file| {
            let _span = current_span.enter();

            let file = match file {
                Ok(f) => f,
                Err(err) => return Some(Err(err).wrap_err("Couldn't list dll file")),
            };

            let path = file.path();

            let classification = match classify_dll_file(&path) {
                Ok(c) => c,
                Err(err) => {
                    error!(?path, ?err, "Couldn't classify file");
                    return None;
                }
            };

            debug!(?path, ?classification, "Classified file");

            OriDll::new(path, classification).map(Ok)
        })
        .collect::<Result<Vec<_>>>()?;

    let installed_path = game_dir.managed.join("Assembly-CSharp.dll");
    let current_idx = all_dlls.iter().position(|dll| dll.path == installed_path);
    let current = current_idx.map(|i| all_dlls[i].clone());

    sort_and_filter_duplicates(&mut all_dlls, current_idx);

    Ok((current, all_dlls))
}

#[instrument(skip_all)]
fn sort_and_filter_duplicates(dlls: &mut Vec<OriDll>, current_idx: Option<usize>) {
    // Place current dll at the end, to retain a copy of it, if it exists
    // This is so the copy is found when checking for whether a backup needs to be made
    if let Some(current_idx) = current_idx {
        let last = dlls.len() - 1;
        dlls.swap(current_idx, last);
    }

    dlls.sort_by_key(|dll| dll.kind);

    let mut prev_kind = None;
    *dlls = mem::take(dlls)
        .into_iter()
        .filter(|dll| !same_as_previous(&mut prev_kind, dll.kind))
        .collect();
}

fn same_as_previous(previous: &mut Option<OriDllKind>, new: OriDllKind) -> bool {
    if let Some(prev) = previous.replace(new) {
        match (prev, new) {
            (OriDllKind::Vanilla, OriDllKind::Vanilla) => true,
            (OriDllKind::Rando(a), OriDllKind::Rando(b)) => a == b,
            (OriDllKind::UnknownRando(a), OriDllKind::UnknownRando(b)) => a == b,
            _ => false,
        }
    } else {
        false
    }
}

use crate::manifest::MANIFEST_NAME;
use crate::t;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const NAMED_PATCH_PREFIX: &str = "patch_";
pub const DEFAULT_PATCH_DIR_NAME: &str = "Patch";
pub const APPLIED_MARKER_FILE_NAME: &str = ".applied_patch.json";
const APPLIED_MARKER_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchAction {
    Apply,
    Rollback,
}

impl PatchAction {
    fn title_key(self) -> &'static str {
        match self {
            Self::Apply => "patch.choose-apply",
            Self::Rollback => "patch.choose-rollback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchCandidate {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedPatchMarker {
    pub format: u32,
    pub apply_id: String,
    pub patch_dir: String,
    pub sequence: u128,
}

/// Convert a user-provided patch name into the directory name used on disk.
///
/// Both `v1.4.0` and `patch_v1.4.0` result in `patch_v1.4.0`.  The returned
/// name is always a single, safe directory component below `base_dir`.
pub fn patch_dir_for_name(base_dir: &Path, patch_name: Option<&str>) -> anyhow::Result<PathBuf> {
    let Some(raw_name) = patch_name else {
        return Ok(base_dir.join(DEFAULT_PATCH_DIR_NAME));
    };

    let raw_name = raw_name.trim();
    let id = raw_name
        .strip_prefix(NAMED_PATCH_PREFIX)
        .unwrap_or(raw_name);

    if !is_valid_patch_id(id) {
        anyhow::bail!("{}", t!("patch.invalid-name", raw_name));
    }

    Ok(base_dir.join(format!("{NAMED_PATCH_PREFIX}{id}")))
}

fn is_valid_patch_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.ends_with('.')
        && !id.ends_with(' ')
        && !id.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
}

/// Require a patch bundle to be a real, direct child directory of the target
/// root.  This protects marker writes and keeps a selected bundle from
/// redirecting file operations through a symlink.
pub fn validate_patch_dir(base_dir: &Path, patch_dir: &Path) -> anyhow::Result<()> {
    let base_abs = std::fs::canonicalize(base_dir)?;
    let patch_metadata = std::fs::symlink_metadata(patch_dir)?;
    if !patch_metadata.file_type().is_dir() {
        anyhow::bail!("{}", t!("patch.unsafe-dir", patch_dir.display()));
    }

    let patch_abs = std::fs::canonicalize(patch_dir)?;
    if patch_abs.parent() != Some(base_abs.as_path()) {
        anyhow::bail!("{}", t!("patch.unsafe-dir", patch_dir.display()));
    }

    Ok(())
}

fn marker_path(patch_dir: &Path) -> PathBuf {
    patch_dir.join(APPLIED_MARKER_FILE_NAME)
}

fn next_sequence() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

/// Record a successful apply in the selected patch directory.
pub fn write_applied_marker(
    base_dir: &Path,
    patch_dir: &Path,
) -> anyhow::Result<AppliedPatchMarker> {
    validate_patch_dir(base_dir, patch_dir)?;

    let sequence = next_sequence();
    let patch_dir_name = patch_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("{}", t!("patch.unsafe-dir", patch_dir.display())))?;
    let marker = AppliedPatchMarker {
        format: APPLIED_MARKER_FORMAT,
        apply_id: format!("{sequence}-{}", std::process::id()),
        patch_dir: patch_dir_name.to_owned(),
        sequence,
    };

    let marker_path = marker_path(patch_dir);
    let temp_path = patch_dir.join(format!("{APPLIED_MARKER_FILE_NAME}.tmp-{sequence}"));
    let content = serde_json::to_string_pretty(&marker)?;
    let write_result = (|| -> io::Result<()> {
        let mut temp_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        temp_file.write_all(content.as_bytes())?;
        temp_file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }

    let rename_result = {
        #[cfg(windows)]
        {
            if std::fs::symlink_metadata(&marker_path).is_ok() {
                let metadata = std::fs::symlink_metadata(&marker_path)?;
                if !metadata.file_type().is_file() {
                    let _ = std::fs::remove_file(&temp_path);
                    anyhow::bail!("{}", t!("patch.unsafe-marker", marker_path.display()));
                }
                std::fs::remove_file(&marker_path)?;
            }
            std::fs::rename(&temp_path, &marker_path)
        }
        #[cfg(not(windows))]
        {
            std::fs::rename(&temp_path, &marker_path)
        }
    };

    if let Err(error) = rename_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }

    Ok(marker)
}

pub fn remove_applied_marker(patch_dir: &Path) -> anyhow::Result<()> {
    crate::path::ensure_no_symlink_components(patch_dir)?;
    let path = marker_path(patch_dir);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        anyhow::bail!("{}", t!("patch.unsafe-marker", path.display()));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

fn load_marker(patch_dir: &Path) -> anyhow::Result<Option<AppliedPatchMarker>> {
    crate::path::ensure_no_symlink_components(patch_dir)?;
    let path = marker_path(patch_dir);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        anyhow::bail!("{}", t!("patch.unsafe-marker", path.display()));
    }

    let marker: AppliedPatchMarker = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    if marker.format != APPLIED_MARKER_FORMAT
        || marker.apply_id.is_empty()
        || marker.patch_dir
            != patch_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
    {
        anyhow::bail!("{}", t!("patch.invalid-marker", path.display()));
    }
    Ok(Some(marker))
}

fn marker_candidates(base_dir: &Path) -> anyhow::Result<Vec<PatchCandidate>> {
    let mut candidates = find_named_patch_dirs(base_dir)?;
    let default_path = base_dir.join(DEFAULT_PATCH_DIR_NAME);
    let default_manifest = default_path.join(MANIFEST_NAME);
    let default_is_dir = std::fs::symlink_metadata(&default_path)
        .is_ok_and(|metadata| metadata.file_type().is_dir());
    let default_has_manifest = std::fs::symlink_metadata(default_manifest)
        .is_ok_and(|metadata| metadata.file_type().is_file());
    if default_is_dir && default_has_manifest {
        candidates.push(PatchCandidate {
            id: DEFAULT_PATCH_DIR_NAME.to_owned(),
            path: default_path,
        });
    }
    Ok(candidates)
}

/// Return the most recently successful applied patch, if its marker is valid.
pub fn latest_applied_patch(
    base_dir: &Path,
) -> anyhow::Result<Option<(PatchCandidate, AppliedPatchMarker)>> {
    let mut latest = None;
    for candidate in marker_candidates(base_dir)? {
        let Some(marker) = load_marker(&candidate.path)? else {
            continue;
        };
        if latest
            .as_ref()
            .is_none_or(|(_, current): &(PatchCandidate, AppliedPatchMarker)| {
                marker.sequence > current.sequence
            })
        {
            latest = Some((candidate, marker));
        }
    }
    Ok(latest)
}

/// Find named patch directories directly below `base_dir`.
///
/// A directory is considered a patch candidate only when its name starts
/// with `patch_`, has a non-empty suffix, and contains a manifest file.  The
/// directory itself must not be a symlink.
pub fn find_named_patch_dirs(base_dir: &Path) -> anyhow::Result<Vec<PatchCandidate>> {
    let entries = match std::fs::read_dir(base_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(id) = name.strip_prefix(NAMED_PATCH_PREFIX) else {
            continue;
        };
        if !is_valid_patch_id(id) {
            continue;
        }

        let path = entry.path();
        let manifest_path = path.join(MANIFEST_NAME);
        let manifest_metadata = match std::fs::symlink_metadata(&manifest_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !manifest_metadata.file_type().is_file() {
            continue;
        }

        candidates.push(PatchCandidate {
            id: id.to_owned(),
            path,
        });
    }

    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(candidates)
}

/// Select a named patch interactively, or fall back to the historical
/// `Patch/` directory when no named patches are present.
pub fn select_patch_dir(base_dir: &Path, action: PatchAction) -> anyhow::Result<Option<PathBuf>> {
    if action == PatchAction::Rollback
        && let Some((candidate, marker)) = latest_applied_patch(base_dir)?
    {
        println!(
            "{}",
            t!("patch.active-selected", marker.apply_id, candidate.id)
        );
        return Ok(Some(candidate.path));
    }

    let candidates = find_named_patch_dirs(base_dir)?;
    if candidates.is_empty() {
        return Ok(Some(base_dir.join(DEFAULT_PATCH_DIR_NAME)));
    }

    println!("{}", t!(action.title_key()));
    for (index, candidate) in candidates.iter().enumerate() {
        println!("{}", t!("patch.choice-item", index + 1, &candidate.id));
    }
    println!("{}", t!("patch.choice-exit"));

    loop {
        print!("{}", t!("patch.choice-prompt"));
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            return Ok(None);
        }

        match input.trim().parse::<usize>() {
            Ok(0) => {
                println!("{}", t!("patch.choice-cancelled"));
                return Ok(None);
            }
            Ok(choice) if (1..=candidates.len()).contains(&choice) => {
                return Ok(Some(candidates[choice - 1].path.clone()));
            }
            _ => println!("{}", t!("patch.choice-invalid", candidates.len())),
        }
    }
}

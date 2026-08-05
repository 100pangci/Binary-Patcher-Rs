use crate::cli::PatchFormat;
use crate::cli::PatchMode;
use crate::fmt::format_size;
use crate::fs::relative_maps;
use crate::hash::sha256_of_file;
use crate::hdiffpatch::{get_diff_thread_count, run_hdiffz, run_hdiffz_mem, run_hdiffz_stream};
use crate::manifest::{AddedEntry, ChangedEntry, DeletedEntry, INSTRUCTIONS_NAME, Manifest};
use crate::path::ensure_parent_dir;
use crate::t;
use anyhow::Context;
use std::path::Path;

#[allow(clippy::needless_pass_by_value)]
pub fn build_patch_bundle(
    base_dir: &Path,
    use_compression: bool,
    mode: PatchMode,
    format: PatchFormat,
) -> anyhow::Result<()> {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");
    let patch_dir = base_dir.join("Patch");

    if patch_dir.exists() {
        eprintln!("{}", t!("bundle.will-clear-patch", patch_dir.display()));
        std::fs::remove_dir_all(&patch_dir)?;
    }
    std::fs::create_dir_all(&patch_dir)?;

    let (old_files, old_dirs) = relative_maps(&old_dir);
    let (new_files, new_dirs) = relative_maps(&new_dir);

    let fast_format = matches!(format, PatchFormat::Fast);

    let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for k in old_files.keys() {
        all_paths.insert(k.clone());
    }
    for k in new_files.keys() {
        all_paths.insert(k.clone());
    }

    let mut manifest = Manifest::default();
    let mut changed_count = 0;
    let mut added_count = 0;
    let mut deleted_count = 0;
    let mut deleted_dirs_count = 0;

    println!("{}", t!("bundle.scanning"));

    for relative_path in all_paths {
        let old_path = old_files.get(&relative_path);
        let new_path = new_files.get(&relative_path);

        match (old_path, new_path) {
            (Some(old), Some(new)) => {
                let patch_output = patch_dir.join(format!("{relative_path}.patch"));
                let entry = process_changed(
                    old,
                    new,
                    &patch_output,
                    use_compression,
                    fast_format,
                    &relative_path,
                    &mode,
                )?;
                if let Some(entry) = entry {
                    manifest.changed.push(entry);
                    changed_count += 1;
                }
            }
            (None, Some(new)) => {
                let added_output = patch_dir.join(format!("{relative_path}.new"));
                ensure_parent_dir(&added_output)?;
                std::fs::copy(new, &added_output)?;
                let new_hash = sha256_of_file(new)?;
                println!("{}", t!("bundle.added", &relative_path));
                manifest.added.push(AddedEntry {
                    path: relative_path.clone(),
                    new_sha256: new_hash,
                    file: format!("{relative_path}.new"),
                });
                added_count += 1;
            }
            (Some(old), None) => {
                let old_hash = sha256_of_file(old)?;
                println!("{}", t!("bundle.deleted", &relative_path));
                manifest.deleted.push(DeletedEntry {
                    path: relative_path.clone(),
                    old_sha256: old_hash,
                });
                deleted_count += 1;
            }
            (None, None) => unreachable!(),
        }
    }

    for rel_path in old_dirs.keys() {
        if !new_dirs.contains_key(rel_path) {
            manifest.deleted_dirs.push(rel_path.clone());
            println!("{}", t!("bundle.deleted-dir", &rel_path));
            deleted_dirs_count += 1;
        }
    }
    manifest
        .deleted_dirs
        .sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));

    manifest.save(&patch_dir)?;
    write_patch_instructions(&patch_dir)?;

    println!("\n{}", t!("bundle.complete"));
    println!("{}", t!("bundle.changed-count", changed_count));
    println!("{}", t!("bundle.added-count", added_count));
    println!("{}", t!("bundle.deleted-count", deleted_count));
    println!("{}", t!("bundle.deleted-dir-count", deleted_dirs_count));
    println!("{}", t!("bundle.output-dir", patch_dir.display()));

    Ok(())
}

fn process_changed(
    old: &Path,
    new: &Path,
    patch_output: &Path,
    use_compression: bool,
    fast_format: bool,
    relative_path: &str,
    mode: &PatchMode,
) -> anyhow::Result<Option<ChangedEntry>> {
    let old_hash = sha256_of_file(old)?;
    let new_hash = sha256_of_file(new)?;
    if old_hash == new_hash {
        return Ok(None);
    }
    println!("{}", t!("bundle.changed", relative_path));

    let old_size = std::fs::metadata(old)?.len();
    let new_size = std::fs::metadata(new)?.len();

    let thread_count = match mode {
        PatchMode::Stream => run_hdiffz_stream(
            old,
            new,
            patch_output,
            get_diff_thread_count(),
            use_compression,
            fast_format,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?,
        PatchMode::Memory => {
            let old_data =
                std::fs::read(old).with_context(|| t!("bundle.failed-read-old", old.display()))?;
            let new_data =
                std::fs::read(new).with_context(|| t!("bundle.failed-read-new", new.display()))?;
            run_hdiffz_mem(
                &old_data,
                &new_data,
                patch_output,
                get_diff_thread_count(),
                use_compression,
                fast_format,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?
        }
        PatchMode::Auto => run_hdiffz(old, new, patch_output, use_compression, fast_format)?,
    };

    print_patch_result(old_size, new_size, patch_output, thread_count)?;
    Ok(Some(ChangedEntry {
        path: relative_path.to_string(),
        old_sha256: old_hash,
        new_sha256: new_hash,
        patch_file: format!("{relative_path}.patch"),
    }))
}

fn print_patch_result(
    old_size: u64,
    new_size: u64,
    patch_file: &Path,
    thread_count: u32,
) -> anyhow::Result<()> {
    let patch_size = std::fs::metadata(patch_file)?.len();
    println!("  {}", "-".repeat(30));
    println!("  {}", t!("bundle.patch-success"));
    println!("    - {}", t!("main.threads-used", thread_count));
    println!("    - {}", t!("main.old-size", format_size(old_size)));
    println!("    - {}", t!("main.new-size", format_size(new_size)));
    println!("    - {}", t!("main.patch-size", format_size(patch_size)));
    println!("  {}", "-".repeat(30));
    Ok(())
}

fn write_patch_instructions(patch_dir: &Path) -> anyhow::Result<()> {
    let lines = [
        "This is an auto-generated patch bundle by binary_patcher.",
        "",
        "Usage:",
        "1. Copy the entire Patch folder to the old version root directory.",
        "2. Place apply_patch in the old version root directory and run it.",
        "3. The program will apply patches according to manifest.json.",
    ];
    std::fs::write(patch_dir.join(INSTRUCTIONS_NAME), lines.join("\n"))?;
    Ok(())
}

pub fn init_workspace(base_dir: &Path) -> anyhow::Result<bool> {
    let mut created = Vec::new();

    for folder_name in &["Old", "New", "Patch"] {
        let folder_path = base_dir.join(folder_name);
        if !folder_path.exists() {
            std::fs::create_dir_all(&folder_path)?;
            created.push(*folder_name);
        }
    }

    if !created.is_empty() {
        println!("{}", t!("bundle.workspace-initialized", created.join(", ")));
    }

    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");

    let old_empty = std::fs::read_dir(&old_dir)?.next().is_none();
    let new_empty = std::fs::read_dir(&new_dir)?.next().is_none();

    if old_empty || new_empty {
        println!("\n{}", t!("bundle.workspace-instructions"));
        println!("{}", t!("bundle.workspace-old"));
        println!("{}", t!("bundle.workspace-new"));
        println!("{}", t!("bundle.workspace-output"));
        println!("\n{}", t!("bundle.workspace-ready"));
        return Ok(false);
    }

    Ok(true)
}

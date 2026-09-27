// ===========================================================================
// Explicit file-name mappings (rename-aware diff)
// ===========================================================================

use binary_patcher::cli::{PatchFormat, PatchMode};
use binary_patcher::manifest::Manifest;
use std::path::Path;

fn copy_tree_files(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(src).unwrap();
            let out_path = dst.join(rel);
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), &out_path).unwrap();
        }
    }
}

fn try_build_bundle(base_dir: &Path) -> anyhow::Result<()> {
    binary_patcher::bundle::build_patch_bundle(base_dir, PatchMode::Memory, PatchFormat::Precise)
}

fn build_bundle(base_dir: &Path) {
    try_build_bundle(base_dir).unwrap();
}

fn write_mapping_file(base_dir: &Path, mappings: &[(&str, &str)]) {
    let mappings: Vec<serde_json::Value> = mappings
        .iter()
        .map(|(old, new)| serde_json::json!({ "old": old, "new": new }))
        .collect();
    let content = serde_json::json!({ "mappings": mappings });
    std::fs::write(
        base_dir.join("file-map.json"),
        serde_json::to_string_pretty(&content).unwrap(),
    )
    .unwrap();
}

fn write_mapping_file_with_delete(base_dir: &Path, mappings: &[(&str, &str, bool)]) {
    let mappings: Vec<serde_json::Value> = mappings
        .iter()
        .map(|(old, new, delete)| {
            serde_json::json!({ "old": old, "new": new, "delete_source": delete })
        })
        .collect();
    let content = serde_json::json!({ "mappings": mappings });
    std::fs::write(
        base_dir.join("file-map.json"),
        serde_json::to_string_pretty(&content).unwrap(),
    )
    .unwrap();
}

fn count_files(root: &Path) -> usize {
    if !root.exists() {
        return 0;
    }
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .count()
}

fn write_file(base_dir: &Path, root: &str, relative: &str, data: &[u8]) {
    let path = base_dir.join(root).join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn setup_game(base_dir: &Path) -> std::path::PathBuf {
    let game = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game);
    copy_tree_files(&base_dir.join("Patch"), &game.join("Patch"));
    game
}

// ===========================================================================
// Bundle scanning
// ===========================================================================

#[test]
fn test_bundle_mapped_changed_counts_and_manifest() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload for foo");
    write_file(base, "New", "foo.chs", b"new payload for foo, different!");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let patch_dir = base.join("Patch");
    let manifest = Manifest::load(&patch_dir).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.added.len(), 0);
    assert_eq!(manifest.deleted.len(), 0);

    let entry = &manifest.changed[0];
    assert_eq!(entry.path, "foo.chs");
    assert_eq!(entry.source_path.as_deref(), Some("foo.pak"));
    assert_eq!(entry.patch_file, "foo.chs.patch");
    assert!(entry.is_renamed());
    assert!(patch_dir.join("foo.chs.patch").is_file());
    assert!(!patch_dir.join("foo.chs.new").exists());
    assert!(!patch_dir.join("foo.pak.patch").exists());
}

#[test]
fn test_bundle_mapped_identical_content_still_renames() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"identical payload");
    write_file(base, "New", "foo.chs", b"identical payload");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1, "rename must not be dropped");
    let entry = &manifest.changed[0];
    assert_eq!(entry.path, "foo.chs");
    assert_eq!(entry.source_path.as_deref(), Some("foo.pak"));
    assert_eq!(entry.old_sha256, entry.new_sha256);
}

#[test]
fn test_bundle_mapped_subdirectory() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(
        base,
        "Old",
        "data/script/foo.pak",
        b"subdirectory old payload",
    );
    write_file(
        base,
        "New",
        "data/script/foo.chs",
        b"subdirectory new payload changed",
    );
    write_mapping_file(base, &[("data/script/foo.pak", "data/script/foo.chs")]);

    build_bundle(base);

    let patch_dir = base.join("Patch");
    let manifest = Manifest::load(&patch_dir).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.added.len(), 0);
    assert_eq!(manifest.deleted.len(), 0);
    assert_eq!(manifest.changed[0].path, "data/script/foo.chs");
    assert_eq!(
        manifest.changed[0].source_path.as_deref(),
        Some("data/script/foo.pak")
    );
    assert!(patch_dir.join("data/script/foo.chs.patch").is_file());
}

#[test]
fn test_bundle_mixed_mapped_normal_added_deleted() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"mapped old");
    write_file(base, "New", "foo.chs", b"mapped new, changed");
    write_file(base, "Old", "same.txt", b"normal old");
    write_file(base, "New", "same.txt", b"normal new, changed");
    write_file(base, "New", "added.txt", b"added file");
    write_file(base, "Old", "dep.txt", b"deleted file");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 2);
    assert_eq!(manifest.added.len(), 1);
    assert_eq!(manifest.deleted.len(), 1);

    let mapped = manifest
        .changed
        .iter()
        .find(|entry| entry.path == "foo.chs")
        .expect("mapped entry");
    assert_eq!(mapped.source_path.as_deref(), Some("foo.pak"));

    let normal = manifest
        .changed
        .iter()
        .find(|entry| entry.path == "same.txt")
        .expect("normal entry");
    assert!(normal.source_path.is_none());
    assert_eq!(normal.patch_file, "same.txt.patch");

    assert_eq!(manifest.added[0].path, "added.txt");
    assert_eq!(manifest.deleted[0].path, "dep.txt");
}

#[test]
fn test_bundle_mapping_ignores_leftover_source_in_new() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "New", "foo.chs", b"new payload changed");
    // 准备 New 时忘记删除旧名文件（与 Old 逐字节相同）。
    write_file(base, "New", "foo.pak", b"old payload");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let patch_dir = base.join("Patch");
    let manifest = Manifest::load(&patch_dir).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.added.len(), 0, "leftover source must not be added");
    assert_eq!(manifest.deleted.len(), 0);
    assert!(
        !patch_dir.join("foo.pak.new").exists(),
        "leftover source must not be copied into the patch"
    );
}

#[test]
fn test_bundle_mapping_target_in_old_is_not_deleted() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "Old", "foo.chs", b"previous target");
    write_file(base, "New", "foo.chs", b"new payload changed");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.added.len(), 0);
    assert_eq!(
        manifest.deleted.len(),
        0,
        "mapping target must not be scheduled for deletion"
    );

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"old payload",
        "mapping source must be kept"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"new payload changed"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"old payload",
        "mapping source must stay untouched"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"previous target",
        "original target content must be restored"
    );
}

// ===========================================================================
// Fidelity invariant: apply(Old) == New even when New keeps the source file
// ===========================================================================

fn list_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("Patch/") || rel.contains(".backup_before_patch") {
            continue;
        }
        files.push(rel);
    }
    files.sort();
    files
}

#[test]
fn test_apply_result_matches_new_with_mapping_and_leftovers() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"mapped source payload");
    write_file(base, "Old", "keep.txt", b"normal old");
    write_file(base, "New", "foo.pak", b"mapped source payload");
    write_file(base, "New", "foo.chs", b"mapped target payload, changed");
    write_file(base, "New", "keep.txt", b"normal new, changed");
    write_file(base, "New", "sub/added.dat", b"brand new file");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();

    // apply(Old) must be byte-identical to New.
    let new_files = list_files(&base.join("New"));
    let game_files = list_files(&game);
    assert_eq!(new_files, game_files, "file sets must match New exactly");
    for rel in &new_files {
        assert_eq!(
            std::fs::read(base.join("New").join(rel)).unwrap(),
            std::fs::read(game.join(rel)).unwrap(),
            "content of {rel} must match New"
        );
    }

    // And rollback must return to exactly Old.
    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    let old_files = list_files(&base.join("Old"));
    let game_files = list_files(&game);
    assert_eq!(old_files, game_files, "rollback must match Old exactly");
    for rel in &old_files {
        assert_eq!(
            std::fs::read(base.join("Old").join(rel)).unwrap(),
            std::fs::read(game.join(rel)).unwrap(),
            "content of {rel} must match Old after rollback"
        );
    }
}

// ===========================================================================
// Apply / rollback
// ===========================================================================

#[test]
fn test_apply_and_rollback_mapped_rename() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "New", "foo.chs", b"new payload with more bytes");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();

    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        std::fs::read(base.join("Old/foo.pak")).unwrap(),
        "mapping source must be kept unchanged"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        std::fs::read(base.join("New/foo.chs")).unwrap()
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();

    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        std::fs::read(base.join("Old/foo.pak")).unwrap(),
        "source must stay untouched"
    );
    assert!(!game.join("foo.chs").exists(), "target must be removed");
}

#[test]
fn test_apply_and_rollback_mapped_identical_content() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"identical payload");
    write_file(base, "New", "foo.chs", b"identical payload");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();

    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"identical payload",
        "mapping source must be kept"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"identical payload"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();

    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"identical payload"
    );
    assert!(!game.join("foo.chs").exists());
}

#[test]
fn test_bundle_and_apply_mapped_empty_files() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"");
    write_file(base, "New", "foo.chs", b"");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.changed[0].path, "foo.chs");
    assert_eq!(manifest.changed[0].source_path.as_deref(), Some("foo.pak"));

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert!(
        game.join("foo.pak").is_file(),
        "mapping source must be kept"
    );
    assert_eq!(std::fs::read(game.join("foo.chs")).unwrap(), b"");

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert!(game.join("foo.pak").is_file());
    assert!(!game.join("foo.chs").exists());
}

#[test]
fn test_apply_and_rollback_mapped_with_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "New", "foo.chs", b"new payload");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("foo.chs"), b"pre-existing target").unwrap();

    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"old payload",
        "mapping source must be kept"
    );
    assert_eq!(std::fs::read(game.join("foo.chs")).unwrap(), b"new payload");

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"old payload",
        "mapping source must stay untouched"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"pre-existing target",
        "original target must be restored"
    );
}

#[test]
fn test_apply_failure_auto_rollback_mapped() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "New", "foo.chs", b"new payload changed");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("Patch/foo.chs.patch"), b"CORRUPTED PATCH").unwrap();

    let result = binary_patcher::apply::apply_bundle(&game);
    assert!(result.is_err(), "apply must fail with a corrupted patch");
    assert_eq!(
        std::fs::read(game.join("foo.pak")).unwrap(),
        b"old payload",
        "source must stay untouched after failed apply"
    );
    assert!(
        !game.join("foo.chs").exists(),
        "target must not remain after failed apply"
    );
}

#[test]
fn test_apply_failure_auto_rollback_mapped_with_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.pak", b"old payload");
    write_file(base, "New", "foo.chs", b"new payload changed");
    write_mapping_file(base, &[("foo.pak", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("foo.chs"), b"pre-existing target").unwrap();
    std::fs::write(game.join("Patch/foo.chs.patch"), b"CORRUPTED PATCH").unwrap();

    let result = binary_patcher::apply::apply_bundle(&game);
    assert!(result.is_err(), "apply must fail with a corrupted patch");
    assert_eq!(std::fs::read(game.join("foo.pak")).unwrap(), b"old payload");
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"pre-existing target",
        "original target must be restored after failed apply"
    );
}

// ===========================================================================
// Journal crash recovery for RenamedPatched
// ===========================================================================

fn write_renamed_journal(patch_dir: &Path, source: &str, target: &str, target_had_backup: bool) {
    let journal = serde_json::json!([{
        "type": "renamed_patched",
        "source": source,
        "target": target,
        "target_had_backup": target_had_backup,
    }]);
    std::fs::write(
        patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME),
        serde_json::to_string(&journal).unwrap(),
    )
    .unwrap();
}

fn create_backup_file(patch_dir: &Path, relative: &str, data: &[u8]) {
    let path = patch_dir
        .join(".backup_before_patch")
        .join(relative)
        .with_file_name(format!(
            "{}.backup_before_patch",
            Path::new(relative).file_name().unwrap().to_string_lossy()
        ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

#[test]
fn test_journal_recovery_renamed_removes_target_keeps_source() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();

    // Crash after the target was written: rollback removes the target and
    // leaves the mapping source completely untouched.
    write_file(base, "", "data/foo.pak", b"original payload");
    write_file(base, "", "data/foo.chs", b"patched payload");
    write_renamed_journal(&patch_dir, "data/foo.pak", "data/foo.chs", false);

    binary_patcher::apply::rollback_from_journal(base, &patch_dir).unwrap();

    assert!(
        !base.join("data/foo.chs").exists(),
        "patched target must be removed"
    );
    assert_eq!(
        std::fs::read(base.join("data/foo.pak")).unwrap(),
        b"original payload",
        "source must stay untouched"
    );
    assert!(
        !patch_dir
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists()
    );
}

#[test]
fn test_journal_recovery_renamed_with_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();

    write_file(base, "", "data/foo.pak", b"original payload");
    write_file(base, "", "data/foo.chs", b"patched payload");
    create_backup_file(&patch_dir, "data/foo.chs", b"original target");
    write_renamed_journal(&patch_dir, "data/foo.pak", "data/foo.chs", true);

    binary_patcher::apply::rollback_from_journal(base, &patch_dir).unwrap();

    assert_eq!(
        std::fs::read(base.join("data/foo.pak")).unwrap(),
        b"original payload",
        "source must stay untouched"
    );
    assert_eq!(
        std::fs::read(base.join("data/foo.chs")).unwrap(),
        b"original target"
    );
}

// ===========================================================================
// Cross-directory mapping: source directories must be protected
// ===========================================================================

#[test]
fn test_bundle_cross_directory_mapping_protects_source_dirs() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "data/foo.aos", b"source payload");
    write_file(base, "New", "chinese/foo.chs", b"target payload changed");
    write_mapping_file(base, &[("data/foo.aos", "chinese/foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert_eq!(manifest.added.len(), 0);
    assert_eq!(manifest.deleted.len(), 0);
    assert!(
        manifest.deleted_dirs.is_empty(),
        "data/ must not be deleted: {:?}",
        manifest.deleted_dirs
    );

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("data/foo.aos")).unwrap(),
        b"source payload",
        "cross-directory source must be kept"
    );
    assert_eq!(
        std::fs::read(game.join("chinese/foo.chs")).unwrap(),
        b"target payload changed"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert!(
        game.join("data/foo.aos").is_file(),
        "cross-directory source must survive rollback"
    );
    assert!(!game.join("chinese/foo.chs").exists());
}

#[test]
fn test_bundle_multi_level_source_dirs_protected() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "a/b/c/foo.aos", b"deep source payload");
    write_file(base, "New", "x/foo.chs", b"deep target payload changed");
    write_mapping_file(base, &[("a/b/c/foo.aos", "x/foo.chs")]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert!(
        manifest.deleted_dirs.is_empty(),
        "a/, a/b, a/b/c must all be protected: {:?}",
        manifest.deleted_dirs
    );

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("a/b/c/foo.aos")).unwrap(),
        b"deep source payload"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert!(game.join("a/b/c/foo.aos").is_file());
    assert!(!game.join("x/foo.chs").exists());
}

// ===========================================================================
// delete_source = true
// ===========================================================================

#[test]
fn test_delete_source_applies_and_rolls_back() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"old source payload");
    write_file(base, "New", "foo.chs", b"new target payload changed");
    write_mapping_file_with_delete(base, &[("foo.aos", "foo.chs", true)]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert!(manifest.changed[0].delete_source);
    assert!(manifest.changed[0].is_renamed());

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert!(
        !game.join("foo.aos").exists(),
        "delete_source=true must remove the source after verification"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"new target payload changed"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.aos")).unwrap(),
        b"old source payload",
        "rollback must restore the deleted source"
    );
    assert!(!game.join("foo.chs").exists());
}

#[test]
fn test_delete_source_with_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"old source payload");
    write_file(base, "New", "foo.chs", b"new target payload");
    write_mapping_file_with_delete(base, &[("foo.aos", "foo.chs", true)]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("foo.chs"), b"pre-existing target").unwrap();

    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert!(!game.join("foo.aos").exists());
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"new target payload"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.aos")).unwrap(),
        b"old source payload",
        "deleted source must be restored"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"pre-existing target",
        "original target must be restored"
    );
}

#[test]
fn test_delete_source_apply_failure_restores_source() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"old source payload");
    write_file(base, "New", "foo.chs", b"new target payload changed");
    write_mapping_file_with_delete(base, &[("foo.aos", "foo.chs", true)]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("Patch/foo.chs.patch"), b"CORRUPTED PATCH").unwrap();

    let result = binary_patcher::apply::apply_bundle(&game);
    assert!(result.is_err(), "apply must fail with a corrupted patch");
    assert_eq!(
        std::fs::read(game.join("foo.aos")).unwrap(),
        b"old source payload",
        "source must be restored after a failed apply"
    );
    assert!(!game.join("foo.chs").exists());
    assert!(
        !game
            .join("Patch")
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "journal must not remain after auto rollback"
    );
}

#[test]
fn test_delete_source_cross_directory() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "data/foo.aos", b"cross old source");
    write_file(base, "New", "localized/foo.chs", b"cross new target");
    write_mapping_file_with_delete(base, &[("data/foo.aos", "localized/foo.chs", true)]);

    build_bundle(base);

    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 1);
    assert!(manifest.changed[0].delete_source);
    assert!(
        manifest.deleted_dirs.iter().any(|dir| dir == "data"),
        "delete_source=true allows the now-empty source dir to be deleted: {:?}",
        manifest.deleted_dirs
    );

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert!(!game.join("data/foo.aos").exists());
    assert!(!game.join("data").exists(), "empty source dir removed");
    assert_eq!(
        std::fs::read(game.join("localized/foo.chs")).unwrap(),
        b"cross new target"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("data/foo.aos")).unwrap(),
        b"cross old source",
        "deleted cross-directory source must be restored"
    );
    assert!(!game.join("localized/foo.chs").exists());
}

#[test]
fn test_delete_source_rejected_when_source_present_in_new() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"same payload");
    write_file(base, "New", "foo.aos", b"same payload");
    write_file(base, "New", "foo.chs", b"target payload");
    write_mapping_file_with_delete(base, &[("foo.aos", "foo.chs", true)]);

    let err = try_build_bundle(base).unwrap_err();
    assert!(
        err.to_string()
            .contains("filemap.source-present-with-delete-source"),
        "unexpected error: {err}"
    );
    assert!(
        !base.join("Patch/manifest.json").exists(),
        "failed bundle must not leave a manifest"
    );
}

#[test]
fn test_journal_recovery_delete_source_restores_source() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();

    // Crash after target was written and source deleted:
    // target exists, source is gone, source backup holds the original.
    write_file(base, "", "data/foo.chs", b"patched payload");
    create_backup_file(&patch_dir, "data/foo.aos", b"original source");
    {
        let journal = serde_json::json!([{
            "type": "renamed_patched",
            "source": "data/foo.aos",
            "target": "data/foo.chs",
            "target_had_backup": false,
            "delete_source": true,
        }]);
        std::fs::write(
            patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME),
            serde_json::to_string(&journal).unwrap(),
        )
        .unwrap();
    }

    binary_patcher::apply::rollback_from_journal(base, &patch_dir).unwrap();

    assert!(!base.join("data/foo.chs").exists());
    assert_eq!(
        std::fs::read(base.join("data/foo.aos")).unwrap(),
        b"original source",
        "deleted source must be restored from its backup"
    );
}

// ===========================================================================
// Source modified in New must be rejected
// ===========================================================================

#[test]
fn test_bundle_rejects_source_modified_in_new() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"original source");
    write_file(base, "New", "foo.aos", b"modified source");
    write_file(base, "New", "foo.chs", b"target payload");
    write_mapping_file(base, &[("foo.aos", "foo.chs")]);

    let err = try_build_bundle(base).unwrap_err();
    assert!(
        err.to_string().contains("filemap.source-modified-in-new"),
        "unexpected error: {err}"
    );
    assert!(
        !base.join("Patch/manifest.json").exists(),
        "failed bundle must not leave a manifest"
    );
}

// ===========================================================================
// Double-apply guard
// ===========================================================================

#[test]
fn test_double_apply_rejected_until_rollback() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "foo.aos", b"old source payload");
    write_file(base, "New", "foo.chs", b"new target payload");
    write_mapping_file(base, &[("foo.aos", "foo.chs")]);
    build_bundle(base);

    let game = setup_game(base);
    std::fs::write(game.join("foo.chs"), b"pre-existing target").unwrap();
    binary_patcher::apply::apply_bundle(&game).unwrap();

    let source_content = std::fs::read(game.join("foo.aos")).unwrap();
    let target_content = std::fs::read(game.join("foo.chs")).unwrap();
    let backup_root = game.join("Patch/.backup_before_patch");
    let backups_before = count_files(&backup_root);
    assert!(backups_before >= 1, "first apply should create backups");

    let err = binary_patcher::apply::apply_bundle(&game).unwrap_err();
    assert!(
        err.to_string().contains("apply.already-applied"),
        "unexpected error: {err}"
    );
    assert_eq!(std::fs::read(game.join("foo.aos")).unwrap(), source_content);
    assert_eq!(std::fs::read(game.join("foo.chs")).unwrap(), target_content);
    assert_eq!(
        count_files(&backup_root),
        backups_before,
        "rejected apply must not create additional backups"
    );
    assert!(
        !game
            .join("Patch")
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "rejected apply must not leave a journal"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.aos")).unwrap(),
        b"old source payload"
    );
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"pre-existing target"
    );
    assert!(
        !game.join("Patch/.applied_patch.json").exists(),
        "rollback must clear the applied marker"
    );

    binary_patcher::apply::apply_bundle(&game).unwrap();
    assert_eq!(
        std::fs::read(game.join("foo.chs")).unwrap(),
        b"new target payload",
        "apply must be allowed again after rollback"
    );
}

// ===========================================================================
// Comprehensive workspace
// ===========================================================================

#[test]
fn test_comprehensive_mapping_workspace() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    write_file(base, "Old", "grp.aos", b"grp base payload");
    write_file(base, "Old", "scr.aos", b"scr base payload");
    write_file(base, "Old", "data/foo.aos", b"nested base payload");
    write_file(base, "Old", "config.ini", b"[main]\nlang=jp\n");

    write_file(base, "New", "grp.aos", b"grp base payload");
    write_file(base, "New", "grp.chs", b"grp localized payload");
    write_file(base, "New", "scr.aos", b"scr base payload");
    write_file(base, "New", "scr.chs", b"scr localized payload");
    write_file(
        base,
        "New",
        "localized/foo.chs",
        b"nested localized payload",
    );
    write_file(base, "New", "config.ini", b"[main]\nlang=cn\n");

    write_mapping_file(
        base,
        &[
            ("grp.aos", "grp.chs"),
            ("scr.aos", "scr.chs"),
            ("data/foo.aos", "localized/foo.chs"),
        ],
    );

    build_bundle(base);
    let manifest = Manifest::load(&base.join("Patch")).unwrap();
    assert_eq!(manifest.changed.len(), 4, "3 mapped + config.ini");
    assert_eq!(manifest.added.len(), 0);
    assert_eq!(manifest.deleted.len(), 0);
    assert!(
        manifest.deleted_dirs.is_empty(),
        "data/ must stay protected: {:?}",
        manifest.deleted_dirs
    );

    let game = setup_game(base);
    binary_patcher::apply::apply_bundle(&game).unwrap();

    // mapped sources are kept byte-identical.
    assert_eq!(
        std::fs::read(game.join("grp.aos")).unwrap(),
        b"grp base payload".as_slice()
    );
    assert_eq!(
        std::fs::read(game.join("scr.aos")).unwrap(),
        b"scr base payload".as_slice()
    );
    assert_eq!(
        std::fs::read(game.join("data/foo.aos")).unwrap(),
        b"nested base payload".as_slice(),
        "nested mapped source must be kept"
    );
    // mapped targets match New.
    assert_eq!(
        std::fs::read(game.join("grp.chs")).unwrap(),
        std::fs::read(base.join("New/grp.chs")).unwrap()
    );
    assert_eq!(
        std::fs::read(game.join("scr.chs")).unwrap(),
        std::fs::read(base.join("New/scr.chs")).unwrap()
    );
    assert_eq!(
        std::fs::read(game.join("localized/foo.chs")).unwrap(),
        std::fs::read(base.join("New/localized/foo.chs")).unwrap()
    );
    // normal same-path change.
    assert_eq!(
        std::fs::read(game.join("config.ini")).unwrap(),
        b"[main]\nlang=cn\n"
    );

    binary_patcher::rollback::rollback_bundle(&game).unwrap();

    for (rel, content) in [
        ("grp.aos", b"grp base payload".as_slice()),
        ("scr.aos", b"scr base payload".as_slice()),
        ("data/foo.aos", b"nested base payload".as_slice()),
        ("config.ini", b"[main]\nlang=jp\n".as_slice()),
    ] {
        assert_eq!(
            std::fs::read(game.join(rel)).unwrap(),
            content,
            "{rel} must be restored byte-identically"
        );
    }
    assert!(!game.join("grp.chs").exists());
    assert!(!game.join("scr.chs").exists());
    assert!(
        !game.join("localized/foo.chs").exists(),
        "generated target must be rolled back"
    );
    assert!(
        game.join("data/foo.aos").is_file(),
        "source dir must survive"
    );
}

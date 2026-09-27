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

fn build_bundle(base_dir: &Path) {
    binary_patcher::bundle::build_patch_bundle(base_dir, PatchMode::Memory, PatchFormat::Precise)
        .unwrap();
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

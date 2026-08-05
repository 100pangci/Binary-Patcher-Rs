mod common;

use common::{all_file_relpaths, build_workspace, copy_tree_files};

// ===========================================================================
// Full integration: bundle -> apply -> rollback
// ===========================================================================

#[test]
fn test_full_workflow() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();

    // Build workspace
    build_workspace(&base_dir);

    // Generate bundle (with compression)
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        true,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let patch_dir = base_dir.join("Patch");
    assert!(patch_dir.join("manifest.json").exists());
    assert!(patch_dir.join("README.txt").exists());

    let manifest = binary_patcher::manifest::Manifest::load(&patch_dir).unwrap();
    assert!(manifest.changed.len() >= 2);
    assert!(manifest.added.len() >= 2);
    assert!(manifest.deleted.len() >= 2);

    // Simulate end-user: copy Old/ -> game dir + Patch/
    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&patch_dir, &game_patch);

    // A stale empty journal must be cleaned up silently before applying
    std::fs::write(
        game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME),
        "[]",
    )
    .unwrap();

    // Apply bundle
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "journal should be removed after a successful apply"
    );

    // Verify applied state matches New/
    let new_files = all_file_relpaths(&base_dir.join("New"));
    let game_files: Vec<String> = all_file_relpaths(&game_dir)
        .into_iter()
        .filter(|f| !f.contains(".backup_before_patch"))
        .collect();

    assert_eq!(new_files, game_files);

    // Verify directories from Old that were deleted are gone
    assert!(
        !game_dir.join("deep").exists(),
        "directory deep/ should have been removed"
    );
    assert!(
        !game_dir.join("deep/nested").exists(),
        "directory deep/nested/ should have been removed"
    );

    // A stale journal must be cleaned up by rollback as well
    std::fs::write(
        game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME),
        r#"[{"type":"patched","path":"config.ini"}]"#,
    )
    .unwrap();

    // Rollback
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "journal should be removed after rollback"
    );

    // Verify rolled back state matches Old/
    let old_files = all_file_relpaths(&base_dir.join("Old"));
    let game_files_after: Vec<String> = all_file_relpaths(&game_dir)
        .into_iter()
        .filter(|f| !f.contains(".backup_before_patch"))
        .collect();

    assert_eq!(old_files, game_files_after);

    // Verify deleted directories are recreated after rollback
    assert!(
        game_dir.join("deep").is_dir(),
        "directory deep/ should be recreated after rollback"
    );
    assert!(
        game_dir.join("deep/nested").is_dir(),
        "directory deep/nested/ should be recreated after rollback"
    );
    assert!(
        game_dir.join("deep/nested/old_cache.tmp").exists(),
        "file deep/nested/old_cache.tmp should be restored after rollback"
    );
}

// ===========================================================================
// Apply failure auto rollback
// ===========================================================================

#[test]
fn test_apply_failure_auto_rollback() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();

    build_workspace(&base_dir);

    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        true,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let patch_dir = base_dir.join("Patch");

    // Simulate end-user: copy Old/ -> game dir + Patch/
    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&patch_dir, &game_patch);

    // Corrupt one patch file to trigger failure after some files are processed
    let mut corrupted = false;
    for entry in std::fs::read_dir(&game_patch).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "patch") {
            std::fs::write(&path, b"CORRUPTED PATCH DATA").unwrap();
            corrupted = true;
            break;
        }
    }
    assert!(corrupted, "should have corrupted at least one patch file");

    // Apply should fail
    let result = binary_patcher::apply::apply_bundle(&game_dir);
    assert!(
        result.is_err(),
        "apply_bundle should fail with corrupted patch"
    );

    // Verify rolled back state matches Old/
    let old_files = all_file_relpaths(&base_dir.join("Old"));
    let game_files_after: Vec<String> = all_file_relpaths(&game_dir)
        .into_iter()
        .filter(|f| !f.contains(".backup_before_patch"))
        .collect();

    assert_eq!(
        old_files, game_files_after,
        "files should be fully rolled back after failed apply"
    );

    // Verify content matches too (byte-level)
    let old_root = base_dir.join("Old");
    for rel in &old_files {
        let old_content = std::fs::read(old_root.join(rel)).unwrap();
        let game_content = std::fs::read(game_dir.join(rel)).unwrap();
        assert_eq!(
            old_content, game_content,
            "file {rel} content should match after rollback"
        );
    }
}

// ===========================================================================
// Patch format: fast vs precise
// ===========================================================================

fn make_test_data() -> (Vec<u8>, Vec<u8>) {
    let mut old_data = Vec::with_capacity(64 * 1024);
    let mut new_data = Vec::with_capacity(64 * 1024);

    for i in 0..4096u32 {
        let val = i.wrapping_mul(0x9E37_79B1).wrapping_add(0x85EB_CA77);
        old_data.extend_from_slice(&val.to_le_bytes());
        new_data.extend_from_slice(&val.to_le_bytes());
    }

    // Modify a chunk in the middle so both algorithms have real diff work
    let offset = 8192;
    for j in 0..512 {
        new_data[offset + j] = new_data[offset + j].wrapping_add(1);
    }

    // Insert a block at the end
    for k in 0..1024u32 {
        new_data.push((k % 256) as u8);
    }

    (old_data, new_data)
}

#[test]
fn test_patch_format_fast_and_precise_both_work() {
    let dir = tempfile::tempdir().unwrap();
    let (old_data, new_data) = make_test_data();

    let old_path = dir.path().join("old.bin");
    let new_path = dir.path().join("new.bin");
    std::fs::write(&old_path, &old_data).unwrap();
    std::fs::write(&new_path, &new_data).unwrap();

    let patch_fast = dir.path().join("patch_fast.hdiff");
    let patch_precise = dir.path().join("patch_precise.hdiff");

    // Create patch with fast format
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_fast, false, true).unwrap();

    // Create patch with precise format
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_precise, false, false)
        .unwrap();

    // Apply fast patch
    let out_fast = dir.path().join("out_fast.bin");
    binary_patcher::hdiffpatch::run_hpatchz(&old_path, &patch_fast, &out_fast).unwrap();
    assert_eq!(std::fs::read(&out_fast).unwrap(), new_data);

    // Apply precise patch
    let out_precise = dir.path().join("out_precise.bin");
    binary_patcher::hdiffpatch::run_hpatchz(&old_path, &patch_precise, &out_precise).unwrap();
    assert_eq!(std::fs::read(&out_precise).unwrap(), new_data);

    let fast_size = std::fs::metadata(&patch_fast).unwrap().len();
    let precise_size = std::fs::metadata(&patch_precise).unwrap().len();

    println!("fast patch: {fast_size} bytes, precise patch: {precise_size} bytes");

    // Fast patch should not be identical to precise (they use different algorithms)
    if fast_size == precise_size {
        let fast_bytes = std::fs::read(&patch_fast).unwrap();
        let precise_bytes = std::fs::read(&patch_precise).unwrap();
        assert_ne!(
            fast_bytes, precise_bytes,
            "Fast and precise patches should differ"
        );
    }
}

// ===========================================================================
// Stream mode workflow
// ===========================================================================

#[test]
fn test_stream_mode_workflow() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();

    // Use same small data to verify stream mode works end-to-end
    std::fs::create_dir_all(base_dir.join("Old")).unwrap();
    std::fs::create_dir_all(base_dir.join("New")).unwrap();
    std::fs::write(base_dir.join("Old/file.txt"), "hello world old").unwrap();
    std::fs::write(base_dir.join("New/file.txt"), "hello world new").unwrap();

    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        true,
        binary_patcher::cli::PatchMode::Stream,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();
    assert!(base_dir.join("Patch/manifest.json").exists());

    let game_dir = base_dir.join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(game_dir.join("file.txt"), "hello world old").unwrap();

    let game_patch = game_dir.join("Patch");
    std::fs::create_dir_all(&game_patch).unwrap();
    std::fs::copy(
        base_dir.join("Patch/manifest.json"),
        game_patch.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(
        base_dir.join("Patch/file.txt.patch"),
        game_patch.join("file.txt.patch"),
    )
    .unwrap();

    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("file.txt")).unwrap(),
        "hello world new"
    );

    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("file.txt")).unwrap(),
        "hello world old"
    );
}

// ===========================================================================
// Single file create + apply round trip
// ===========================================================================

#[test]
fn test_single_file_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.bin");
    let new_path = dir.path().join("new.bin");
    let patch_path = dir.path().join("patch.hdiff");
    let output_path = dir.path().join("output.bin");

    std::fs::write(&old_path, vec![0u8; 256]).unwrap();
    std::fs::write(&new_path, vec![0xFFu8; 256]).unwrap();

    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, true, false).unwrap();
    assert!(patch_path.exists());
    assert!(std::fs::metadata(&patch_path).unwrap().len() > 0);

    binary_patcher::hdiffpatch::run_hpatchz(&old_path, &patch_path, &output_path).unwrap();
    assert_eq!(std::fs::read(&output_path).unwrap(), vec![0xFFu8; 256]);
}

// ===========================================================================
// Single file apply via apply_patch CLI helper
// ===========================================================================

#[test]
fn test_apply_single_patch() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.txt");
    let new_path = dir.path().join("new.txt");
    let patch_path = dir.path().join("patch.hdiff");
    let output_path = dir.path().join("output.txt");

    std::fs::write(&old_path, "old content").unwrap();
    std::fs::write(&new_path, "new content with extra data!").unwrap();

    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, true, false).unwrap();
    binary_patcher::apply::apply_single_patch(
        &old_path.to_string_lossy(),
        &patch_path.to_string_lossy(),
        &output_path.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "new content with extra data!"
    );
}

// ===========================================================================
// No-compress mode
// ===========================================================================

#[test]
fn test_no_compress_workflow() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    std::fs::create_dir_all(base_dir.join("Old")).unwrap();
    std::fs::create_dir_all(base_dir.join("New")).unwrap();
    std::fs::write(
        base_dir.join("Old/a.txt"),
        "old data that is long enough to diff",
    )
    .unwrap();
    std::fs::write(
        base_dir.join("New/a.txt"),
        "new data that is long enough to diff",
    )
    .unwrap();

    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        false,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();
    assert!(base_dir.join("Patch/manifest.json").exists());

    let game_dir = base_dir.join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(
        game_dir.join("a.txt"),
        "old data that is long enough to diff",
    )
    .unwrap();
    let game_patch = game_dir.join("Patch");
    std::fs::create_dir_all(&game_patch).unwrap();
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("a.txt")).unwrap(),
        "new data that is long enough to diff"
    );
}

// ===========================================================================
// Rollback: empty directory cleanup
// ===========================================================================

#[test]
fn test_rollback_cleanup_empty_dirs() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();

    // Create Old/ with only one file (no extra subdirectory)
    std::fs::create_dir_all(base_dir.join("Old")).unwrap();
    std::fs::write(base_dir.join("Old/existing.txt"), "old content").unwrap();

    // Create New/ with the same file + a new file in a new subdirectory
    std::fs::create_dir_all(base_dir.join("New")).unwrap();
    std::fs::write(base_dir.join("New/existing.txt"), "new content").unwrap();
    std::fs::create_dir_all(base_dir.join("New/new_sub")).unwrap();
    std::fs::write(base_dir.join("New/new_sub/added.txt"), "added file").unwrap();

    // Bundle
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        true,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    // Simulate user directory
    let game_dir = base_dir.join("game");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(game_dir.join("existing.txt"), "old content").unwrap();
    // Copy Patch
    let game_patch = game_dir.join("Patch");
    std::fs::create_dir_all(&game_patch).unwrap();
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    // Apply
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    assert!(game_dir.join("new_sub/added.txt").exists());

    // Rollback
    // Feed "y" for backup deletion prompt and "y" for confirmation
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    // The added file should be gone
    assert!(!game_dir.join("new_sub/added.txt").exists());
    // The empty new_sub/ directory should be cleaned up
    assert!(
        !game_dir.join("new_sub").exists(),
        "empty directory new_sub/ should be removed after rollback"
    );
}

// ===========================================================================
// Crash recovery: persistent journal
// ===========================================================================

#[test]
fn test_journal_rollback_restores_all_entry_types() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    let backup_root = patch_dir.join(".backup_before_patch");
    let journal_path = patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME);

    // patched: target holds the patched content, backup holds the original
    std::fs::create_dir_all(base.join("sub")).unwrap();
    std::fs::write(base.join("sub/data.txt"), "PATCHED").unwrap();
    std::fs::create_dir_all(backup_root.join("sub")).unwrap();
    std::fs::write(
        backup_root.join("sub/data.txt.backup_before_patch"),
        "ORIGINAL",
    )
    .unwrap();

    // deleted: target was removed, backup exists
    std::fs::create_dir_all(&backup_root).unwrap();
    std::fs::write(
        backup_root.join("gone.txt.backup_before_patch"),
        "ORIGINAL GONE",
    )
    .unwrap();

    // added (had_backup=false): extra file left on disk, must be removed
    std::fs::write(base.join("extra.txt"), "EXTRA").unwrap();

    // deleted_dir: directory missing, must be recreated
    assert!(!base.join("removed_dir").exists());

    std::fs::write(
        &journal_path,
        r#"[
            {"type":"patched","path":"sub/data.txt"},
            {"type":"deleted","path":"gone.txt"},
            {"type":"added","path":"extra.txt","had_backup":false},
            {"type":"deleted_dir","path":"removed_dir"}
        ]"#,
    )
    .unwrap();

    binary_patcher::apply::rollback_from_journal(base, &patch_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(base.join("sub/data.txt")).unwrap(),
        "ORIGINAL"
    );
    assert_eq!(
        std::fs::read_to_string(base.join("gone.txt")).unwrap(),
        "ORIGINAL GONE"
    );
    assert!(!base.join("extra.txt").exists());
    assert!(base.join("removed_dir").is_dir());
    assert!(
        !journal_path.exists(),
        "journal should be removed after rollback"
    );
}

#[test]
fn test_journal_rejects_path_traversal() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    let journal_path = patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    std::fs::create_dir_all(&patch_dir).unwrap();

    std::fs::write(
        &journal_path,
        r#"[{"type":"patched","path":"../evil.txt"}]"#,
    )
    .unwrap();

    let result = binary_patcher::apply::rollback_from_journal(base, &patch_dir);
    assert!(result.is_err());
    assert!(
        journal_path.exists(),
        "journal must be preserved when recovery fails"
    );
}

#[test]
fn test_journal_malformed_json_errors() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    let journal_path = patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    std::fs::create_dir_all(&patch_dir).unwrap();

    std::fs::write(&journal_path, "{not valid json").unwrap();

    let result = binary_patcher::apply::rollback_from_journal(base, &patch_dir);
    assert!(result.is_err());
    assert!(journal_path.exists());
}

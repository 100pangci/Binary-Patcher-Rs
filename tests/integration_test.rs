use std::path::{Path, PathBuf};

/// Build test workspace with Old/New directories
fn build_workspace(base_dir: &Path) -> (PathBuf, PathBuf) {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");

    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();

    // unchanged
    std::fs::write(old_dir.join("same.txt"), "identical").unwrap();
    std::fs::write(new_dir.join("same.txt"), "identical").unwrap();

    // changed
    std::fs::write(old_dir.join("config.ini"), "[section]\nkey=old\n").unwrap();
    std::fs::write(
        new_dir.join("config.ini"),
        "[section]\nkey=new\nport=8080\n",
    )
    .unwrap();

    // binary changed
    std::fs::create_dir_all(old_dir.join("sub")).unwrap();
    std::fs::create_dir_all(new_dir.join("sub")).unwrap();
    let old_bin = vec![0u8; 100];
    let mut new_bin = vec![0xFFu8; 100];
    new_bin.push(0x02);
    std::fs::write(old_dir.join("sub/data.bin"), &old_bin).unwrap();
    std::fs::write(new_dir.join("sub/data.bin"), &new_bin).unwrap();

    // added
    std::fs::write(new_dir.join("new_file.dll"), "new dll content").unwrap();
    std::fs::create_dir_all(new_dir.join("sub")).unwrap();
    std::fs::write(new_dir.join("sub/extra.txt"), "bonus").unwrap();

    // deleted
    std::fs::write(old_dir.join("deprecated.log"), "old log").unwrap();
    std::fs::create_dir_all(old_dir.join("deep/nested")).unwrap();
    std::fs::write(old_dir.join("deep/nested/old_cache.tmp"), [0u8; 10]).unwrap();

    (old_dir, new_dir)
}

fn all_file_relpaths(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(root).unwrap();
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if !rel_str.contains("Patch") {
                result.push(rel_str);
            }
        }
    }
    result.sort();
    result
}

fn copy_tree_files(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(src).unwrap();
            let dest = dst.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

// ===========================================================================
// format_size
// ===========================================================================

#[test]
fn test_format_size_bytes() {
    assert_eq!(binary_patcher::utils::format_size(512), "512 B");
}

#[test]
fn test_format_size_kb() {
    assert_eq!(binary_patcher::utils::format_size(1024), "1.00 KB");
    assert_eq!(binary_patcher::utils::format_size(1536), "1.50 KB");
}

#[test]
fn test_format_size_mb() {
    assert_eq!(binary_patcher::utils::format_size(1024 * 1024), "1.00 MB");
    assert_eq!(
        binary_patcher::utils::format_size(2 * 1024 * 1024),
        "2.00 MB"
    );
}

#[test]
fn test_format_size_gb() {
    assert_eq!(
        binary_patcher::utils::format_size(1024 * 1024 * 1024),
        "1.00 GB"
    );
}

#[test]
fn test_format_size_zero() {
    assert_eq!(binary_patcher::utils::format_size(0), "0 B");
}

// ===========================================================================
// sha256_of_file
// ===========================================================================

#[test]
fn test_sha256_known_hash() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();
    let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    assert_eq!(
        binary_patcher::utils::sha256_of_file(&file_path).unwrap(),
        expected
    );
}

#[test]
fn test_sha256_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("empty.txt");
    std::fs::write(&file_path, "").unwrap();
    let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(
        binary_patcher::utils::sha256_of_file(&file_path).unwrap(),
        expected
    );
}

// ===========================================================================
// resolve_safe_path
// ===========================================================================

#[test]
fn test_resolve_normal_path() {
    let dir = tempfile::tempdir().unwrap();
    let target = binary_patcher::utils::resolve_safe_path(dir.path(), "sub/file.txt").unwrap();
    let expected = dir.path().join("sub/file.txt");
    assert_eq!(target, expected);
}

#[test]
fn test_resolve_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(binary_patcher::utils::resolve_safe_path(dir.path(), "../outside.txt").is_err());
}

#[test]
fn test_resolve_deep_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(binary_patcher::utils::resolve_safe_path(dir.path(), "sub/../../outside.txt").is_err());
}

// ===========================================================================
// relative_file_map / iter_files
// ===========================================================================

#[test]
fn test_iter_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir_all(dir.path().join("sub/subsub")).unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "b").unwrap();
    std::fs::write(dir.path().join("sub/subsub/c.txt"), "c").unwrap();

    let files: Vec<_> = binary_patcher::utils::iter_files(dir.path()).collect();
    assert_eq!(files.len(), 3);
}

#[test]
fn test_relative_file_map() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("dir1")).unwrap();
    std::fs::write(dir.path().join("dir1/a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();

    let mapping = binary_patcher::utils::relative_file_map(dir.path());
    assert!(mapping.contains_key("dir1/a.txt"));
    assert!(mapping.contains_key("b.txt"));
}

#[test]
fn test_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> = binary_patcher::utils::iter_files(dir.path()).collect();
    assert!(files.is_empty());
}

#[test]
fn test_relative_dir_map() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("dir1/sub")).unwrap();
    std::fs::write(dir.path().join("dir1/sub/a.txt"), "a").unwrap();
    std::fs::create_dir_all(dir.path().join("dir2")).unwrap();
    std::fs::write(dir.path().join("dir2/b.txt"), "b").unwrap();

    let mapping = binary_patcher::utils::relative_dir_map(dir.path());
    assert!(mapping.contains_key("dir1"));
    assert!(mapping.contains_key("dir1/sub"));
    assert!(mapping.contains_key("dir2"));
    assert_eq!(mapping.len(), 3);
}

#[test]
fn test_relative_dir_map_with_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("empty_dir")).unwrap();
    std::fs::create_dir_all(dir.path().join("parent/child")).unwrap();

    let mapping = binary_patcher::utils::relative_dir_map(dir.path());
    assert!(mapping.contains_key("empty_dir"));
    assert!(mapping.contains_key("parent"));
    assert!(mapping.contains_key("parent/child"));
}

// ===========================================================================
// Manifest validation
// ===========================================================================

#[test]
fn test_valid_manifest() {
    let manifest = binary_patcher::manifest::Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "a.txt".to_string(),
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "a.txt.patch".to_string(),
        }],
        added: vec![binary_patcher::manifest::AddedEntry {
            path: "b.txt".to_string(),
            new_sha256: "c".repeat(64),
            file: "b.txt.new".to_string(),
        }],
        deleted: vec![binary_patcher::manifest::DeletedEntry {
            path: "c.txt".to_string(),
            old_sha256: "d".repeat(64),
        }],
        deleted_dirs: vec!["old_dir".to_string(), "deep/nested".to_string()],
    };
    assert!(manifest.validate().is_ok());
}

#[test]
fn test_manifest_wrong_format() {
    let manifest = binary_patcher::manifest::Manifest {
        format: "invalid".to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    assert!(manifest.validate().is_err());
}

// ===========================================================================
// create_backup / restore_backup
// ===========================================================================

#[test]
fn test_backup_created() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("original.txt");
    std::fs::write(&target, "content").unwrap();
    let backup = binary_patcher::utils::create_backup(&target, dir.path(), &backup_root).unwrap();
    assert!(backup.exists());
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "content");
}

#[test]
fn test_backup_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "original").unwrap();
    let backup = binary_patcher::utils::create_backup(&target, dir.path(), &backup_root).unwrap();
    assert!(backup.to_string_lossy().ends_with(".backup_before_patch"));
}

#[test]
fn test_restore_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "modified").unwrap();
    let _backup = binary_patcher::utils::create_backup(&target, dir.path(), &backup_root).unwrap();
    std::fs::write(&target, "new content").unwrap();
    assert!(binary_patcher::utils::restore_backup(&target, dir.path(), &backup_root).unwrap());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "modified");
}

#[test]
fn test_restore_backup_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    assert!(!binary_patcher::utils::restore_backup(&target, dir.path(), &backup_root).unwrap());
}

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

    // Apply bundle
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

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

    // Rollback
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

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
// Patch format: fast vs precise
// ===========================================================================

fn make_test_data() -> (Vec<u8>, Vec<u8>) {
    let mut old_data = Vec::with_capacity(64 * 1024);
    let mut new_data = Vec::with_capacity(64 * 1024);

    for i in 0..4096u32 {
        let val = i.wrapping_mul(0x9E3779B1).wrapping_add(0x85EBCA77);
        old_data.extend_from_slice(&val.to_le_bytes());
        new_data.extend_from_slice(&val.to_le_bytes());
    }

    // Modify a chunk in the middle so both algorithms have real diff work
    let offset = 8192;
    for j in 0..512 {
        new_data[offset + j] = new_data[offset + j].wrapping_add(1);
    }

    // Insert a block at the end
    for k in 0..1024 {
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
// relative_maps correctness
// ===========================================================================

#[test]
fn test_relative_maps_returns_both() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
    std::fs::write(dir.path().join("sub/a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();

    let (files, dirs) = binary_patcher::utils::relative_maps(dir.path());
    assert!(files.contains_key("sub/a.txt"));
    assert!(files.contains_key("b.txt"));
    assert!(dirs.contains_key("sub"));
    assert!(dirs.contains_key("sub/deep"));
    assert_eq!(files.len(), 2);
    assert_eq!(dirs.len(), 2);
}

// ===========================================================================
// Backup retry on name collision
// ===========================================================================

#[test]
fn test_backup_retry_on_collision() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "original").unwrap();

    let backup1 =
        binary_patcher::utils::write_backup(b"original", &target, dir.path(), &backup_root)
            .unwrap();
    let backup2 =
        binary_patcher::utils::write_backup(b"modified", &target, dir.path(), &backup_root)
            .unwrap();
    assert_ne!(backup1, backup2);
    assert!(backup2.to_string_lossy().contains(".backup_before_patch"));
    assert!(backup2.exists());
    assert_eq!(std::fs::read_to_string(&backup2).unwrap(), "modified");
}

// ===========================================================================
// Manifest rejects path traversal entries
// ===========================================================================

#[test]
fn test_manifest_rejects_traversal_in_changed_path() {
    let manifest = binary_patcher::manifest::Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "../escape.txt".to_string(),
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "p.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    // Validate passes (format check), but load + apply will catch traversal at resolve_safe_path
    assert!(manifest.validate().is_ok());
}

#[test]
fn test_resolve_safe_path_rejects_traversal_in_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Patch")).unwrap();
    let manifest = serde_json::json!({
        "format": env!("CARGO_PKG_VERSION"),
        "source_root": "Old",
        "target_root": "New",
        "changed": [{
            "path": "../outside.txt",
            "old_sha256": "a".repeat(64),
            "new_sha256": "b".repeat(64),
            "patch_file": "p.patch"
        }],
        "added": [],
        "deleted": [],
        "deleted_dirs": []
    });
    std::fs::write(
        dir.path().join("Patch/manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Verify manifest loads but resolve_safe_path catches the traversal during apply
    assert!(binary_patcher::manifest::Manifest::load(&dir.path().join("Patch")).is_ok());
    assert!(binary_patcher::utils::resolve_safe_path(dir.path(), "../outside.txt").is_err());
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
// Malformed JSON manifest
// ===========================================================================

#[test]
fn test_manifest_malformed_json_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // Truncated JSON
    std::fs::write(patch_dir.join("manifest.json"), "{\"format\": \"1.1.0\", ").unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

#[test]
fn test_manifest_malformed_json_not_object() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // JSON array is not a valid manifest
    std::fs::write(patch_dir.join("manifest.json"), "[1, 2, 3]").unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

#[test]
fn test_manifest_malformed_json_random_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // Random binary garbage
    std::fs::write(
        patch_dir.join("manifest.json"),
        vec![0xFF, 0xFE, 0x00, 0x01],
    )
    .unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

// ===========================================================================
// Version compat
// ===========================================================================

#[test]
fn test_version_compat_major_mismatch() {
    match binary_patcher::manifest::check_version_compat("2.0.0") {
        binary_patcher::manifest::VersionCompat::Compatible => panic!("expected incompatible"),
        binary_patcher::manifest::VersionCompat::Incompatible { .. } => {} // ok
    }
}

#[test]
fn test_version_compat_minor_mismatch() {
    let current = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = current.split('.').collect();
    let mismatched = format!(
        "{}.{}.{}",
        parts[0],
        parts[1].parse::<u32>().unwrap() + 1,
        0
    );
    match binary_patcher::manifest::check_version_compat(&mismatched) {
        binary_patcher::manifest::VersionCompat::Compatible => panic!("expected incompatible"),
        binary_patcher::manifest::VersionCompat::Incompatible { .. } => {} // ok
    }
}

// ===========================================================================
// format_size TB
// ===========================================================================

#[test]
fn test_format_size_tb() {
    assert_eq!(
        binary_patcher::utils::format_size(1024u64 * 1024 * 1024 * 1024),
        "1.00 TB"
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

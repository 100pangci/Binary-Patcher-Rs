use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Create mock hdiffz/hpatchz executables for testing
fn setup_mock_tools(test_dir: &Path) {
    let bin_dir = test_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    #[cfg(windows)]
    {
        // Mock hdiffz: copies third arg (new) to fourth arg (patch)
        let hdiffz = "@echo off\r\ncopy /y \"%~3\" \"%~4\" >nul\r\nexit /b 0\r\n";
        std::fs::write(bin_dir.join("hdiffz.bat"), hdiffz).unwrap();
        // Mock hpatchz: copies third arg (patch) to fourth arg (output)
        let hpatchz = "@echo off\r\ncopy /y \"%~3\" \"%~4\" >nul\r\nexit /b 0\r\n";
        std::fs::write(bin_dir.join("hpatchz.bat"), hpatchz).unwrap();
    }

    #[cfg(unix)]
    {
        let hdiffz = "#!/bin/sh\ncp -- \"$3\" \"$4\"\n";
        let hpatchz = "#!/bin/sh\ncp -- \"$3\" \"$4\"\n";
        let hdiffz_path = bin_dir.join("hdiffz");
        let hpatchz_path = bin_dir.join("hpatchz");
        std::fs::write(&hdiffz_path, hdiffz).unwrap();
        std::fs::write(&hpatchz_path, hpatchz).unwrap();
        std::fs::set_permissions(&hdiffz_path, PermissionsExt::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&hpatchz_path, PermissionsExt::from_mode(0o755)).unwrap();
    }
}

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
    std::fs::write(new_dir.join("config.ini"), "[section]\nkey=new\nport=8080\n").unwrap();

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
    std::fs::write(old_dir.join("deep/nested/old_cache.tmp"), &[0u8; 10]).unwrap();

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
    assert_eq!(binary_patcher::utils::format_size(2 * 1024 * 1024), "2.00 MB");
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
    assert!(
        binary_patcher::utils::resolve_safe_path(dir.path(), "../outside.txt").is_err()
    );
}

#[test]
fn test_resolve_deep_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        binary_patcher::utils::resolve_safe_path(dir.path(), "sub/../../outside.txt").is_err()
    );
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
        format: 1,
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
        format: 2,
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
    setup_mock_tools(&base_dir);

    // Change working directory to base_dir for tool finding
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base_dir).unwrap();

    // Build workspace
    build_workspace(&base_dir);

    // Generate bundle (with compression)
    binary_patcher::bundle::build_patch_bundle(&base_dir, true, binary_patcher::cli::PatchMode::Memory, binary_patcher::cli::PatchFormat::Precise).unwrap();

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
    assert!(!game_dir.join("deep").exists(), "directory deep/ should have been removed");
    assert!(!game_dir.join("deep/nested").exists(), "directory deep/nested/ should have been removed");

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
    assert!(game_dir.join("deep").is_dir(), "directory deep/ should be recreated after rollback");
    assert!(game_dir.join("deep/nested").is_dir(), "directory deep/nested/ should be recreated after rollback");
    assert!(game_dir.join("deep/nested/old_cache.tmp").exists(), "file deep/nested/old_cache.tmp should be restored after rollback");

    // Restore working directory
    std::env::set_current_dir(&orig_dir).unwrap();
}

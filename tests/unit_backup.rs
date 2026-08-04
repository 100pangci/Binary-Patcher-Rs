// ===========================================================================
// create_backup / write_backup / restore_backup
// ===========================================================================

#[test]
fn test_backup_created() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("original.txt");
    std::fs::write(&target, "content").unwrap();
    let backup = binary_patcher::backup::create_backup(&target, dir.path(), &backup_root).unwrap();
    assert!(backup.exists());
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "content");
}

#[test]
fn test_backup_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "original").unwrap();
    let backup = binary_patcher::backup::create_backup(&target, dir.path(), &backup_root).unwrap();
    assert!(backup.to_string_lossy().ends_with(".backup_before_patch"));
}

#[test]
fn test_restore_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "modified").unwrap();
    let _backup = binary_patcher::backup::create_backup(&target, dir.path(), &backup_root).unwrap();
    std::fs::write(&target, "new content").unwrap();
    assert!(binary_patcher::backup::restore_backup(&target, dir.path(), &backup_root).unwrap());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "modified");
}

#[test]
fn test_restore_backup_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    assert!(!binary_patcher::backup::restore_backup(&target, dir.path(), &backup_root).unwrap());
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
        binary_patcher::backup::write_backup(b"original", &target, dir.path(), &backup_root)
            .unwrap();
    let backup2 =
        binary_patcher::backup::write_backup(b"modified", &target, dir.path(), &backup_root)
            .unwrap();
    assert_ne!(backup1, backup2);
    assert!(backup2.to_string_lossy().contains(".backup_before_patch"));
    assert!(backup2.exists());
    assert_eq!(std::fs::read_to_string(&backup2).unwrap(), "modified");
}

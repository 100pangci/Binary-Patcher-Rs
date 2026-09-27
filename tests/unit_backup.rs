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

#[test]
fn test_restore_backup_copy_keeps_backup_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("file.txt");
    std::fs::write(&target, "original").unwrap();
    let backup = binary_patcher::backup::create_backup(&target, dir.path(), &backup_root).unwrap();
    std::fs::write(&target, "patched").unwrap();

    assert!(
        binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root).unwrap()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
    assert!(
        backup.exists(),
        "copy restore must keep the backup so rollback can be retried"
    );

    // 重复恢复幂等：可重入 rollback 依赖该性质。
    assert!(
        binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root).unwrap()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
}

#[test]
fn test_restore_backup_copy_failure_keeps_target_and_cleans_temp() {
    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    std::fs::create_dir_all(&backup_root).unwrap();
    let backup_content = "backup content";
    std::fs::write(
        backup_root.join("file.txt.backup_before_patch"),
        backup_content,
    )
    .unwrap();

    // 目标被用户换成了非空目录：最终替换必然失败。
    let target = dir.path().join("file.txt");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("user.txt"), "user data").unwrap();

    let result = binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root);
    assert!(result.is_err(), "replacing a directory must fail");

    // 复制/替换失败时原 target 必须保持不变，临时文件清理，备份保留用于重试。
    assert!(target.is_dir(), "original target must stay untouched");
    assert_eq!(
        std::fs::read_to_string(target.join("user.txt")).unwrap(),
        "user data"
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".restore-"))
        .map(|entry| entry.file_name())
        .collect();
    assert!(
        leftovers.is_empty(),
        "restore temp files must be cleaned: {leftovers:?}"
    );
    assert!(backup_root.join("file.txt.backup_before_patch").exists());

    // 处理掉冲突后重试必须成功。
    std::fs::remove_dir_all(&target).unwrap();
    assert!(
        binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root).unwrap()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), backup_content);
}

#[cfg(windows)]
#[test]
fn test_restore_backup_copy_locked_target_fails_safely() {
    use std::os::windows::fs::OpenOptionsExt;

    // FILE_SHARE_READ：仅允许读共享（不含 FILE_SHARE_DELETE），替换 target 必失败。
    const FILE_SHARE_READ: u32 = 0x1;

    let dir = tempfile::tempdir().unwrap();
    let backup_root = dir.path().join("backups");
    let target = dir.path().join("locked.txt");
    std::fs::write(&target, "original").unwrap();
    let backup = binary_patcher::backup::create_backup(&target, dir.path(), &backup_root).unwrap();
    std::fs::write(&target, "patched").unwrap();

    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&target)
        .unwrap();

    let result = binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root);
    assert!(result.is_err(), "locked target must fail restore");

    drop(lock);

    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "patched",
        "failed restore must leave the original target untouched"
    );
    assert!(backup.exists(), "backup must be kept for retry");

    // 解除锁定后重试必须成功。
    assert!(
        binary_patcher::backup::restore_backup_copy(&target, dir.path(), &backup_root).unwrap()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
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

#[cfg(unix)]
#[test]
fn test_backup_root_symlink_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let backup_root = root.path().join("backups");
    let target = root.path().join("file.txt");
    std::fs::write(&target, "original").unwrap();
    std::os::unix::fs::symlink(outside.path(), &backup_root).unwrap();

    assert!(
        binary_patcher::backup::write_backup(b"original", &target, root.path(), &backup_root)
            .is_err()
    );
    assert!(
        binary_patcher::fs::iter_files(outside.path())
            .next()
            .is_none()
    );
}

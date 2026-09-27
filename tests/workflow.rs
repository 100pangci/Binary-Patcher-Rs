mod common;

use common::{all_file_relpaths, build_workspace, copy_tree_files};
use std::io::Write;
use std::process::{Command, Stdio};

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
    assert!(
        game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "successful apply should write an applied-patch marker"
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

    // Rollback（成功 apply 后无 journal，凭有效 marker 走完整 manifest 回滚）
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "journal should be removed after rollback"
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "rollback should remove the applied-patch marker"
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
// Rollback guard for ordinary (non-mapping) patches
// ===========================================================================

#[test]
fn test_rollback_plain_patch_without_apply_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    // 未 Apply 就 rollback：必须拒绝，且不修改任何文件（changed/added/deleted 均覆盖）。
    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("rollback.not-applied"),
        "unexpected error: {err}"
    );

    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(
        !game_dir.join("new_file.dll").exists(),
        "added file must not appear on a rejected rollback"
    );
    assert!(
        game_dir.join("deprecated.log").exists(),
        "deleted file must not be touched on a rejected rollback"
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists()
    );
    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists()
    );
}

#[test]
fn test_rollback_plain_patch_with_journal_uses_crash_recovery() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    assert!(game_patch.join(".applied_patch.json").exists());

    // 模拟 apply 已写 marker、journal 残留：回滚必须走 journal 精准恢复，
    // 而不是按完整 manifest 回滚。
    std::fs::write(
        game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME),
        r#"[{"type":"patched","path":"config.ini"}]"#,
    )
    .unwrap();

    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n",
        "journal recovery must restore the patched file"
    );
    assert!(
        game_dir.join("new_file.dll").exists(),
        "manifest rollback must not run when a journal is present"
    );
    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists(),
        "journal must be removed after recovery"
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "residual marker must be cleared after journal recovery"
    );
}

// ===========================================================================
// Rollback preflight: user-modified apply results must be rejected untouched
// ===========================================================================

fn build_applied_workspace(base_dir: &std::path::Path) -> std::path::PathBuf {
    build_workspace(base_dir);
    binary_patcher::bundle::build_patch_bundle(
        base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    copy_tree_files(&base_dir.join("Patch"), &game_dir.join("Patch"));
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    game_dir
}

#[test]
fn test_rollback_rejects_user_modified_added_file_without_touching_files() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");

    // 用户修改 apply 生成的新增文件：rollback 必须拒绝，且不修改任何文件。
    std::fs::write(game_dir.join("new_file.dll"), "user modified added").unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("rollback.preflight-added"),
        "unexpected error: {err}"
    );

    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll")).unwrap(),
        "user modified added",
        "user-modified added file must stay untouched"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=new\nport=8080\n",
        "rejected rollback must not restore changed files"
    );
    assert!(
        !game_dir.join("deprecated.log").exists(),
        "rejected rollback must not restore deleted files"
    );
    assert!(
        game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "rejected rollback must keep the applied marker"
    );
}

#[test]
fn test_rollback_rejects_user_modified_changed_file_without_touching_files() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");

    // 用户修改 apply 后的变更文件：rollback 必须拒绝，且不修改任何文件。
    std::fs::write(game_dir.join("config.ini"), "user tampered config").unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("rollback.preflight-changed"),
        "unexpected error: {err}"
    );

    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "user tampered config",
        "user-modified changed file must stay untouched"
    );
    assert!(
        game_dir.join("new_file.dll").exists(),
        "rejected rollback must not remove added files"
    );
    assert!(
        game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "rejected rollback must keep the applied marker"
    );
}

#[test]
fn test_rollback_rejects_added_target_replaced_by_non_empty_directory() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);

    // added 文件被用户替换为非空目录：绝不能递归删除。
    std::fs::remove_file(game_dir.join("new_file.dll")).unwrap();
    std::fs::create_dir(game_dir.join("new_file.dll")).unwrap();
    std::fs::write(game_dir.join("new_file.dll/user_data.txt"), "keep me").unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("rollback.preflight-added"),
        "unexpected error: {err}"
    );

    assert!(
        game_dir.join("new_file.dll").is_dir(),
        "directory must not be deleted"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll/user_data.txt")).unwrap(),
        "keep me",
        "user data inside the directory must survive"
    );
}

#[test]
fn test_rollback_restores_added_target_overwritten_user_file() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    // 用户在 added 目标位置已有自己的文件：apply 必须先备份再覆盖。
    std::fs::write(game_dir.join("new_file.dll"), "user original dll").unwrap();

    binary_patcher::apply::apply_bundle(&game_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll")).unwrap(),
        "new dll content"
    );

    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll")).unwrap(),
        "user original dll",
        "rollback must restore the pre-existing file at an added target"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(
        !game_dir.join("sub/extra.txt").exists(),
        "added files without a backup must still be removed"
    );
    assert!(
        game_dir.join("deprecated.log").exists(),
        "deleted files must be restored"
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "successful rollback must clear the applied marker"
    );
}

#[test]
fn test_rollback_resumes_after_partial_restore() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    copy_tree_files(&base_dir.join("Patch"), &game_dir.join("Patch"));
    std::fs::write(game_dir.join("new_file.dll"), "user original dll").unwrap();
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

    // 模拟上一次 rollback 中途失败后留下的状态：
    // changed 已恢复为旧内容，added（覆盖用户文件）已从备份恢复，备份保留。
    std::fs::write(game_dir.join("config.ini"), "[section]\nkey=old\n").unwrap();
    let added_target = game_dir.join("new_file.dll");
    let backup_root = game_dir.join("Patch/.backup_before_patch");
    let backup = binary_patcher::backup::find_backup(&added_target, &game_dir, &backup_root)
        .unwrap()
        .expect("added target must have a backup");
    std::fs::copy(&backup, &added_target).unwrap();

    // 再次 rollback 必须能通过 preflight 并继续完成，而不是被卡死。
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll")).unwrap(),
        "user original dll"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(!game_dir.join("sub/extra.txt").exists());
    assert!(game_dir.join("deprecated.log").exists());
    assert!(game_dir.join("deep/nested/old_cache.tmp").exists());
    assert!(
        !game_dir
            .join("Patch")
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists()
    );
}

#[test]
fn test_rollback_restores_missing_added_target_from_backup() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    copy_tree_files(&base_dir.join("Patch"), &game_dir.join("Patch"));
    std::fs::write(game_dir.join("new_file.dll"), "user original dll").unwrap();
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

    // 模拟上一次 rollback 删除了目标、但尚未从备份恢复就中断。
    std::fs::remove_file(game_dir.join("new_file.dll")).unwrap();

    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert_eq!(
        std::fs::read_to_string(game_dir.join("new_file.dll")).unwrap(),
        "user original dll",
        "missing added target must be restored from its backup"
    );
}

#[cfg(windows)]
#[test]
fn test_rollback_can_retry_after_midway_failure() {
    use std::os::windows::fs::OpenOptionsExt;

    // FILE_SHARE_READ：仅允许读共享（不含 FILE_SHARE_DELETE）。
    const FILE_SHARE_READ: u32 = 0x1;

    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");
    let marker_path = game_patch.join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME);

    // 仅允许读共享的句柄：目标文件无法删除，模拟 rollback 在第一个
    // changed 条目上中途失败。preflight 只读不受影响。
    let blocked = game_dir.join("config.ini");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&blocked)
        .unwrap();

    let result = binary_patcher::rollback::rollback_bundle(&game_dir);
    assert!(result.is_err(), "locked target must fail rollback");
    assert!(
        marker_path.exists(),
        "failed rollback must keep the applied marker"
    );

    drop(lock);

    // 再次 rollback 必须能继续完成。
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert!(!marker_path.exists());
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(game_dir.join("deprecated.log").exists());
    assert!(game_dir.join("deep/nested/old_cache.tmp").exists());
    assert!(!game_dir.join("new_file.dll").exists());
}

// ===========================================================================
// Deleted files: missing / hash mismatch must abort and roll back
// ===========================================================================

#[test]
fn test_apply_deleted_missing_file_aborts_and_rolls_back() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    // 删除 manifest 声明的文件，apply 必须报错而不是静默跳过。
    std::fs::remove_file(game_dir.join("deprecated.log")).unwrap();

    let err = binary_patcher::apply::apply_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("apply.deleted-missing"),
        "unexpected error: {err}"
    );

    // 事务回滚：changed 恢复、added 清除。
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(!game_dir.join("new_file.dll").exists());
    assert!(
        !game_patch
            .join(binary_patcher::apply::JOURNAL_FILE_NAME)
            .exists()
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists()
    );
}

#[test]
fn test_apply_deleted_file_sha_mismatch_aborts_and_rolls_back() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    build_workspace(&base_dir);
    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    // 用户修改了 manifest 声明删除的文件：必须以 SHA256 不匹配报错。
    std::fs::write(game_dir.join("deprecated.log"), "tampered by user").unwrap();

    let err = binary_patcher::apply::apply_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("apply.deleted-sha-mismatch"),
        "unexpected error: {err}"
    );

    assert_eq!(
        std::fs::read_to_string(game_dir.join("deprecated.log")).unwrap(),
        "tampered by user",
        "mismatching file must be left untouched"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(!game_dir.join("new_file.dll").exists());
    assert!(
        game_dir.join("deep/nested/old_cache.tmp").exists(),
        "previously deleted files must be restored by the transaction rollback"
    );
}

// ===========================================================================
// Deleted directories: only empty directories are removed
// ===========================================================================

#[test]
fn test_apply_keeps_non_empty_deleted_dir_with_user_file() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();

    std::fs::create_dir_all(base_dir.join("Old/removed")).unwrap();
    std::fs::create_dir_all(base_dir.join("New")).unwrap();
    std::fs::write(base_dir.join("Old/removed/declared.tmp"), b"declared").unwrap();
    std::fs::write(base_dir.join("New/keep.txt"), b"kept").unwrap();

    binary_patcher::bundle::build_patch_bundle(
        &base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();
    let manifest = binary_patcher::manifest::Manifest::load(&base_dir.join("Patch")).unwrap();
    assert!(
        manifest
            .deleted
            .iter()
            .any(|e| e.path == "removed/declared.tmp")
    );
    assert!(manifest.deleted_dirs.iter().any(|d| d == "removed"));

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    copy_tree_files(&base_dir.join("Patch"), &game_dir.join("Patch"));
    // manifest 未声明的用户文件：目录非空，必须保留。
    std::fs::write(game_dir.join("removed/user_note.txt"), b"user data").unwrap();

    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

    assert!(
        !game_dir.join("removed/declared.tmp").exists(),
        "declared deleted file must be removed"
    );
    assert!(game_dir.join("removed").is_dir(), "directory must be kept");
    assert_eq!(
        std::fs::read(game_dir.join("removed/user_note.txt")).unwrap(),
        b"user data",
        "undeclared user file must never be deleted"
    );

    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();
    assert_eq!(
        std::fs::read(game_dir.join("removed/declared.tmp")).unwrap(),
        b"declared",
        "rollback must restore the declared deleted file"
    );
    assert_eq!(
        std::fs::read(game_dir.join("removed/user_note.txt")).unwrap(),
        b"user data"
    );
}

// ===========================================================================
// Streaming apply helper (large-file path)
// ===========================================================================

#[test]
fn test_apply_patch_stream_writes_output_file() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.bin");
    let new_path = dir.path().join("new.bin");
    let patch_path = dir.path().join("patch.hdiff");
    let output_path = dir.path().join("output.bin");

    let mut old_data = vec![0u8; 256 * 1024];
    for (i, byte) in old_data.iter_mut().enumerate() {
        *byte = (i % 251) as u8;
    }
    let mut new_data = old_data.clone();
    new_data.extend_from_slice(b"streamed tail");
    new_data[1024..2048].fill(0xAB);
    std::fs::write(&old_path, &old_data).unwrap();
    std::fs::write(&new_path, &new_data).unwrap();

    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, true).unwrap();
    let patch_data = std::fs::read(&patch_path).unwrap();
    binary_patcher::hdiffpatch::apply_patch_stream(&old_path, &patch_data, &output_path, 2)
        .unwrap();
    assert_eq!(std::fs::read(&output_path).unwrap(), new_data);

    // 空输出同样走流式路径。
    let empty_new = dir.path().join("empty.bin");
    std::fs::write(&empty_new, []).unwrap();
    let empty_patch = dir.path().join("empty.hdiff");
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &empty_new, &empty_patch, true).unwrap();
    let empty_patch_data = std::fs::read(&empty_patch).unwrap();
    binary_patcher::hdiffpatch::apply_patch_stream(&old_path, &empty_patch_data, &output_path, 2)
        .unwrap();
    assert_eq!(std::fs::read(&output_path).unwrap(), b"");
}

#[test]
fn test_apply_patch_auto_stream_fallback_does_not_read_output_back() {
    let dir = tempfile::tempdir().unwrap();
    let old_path = dir.path().join("old.bin");
    let new_path = dir.path().join("new.bin");
    let patch_path = dir.path().join("patch.hdiff");
    let output_path = dir.path().join("output.bin");

    let mut old_data = vec![0u8; 128 * 1024];
    for (i, byte) in old_data.iter_mut().enumerate() {
        *byte = (i % 251) as u8;
    }
    let mut new_data = old_data.clone();
    new_data.extend_from_slice(b"fallback tail");
    new_data[2048..4096].fill(0x5A);
    std::fs::write(&old_path, &old_data).unwrap();
    std::fs::write(&new_path, &new_data).unwrap();
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, true).unwrap();
    let patch_data = std::fs::read(&patch_path).unwrap();

    // 阈值为 0 强制走流式回退：返回 Streamed，输出绝不读回内存。
    let result = binary_patcher::hdiffpatch::apply_patch_auto_with_limit(
        old_data.clone(),
        &old_path,
        patch_data.clone(),
        &output_path,
        2,
        0,
    )
    .unwrap();
    assert!(matches!(
        result,
        binary_patcher::hdiffpatch::AppliedOutput::Streamed
    ));
    assert_eq!(std::fs::read(&output_path).unwrap(), new_data);

    // 正常小文件快速路径仍返回内存数据。
    let output_mem = dir.path().join("output_mem.bin");
    let result = binary_patcher::hdiffpatch::apply_patch_auto(
        old_data,
        &old_path,
        patch_data,
        &output_mem,
        2,
    )
    .unwrap();
    assert!(matches!(
        result,
        binary_patcher::hdiffpatch::AppliedOutput::InMemory(_)
    ));
    assert_eq!(std::fs::read(&output_mem).unwrap(), new_data);
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

#[cfg(unix)]
#[test]
fn test_apply_rejects_symlink_target_without_touching_outside() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base_dir = root.path();
    build_workspace(base_dir);
    binary_patcher::bundle::build_patch_bundle(
        base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    let outside_config = outside.path().join("config.ini");
    std::fs::write(&outside_config, "must remain untouched").unwrap();
    std::fs::remove_file(game_dir.join("config.ini")).unwrap();
    std::os::unix::fs::symlink(&outside_config, game_dir.join("config.ini")).unwrap();

    let result = binary_patcher::apply::apply_bundle(&game_dir);
    assert!(result.is_err(), "apply must reject a symlink target");
    assert_eq!(
        std::fs::read_to_string(&outside_config).unwrap(),
        "must remain untouched"
    );
    assert!(
        std::fs::symlink_metadata(game_dir.join("config.ini"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn test_rollback_rejects_symlink_target_without_touching_outside() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base_dir = root.path();
    build_workspace(base_dir);
    binary_patcher::bundle::build_patch_bundle(
        base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);
    binary_patcher::apply::apply_bundle(&game_dir).unwrap();

    let outside_config = outside.path().join("config.ini");
    std::fs::write(&outside_config, "must remain untouched").unwrap();
    std::fs::remove_file(game_dir.join("config.ini")).unwrap();
    std::os::unix::fs::symlink(&outside_config, game_dir.join("config.ini")).unwrap();

    let result = binary_patcher::rollback::rollback_bundle(&game_dir);
    assert!(result.is_err(), "rollback must reject a symlink target");
    assert_eq!(
        std::fs::read_to_string(&outside_config).unwrap(),
        "must remain untouched"
    );
    assert!(
        game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists(),
        "failed rollback must not clear the applied marker"
    );
}

#[cfg(unix)]
#[test]
fn test_apply_rejects_symlink_patch_resource() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base_dir = root.path();
    build_workspace(base_dir);
    binary_patcher::bundle::build_patch_bundle(
        base_dir,
        binary_patcher::cli::PatchMode::Memory,
        binary_patcher::cli::PatchFormat::Precise,
    )
    .unwrap();

    let game_dir = base_dir.join("game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    let game_patch = game_dir.join("Patch");
    copy_tree_files(&base_dir.join("Patch"), &game_patch);

    let manifest = binary_patcher::manifest::Manifest::load(&game_patch).unwrap();
    let patch_rel = manifest.changed[0].patch_file.clone();
    let patch_path = game_patch.join(&patch_rel);
    let outside_patch = outside.path().join("payload.patch");
    std::fs::copy(&patch_path, &outside_patch).unwrap();
    std::fs::remove_file(&patch_path).unwrap();
    std::os::unix::fs::symlink(&outside_patch, &patch_path).unwrap();

    let result = binary_patcher::apply::apply_bundle(&game_dir);
    assert!(
        result.is_err(),
        "apply must reject a symlink patch resource"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
    assert!(
        !game_patch
            .join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME)
            .exists()
    );
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
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_fast, true).unwrap();

    // Create patch with precise format
    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_precise, false).unwrap();

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

    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, false).unwrap();
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

    binary_patcher::hdiffpatch::run_hdiffz(&old_path, &new_path, &patch_path, false).unwrap();
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

#[cfg(unix)]
#[test]
fn test_journal_symlink_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let base = root.path();
    let patch_dir = base.join("Patch");
    let journal_path = patch_dir.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    std::fs::create_dir_all(&patch_dir).unwrap();

    let outside_journal = outside.path().join("journal.json");
    std::fs::write(&outside_journal, "[]").unwrap();
    std::os::unix::fs::symlink(&outside_journal, &journal_path).unwrap();

    let result = binary_patcher::apply::rollback_from_journal(base, &patch_dir);
    assert!(result.is_err());
    assert!(journal_path.exists());
    assert_eq!(std::fs::read_to_string(&outside_journal).unwrap(), "[]");
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

#[test]
fn test_journal_partial_recovery_failure_keeps_journal_and_marker() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");
    let journal_path = game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    let marker_path = game_patch.join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME);

    // added 目标被用户替换为目录，journal 恢复到该条目必然失败。
    std::fs::remove_file(game_dir.join("new_file.dll")).unwrap();
    std::fs::create_dir(game_dir.join("new_file.dll")).unwrap();
    std::fs::write(game_dir.join("new_file.dll/user.txt"), "user data").unwrap();
    std::fs::write(
        &journal_path,
        r#"[
            {"type":"patched","path":"config.ini"},
            {"type":"added","path":"new_file.dll","had_backup":false}
        ]"#,
    )
    .unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("journal.rollback-failed"),
        "unexpected error: {err}"
    );

    // 失败条目之前的条目已经恢复，失败的条目原样保留。
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n",
        "already recovered entries must stay recovered"
    );
    assert!(
        game_dir.join("new_file.dll").is_dir(),
        "conflicting path must stay untouched"
    );
    assert!(
        journal_path.exists(),
        "partially failed recovery must keep the journal"
    );
    assert!(
        marker_path.exists(),
        "partially failed recovery must keep the applied marker"
    );

    // 处理掉冲突后再次 rollback：应继续完成并清理 journal 与 marker。
    std::fs::remove_dir_all(game_dir.join("new_file.dll")).unwrap();
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert!(!journal_path.exists(), "journal must be removed on success");
    assert!(!marker_path.exists(), "marker must be cleared on success");
    assert!(!game_dir.join("new_file.dll").exists());
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
}

#[test]
fn test_journal_missing_backup_counts_as_failure_and_keeps_journal() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");
    let backup_root = game_patch.join(".backup_before_patch");
    let journal_path = game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    let marker_path = game_patch.join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME);

    // 删掉必要备份：恢复该条目必须失败，绝不能当成功跳过。
    let backup =
        binary_patcher::backup::find_backup(&game_dir.join("config.ini"), &game_dir, &backup_root)
            .unwrap()
            .expect("config.ini must have a backup after apply");
    std::fs::remove_file(&backup).unwrap();

    std::fs::write(&journal_path, r#"[{"type":"patched","path":"config.ini"}]"#).unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("journal.rollback-failed"),
        "unexpected error: {err}"
    );

    assert!(
        journal_path.exists(),
        "missing backup must keep the journal for retry"
    );
    assert!(
        marker_path.exists(),
        "missing backup must keep the applied marker for retry"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=new\nport=8080\n",
        "failed restore must leave the target untouched"
    );
}

#[test]
fn test_journal_recovery_keeps_backups_for_retry() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path().to_path_buf();
    let game_dir = build_applied_workspace(&base_dir);
    let game_patch = game_dir.join("Patch");
    let backup_root = game_patch.join(".backup_before_patch");
    let journal_path = game_patch.join(binary_patcher::apply::JOURNAL_FILE_NAME);
    let marker_path = game_patch.join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME);

    // 逆序恢复：missing.txt 无备份先失败，config.ini 随后成功恢复；
    // 采用保留备份的 copy 方式，config.ini 的备份必须仍在，供重试使用。
    std::fs::write(
        &journal_path,
        r#"[
            {"type":"patched","path":"config.ini"},
            {"type":"patched","path":"missing.txt"}
        ]"#,
    )
    .unwrap();

    let err = binary_patcher::rollback::rollback_bundle(&game_dir).unwrap_err();
    assert!(
        err.to_string().contains("journal.rollback-failed"),
        "unexpected error: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n",
        "successful entries must already be restored"
    );
    assert!(
        binary_patcher::backup::find_backup(&game_dir.join("config.ini"), &game_dir, &backup_root)
            .unwrap()
            .is_some(),
        "journal recovery must keep the backup so the retry can complete"
    );
    assert!(journal_path.exists());
    assert!(marker_path.exists());

    // 处理掉无备份的条目后重试：必须完成并清理 journal 与 marker。
    std::fs::write(&journal_path, r#"[{"type":"patched","path":"config.ini"}]"#).unwrap();
    binary_patcher::rollback::rollback_bundle(&game_dir).unwrap();

    assert!(!journal_path.exists(), "journal must be removed on success");
    assert!(!marker_path.exists(), "marker must be cleared on success");
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        "[section]\nkey=old\n"
    );
}

#[test]
fn test_named_patch_cli_selection_marker_and_rollback() {
    let root = tempfile::tempdir().unwrap();
    let base_dir = root.path();
    build_workspace(base_dir);

    let base_arg = base_dir.to_str().unwrap();
    for patch_name in ["v1.4.0", "security_hotfix"] {
        let output = Command::new(env!("CARGO_BIN_EXE_binary_patcher"))
            .args([
                "--lang",
                "en",
                "--patch-name",
                patch_name,
                "bundle",
                "--base-dir",
                base_arg,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bundle failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let game_dir = base_dir.join("named-game");
    copy_tree_files(&base_dir.join("Old"), &game_dir);
    for patch_name in ["v1.4.0", "security_hotfix"] {
        copy_tree_files(
            &base_dir.join(format!("Patch_{patch_name}")),
            &game_dir.join(format!("Patch_{patch_name}")),
        );
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_apply_patch"))
        .args(["--base-dir", game_dir.to_str().unwrap(), "--lang", "en"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Candidates are sorted by id: security_hotfix is 1, v1.4.0 is 2.
    child.stdin.take().unwrap().write_all(b"2\n").unwrap();
    let apply_output = child.wait_with_output().unwrap();
    assert!(
        apply_output.status.success(),
        "apply failed: stdout={} stderr={}",
        String::from_utf8_lossy(&apply_output.stdout),
        String::from_utf8_lossy(&apply_output.stderr)
    );
    let apply_stdout = String::from_utf8_lossy(&apply_output.stdout);
    assert!(apply_stdout.contains("Which patch should be applied?"));
    assert!(apply_stdout.contains("Apply identifier:"));
    assert!(game_dir.join("Patch_v1.4.0/.applied_patch.json").is_file());
    assert!(
        !game_dir
            .join("Patch_security_hotfix/.applied_patch.json")
            .exists()
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        std::fs::read_to_string(base_dir.join("New/config.ini")).unwrap()
    );

    let rollback_output = Command::new(env!("CARGO_BIN_EXE_rollback_patch"))
        .args(["--base-dir", game_dir.to_str().unwrap(), "--lang", "en"])
        .output()
        .unwrap();
    assert!(
        rollback_output.status.success(),
        "rollback failed: stdout={} stderr={}",
        String::from_utf8_lossy(&rollback_output.stdout),
        String::from_utf8_lossy(&rollback_output.stderr)
    );
    let rollback_stdout = String::from_utf8_lossy(&rollback_output.stdout);
    assert!(rollback_stdout.contains("latest apply identifier"));
    assert!(!game_dir.join("Patch_v1.4.0/.applied_patch.json").exists());
    assert_eq!(
        std::fs::read_to_string(game_dir.join("config.ini")).unwrap(),
        std::fs::read_to_string(base_dir.join("Old/config.ini")).unwrap()
    );
}

// ===========================================================================
// resolve_safe_path
// ===========================================================================

#[test]
fn test_resolve_normal_path() {
    let dir = tempfile::tempdir().unwrap();
    let target = binary_patcher::path::resolve_safe_path(dir.path(), "sub/file.txt").unwrap();
    let expected = dir.path().join("sub/file.txt");
    assert_eq!(target, expected);
}

#[test]
fn test_resolve_rejects_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(binary_patcher::path::resolve_safe_path(dir.path(), "../outside.txt").is_err());
}

#[test]
fn test_resolve_deep_traversal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(binary_patcher::path::resolve_safe_path(dir.path(), "sub/../../outside.txt").is_err());
}

#[cfg(unix)]
#[test]
fn test_resolve_rejects_symlink_components() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();

    assert!(binary_patcher::path::resolve_safe_path(root.path(), "link/secret.txt").is_err());
    assert!(binary_patcher::path::resolve_safe_path(root.path(), "link").is_err());
    assert!(binary_patcher::path::resolve_safe_path(root.path(), "link/../safe.txt").is_err());
}

#[cfg(unix)]
#[test]
fn test_ensure_parent_dir_rejects_symlink_components() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();

    let result = binary_patcher::path::ensure_parent_dir(&root.path().join("link/new/file.txt"));
    assert!(result.is_err());
    assert!(!outside.path().join("new/file.txt").exists());
}

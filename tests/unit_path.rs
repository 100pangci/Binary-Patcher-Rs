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

// ===========================================================================
// iter_files / relative_file_map / relative_dir_map / relative_maps
// ===========================================================================

#[test]
fn test_iter_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir_all(dir.path().join("sub/subsub")).unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "b").unwrap();
    std::fs::write(dir.path().join("sub/subsub/c.txt"), "c").unwrap();

    let files: Vec<_> = binary_patcher::fs::iter_files(dir.path()).collect();
    assert_eq!(files.len(), 3);
}

#[test]
fn test_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> = binary_patcher::fs::iter_files(dir.path()).collect();
    assert!(files.is_empty());
}

#[test]
fn test_relative_file_map() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("dir1")).unwrap();
    std::fs::write(dir.path().join("dir1/a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();

    let mapping = binary_patcher::fs::relative_file_map(dir.path());
    assert!(mapping.contains_key("dir1/a.txt"));
    assert!(mapping.contains_key("b.txt"));
}

#[test]
fn test_relative_dir_map() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("dir1/sub")).unwrap();
    std::fs::write(dir.path().join("dir1/sub/a.txt"), "a").unwrap();
    std::fs::create_dir_all(dir.path().join("dir2")).unwrap();
    std::fs::write(dir.path().join("dir2/b.txt"), "b").unwrap();

    let mapping = binary_patcher::fs::relative_dir_map(dir.path());
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

    let mapping = binary_patcher::fs::relative_dir_map(dir.path());
    assert!(mapping.contains_key("empty_dir"));
    assert!(mapping.contains_key("parent"));
    assert!(mapping.contains_key("parent/child"));
}

#[test]
fn test_relative_maps_returns_both() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
    std::fs::write(dir.path().join("sub/a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();

    let (files, dirs) = binary_patcher::fs::relative_maps(dir.path());
    assert!(files.contains_key("sub/a.txt"));
    assert!(files.contains_key("b.txt"));
    assert!(dirs.contains_key("sub"));
    assert!(dirs.contains_key("sub/deep"));
    assert_eq!(files.len(), 2);
    assert_eq!(dirs.len(), 2);
}

// ===========================================================================
// file-map.json loading and validation
// ===========================================================================

use binary_patcher::file_map::{FileMap, FileMapping, load_file_map, validate_file_map};

fn write_file_map(base_dir: &std::path::Path, old: &str, new: &str) {
    std::fs::write(base_dir.join("file-map.json"), file_map_json(old, new)).unwrap();
}

fn file_map_json(old: &str, new: &str) -> String {
    serde_json::json!({ "mappings": [{ "old": old, "new": new }] }).to_string()
}

fn workspace() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let old_dir = root.path().join("Old");
    let new_dir = root.path().join("New");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();
    (root, old_dir, new_dir)
}

#[test]
fn test_missing_file_map_is_none() {
    let root = tempfile::tempdir().unwrap();
    assert!(load_file_map(root.path()).unwrap().is_none());
}

#[test]
fn test_load_file_map_normalizes_backslashes() {
    let (root, _, _) = workspace();
    std::fs::create_dir_all(root.path().join("Old/data")).unwrap();
    std::fs::create_dir_all(root.path().join("New/data")).unwrap();
    std::fs::write(root.path().join("Old/data/a.pak"), b"a").unwrap();
    std::fs::write(root.path().join("New/data/a.chs"), b"b").unwrap();
    write_file_map(root.path(), r"data\a.pak", r"data\a.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert_eq!(file_map.mappings.len(), 1);
    assert_eq!(file_map.mappings[0].old_path(), "data/a.pak");
    assert_eq!(file_map.mappings[0].new_path(), "data/a.chs");
}

#[test]
fn test_load_file_map_malformed_json_errors() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file-map.json"), "{not valid json").unwrap();
    assert!(load_file_map(root.path()).is_err());
}

#[test]
fn test_load_file_map_directory_errors() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("file-map.json")).unwrap();
    assert!(load_file_map(root.path()).is_err());
}

#[test]
fn test_validate_accepts_valid_mapping() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::create_dir_all(old_dir.join("data")).unwrap();
    std::fs::create_dir_all(new_dir.join("data")).unwrap();
    std::fs::write(old_dir.join("data/a.pak"), b"a").unwrap();
    std::fs::write(new_dir.join("data/a.chs"), b"b").unwrap();
    write_file_map(root.path(), "data/a.pak", "data/a.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_ok());
}

#[test]
fn test_validate_rejects_missing_old() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(new_dir.join("a.chs"), b"b").unwrap();
    write_file_map(root.path(), "not-found.pak", "a.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    let err = validate_file_map(&file_map, &old_dir, &new_dir).unwrap_err();
    assert!(
        err.to_string().contains("filemap.old-not-found"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_validate_rejects_missing_new() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(old_dir.join("a.pak"), b"a").unwrap();
    write_file_map(root.path(), "a.pak", "not-found.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    let err = validate_file_map(&file_map, &old_dir, &new_dir).unwrap_err();
    assert!(
        err.to_string().contains("filemap.new-not-found"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_validate_rejects_duplicate_old() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(old_dir.join("a.pak"), b"a").unwrap();
    std::fs::write(new_dir.join("a.chs"), b"b").unwrap();
    std::fs::write(new_dir.join("b.chs"), b"c").unwrap();
    let map = serde_json::json!({
        "mappings": [
            { "old": "a.pak", "new": "a.chs" },
            { "old": "a.pak", "new": "b.chs" }
        ]
    });
    std::fs::write(root.path().join("file-map.json"), map.to_string()).unwrap();

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_err());
}

#[test]
fn test_validate_rejects_duplicate_new() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(old_dir.join("a.pak"), b"a").unwrap();
    std::fs::write(old_dir.join("b.pak"), b"b").unwrap();
    std::fs::write(new_dir.join("a.chs"), b"c").unwrap();
    let map = serde_json::json!({
        "mappings": [
            { "old": "a.pak", "new": "a.chs" },
            { "old": "b.pak", "new": "a.chs" }
        ]
    });
    std::fs::write(root.path().join("file-map.json"), map.to_string()).unwrap();

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_err());
}

#[test]
fn test_validate_rejects_old_equals_new() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::create_dir_all(old_dir.join("data")).unwrap();
    std::fs::create_dir_all(new_dir.join("data")).unwrap();
    std::fs::write(old_dir.join("data/foo.pak"), b"a").unwrap();
    std::fs::write(new_dir.join("data/foo.pak"), b"a").unwrap();
    write_file_map(root.path(), "data/foo.pak", "data/foo.pak");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    let err = validate_file_map(&file_map, &old_dir, &new_dir).unwrap_err();
    assert!(
        err.to_string().contains("filemap.same-path"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_validate_rejects_old_equals_new_even_if_missing() {
    let (root, old_dir, new_dir) = workspace();
    write_file_map(root.path(), "data/foo.pak", "data/foo.pak");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    let err = validate_file_map(&file_map, &old_dir, &new_dir).unwrap_err();
    assert!(
        err.to_string().contains("filemap.same-path"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_validate_rejects_path_traversal() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(new_dir.join("a.chs"), b"b").unwrap();
    write_file_map(root.path(), "../a.pak", "a.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_err());
}

#[test]
fn test_validate_rejects_absolute_path() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(old_dir.join("a.pak"), b"a").unwrap();
    std::fs::write(new_dir.join("a.chs"), b"b").unwrap();

    let absolute = new_dir.join("a.chs").to_string_lossy().to_string();
    write_file_map(root.path(), "a.pak", &absolute);

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_err());
}

#[test]
fn test_validate_rejects_empty_paths() {
    let (_, old_dir, new_dir) = workspace();
    let empty_old = FileMap {
        mappings: vec![FileMapping {
            old: String::new(),
            new: "a.chs".to_string(),
        }],
    };
    assert!(validate_file_map(&empty_old, &old_dir, &new_dir).is_err());

    let empty_new = FileMap {
        mappings: vec![FileMapping {
            old: "a.pak".to_string(),
            new: String::new(),
        }],
    };
    assert!(validate_file_map(&empty_new, &old_dir, &new_dir).is_err());
}

#[cfg(unix)]
#[test]
fn test_validate_rejects_symlinked_old() {
    let (root, old_dir, new_dir) = workspace();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("outside.pak");
    std::fs::write(&outside_file, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside_file, old_dir.join("a.pak")).unwrap();
    std::fs::write(new_dir.join("a.chs"), b"b").unwrap();
    write_file_map(root.path(), "a.pak", "a.chs");

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    assert!(validate_file_map(&file_map, &old_dir, &new_dir).is_err());
}

#[cfg(unix)]
#[test]
fn test_load_rejects_symlinked_file_map() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_map = outside.path().join("file-map.json");
    std::fs::write(&outside_map, r#"{"mappings":[]}"#).unwrap();
    std::os::unix::fs::symlink(&outside_map, root.path().join("file-map.json")).unwrap();

    assert!(load_file_map(root.path()).is_err());
}

#[test]
fn test_validate_accepts_empty_mappings() {
    let (_, old_dir, new_dir) = workspace();
    let empty = FileMap::default();
    assert!(empty.is_empty());
    assert!(validate_file_map(&empty, &old_dir, &new_dir).is_ok());
}

#[test]
fn test_validate_rejects_chained_mapping() {
    let (root, old_dir, new_dir) = workspace();
    std::fs::write(old_dir.join("a.pak"), b"a").unwrap();
    std::fs::write(old_dir.join("b.pak"), b"b").unwrap();
    std::fs::write(new_dir.join("b.pak"), b"x").unwrap();
    std::fs::write(new_dir.join("c.chs"), b"c").unwrap();
    let map = serde_json::json!({
        "mappings": [
            { "old": "a.pak", "new": "b.pak" },
            { "old": "b.pak", "new": "c.chs" }
        ]
    });
    std::fs::write(root.path().join("file-map.json"), map.to_string()).unwrap();

    let file_map = load_file_map(root.path()).unwrap().unwrap();
    let err = validate_file_map(&file_map, &old_dir, &new_dir).unwrap_err();
    assert!(
        err.to_string().contains("filemap.chained"),
        "unexpected error: {err}"
    );
}

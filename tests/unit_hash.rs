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
        binary_patcher::hash::sha256_of_file(&file_path).unwrap(),
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
        binary_patcher::hash::sha256_of_file(&file_path).unwrap(),
        expected
    );
}

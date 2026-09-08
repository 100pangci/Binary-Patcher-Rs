use binary_patcher::patch::{
    find_named_patch_dirs, latest_applied_patch, patch_dir_for_name, remove_applied_marker,
    write_applied_marker,
};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn test_patch_name_is_normalized_and_confined() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(
        patch_dir_for_name(root.path(), Some("v1.4.0")).unwrap(),
        root.path().join("patch_v1.4.0")
    );
    assert_eq!(
        patch_dir_for_name(root.path(), Some("patch_v1.4.0")).unwrap(),
        root.path().join("patch_v1.4.0")
    );
    assert_eq!(
        patch_dir_for_name(root.path(), None).unwrap(),
        root.path().join("Patch")
    );

    for invalid in [
        "",
        "..",
        "../escape",
        "nested/name",
        "nested\\name",
        "bad:name",
    ] {
        assert!(
            patch_dir_for_name(root.path(), Some(invalid)).is_err(),
            "expected invalid patch name: {invalid}"
        );
    }
}

#[test]
fn test_named_patch_discovery_requires_manifest() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("patch_v1.4.0")).unwrap();
    std::fs::write(root.path().join("patch_v1.4.0/manifest.json"), "{}").unwrap();
    std::fs::create_dir(root.path().join("patch_security_hotfix")).unwrap();
    std::fs::write(
        root.path().join("patch_security_hotfix/manifest.json"),
        "{}",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("patch_incomplete")).unwrap();
    std::fs::create_dir(root.path().join("release_v1.4.0")).unwrap();

    let candidates = find_named_patch_dirs(root.path()).unwrap();
    let ids: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    assert_eq!(ids, ["security_hotfix", "v1.4.0"]);
}

#[test]
fn test_applied_marker_is_found_and_removed() {
    let root = tempfile::tempdir().unwrap();
    let patch_dir = root.path().join("patch_v1.4.0");
    std::fs::create_dir(&patch_dir).unwrap();
    std::fs::write(patch_dir.join("manifest.json"), "{}").unwrap();

    let marker = write_applied_marker(root.path(), &patch_dir).unwrap();
    assert!(!marker.apply_id.is_empty());
    assert_eq!(marker.patch_dir, "patch_v1.4.0");

    let latest = latest_applied_patch(root.path()).unwrap().unwrap();
    assert_eq!(latest.0.id, "v1.4.0");
    assert_eq!(latest.1.apply_id, marker.apply_id);

    remove_applied_marker(&patch_dir).unwrap();
    assert!(latest_applied_patch(root.path()).unwrap().is_none());
}

#[test]
fn test_apply_choice_zero_exits_without_touching_target() {
    let root = tempfile::tempdir().unwrap();
    for name in ["patch_v1.4.0", "patch_security_hotfix"] {
        let patch_dir = root.path().join(name);
        std::fs::create_dir(&patch_dir).unwrap();
        std::fs::write(patch_dir.join("manifest.json"), "{}").unwrap();
    }
    std::fs::write(root.path().join("config.ini"), "original").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_apply_patch"))
        .args(["--base-dir", root.path().to_str().unwrap(), "--lang", "en"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"invalid\n0\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Which patch should be applied?"));
    assert!(stdout.contains("Invalid choice"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("config.ini")).unwrap(),
        "original"
    );
    assert!(
        !root
            .path()
            .join("patch_v1.4.0/.applied_patch.json")
            .exists()
    );
    assert!(
        !root
            .path()
            .join("patch_security_hotfix/.applied_patch.json")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn test_patch_directory_symlink_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("patch_external")).unwrap();

    assert!(
        binary_patcher::patch::validate_patch_dir(root.path(), &root.path().join("patch_external"))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn test_applied_marker_symlink_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let patch_dir = root.path().join("patch_v1.4.0");
    std::fs::create_dir(&patch_dir).unwrap();
    std::fs::write(patch_dir.join("manifest.json"), "{}").unwrap();
    let outside_marker = root.path().join("outside-marker.json");
    std::fs::write(&outside_marker, "{}").unwrap();
    std::os::unix::fs::symlink(
        &outside_marker,
        patch_dir.join(binary_patcher::patch::APPLIED_MARKER_FILE_NAME),
    )
    .unwrap();

    assert!(latest_applied_patch(root.path()).is_err());
    assert!(remove_applied_marker(&patch_dir).is_err());
    assert!(outside_marker.exists());
}

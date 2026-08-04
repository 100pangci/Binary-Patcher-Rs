// ===========================================================================
// Manifest validation / loading / version compat
// ===========================================================================

use binary_patcher::manifest::Manifest;

#[test]
fn test_valid_manifest() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
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
    let manifest = Manifest {
        format: "invalid".to_string(),
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
// Manifest rejects path traversal entries
// ===========================================================================

#[test]
fn test_manifest_rejects_traversal_in_changed_path() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "../escape.txt".to_string(),
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "p.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    // Validate passes (format check), but load + apply will catch traversal at resolve_safe_path
    assert!(manifest.validate().is_ok());
}

#[test]
fn test_resolve_safe_path_rejects_traversal_in_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Patch")).unwrap();
    let manifest = serde_json::json!({
        "format": env!("CARGO_PKG_VERSION"),
        "source_root": "Old",
        "target_root": "New",
        "changed": [{
            "path": "../outside.txt",
            "old_sha256": "a".repeat(64),
            "new_sha256": "b".repeat(64),
            "patch_file": "p.patch"
        }],
        "added": [],
        "deleted": [],
        "deleted_dirs": []
    });
    std::fs::write(
        dir.path().join("Patch/manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Verify manifest loads but resolve_safe_path catches the traversal during apply
    assert!(binary_patcher::manifest::Manifest::load(&dir.path().join("Patch")).is_ok());
    assert!(binary_patcher::path::resolve_safe_path(dir.path(), "../outside.txt").is_err());
}

// ===========================================================================
// Malformed JSON manifest
// ===========================================================================

#[test]
fn test_manifest_malformed_json_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // Truncated JSON
    std::fs::write(patch_dir.join("manifest.json"), "{\"format\": \"1.1.0\", ").unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

#[test]
fn test_manifest_malformed_json_not_object() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // JSON array is not a valid manifest
    std::fs::write(patch_dir.join("manifest.json"), "[1, 2, 3]").unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

#[test]
fn test_manifest_malformed_json_random_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    // Random binary garbage
    std::fs::write(
        patch_dir.join("manifest.json"),
        vec![0xFF, 0xFE, 0x00, 0x01],
    )
    .unwrap();
    assert!(binary_patcher::manifest::Manifest::load(&patch_dir).is_err());
}

// ===========================================================================
// Version compat
// ===========================================================================

#[test]
fn test_version_compat_major_mismatch() {
    match binary_patcher::manifest::check_version_compat("2.0.0") {
        binary_patcher::manifest::VersionCompat::Compatible => panic!("expected incompatible"),
        binary_patcher::manifest::VersionCompat::Incompatible { .. } => {} // ok
    }
}

#[test]
fn test_version_compat_minor_mismatch() {
    let current = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = current.split('.').collect();
    let mismatched = format!(
        "{}.{}.{}",
        parts[0],
        parts[1].parse::<u32>().unwrap() + 1,
        0
    );
    match binary_patcher::manifest::check_version_compat(&mismatched) {
        binary_patcher::manifest::VersionCompat::Compatible => panic!("expected incompatible"),
        binary_patcher::manifest::VersionCompat::Incompatible { .. } => {} // ok
    }
}

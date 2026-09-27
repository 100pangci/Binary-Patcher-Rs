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
            source_path: None,
            delete_source: false,
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
            source_path: None,
            delete_source: false,
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

#[cfg(unix)]
#[test]
fn test_manifest_symlink_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let patch_dir = root.path().join("Patch");
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(&patch_dir).unwrap();

    let manifest_path = outside.path().join("manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&Manifest::default()).unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&manifest_path, patch_dir.join("manifest.json")).unwrap();

    assert!(Manifest::load(&patch_dir).is_err());
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

// ===========================================================================
// Rename-aware changed entries / source_path backward compatibility
// ===========================================================================

#[test]
fn test_legacy_manifest_without_source_path_loads() {
    let dir = tempfile::tempdir().unwrap();
    let patch_dir = dir.path().join("Patch");
    std::fs::create_dir_all(&patch_dir).unwrap();
    let manifest = serde_json::json!({
        "format": env!("CARGO_PKG_VERSION"),
        "source_root": "Old",
        "target_root": "New",
        "changed": [{
            "path": "data/foo.pak",
            "old_sha256": "a".repeat(64),
            "new_sha256": "b".repeat(64),
            "patch_file": "data/foo.pak.patch"
        }],
        "added": [],
        "deleted": [],
        "deleted_dirs": []
    });
    std::fs::write(
        patch_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let loaded = Manifest::load(&patch_dir).unwrap();
    assert_eq!(loaded.changed.len(), 1);
    assert!(loaded.changed[0].source_path.is_none());
    assert!(!loaded.changed[0].is_renamed());
    assert_eq!(loaded.changed[0].old_relative_path(), "data/foo.pak");
}

#[test]
fn test_changed_entry_serde_source_path() {
    let normal = binary_patcher::manifest::ChangedEntry {
        path: "data/foo.pak".to_string(),
        source_path: None,
        delete_source: false,
        old_sha256: "a".repeat(64),
        new_sha256: "b".repeat(64),
        patch_file: "data/foo.pak.patch".to_string(),
    };
    let json = serde_json::to_string(&normal).unwrap();
    assert!(
        !json.contains("source_path"),
        "normal entry must not serialize source_path: {json}"
    );
    assert!(
        !json.contains("delete_source"),
        "normal entry must not serialize delete_source: {json}"
    );

    let renamed = binary_patcher::manifest::ChangedEntry {
        path: "data/foo.chs".to_string(),
        source_path: Some("data/foo.pak".to_string()),
        delete_source: true,
        old_sha256: "a".repeat(64),
        new_sha256: "b".repeat(64),
        patch_file: "data/foo.chs.patch".to_string(),
    };
    let json = serde_json::to_string(&renamed).unwrap();
    assert!(json.contains("\"source_path\":\"data/foo.pak\""), "{json}");
    assert!(json.contains("\"delete_source\":true"), "{json}");
    assert!(renamed.is_renamed());
    assert_eq!(renamed.old_relative_path(), "data/foo.pak");

    let parsed: binary_patcher::manifest::ChangedEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.source_path.as_deref(), Some("data/foo.pak"));
    assert!(parsed.delete_source);

    // delete_source 缺省按 false 读取（兼容旧 manifest）。
    let legacy: binary_patcher::manifest::ChangedEntry = serde_json::from_str(
        r#"{"path":"a.chs","source_path":"a.pak","old_sha256":"aa","new_sha256":"bb","patch_file":"a.chs.patch"}"#,
    )
    .unwrap();
    assert!(!legacy.delete_source);
}

#[test]
fn test_manifest_rejects_delete_source_without_source_path() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "a.txt".to_string(),
            source_path: None,
            delete_source: true,
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "a.txt.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    let err = manifest.validate().unwrap_err();
    assert!(
        err.to_string()
            .contains("manifest.changed-delete-source-without-source"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_manifest_rejects_empty_source_path() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "a.txt".to_string(),
            source_path: Some(String::new()),
            delete_source: false,
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "a.txt.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    assert!(manifest.validate().is_err());
}

#[test]
fn test_manifest_rejects_source_same_as_target_with_dot_variant() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "data/foo.bin".to_string(),
            source_path: Some("./data/foo.bin".to_string()),
            delete_source: false,
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "data/foo.bin.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    let err = manifest.validate().unwrap_err();
    assert!(
        err.to_string()
            .contains("manifest.changed-source-same-as-target"),
        "unexpected error: {err}"
    );
}

#[cfg(windows)]
#[test]
fn test_manifest_rejects_source_same_as_target_case_variant_on_windows() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "data/Foo.bin".to_string(),
            source_path: Some("data/foo.bin".to_string()),
            delete_source: false,
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "data/Foo.bin.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    let err = manifest.validate().unwrap_err();
    assert!(
        err.to_string()
            .contains("manifest.changed-source-same-as-target"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_manifest_accepts_distinct_source_target() {
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![binary_patcher::manifest::ChangedEntry {
            path: "data/foo.chs".to_string(),
            source_path: Some("data/foo.pak".to_string()),
            delete_source: false,
            old_sha256: "a".repeat(64),
            new_sha256: "b".repeat(64),
            patch_file: "data/foo.chs.patch".to_string(),
        }],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    assert!(manifest.validate().is_ok());
}

#[test]
fn test_manifest_rejects_duplicate_patch_resource() {
    let entry = |path: &str| binary_patcher::manifest::ChangedEntry {
        path: path.to_string(),
        source_path: None,
        delete_source: false,
        old_sha256: "a".repeat(64),
        new_sha256: "b".repeat(64),
        patch_file: "same.patch".to_string(),
    };
    let manifest = Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed: vec![entry("a.txt"), entry("b.txt")],
        added: vec![],
        deleted: vec![],
        deleted_dirs: vec![],
    };
    assert!(manifest.validate().is_err());
}

// ===========================================================================
// Cross-category logical path conflicts
// ===========================================================================

fn changed_entry(
    path: &str,
    source_path: Option<&str>,
    patch_file: &str,
) -> binary_patcher::manifest::ChangedEntry {
    binary_patcher::manifest::ChangedEntry {
        path: path.to_string(),
        source_path: source_path.map(str::to_string),
        delete_source: false,
        old_sha256: "a".repeat(64),
        new_sha256: "b".repeat(64),
        patch_file: patch_file.to_string(),
    }
}

fn added_entry(path: &str) -> binary_patcher::manifest::AddedEntry {
    binary_patcher::manifest::AddedEntry {
        path: path.to_string(),
        new_sha256: "c".repeat(64),
        file: format!("{path}.new"),
    }
}

fn deleted_entry(path: &str) -> binary_patcher::manifest::DeletedEntry {
    binary_patcher::manifest::DeletedEntry {
        path: path.to_string(),
        old_sha256: "d".repeat(64),
    }
}

fn manifest_with(
    changed: Vec<binary_patcher::manifest::ChangedEntry>,
    added: Vec<binary_patcher::manifest::AddedEntry>,
    deleted: Vec<binary_patcher::manifest::DeletedEntry>,
) -> Manifest {
    Manifest {
        format: env!("CARGO_PKG_VERSION").to_string(),
        source_root: "Old".to_string(),
        target_root: "New".to_string(),
        changed,
        added,
        deleted,
        deleted_dirs: vec![],
    }
}

fn assert_logical_path_conflict(manifest: &Manifest) {
    let err = manifest.validate().unwrap_err();
    assert!(
        err.to_string().contains("manifest.logical-path-conflict"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_manifest_rejects_duplicate_changed_targets() {
    let manifest = manifest_with(
        vec![
            changed_entry("dup.bin", None, "dup-1.patch"),
            changed_entry("dup.bin", None, "dup-2.patch"),
        ],
        vec![],
        vec![],
    );
    assert_logical_path_conflict(&manifest);
}

#[test]
fn test_manifest_rejects_changed_added_path_conflict() {
    let manifest = manifest_with(
        vec![changed_entry("same.bin", None, "same.bin.patch")],
        vec![added_entry("same.bin")],
        vec![],
    );
    assert_logical_path_conflict(&manifest);
}

#[test]
fn test_manifest_rejects_added_deleted_path_conflict() {
    let manifest = manifest_with(
        vec![],
        vec![added_entry("same.bin")],
        vec![deleted_entry("same.bin")],
    );
    assert_logical_path_conflict(&manifest);
}

#[test]
fn test_manifest_rejects_mapping_source_conflict_with_deleted() {
    let manifest = manifest_with(
        vec![changed_entry(
            "data/foo.chs",
            Some("data/foo.pak"),
            "data/foo.chs.patch",
        )],
        vec![],
        vec![deleted_entry("data/foo.pak")],
    );
    assert_logical_path_conflict(&manifest);
}

#[test]
fn test_manifest_rejects_dot_normalized_path_conflict() {
    let manifest = manifest_with(
        vec![changed_entry("data/foo.bin", None, "data/foo.bin.patch")],
        vec![added_entry("./data/foo.bin")],
        vec![],
    );
    assert_logical_path_conflict(&manifest);
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn test_manifest_rejects_case_variant_path_conflict() {
    let manifest = manifest_with(
        vec![changed_entry("Data/FOO.bin", None, "Data/FOO.bin.patch")],
        vec![added_entry("data/foo.bin")],
        vec![],
    );
    assert_logical_path_conflict(&manifest);
}

// ===========================================================================
// Mapping schema version guard
// ===========================================================================

#[test]
fn test_version_compat_rejects_pre_mapping_version() {
    // 1.3.x 工具不认识 source_path / delete_source 语义，必须判为不兼容，
    // 不允许静默按旧语义执行。当前工具为 1.4.x。
    match binary_patcher::manifest::check_version_compat("1.3.1") {
        binary_patcher::manifest::VersionCompat::Incompatible { .. } => {}
        binary_patcher::manifest::VersionCompat::Compatible => {
            panic!("1.3.x manifests must be incompatible with the mapping-aware tool")
        }
    }
}

#[test]
fn test_current_manifest_version_matches_crate_version() {
    let manifest = Manifest::default();
    assert_eq!(manifest.format, env!("CARGO_PKG_VERSION"));
    assert!(
        manifest.format.starts_with("1.4."),
        "mapping schema requires a minor bump to 1.4.0, got {}",
        manifest.format
    );
}

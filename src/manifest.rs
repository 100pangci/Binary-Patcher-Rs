use crate::t;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;

pub const MANIFEST_NAME: &str = "manifest.json";
pub const INSTRUCTIONS_NAME: &str = "README.txt";
pub const WORKSPACE_DIRS: [&str; 3] = ["Old", "New", "Patch"];
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedEntry {
    /// 最终目标文件路径（New 侧相对路径）。
    pub path: String,

    /// 显式映射的旧文件路径（Old 侧相对路径）。
    /// 缺省时表示普通同路径修改，兼容旧 manifest。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,

    /// 应用成功后是否删除映射源文件。仅对显式映射有效，缺省 `false`。
    #[serde(default, skip_serializing_if = "is_false")]
    pub delete_source: bool,

    pub old_sha256: String,
    pub new_sha256: String,
    pub patch_file: String,
}

// serde 的 skip_serializing_if 要求接收引用，签名无法改为按值传递。
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

impl ChangedEntry {
    /// apply / rollback 使用的旧文件相对路径；无映射时与 `path` 相同。
    pub fn old_relative_path(&self) -> &str {
        self.source_path.as_deref().unwrap_or(&self.path)
    }

    /// 该条目是否为显式文件名映射（旧路径与目标路径不同）。
    pub fn is_renamed(&self) -> bool {
        self.source_path
            .as_deref()
            .is_some_and(|source| source != self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddedEntry {
    pub path: String,
    pub new_sha256: String,
    #[serde(rename = "file")]
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedEntry {
    pub path: String,
    pub old_sha256: String,
}

#[derive(Debug, Clone)]
pub enum VersionCompat {
    Compatible,
    Incompatible { manifest: String, tool: String },
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() == 3 {
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    } else if parts.len() == 1 {
        Some((parts[0].parse().ok()?, 0, 0))
    } else {
        None
    }
}

pub fn check_version_compat(manifest_version: &str) -> VersionCompat {
    let Some(manifest_ver) = parse_semver(manifest_version) else {
        return VersionCompat::Incompatible {
            manifest: manifest_version.to_string(),
            tool: PACKAGE_VERSION.to_string(),
        };
    };
    let Some(tool_ver) = parse_semver(PACKAGE_VERSION) else {
        return VersionCompat::Incompatible {
            manifest: manifest_version.to_string(),
            tool: PACKAGE_VERSION.to_string(),
        };
    };
    if manifest_ver.0 == tool_ver.0 && manifest_ver.1 == tool_ver.1 {
        VersionCompat::Compatible
    } else {
        VersionCompat::Incompatible {
            manifest: manifest_version.to_string(),
            tool: PACKAGE_VERSION.to_string(),
        }
    }
}

fn deserialize_format<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct FormatVisitor;
    impl Visitor<'_> for FormatVisitor {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a semver string like \"1.1.0\" or an integer like 1")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(format!("{v}.0.0"))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(format!("{v}.0.0"))
        }
    }
    deserializer.deserialize_any(FormatVisitor)
}

fn serialize_format<S>(format: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(format)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(
        deserialize_with = "deserialize_format",
        serialize_with = "serialize_format"
    )]
    pub format: String,
    pub source_root: String,
    pub target_root: String,
    pub changed: Vec<ChangedEntry>,
    pub added: Vec<AddedEntry>,
    pub deleted: Vec<DeletedEntry>,
    #[serde(default)]
    pub deleted_dirs: Vec<String>,
}

fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            format: PACKAGE_VERSION.to_string(),
            source_root: "Old".to_string(),
            target_root: "New".to_string(),
            changed: Vec::new(),
            added: Vec::new(),
            deleted: Vec::new(),
            deleted_dirs: Vec::new(),
        }
    }
}

impl Manifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if parse_semver(&self.format).is_none() {
            anyhow::bail!("{}", t!("manifest.format-invalid", self.format));
        }

        let mut patch_files = std::collections::BTreeSet::new();
        for (idx, item) in self.changed.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("{}", t!("manifest.changed-path-empty", idx));
            }
            if item.source_path.as_deref().is_some_and(str::is_empty) {
                anyhow::bail!("{}", t!("manifest.changed-source-path-empty", idx));
            }
            // source 与 target 即使字符串不同，也可能经规范化（`./`、重复分隔符）
            // 或 Windows 大小写等价后指向同一文件；此类条目在 apply 时会导致
            // 自覆盖 / 自删除，必须在 manifest 阶段拒绝。
            if let Some(source) = item.source_path.as_deref()
                && source != item.path
                && crate::file_map::is_same_logical_path(source, &item.path)?
            {
                anyhow::bail!(
                    "{}",
                    t!(
                        "manifest.changed-source-same-as-target",
                        idx,
                        source,
                        item.path
                    )
                );
            }
            if item.delete_source && !item.is_renamed() {
                anyhow::bail!(
                    "{}",
                    t!("manifest.changed-delete-source-without-source", idx)
                );
            }
            if item.old_sha256.is_empty() {
                anyhow::bail!("{}", t!("manifest.changed-missing-old-sha", idx));
            }
            if !is_valid_sha256(&item.old_sha256) {
                anyhow::bail!(
                    "{}",
                    t!("manifest.changed-invalid-old-sha", idx, item.old_sha256)
                );
            }
            if item.new_sha256.is_empty() {
                anyhow::bail!("{}", t!("manifest.changed-missing-new-sha", idx));
            }
            if !is_valid_sha256(&item.new_sha256) {
                anyhow::bail!(
                    "{}",
                    t!("manifest.changed-invalid-new-sha", idx, item.new_sha256)
                );
            }
            if item.patch_file.is_empty() {
                anyhow::bail!("{}", t!("manifest.changed-missing-patch", idx));
            }
            if !patch_files.insert(item.patch_file.as_str()) {
                anyhow::bail!(
                    "{}",
                    t!("manifest.changed-duplicate-patch", idx, item.patch_file)
                );
            }
        }

        for (idx, item) in self.added.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("{}", t!("manifest.added-path-empty", idx));
            }
            if item.new_sha256.is_empty() {
                anyhow::bail!("{}", t!("manifest.added-missing-sha", idx));
            }
            if !is_valid_sha256(&item.new_sha256) {
                anyhow::bail!("{}", t!("manifest.added-invalid-sha", idx, item.new_sha256));
            }
            if item.file.is_empty() {
                anyhow::bail!("{}", t!("manifest.added-missing-file", idx));
            }
        }

        for (idx, item) in self.deleted.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("{}", t!("manifest.deleted-path-empty", idx));
            }
            if item.old_sha256.is_empty() {
                anyhow::bail!("{}", t!("manifest.deleted-missing-sha", idx));
            }
            if !is_valid_sha256(&item.old_sha256) {
                anyhow::bail!(
                    "{}",
                    t!("manifest.deleted-invalid-sha", idx, item.old_sha256)
                );
            }
        }

        for (idx, item) in self.deleted_dirs.iter().enumerate() {
            if item.is_empty() {
                anyhow::bail!("{}", t!("manifest.deleted-dir-empty", idx));
            }
        }

        Ok(())
    }

    pub fn load(patch_dir: &Path) -> anyhow::Result<Self> {
        crate::path::ensure_no_symlink_components(patch_dir)?;
        match std::fs::symlink_metadata(patch_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                anyhow::bail!("{}", t!("path.symlink", patch_dir.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let manifest_path = crate::path::resolve_safe_path(patch_dir, MANIFEST_NAME)?;
        if !manifest_path.exists() {
            anyhow::bail!("{}", t!("manifest.not-found", manifest_path.display()));
        }
        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(&content)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn save(&self, patch_dir: &Path) -> anyhow::Result<()> {
        self.validate()?;
        let manifest_path = crate::path::resolve_safe_path(patch_dir, MANIFEST_NAME)?;
        crate::path::ensure_parent_dir(&manifest_path)?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(manifest_path, content)?;
        Ok(())
    }
}

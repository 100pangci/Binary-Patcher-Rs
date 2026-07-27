//! 补丁清单（Manifest）定义、JSON 序列化、格式校验和版本兼容检查。

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;

/// Manifest 文件名。
pub const MANIFEST_NAME: &str = "manifest.json";
/// 使用说明文件名。
pub const INSTRUCTIONS_NAME: &str = "README.txt";
/// 工作区目录名。
pub const WORKSPACE_DIRS: [&str; 3] = ["Old", "New", "Patch"];
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Manifest 中一条变更记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedEntry {
    /// 相对路径（Unix 分隔符）。
    pub path: String,
    /// 旧文件 SHA256 校验和。
    pub old_sha256: String,
    /// 新文件 SHA256 校验和。
    pub new_sha256: String,
    /// 补丁文件路径（相对 Patch 目录）。
    pub patch_file: String,
}

/// Manifest 中一条新增记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddedEntry {
    /// 相对路径（Unix 分隔符）。
    pub path: String,
    /// 新文件 SHA256 校验和。
    pub new_sha256: String,
    /// 新增文件副本路径（相对 Patch 目录）。
    #[serde(rename = "file")]
    pub file: String,
}

/// Manifest 中一条删除记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedEntry {
    /// 相对路径（Unix 分隔符）。
    pub path: String,
    /// 删除文件的 SHA256 校验和。
    pub old_sha256: String,
}

/// 版本兼容性检查结果。
#[derive(Debug, Clone)]
pub enum VersionCompat {
    /// 完全兼容（major.minor 相同）
    Compatible,
    /// 不兼容（major 或 minor 不同）
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
        // Legacy: "1" → (1, 0, 0)
        Some((parts[0].parse().ok()?, 0, 0))
    } else {
        None
    }
}

/// 检查 manifest 版本与当前工具版本的兼容性（major.minor 必须相同）。
pub fn check_version_compat(manifest_version: &str) -> VersionCompat {
    let manifest_ver = match parse_semver(manifest_version) {
        Some(v) => v,
        None => {
            return VersionCompat::Incompatible {
                manifest: manifest_version.to_string(),
                tool: PACKAGE_VERSION.to_string(),
            };
        }
    };
    let tool_ver = match parse_semver(PACKAGE_VERSION) {
        Some(v) => v,
        None => {
            return VersionCompat::Incompatible {
                manifest: manifest_version.to_string(),
                tool: PACKAGE_VERSION.to_string(),
            };
        }
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
    impl<'de> Visitor<'de> for FormatVisitor {
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

/// 补丁清单，描述 Old → New 之间所有变更。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// 创建此 manifest 的工具版本（semver）。
    #[serde(
        deserialize_with = "deserialize_format",
        serialize_with = "serialize_format"
    )]
    pub format: String,
    /// 源目录名（默认 Old）。
    pub source_root: String,
    /// 目标目录名（默认 New）。
    pub target_root: String,
    /// 已变更的文件列表。
    pub changed: Vec<ChangedEntry>,
    /// 新增的文件列表。
    pub added: Vec<AddedEntry>,
    /// 删除的文件列表。
    pub deleted: Vec<DeletedEntry>,
    /// 删除的目录列表（最深优先排序）。
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
    /// 校验 manifest 所有字段的完整性和格式（SHA256 长度/内容、路径非空等）。
    pub fn validate(&self) -> anyhow::Result<()> {
        if parse_semver(&self.format).is_none() {
            anyhow::bail!("manifest format 版本格式无效: {}", self.format);
        }

        for (idx, item) in self.changed.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("manifest changed[{idx}] path 为空");
            }
            if item.old_sha256.is_empty() {
                anyhow::bail!("manifest changed[{idx}] 缺少字段 'old_sha256'");
            }
            if !is_valid_sha256(&item.old_sha256) {
                anyhow::bail!(
                    "manifest changed[{idx}] old_sha256 格式无效: {}",
                    item.old_sha256
                );
            }
            if item.new_sha256.is_empty() {
                anyhow::bail!("manifest changed[{idx}] 缺少字段 'new_sha256'");
            }
            if !is_valid_sha256(&item.new_sha256) {
                anyhow::bail!(
                    "manifest changed[{idx}] new_sha256 格式无效: {}",
                    item.new_sha256
                );
            }
            if item.patch_file.is_empty() {
                anyhow::bail!("manifest changed[{idx}] 缺少字段 'patch_file'");
            }
        }

        for (idx, item) in self.added.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("manifest added[{idx}] path 为空");
            }
            if item.new_sha256.is_empty() {
                anyhow::bail!("manifest added[{idx}] 缺少字段 'new_sha256'");
            }
            if !is_valid_sha256(&item.new_sha256) {
                anyhow::bail!(
                    "manifest added[{idx}] new_sha256 格式无效: {}",
                    item.new_sha256
                );
            }
            if item.file.is_empty() {
                anyhow::bail!("manifest added[{idx}] 缺少字段 'file'");
            }
        }

        for (idx, item) in self.deleted.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("manifest deleted[{idx}] path 为空");
            }
            if item.old_sha256.is_empty() {
                anyhow::bail!("manifest deleted[{idx}] 缺少字段 'old_sha256'");
            }
            if !is_valid_sha256(&item.old_sha256) {
                anyhow::bail!(
                    "manifest deleted[{idx}] old_sha256 格式无效: {}",
                    item.old_sha256
                );
            }
        }

        for (idx, item) in self.deleted_dirs.iter().enumerate() {
            if item.is_empty() {
                anyhow::bail!("manifest deleted_dirs[{idx}] 路径为空");
            }
        }

        Ok(())
    }

    /// 从 Patch 目录加载 manifest.json 并校验。
    pub fn load(patch_dir: &Path) -> anyhow::Result<Self> {
        let manifest_path = patch_dir.join(MANIFEST_NAME);
        if !manifest_path.exists() {
            anyhow::bail!("未找到补丁清单文件 '{}'", manifest_path.display());
        }
        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(&content)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 校验后序列化写入 Patch/manifest.json。
    pub fn save(&self, patch_dir: &Path) -> anyhow::Result<()> {
        self.validate()?;
        crate::utils::ensure_parent_dir(&patch_dir.join(MANIFEST_NAME))?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(patch_dir.join(MANIFEST_NAME), content)?;
        Ok(())
    }
}

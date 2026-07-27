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
    pub path: String,
    pub old_sha256: String,
    pub new_sha256: String,
    pub patch_file: String,
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

        for (idx, item) in self.changed.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("{}", t!("manifest.changed-path-empty", idx));
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
        let manifest_path = patch_dir.join(MANIFEST_NAME);
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
        crate::path::ensure_parent_dir(&patch_dir.join(MANIFEST_NAME))?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(patch_dir.join(MANIFEST_NAME), content)?;
        Ok(())
    }
}

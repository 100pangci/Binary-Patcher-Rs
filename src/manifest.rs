use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MANIFEST_NAME: &str = "manifest.json";
pub const INSTRUCTIONS_NAME: &str = "README.txt";
pub const WORKSPACE_DIRS: [&str; 3] = ["Old", "New", "Patch"];

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub source_root: String,
    pub target_root: String,
    pub changed: Vec<ChangedEntry>,
    pub added: Vec<AddedEntry>,
    pub deleted: Vec<DeletedEntry>,
    #[serde(default)]
    pub deleted_dirs: Vec<String>,
}

impl Manifest {
    pub fn new() -> Self {
        Self {
            format: 1,
            source_root: "Old".to_string(),
            target_root: "New".to_string(),
            changed: Vec::new(),
            added: Vec::new(),
            deleted: Vec::new(),
            deleted_dirs: Vec::new(),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.format != 1 {
            anyhow::bail!("不支持的 manifest 格式版本: {}。当前工具仅支持格式版本 1。", self.format);
        }

        for (idx, item) in self.changed.iter().enumerate() {
            if item.path.is_empty() {
                anyhow::bail!("manifest changed[{idx}] path 为空");
            }
            if item.old_sha256.is_empty() {
                anyhow::bail!("manifest changed[{idx}] 缺少字段 'old_sha256'");
            }
            if item.new_sha256.is_empty() {
                anyhow::bail!("manifest changed[{idx}] 缺少字段 'new_sha256'");
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
        }

        for (idx, item) in self.deleted_dirs.iter().enumerate() {
            if item.is_empty() {
                anyhow::bail!("manifest deleted_dirs[{idx}] 路径为空");
            }
        }

        Ok(())
    }

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

    pub fn save(&self, patch_dir: &Path) -> anyhow::Result<()> {
        crate::utils::ensure_parent_dir(&patch_dir.join(MANIFEST_NAME))?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(patch_dir.join(MANIFEST_NAME), content)?;
        Ok(())
    }

}

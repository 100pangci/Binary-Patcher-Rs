//! 显式文件名映射（rename-aware diff）。
//!
//! 工作目录根目录下的可选文件 `file-map.json` 允许用户手动声明
//! `Old/<old>` 与 `New/<new>` 属于同一个逻辑文件。整包补丁会为这类路径
//! 生成二进制差分（而不是识别为「删除 + 新增」），apply / rollback /
//! journal 崩溃恢复负责完成 `old -> new` 的路径迁移。
//!
//! 映射完全显式：程序不会根据文件名、basename、扩展名、大小或哈希
//! 自动猜测映射关系。`file-map.json` 不存在时行为与旧版本完全一致。

use crate::t;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const FILE_MAP_NAME: &str = "file-map.json";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileMap {
    #[serde(default)]
    pub mappings: Vec<FileMapping>,
}

impl FileMap {
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileMapping {
    pub old: String,
    pub new: String,
}

impl FileMapping {
    /// `Old/` 下的相对路径，Windows 分隔符已规范化为 `/`。
    pub fn old_path(&self) -> String {
        normalize_mapping_path(&self.old)
    }

    /// `New/` 下的相对路径，Windows 分隔符已规范化为 `/`。
    pub fn new_path(&self) -> String {
        normalize_mapping_path(&self.new)
    }
}

/// 将路径规范化为 manifest 使用的 `/` 相对路径形式。
pub fn normalize_mapping_path(raw: &str) -> String {
    raw.replace('\\', "/")
}

/// 读取工作目录根目录下的可选 `file-map.json`。
///
/// 返回 `Ok(None)` 表示文件不存在（opt-in 功能关闭）。
pub fn load_file_map(base_dir: &Path) -> anyhow::Result<Option<FileMap>> {
    let path = base_dir.join(FILE_MAP_NAME);
    crate::path::ensure_no_symlink_components(&path)?;

    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        anyhow::bail!("{}", t!("filemap.not-a-file", path.display()));
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", t!("filemap.read-failed", path.display())))?;
    let file_map: FileMap = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("{}: {e}", t!("filemap.parse-failed", path.display())))?;
    Ok(Some(file_map))
}

/// 严格校验显式映射，任何问题直接报错，绝不静默忽略。
///
/// 校验项：
/// - 空路径、`old == new`
/// - 旧/新文件必须存在
/// - 路径安全（复用 `resolve_safe_path`，拒绝 `../`、绝对路径与符号链接）
/// - 严格一对一（同一旧文件或同一新文件不得映射多次，含同一物理文件）
pub fn validate_file_map(file_map: &FileMap, old_dir: &Path, new_dir: &Path) -> anyhow::Result<()> {
    let mut seen_old: BTreeSet<PathBuf> = BTreeSet::new();
    let mut seen_new: BTreeSet<PathBuf> = BTreeSet::new();
    let mut old_names: BTreeSet<String> = BTreeSet::new();
    let mut new_names: BTreeSet<String> = BTreeSet::new();

    for mapping in &file_map.mappings {
        let old = mapping.old_path();
        let new = mapping.new_path();

        old_names.insert(old.clone());
        new_names.insert(new.clone());

        if old.is_empty() {
            anyhow::bail!("{}", t!("filemap.empty-old"));
        }
        if new.is_empty() {
            anyhow::bail!("{}", t!("filemap.empty-new"));
        }
        if old == new {
            anyhow::bail!("{}", t!("filemap.same-path", old));
        }

        let old_path = crate::path::resolve_safe_path(old_dir, &old)?;
        let new_path = crate::path::resolve_safe_path(new_dir, &new)?;

        if !old_path.is_file() {
            anyhow::bail!("{}", t!("filemap.old-not-found", old));
        }
        if !new_path.is_file() {
            anyhow::bail!("{}", t!("filemap.new-not-found", new));
        }

        let old_real = std::fs::canonicalize(&old_path)?;
        let new_real = std::fs::canonicalize(&new_path)?;
        if old_real == new_real {
            anyhow::bail!("{}", t!("filemap.same-path", old));
        }
        if !seen_old.insert(old_real) {
            anyhow::bail!("{}", t!("filemap.duplicate-old", old));
        }
        if !seen_new.insert(new_real) {
            anyhow::bail!("{}", t!("filemap.duplicate-new", new));
        }
    }

    // 链式映射（A->B 且 B->C，即同一路径既是源又是目标）无法确定应用顺序，
    // 直接拒绝，要求用户拆分为独立映射。
    if let Some(chain) = old_names.intersection(&new_names).next() {
        anyhow::bail!("{}", t!("filemap.chained", chain));
    }

    Ok(())
}

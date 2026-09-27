//! 显式文件名映射（mapping-aware diff）。
//!
//! 工作目录根目录下的可选文件 `file-map.json` 允许用户手动声明
//! `Old/<old>` 与 `New/<new>` 属于同一个逻辑文件。mapping source 作为差分
//! 基础文件，整包补丁会用它为 target 生成二进制差分（而不是识别为
//! 「删除 + 新增」）。apply / rollback / journal 崩溃恢复会同时维护 source
//! 与 target：source 默认保持原样，仅当 `delete_source=true` 时在 target
//! 校验成功后删除（删除前先备份，可回滚恢复）。
//!
//! 映射完全显式：程序不会根据文件名、basename、扩展名、大小或哈希
//! 自动猜测映射关系。`file-map.json` 不存在时行为与旧版本完全一致。

use crate::t;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

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
    /// 应用成功后是否删除映射源文件。默认 `false`：
    /// 源文件作为保持不变的差分基础文件，apply 后继续保留。
    #[serde(default)]
    pub delete_source: bool,
}

/// 规范化后的映射路径。
///
/// - [`MappingPath::relative`]：用于实际文件访问与 manifest 的规范相对路径
///   （`/` 分隔、已清理 `.` 与重复分隔符、保留原始大小写）
/// - [`MappingPath::key`]：仅用于比较/去重的键（Windows 下大小写不敏感），
///   绝不用于文件访问
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingPath {
    relative: String,
    key: String,
}

impl MappingPath {
    /// 实际文件访问 / manifest 使用的相对路径。
    pub fn relative(&self) -> &str {
        &self.relative
    }

    /// 比较/去重使用的键（不改变实际文件名）。
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn is_empty(&self) -> bool {
        self.relative.is_empty()
    }
}

impl FileMapping {
    /// `Old/` 侧的规范化路径。
    pub fn old_path(&self) -> anyhow::Result<MappingPath> {
        normalize_mapping_path(&self.old)
    }

    /// `New/` 侧的规范化路径。
    pub fn new_path(&self) -> anyhow::Result<MappingPath> {
        normalize_mapping_path(&self.new)
    }
}

/// 将用户输入的映射路径规范化为「实际路径 + 比较键」。
///
/// 规则：
/// - `\` 统一为 `/`
/// - 移除 `.` 段、折叠重复分隔符
/// - 拒绝 `..` 与绝对路径（`/foo`、`C:\foo`、`\\server\share`）
///
/// 比较键在 Windows 下按 ASCII 大小写不敏感生成；实际访问路径保留原始
/// 大小写，并继续由 `resolve_safe_path` 做穿越 / 符号链接防护。
pub fn normalize_mapping_path(raw: &str) -> anyhow::Result<MappingPath> {
    let unified = raw.replace('\\', "/");
    if looks_absolute(&unified) {
        anyhow::bail!("{}", t!("filemap.absolute", raw));
    }

    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => anyhow::bail!("{}", t!("filemap.parent-dir", raw)),
            _ => parts.push(part),
        }
    }

    let relative = parts.join("/");
    let key = comparison_key(&relative);
    Ok(MappingPath { relative, key })
}

/// 判别路径是否为绝对路径（跨平台确定：`/foo`、`C:/foo`、`//server/share`）。
///
/// Windows 盘符前缀在所有平台都拒绝，保证同一份 `file-map.json` 的行为一致；
/// `C:foo`（盘符相对）由 Windows 上的 `resolve_safe_path` 前缀检查兜底。
fn looks_absolute(unified: &str) -> bool {
    if unified.starts_with('/') || Path::new(unified).is_absolute() {
        return true;
    }
    let bytes = unified.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// 统一的大小写不敏感比较键（Windows 下 ASCII 小写化）。
///
/// 仅用于匹配/排除/去重，实际文件访问必须使用真实路径。
#[cfg(windows)]
pub fn comparison_key(relative: &str) -> String {
    relative.to_ascii_lowercase()
}

/// 统一的大小写不敏感比较键（非 Windows 保持原样）。
///
/// 仅用于匹配/排除/去重，实际文件访问必须使用真实路径。
#[cfg(not(windows))]
pub fn comparison_key(relative: &str) -> String {
    relative.to_string()
}

/// 两个路径字符串是否指向同一逻辑文件。
///
/// 使用 [`normalize_mapping_path`] 的规范化结果与比较键判断，
/// 因此 `./foo.bin` 与 `foo.bin`、Windows 下仅大小写不同的路径都会被判定为相同。
pub fn is_same_logical_path(left: &str, right: &str) -> anyhow::Result<bool> {
    let left = normalize_mapping_path(left)?;
    let right = normalize_mapping_path(right)?;
    Ok(left.key() == right.key())
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
/// - 路径规范化（拒绝空路径、`..`、绝对路径）
/// - 同一比较键下 `old == new`、重复 old、重复 new、链式映射
/// - 旧/新文件必须存在
/// - 路径安全（复用 `resolve_safe_path`，拒绝 `../`、绝对路径与符号链接）
pub fn validate_file_map(file_map: &FileMap, old_dir: &Path, new_dir: &Path) -> anyhow::Result<()> {
    let mut normalized: Vec<(MappingPath, MappingPath)> =
        Vec::with_capacity(file_map.mappings.len());
    let mut seen_old: BTreeSet<String> = BTreeSet::new();
    let mut seen_new: BTreeSet<String> = BTreeSet::new();
    let mut old_keys: BTreeSet<String> = BTreeSet::new();
    let mut new_keys: BTreeSet<String> = BTreeSet::new();

    for mapping in &file_map.mappings {
        let old = mapping.old_path()?;
        let new = mapping.new_path()?;

        if old.is_empty() {
            anyhow::bail!("{}", t!("filemap.empty-old"));
        }
        if new.is_empty() {
            anyhow::bail!("{}", t!("filemap.empty-new"));
        }
        if old.key() == new.key() {
            anyhow::bail!("{}", t!("filemap.same-path", old.relative()));
        }
        if !seen_old.insert(old.key().to_string()) {
            anyhow::bail!("{}", t!("filemap.duplicate-old", old.relative()));
        }
        if !seen_new.insert(new.key().to_string()) {
            anyhow::bail!("{}", t!("filemap.duplicate-new", new.relative()));
        }
        old_keys.insert(old.key().to_string());
        new_keys.insert(new.key().to_string());
        normalized.push((old, new));
    }

    // 链式映射（A->B 且 B->C，即同一路径既是源又是目标）无法确定应用顺序，
    // 直接拒绝，要求用户拆分为独立映射。
    if let Some(chain) = old_keys.intersection(&new_keys).next() {
        anyhow::bail!("{}", t!("filemap.chained", chain));
    }

    for (old, new) in &normalized {
        let old_path = crate::path::resolve_safe_path(old_dir, old.relative())?;
        let new_path = crate::path::resolve_safe_path(new_dir, new.relative())?;

        if !old_path.is_file() {
            anyhow::bail!("{}", t!("filemap.old-not-found", old.relative()));
        }
        if !new_path.is_file() {
            anyhow::bail!("{}", t!("filemap.new-not-found", new.relative()));
        }

        // 额外兜底：同一物理文件（大小写不敏感文件系统等），键机制已覆盖，
        // 这里保留 canonicalize 检查。
        let old_real = std::fs::canonicalize(&old_path)?;
        let new_real = std::fs::canonicalize(&new_path)?;
        if old_real == new_real {
            anyhow::bail!("{}", t!("filemap.same-path", old.relative()));
        }
    }

    Ok(())
}

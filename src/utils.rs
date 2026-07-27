use std::path::{Path, PathBuf};
use std::io::Read;
use ring::digest::{Context, SHA256};
use walkdir::WalkDir;

pub fn pause_if_needed() {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return;
    }
    println!("\n按 Enter 键退出...");
    let _ = std::io::stdin().read_line(&mut String::new());
}

pub fn format_size(size_bytes: u64) -> String {
    if size_bytes < 1024 {
        format!("{size_bytes} B")
    } else if size_bytes < 1024 * 1024 {
        format!("{:.2} KB", size_bytes as f64 / 1024.0)
    } else if size_bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", size_bytes as f64 / (1024.0 * 1024.0))
    } else if size_bytes < 1024u64 * 1024 * 1024 * 1024 {
        format!("{:.2} GB", size_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.2} TB", size_bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0))
    }
}

pub fn sha256_of_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut ctx = Context::new(&SHA256);
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        ctx.update(&buffer[..bytes_read]);
    }
    Ok(sha256_hex(ctx.finish()))
}

pub fn sha256_of_bytes(data: &[u8]) -> String {
    let mut ctx = Context::new(&SHA256);
    ctx.update(data);
    sha256_hex(ctx.finish())
}

fn sha256_hex(digest: ring::digest::Digest) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn iter_files(base_dir: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(base_dir)
        .into_iter()
        .filter_map(move |entry| {
            let entry = entry.ok()?;
            if entry.file_type().is_file() {
                Some(entry.path().to_path_buf())
            } else {
                None
            }
        })
}

pub fn relative_dir_map(base_dir: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    let mut dirs = std::collections::BTreeMap::new();
    let base = base_dir.to_path_buf();
    for entry in WalkDir::new(base_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir()
            && let Ok(rel) = entry.path().strip_prefix(&base)
        {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if !rel_str.is_empty() {
                    dirs.insert(rel_str, entry.path().to_path_buf());
                }
            }
    }
    dirs
}

pub fn relative_file_map(base_dir: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    let mut files = std::collections::BTreeMap::new();
    for path in iter_files(base_dir) {
        if let Ok(rel) = path.strip_prefix(base_dir) {
            files.insert(rel.to_string_lossy().replace('\\', "/"), path);
        }
    }
    files
}

pub fn resolve_safe_path(base_dir: &Path, relative_path: &str) -> anyhow::Result<PathBuf> {
    let base_abs = std::path::absolute(base_dir)?;
    let mut result = base_abs.clone();

    for component in Path::new(relative_path).components() {
        match component {
            std::path::Component::ParentDir => {
                if !result.pop() {
                    anyhow::bail!("路径穿越检测: {relative_path} 解析后超出基础目录");
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(c) => result.push(c),
            _ => {
                anyhow::bail!("路径穿越检测: {relative_path} 包含绝对路径组件");
            }
        }
    }

    if result.starts_with(&base_abs) {
        Ok(result)
    } else {
        anyhow::bail!("路径穿越检测: {relative_path} 解析后超出基础目录")
    }
}

pub fn display_path(path: &Path, base_dir: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(base_dir) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().to_string()
    }
}

pub const BACKUP_SUFFIX: &str = ".backup_before_patch";

pub fn backup_root_dir(patch_dir: &Path) -> PathBuf {
    patch_dir.join(".backup_before_patch")
}

pub fn create_backup(target_path: &Path, base_dir: &Path, backup_root: &Path) -> anyhow::Result<PathBuf> {
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("无效的文件路径: {}", target_path.display()))?;

    let rel = target_path.parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));

    let backup_dir = backup_root.join(rel);
    ensure_parent_dir(&backup_dir.join(file_name))?;

    let backup_name = format!("{file_name}{BACKUP_SUFFIX}");
    let mut backup_path = backup_dir.join(&backup_name);

    if backup_path.exists() {
        let timestamp = chrono::Local::now().format(".%Y%m%d%H%M%S");
        backup_path = backup_dir.join(format!("{file_name}{BACKUP_SUFFIX}{timestamp}"));
    }

    std::fs::copy(target_path, &backup_path)?;
    Ok(backup_path)
}

pub fn restore_backup(target_path: &Path, base_dir: &Path, backup_root: &Path) -> anyhow::Result<bool> {
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("无效的文件路径: {}", target_path.display()))?;

    let backup_prefix = format!("{file_name}{BACKUP_SUFFIX}");

    let find_newest = |dir: &Path| -> Option<PathBuf> {
        std::fs::read_dir(dir).ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&backup_prefix))
            })
            .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
    };

    let do_restore = |backup_path: &Path| -> anyhow::Result<bool> {
        ensure_parent_dir(target_path)?;
        std::fs::copy(backup_path, target_path)?;
        std::fs::remove_file(backup_path)?;
        Ok(true)
    };

    // Try new backup location first
    let rel = target_path.parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));
    let backup_dir = backup_root.join(rel);
    if let Some(path) = find_newest(&backup_dir) {
        return do_restore(&path);
    }

    // Fall back to old-style in-place backup (backwards compatibility)
    let parent = target_path.parent().unwrap_or(Path::new("."));
    if let Some(path) = find_newest(parent) {
        return do_restore(&path);
    }

    Ok(false)
}

pub fn copy_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    ensure_parent_dir(dst)?;
    std::fs::copy(src, dst)?;
    Ok(())
}

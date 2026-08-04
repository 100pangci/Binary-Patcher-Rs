use std::path::{Path, PathBuf};

/// Build test workspace with Old/New directories
pub fn build_workspace(base_dir: &Path) -> (PathBuf, PathBuf) {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");

    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();

    // unchanged
    std::fs::write(old_dir.join("same.txt"), "identical").unwrap();
    std::fs::write(new_dir.join("same.txt"), "identical").unwrap();

    // changed
    std::fs::write(old_dir.join("config.ini"), "[section]\nkey=old\n").unwrap();
    std::fs::write(
        new_dir.join("config.ini"),
        "[section]\nkey=new\nport=8080\n",
    )
    .unwrap();

    // binary changed
    std::fs::create_dir_all(old_dir.join("sub")).unwrap();
    std::fs::create_dir_all(new_dir.join("sub")).unwrap();
    let old_bin = vec![0u8; 100];
    let mut new_bin = vec![0xFFu8; 100];
    new_bin.push(0x02);
    std::fs::write(old_dir.join("sub/data.bin"), &old_bin).unwrap();
    std::fs::write(new_dir.join("sub/data.bin"), &new_bin).unwrap();

    // added
    std::fs::write(new_dir.join("new_file.dll"), "new dll content").unwrap();
    std::fs::create_dir_all(new_dir.join("sub")).unwrap();
    std::fs::write(new_dir.join("sub/extra.txt"), "bonus").unwrap();

    // deleted
    std::fs::write(old_dir.join("deprecated.log"), "old log").unwrap();
    std::fs::create_dir_all(old_dir.join("deep/nested")).unwrap();
    std::fs::write(old_dir.join("deep/nested/old_cache.tmp"), [0u8; 10]).unwrap();

    (old_dir, new_dir)
}

pub fn all_file_relpaths(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(root).unwrap();
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if !rel_str.contains("Patch") {
                result.push(rel_str);
            }
        }
    }
    result.sort();
    result
}

pub fn copy_tree_files(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(src).unwrap();
            let out_path = dst.join(rel);
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), &out_path).unwrap();
        }
    }
}

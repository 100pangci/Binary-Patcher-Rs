use std::path::{Path, PathBuf};

fn check_path_traversal(rest: &str, entry_name: &str) {
    for c in std::path::Path::new(rest).components() {
        match c {
            std::path::Component::ParentDir => {
                panic!("ZIP 包包含路径遍历攻击: {entry_name}");
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                panic!("ZIP 包包含绝对路径: {entry_name}");
            }
            _ => {}
        }
    }
}

fn extract_zip_entries(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    root_prefix: &str,
    output_dir: &Path,
) {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .unwrap_or_else(|e| panic!("Failed to read zip entry {i}: {e}"));
        let entry_name = entry.name().replace('\\', "/");
        if let Some(rest) = entry_name.strip_prefix(root_prefix) {
            if rest.is_empty() || rest.ends_with('/') {
                continue;
            }
            check_path_traversal(rest, &entry_name);
            let out_path = output_dir.join(rest);
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p)
                    .unwrap_or_else(|e| panic!("Failed to create directory {}: {e}", p.display()));
            }
            let mut out_file = std::fs::File::create(&out_path)
                .unwrap_or_else(|e| panic!("Failed to create {}: {e}", out_path.display()));
            std::io::copy(&mut entry, &mut out_file)
                .unwrap_or_else(|e| panic!("Failed to extract {}: {e}", entry_name));
        }
    }
}

pub fn download_zlib(version: &str, cache_dir: &Path) -> PathBuf {
    let dir_name = format!("zlib-{version}");
    let zlib_dir = cache_dir.join(&dir_name);
    if zlib_dir.exists() {
        return zlib_dir;
    }
    println!("cargo:warning=Downloading zlib {version}...");
    let url = format!("https://github.com/madler/zlib/archive/refs/tags/v{version}.zip");
    let client = reqwest::blocking::Client::builder()
        .user_agent("BinaryPatcher-BuildScript/2.0")
        .build()
        .expect("Failed to create HTTP client for zlib download");
    let response = client.get(&url).send().expect("Failed to download zlib");
    let bytes = response.bytes().expect("Failed to read zlib archive");

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).expect("Failed to read zlib zip archive");
    let root_prefix = format!("{dir_name}/");
    extract_zip_entries(&mut archive, &root_prefix, &zlib_dir);
    println!(
        "cargo:warning=zlib {version} extracted to {}",
        zlib_dir.display()
    );

    let zlib_h = zlib_dir.join("zlib.h");
    let content = std::fs::read_to_string(&zlib_h)
        .unwrap_or_else(|e| panic!("zlib integrity check: cannot read zlib.h: {e}"));
    if !content.contains("ZLIB_VERSION") {
        panic!("zlib integrity check failed: zlib.h missing ZLIB_VERSION");
    }

    zlib_dir
}

fn get_latest_tag(cache_dir: &Path) -> String {
    let version_file = cache_dir.join("version.txt");
    if let Ok(v) = std::fs::read_to_string(&version_file) {
        let v = v.trim();
        if !v.is_empty() {
            println!("cargo:warning=Using cached HDiffPatch version: {v}");
            return v.to_string();
        }
    }

    let mut client_builder =
        reqwest::blocking::Client::builder().user_agent("BinaryPatcher-BuildScript/2.0");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        let mut h = reqwest::header::HeaderMap::new();
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            h.insert(reqwest::header::AUTHORIZATION, val);
        }
        client_builder = client_builder.default_headers(h);
    }
    let client = client_builder
        .build()
        .expect("Failed to create HTTP client");

    println!("cargo:warning=Fetching latest HDiffPatch release from GitHub API...");
    let mut tag_name: Option<String> = None;

    if let Ok(resp) = client.get(super::HDIFFPATCH_REPO_API).send()
        && let Ok(release) = resp.json::<serde_json::Value>()
    {
        tag_name = release["tag_name"].as_str().map(|s| s.to_string());
    }

    if tag_name.is_none() {
        println!("cargo:warning=API failed, falling back to scraping releases page...");
        if let Ok(resp) = client
            .get("https://github.com/sisong/HDiffPatch/releases/latest")
            .send()
            && let Ok(html) = resp.text()
        {
            for line in html.lines() {
                if let Some(start) = line.find("/sisong/HDiffPatch/releases/tag/v") {
                    let rest = &line[start..];
                    if let Some(end) = rest.find('"') {
                        let tag = rest[..end].rsplit('/').next().unwrap_or("");
                        if !tag.is_empty() {
                            tag_name = Some(tag.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    let tag_name = tag_name.unwrap_or_else(|| {
        panic!(
            "Failed to determine latest HDiffPatch version. \
             Check your network connection or set GITHUB_TOKEN environment variable."
        )
    });

    println!("cargo:warning=Latest HDiffPatch release: {tag_name}");
    std::fs::create_dir_all(cache_dir)
        .unwrap_or_else(|e| panic!("Failed to create cache directory {}: {e}", cache_dir.display()));
    std::fs::write(&version_file, &tag_name)
        .unwrap_or_else(|e| panic!("Failed to write version file {}: {e}", version_file.display()));
    tag_name
}

pub fn download_and_extract(zip_path: &PathBuf, expected_dir: &PathBuf) {
    let cache_dir = expected_dir.parent().unwrap();

    let client = reqwest::blocking::Client::builder()
        .user_agent("BinaryPatcher-BuildScript/2.0")
        .build()
        .expect("Failed to create HTTP client");

    let tag_name = get_latest_tag(cache_dir);

    let download_url =
        format!("https://github.com/sisong/HDiffPatch/archive/refs/tags/{tag_name}.zip");

    println!("cargo:warning=Downloading HDiffPatch {tag_name}...");

    let response = client
        .get(&download_url)
        .send()
        .expect("Failed to download HDiffPatch");

    let zip_bytes = response.bytes().expect("Failed to read response bytes");

    std::fs::create_dir_all(
        zip_path.parent().expect("zip_path must have a parent directory"),
    )
    .expect("Failed to create cache directory");
    std::fs::write(zip_path, &zip_bytes).expect("Failed to save HDiffPatch archive");

    if expected_dir.exists() {
        std::fs::remove_dir_all(expected_dir)
            .unwrap_or_else(|e| panic!("Failed to clear old extraction {}: {e}", expected_dir.display()));
    }

    let cursor = std::io::Cursor::new(zip_bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).expect("Failed to read HDiffPatch zip archive");

    let archive_version = tag_name.strip_prefix('v').unwrap_or(&tag_name);
    let root_prefix = format!("HDiffPatch-{archive_version}/");
    extract_zip_entries(&mut archive, &root_prefix, expected_dir);

    let check_file = expected_dir
        .join("libHDiffPatch")
        .join("HPatch")
        .join("patch.h");
    if !check_file.exists() {
        panic!(
            "HDiffPatch extraction failed: {} not found",
            check_file.display()
        );
    }

    println!(
        "cargo:warning=HDiffPatch {tag_name} extracted to {}",
        expected_dir.display()
    );
}

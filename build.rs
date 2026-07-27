use std::path::{Path, PathBuf};

const HDIFFPATCH_REPO_API: &str = "https://api.github.com/repos/sisong/HDiffPatch/releases/latest";

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

fn download_zlib(version: &str, cache_dir: &Path) -> PathBuf {
    let dir_name = format!("zlib-{version}");
    let zlib_dir = cache_dir.join(&dir_name);
    if zlib_dir.exists() {
        return zlib_dir;
    }
    println!("cargo:warning=Downloading zlib {version}...");
    let url = format!("https://github.com/madler/zlib/archive/refs/tags/v{version}.zip");
    let client = reqwest::blocking::Client::builder()
        .user_agent("BinaryPatcher-BuildScript/2.0")
        .build().unwrap();
    let response = client.get(&url).send().expect("Failed to download zlib");
    let bytes = response.bytes().expect("Failed to read zlib archive");

    // Extract zip
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(cursor).expect("Failed to read zlib zip archive");
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().replace('\\', "/");
        let root_prefix = format!("{dir_name}/");
        if let Some(rest) = name.strip_prefix(&root_prefix) {
            if rest.is_empty() || rest.ends_with('/') { continue; }
            check_path_traversal(rest, &name);
            let out_path = zlib_dir.join(rest);
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).ok();
            }
            let mut out_file = std::fs::File::create(&out_path)
                .unwrap_or_else(|e| panic!("Failed to create {}: {e}", out_path.display()));
            std::io::copy(&mut entry, &mut out_file).unwrap();
        }
    }
    println!("cargo:warning=zlib {version} extracted to {}", zlib_dir.display());

    // Integrity check: verify zlib.h exists and contains ZLIB_VERSION
    let zlib_h = zlib_dir.join("zlib.h");
    let content = std::fs::read_to_string(&zlib_h)
        .unwrap_or_else(|e| panic!("zlib integrity check: cannot read zlib.h: {e}"));
    if !content.contains("ZLIB_VERSION") {
        panic!("zlib integrity check failed: zlib.h missing ZLIB_VERSION");
    }

    zlib_dir
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let cache_dir = manifest_dir.join("target").join(".hdiffpatch-cache");
    let hd_path = cache_dir.join("src");
    let zip_path = cache_dir.join("hdiffpatch.zip");

    if !hd_path.exists() {
        download_and_extract(&zip_path, &hd_path);
    }

    // Download and compile zlib
    let zlib_version = "1.3.1";
    let zlib_dir = download_zlib(zlib_version, &cache_dir);

    let mut zlib_build = cc::Build::new();
    zlib_build.define("NDEBUG", None);
    zlib_build.opt_level(3);
    zlib_build.include(&zlib_dir);
    for f in &["adler32", "compress", "crc32", "deflate", "inflate",
               "inftrees", "inffast", "trees", "uncompr", "zutil"] {
        zlib_build.file(zlib_dir.join(format!("{f}.c")));
    }
    // Define NO_GZCOMPRESS and similar to reduce size
    zlib_build.define("NO_GZCOMPRESS", None);
    zlib_build.define("NO_GZIP", None);
    zlib_build.compile("zlib");

    let src_dir = hd_path.join("libHDiffPatch");
    let parallel_dir = hd_path.join("libParallel");

    let includes = &[
        &hd_path, &src_dir, &src_dir.join("HDiff"), &src_dir.join("HPatch"),
        &src_dir.join("HPatch").join("hpatch_mt"), &src_dir.join("HPatchLite"),
        &parallel_dir, &hd_path.join("dirDiffPatch"),
        &hd_path.join("bsdiff_wrapper"), &hd_path.join("vcdiff_wrapper"),
        &zlib_dir,
    ];

    // Compile C files (not C++)
    let mut c_build = cc::Build::new();
    c_build.define("NDEBUG", None);          // disable assert()
    c_build.define("_IS_RUN_MEM_SAFE_CHECK", "0"); // disable runtime bounds checks
    c_build.define("_IS_OUT_DIFF_INFO", "0"); // suppress HDiffPatch progress logs
    c_build.opt_level(3);                    // -O3 (upstream default)
    for inc in includes {
        c_build.include(inc);
    }
    c_build.include("vendor/hdiffpatch-sys");
    if !cfg!(windows) {
        c_build.flag("-pthread");
    }

    c_build.file(src_dir.join("HPatch").join("patch.c"));
    c_build.file(src_dir.join("HPatchLite").join("hpatch_lite.c"));
    c_build.file(hd_path.join("file_for_patch.c"));
    c_build.file(src_dir.join("HDiff").join("private_diff").join("limit_mem_diff").join("adler_roll.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("_hpatch_mt.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("_houtput_mt.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("_hinput_mt.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("_hcache_window_old_mt.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("_hcache_old_mt.c"));
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("hpatch_mt.c"));
    c_build.file(parallel_dir.join("parallel_import_c.c"));
    c_build.compile("hdiffpatch_c");

    // Compile C++ files
    let mut cpp_build = cc::Build::new();
    cpp_build.define("NDEBUG", None);
    cpp_build.define("_IS_RUN_MEM_SAFE_CHECK", "0");
    cpp_build.define("_IS_OUT_DIFF_INFO", "0"); // suppress HDiffPatch progress logs
    cpp_build.opt_level(3);
    for inc in includes {
        cpp_build.include(inc);
    }
    cpp_build.include("vendor/hdiffpatch-sys");
    cpp_build.cpp(true);
    cpp_build.flag_if_supported("-std=c++11");
    cpp_build.flag_if_supported("/std:c++11");
    if !cfg!(windows) {
        cpp_build.flag("-pthread");
    }
    // Define zlib compression plugin for the wrapper
    cpp_build.define("_CompressPlugin_zlib", None);

    cpp_build.file(src_dir.join("HDiff").join("diff.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("suffix_string.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("bytes_rle.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("compress_detect.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("match_block.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("match_inplace.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("limit_mem_diff").join("digest_matcher.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("limit_mem_diff").join("stream_serialize.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("window_diff").join("window_matcher.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("window_diff").join("covers_range.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("libdivsufsort").join("divsufsort.cpp"));
    cpp_build.file(src_dir.join("HDiff").join("private_diff").join("libdivsufsort").join("divsufsort64.cpp"));
    cpp_build.file(parallel_dir.join("parallel_channel.cpp"));
    cpp_build.file(hd_path.join("compress_parallel.cpp"));
    cpp_build.file(Path::new("vendor").join("hdiffpatch-sys").join("hdiffpatch_wrapper.cpp"));
    cpp_build.compile("hdiffpatch_cpp");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/hdiffpatch-sys/hdiffpatch_wrapper.cpp");
    println!("cargo:rerun-if-changed=vendor/hdiffpatch-sys/hdiffpatch_wrapper.h");
}

fn get_latest_tag(cache_dir: &Path) -> String {
    let version_file = cache_dir.join("version.txt");

    // Use cached version if available
    if let Ok(v) = std::fs::read_to_string(&version_file) {
        let v = v.trim();
        if !v.is_empty() {
            println!("cargo:warning=Using cached HDiffPatch version: {v}");
            return v.to_string();
        }
    }

    // Build authenticated client (uses GITHUB_TOKEN in CI for higher rate limits)
    let mut client_builder = reqwest::blocking::Client::builder()
        .user_agent("BinaryPatcher-BuildScript/2.0");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        let mut h = reqwest::header::HeaderMap::new();
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            h.insert(reqwest::header::AUTHORIZATION, val);
        }
        client_builder = client_builder.default_headers(h);
    }
    let client = client_builder.build().expect("Failed to create HTTP client");

    // Try GitHub API to find latest release tag
    println!("cargo:warning=Fetching latest HDiffPatch release from GitHub API...");
    let mut tag_name: Option<String> = None;

    if let Ok(resp) = client.get(HDIFFPATCH_REPO_API).send()
        && let Ok(release) = resp.json::<serde_json::Value>()
    {
        tag_name = release["tag_name"].as_str().map(|s| s.to_string());
    }

    // Fallback: scrape the releases page HTML for the latest tag
    if tag_name.is_none() {
        println!("cargo:warning=API failed, falling back to scraping releases page...");
        if let Ok(resp) = client
            .get("https://github.com/sisong/HDiffPatch/releases/latest")
            .send()
            && let Ok(html) = resp.text()
        {
            // Look for /sisong/HDiffPatch/releases/tag/vX.Y.Z in the HTML
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
    std::fs::create_dir_all(cache_dir).ok();
    std::fs::write(&version_file, &tag_name).ok();
    tag_name
}

fn download_and_extract(zip_path: &PathBuf, expected_dir: &PathBuf) {
    let cache_dir = expected_dir.parent().unwrap();

    let client = reqwest::blocking::Client::builder()
        .user_agent("BinaryPatcher-BuildScript/2.0")
        .build()
        .expect("Failed to create HTTP client");

    let tag_name = get_latest_tag(cache_dir);

    let download_url = format!(
        "https://github.com/sisong/HDiffPatch/archive/refs/tags/{tag_name}.zip"
    );

    println!("cargo:warning=Downloading HDiffPatch {tag_name}...");

    let response = client
        .get(&download_url)
        .send()
        .expect("Failed to download HDiffPatch");

    let zip_bytes = response
        .bytes()
        .expect("Failed to read response bytes");

    std::fs::create_dir_all(zip_path.parent().unwrap())
        .expect("Failed to create cache directory");
    std::fs::write(zip_path, &zip_bytes)
        .expect("Failed to save HDiffPatch archive");

    // Clear expected_dir if it exists
    if expected_dir.exists() {
        std::fs::remove_dir_all(expected_dir).ok();
    }

    let cursor = std::io::Cursor::new(&zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .expect("Failed to read HDiffPatch zip archive");

    // GitHub strips the "v" prefix from tag names in archive root directory names.
    // e.g. tag "v5.1.0" produces archive root "HDiffPatch-5.1.0/"
    let archive_version = tag_name.strip_prefix('v').unwrap_or(&tag_name);
    let root_prefix = format!("HDiffPatch-{archive_version}/");

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let entry_name = entry.name().to_string();
        let entry_name_norm = entry_name.replace('\\', "/");

        if let Some(rest) = entry_name_norm.strip_prefix(&root_prefix) {
            if rest.is_empty() || rest.ends_with('/') { continue; }
            check_path_traversal(rest, &entry_name_norm);
            let out_path = expected_dir.join(rest);
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).ok();
            }
            let mut out_file = std::fs::File::create(&out_path)
                .unwrap_or_else(|e| panic!("Failed to create {}: {e}", out_path.display()));
            std::io::copy(&mut entry, &mut out_file)
                .unwrap_or_else(|e| panic!("Failed to extract {}: {e}", entry_name_norm));
        }
    }

    // Verify extraction
    let check_file = expected_dir.join("libHDiffPatch").join("HPatch").join("patch.h");
    if !check_file.exists() {
        panic!("HDiffPatch extraction failed: {} not found", check_file.display());
    }

    println!("cargo:warning=HDiffPatch {tag_name} extracted to {}", expected_dir.display());
}

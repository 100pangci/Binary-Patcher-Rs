# Binary Patcher

[中文](README.md) | [日本語](README.ja.md)

---

A tool for creating and applying binary patches with full-directory workflow support.
Powered by [HDiffPatch](https://github.com/sisong/HDiffPatch) via FFI static linking — the C library is downloaded and compiled automatically at build time.

## Features

- **Single-file patch** — create / apply a patch between two files
- **Directory bundle** — compare `Old/` vs `New/`, auto-generate `manifest.json` + patches + new files
- **File-name mappings** — declare `Old/A -> New/B` in an optional `file-map.json` to diff renamed files instead of delete + add
- **One-click apply** — `apply_patch` reads the manifest, verifies SHA256, backs up originals, applies patches
- **One-click rollback** — `rollback_patch` restores backups and removes added files
- **Adaptive memory/streaming** — `--mode auto` tries in-memory first, auto-falls back to streaming per file on OOM
- **Low-memory streaming** — `--mode stream` forces file-stream mode for diff creation, reducing memory for large files
- **Safety guarantees**:
  - Path traversal protection (`../` blocked)
  - SHA256 verification before and after patching
  - Automatic rollback on verification failure
  - Timestamped backups (no silent overwrite)
  - Manifest format validation

## Binaries

| Binary | Purpose |
|--------|---------|
| `binary_patcher` | Create patches (single-file and directory bundle) |
| `apply_patch` | Apply a patch bundle to a target directory |
| `rollback_patch` | Roll back a previously applied patch bundle |

## Installation

### Build from source

```sh
git clone https://github.com/100pangci/binary_patcher.git
cd binary_patcher
cargo build --release
```

The build automatically downloads and statically links the HDiffPatch C library — no extra dependencies required.
The compiled binaries are located at `target/release/`.

### Packaging

Run `scripts/build.ps1` to build and package into `Releases/binary_patcher_toolkit.zip`:

```powershell
.\scripts\build.ps1
```

## Quick Start

### 1. Generate a directory patch bundle

Directory layout:

```
Old/          ← place the old version here
New/          ← place the new version here
Patch/        ← created automatically
```

**First run:**

```sh
binary_patcher
```

The tool creates `Old/`, `New/`, `Patch/` directories. Populate `Old/` with the old version and `New/` with the new version.

**Second run:**

```sh
binary_patcher
```

The tool scans `Old/` and `New/`, computes SHA256 for every file, compares them, and generates:

- `Patch/manifest.json` — change manifest
- `Patch/**/*.patch` — binary patches for changed files
- `Patch/**/*.new` — copies of new files
- `Patch/README.txt` — instructions for end users

### 2. Apply a patch bundle

```
old-version root/
├── apply_patch
├── Patch/
│   ├── manifest.json
│   ├── ... .patch
│   └── ... .new
```

```sh
./apply_patch
```

The tool:

1. Validates each file against `old_sha256`
2. Backs up originals as `*.backup_before_patch`
3. Applies patches via the HDiffPatch engine
4. Verifies output against `new_sha256`
5. Copies new files, deletes removed files

### 3. Roll back

```sh
./rollback_patch
```

Restores `*.backup_before_patch` backups and removes files that were added by the patch.

### 4. File-name mappings (rename-aware diff)

When the same logical file uses a different name or extension in the old and new versions,
the default behavior treats it as "delete the old file + add the new file", and the new file
is copied in full into the patch bundle. To make the tool treat two different paths as the
same logical file and produce a binary diff instead, place an optional `file-map.json` in the
workspace root and declare the mapping explicitly:

```text
Old/
└── data/
    └── package.bin

New/
└── data/
    └── package_v2.dat

file-map.json
```

```json
{
  "mappings": [
    {
      "old": "data/package.bin",
      "new": "data/package_v2.dat"
    }
  ]
}
```

This makes Binary Patcher diff the two paths as one logical file rather than reporting the
old file as deleted and the new file as added.

> A mapping only designates an **unchanged diff base file**; the source is preserved by
> default. This is a cross-name diff base mapping, not a destructive filesystem rename.
> （文件映射只指定差分基础文件，源文件默认会保留，并不是破坏性的文件重命名。）

The generated manifest entry looks like:

```json
{
  "path": "data/package_v2.dat",
  "source_path": "data/package.bin",
  "old_sha256": "...",
  "new_sha256": "...",
  "patch_file": "data/package_v2.dat.patch"
}
```

With the default `delete_source: false`:

- On apply: the mapping source is used **only as the diff input and is never modified,
  backed up or deleted**; the target is produced from the diff (an existing target is backed
  up first) and verified against SHA256.
- On rollback: the target is removed, or its original content restored if it existed before
  apply. The source file is always left untouched.
- Mapped source/target paths are **excluded entirely** from the ordinary scan: a mapping-source
  file that remains in `New/` is treated as unchanged, is never copied in full as an added
  file, and still exists after apply.

To remove the source after a successful apply (classic rename behavior), set the flag on the
mapping explicitly:

```json
{
  "mappings": [
    {
      "old": "data/package.bin",
      "new": "data/package_v2.dat",
      "delete_source": true
    }
  ]
}
```

- With `delete_source: true`, apply generates the target from the diff and verifies its
  SHA256 first; **the source is deleted only after verification succeeds**. The source is
  backed up before deletion, so auto-rollback and rollback_patch can always restore it.
- `delete_source: true` requires that `New/` does not contain the same source path (otherwise
  the applied result could not match `New/`; the bundle fails with an error).
- Missing or omitted `delete_source` is treated as `false`, compatible with old
  `file-map.json` files and old manifests.
- Plain same-path changed entries cannot use `delete_source: true`.

Other rules:

- If the mapping target already exists in `Old/`, its content is overwritten by the mapping
  result (backed up before apply, restored on rollback) instead of being misreported as deleted.
- Identical content with only a different name is supported too (a minimal diff patch is generated).
- Mappings are **never guessed**: the tool does not infer them from file names, extensions,
  sizes or hashes.
- Paths are relative to `Old/` and `New/`, using `/` separators (Windows `\` is normalized;
  `./` and repeated separators are folded).
- Validation is strict: both files must exist; one old file cannot map to multiple new files
  and multiple old files cannot map to one new file; `old == new` (including after
  normalization, or case-only differences on Windows) is rejected with a hint to remove the
  entry; chained mappings (A→B and B→C) are rejected.
- If `New/` contains a file with the same path as a mapping source, its content must be
  identical to `Old/`, otherwise the bundle fails (a mapping source is an unchanged diff base).
- Path safety reuses the existing checks: `../`, absolute paths, symlinks and escapes are rejected.
- Without `file-map.json`, behavior is exactly the same as before (opt-in).

## CLI Reference

### `binary_patcher`

| Command | Description |
|---------|-------------|
| *(no arguments)* | Workspace mode: init `Old/`/`New/`/`Patch/`, then build bundle |
| `create <old> <new> <patch>` | Create a single patch file from two files |
| `apply <old> <patch> <output>` | Apply a single patch file |
| `bundle --base-dir <path>` | Build a bundle using a specific workspace directory |
| `--mode auto/stream/memory` | Diff mode: `auto` automatic (default), `stream` low-memory streaming, `memory` all-in-memory best quality |
| `--format precise/fast` | Diff algorithm: `precise` suffix-string (smaller patch, default), `fast` hash-based (faster) |

### `apply_patch`

| Argument | Description |
|----------|-------------|
| `--base-dir <path>` | Root directory of the old version (must contain `Patch/`), defaults to `.` |

```sh
./apply_patch
```

### `rollback_patch`

| Argument | Description |
|----------|-------------|
| `--base-dir <path>` | Root directory of the old version (must contain `Patch/`), defaults to `.` |

```sh
./rollback_patch
```

## Project Structure

```
.
├── build.rs                 # Build entry (delegates to build_script/)
├── build_script/            # Build modules
│   ├── mod.rs               # Build orchestration
│   ├── download.rs          # Auto-download HDiffPatch / zlib (inflate only)
│   └── compile.rs           # C/C++ compilation
├── e2e.ps1                  # End-to-end CLI smoke test (Windows)
├── e2e.sh                   # End-to-end CLI smoke test (Linux)
├── LICENSE                  # MPL-2.0
├── README.md                # 中文
├── README.en.md             # English
├── README.ja.md             # 日本語
├── .github/workflows/
│   ├── ci.yml               # CI: cargo check (Linux) + test (multi-platform)
│   └── build.yml            # Release: build → package → GitHub Release
├── scripts/
│   ├── build.ps1            # Windows one-click build + package
│   └── gen_test_data.ps1    # Test data generator
├── vendor/
│   └── hdiffpatch-sys/      # HDiffPatch C/C++ wrapper code
├── Cargo.toml
├── src/
│   ├── lib.rs               # Library root, re-exports all modules
│   ├── main.rs              # binary_patcher entry point
│   ├── backup.rs            # File backup and restore
│   ├── bin/
│   │   ├── apply_patch.rs   # apply_patch entry point
│   │   └── rollback_patch.rs# rollback_patch entry point
│   ├── cli.rs               # CLI argument parsing (clap)
│   ├── ffi.rs               # HDiffPatch C library FFI bindings
│   ├── file_map.rs          # file-map.json loading and explicit mapping validation
│   ├── fmt.rs               # Formatting utilities (file size, terminal pause)
│   ├── fs.rs                # Filesystem traversal and mapping
│   ├── hash.rs              # SHA256 hashing
│   ├── hdiffpatch.rs        # Patch create/apply invocation wrapper
│   ├── manifest.rs          # Manifest type, JSON serialization, validation
│   ├── path.rs              # Safe path resolution and traversal protection
│   ├── bundle.rs            # Bundle creation (Old/New → Patch)
│   ├── apply.rs             # Bundle application logic
│   └── rollback.rs          # Bundle rollback logic
└── tests/
    ├── common/mod.rs       # Shared test helpers (workspace setup, file walk, tree copy)
    ├── unit_fmt.rs         # format_size unit tests
    ├── unit_hash.rs        # SHA256 unit tests
    ├── unit_path.rs        # Safe path resolution unit tests
    ├── unit_fs.rs          # Filesystem traversal/mapping unit tests
    ├── unit_file_map.rs    # file-map.json loading/validation unit tests
    ├── unit_manifest.rs    # Manifest validation/loading unit tests
    ├── unit_backup.rs      # Backup/restore unit tests
    ├── workflow.rs         # End-to-end integration tests
    └── workflow_mapping.rs # File-name mapping end-to-end integration tests
```

## Security

| Feature | Description |
|---------|-------------|
| **Path traversal protection** | All manifest paths are validated; `../` escape attempts are rejected |
| **Manifest validation** | Schema, field types, and format version are verified on load; duplicate patch resources are rejected |
| **Mapping validation** | `file-map.json` enforces one-to-one mappings, file existence and path safety; mappings are never guessed |
| **SHA256 verification** | Files are hashed before and after patching; mismatches trigger automatic rollback |
| **Safe backups** | Backups use `.backup_before_patch` suffix; existing backups get a timestamp suffix |

## Development

### Prerequisites

- Rust 2024 edition (MSRV 1.85+)

### Commands

```sh
# Build
cargo build

# Run all tests (unit + integration)
cargo test

# Run only the end-to-end integration tests with verbose output
cargo test --test workflow -- --nocapture

# Release build
cargo build --release
```

### Windows one-click build

```powershell
.\scripts\build.ps1
```

The script:
1. Runs `cargo build --release` to compile all three binaries (build.rs auto-downloads & compiles HDiffPatch C library)
2. Packages everything into `Releases/binary_patcher_toolkit.zip`

### CI / CD

This project uses GitHub Actions:

| Workflow | Trigger | Contents |
|----------|---------|----------|
| **CI** | push / PR | `cargo check` (Linux) + `cargo test` (Windows / Linux / macOS) |
| **Build & Release** | tag `v*` / manual | `cargo build --release` → download HDiffPatch tools → package → publish to GitHub Release |

### TODO

- [x] Provide pre-built binary downloads

## Technical Stack

| Area | Choice |
|------|--------|
| Language | Rust (edition 2024) |
| CLI framework | clap (derive) |
| Serialization | serde + serde_json |
| Hashing | SHA-256 (ring + hex, assembly-optimized) |
| Directory walk | walkdir |
| Time handling | chrono |
| TTY detection | std::io::IsTerminal (stdlib) |
| Error handling | anyhow |
| Build deps | cc (C/C++ compile), reqwest + zip (auto-download HDiffPatch) |
| Patch engine | [HDiffPatch](https://github.com/sisong/HDiffPatch) (FFI static link) |

## License

Licensed under the [Mozilla Public License 2.0](LICENSE).

## Acknowledgements

- [HDiffPatch](https://github.com/sisong/HDiffPatch) — the binary diff / patch engine
- The original [binary_patcher](https://github.com/100pangci/binary_patcher) Python project

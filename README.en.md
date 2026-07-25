# Binary Patcher

[中文](README.md) | [日本語](README.ja.md)

---

A tool for creating and applying binary patches with full-directory workflow support.
Powered by [HDiffPatch](https://github.com/sisong/HDiffPatch) via FFI static linking — the C library is downloaded and compiled automatically at build time.

## Features

- **Single-file patch** — create / apply a patch between two files
- **Directory bundle** — compare `Old/` vs `New/`, auto-generate `manifest.json` + patches + new files
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

## CLI Reference

### `binary_patcher`

| Command | Description |
|---------|-------------|
| *(no arguments)* | Workspace mode: init `Old/`/`New/`/`Patch/`, then build bundle |
| `create <old> <new> <patch>` | Create a single patch file from two files |
| `apply <old> <patch> <output>` | Apply a single patch file |
| `bundle --base-dir <path>` | Build a bundle using a specific workspace directory |
| `--no-compress` | Disable patch compression (default: zlib compression enabled) |
| `--mode auto/stream/memory` | Diff mode: `auto` automatic (default), `stream` low-memory streaming, `memory` all-in-memory best quality |

### `apply_patch`

```sh
./apply_patch
```

### `rollback_patch`

```sh
./rollback_patch
```

## Project Structure

```
.
├── build.rs                 # Build script: auto-download & compile HDiffPatch C library
├── .github/workflows/
│   ├── ci.yml               # CI: cargo check + test (multi-platform)
│   └── build.yml            # Release: lint → test → build → GitHub Release
├── scripts/
│   ├── build.ps1            # Windows one-click build + package
│   └── gen_test_data.ps1    # Test data generator
├── vendor/
│   └── hdiffpatch-sys/      # HDiffPatch C/C++ wrapper code
├── Cargo.toml
├── src/
│   ├── lib.rs               # Library root, re-exports all modules
│   ├── main.rs              # binary_patcher entry point
│   ├── bin/
│   │   ├── apply_patch.rs   # apply_patch entry point
│   │   └── rollback_patch.rs# rollback_patch entry point
│   ├── cli.rs               # CLI argument parsing (clap)
│   ├── ffi.rs               # HDiffPatch C library FFI bindings
│   ├── hdiffpatch.rs        # Patch create/apply invocation wrapper
│   ├── utils.rs             # SHA256, file ops, path safety, backup
│   ├── manifest.rs          # Manifest type, JSON serialization, validation
│   ├── bundle.rs            # Bundle creation (Old/New → Patch)
│   ├── apply.rs             # Bundle application logic
│   └── rollback.rs          # Bundle rollback logic
└── tests/
    └── integration_test.rs  # Unit + full-workflow integration tests
```

## Security

| Feature | Description |
|---------|-------------|
| **Path traversal protection** | All manifest paths are validated; `../` escape attempts are rejected |
| **Manifest validation** | Schema, field types, and format version are verified on load |
| **SHA256 verification** | Files are hashed before and after patching; mismatches trigger automatic rollback |
| **Safe backups** | Backups use `.backup_before_patch` suffix; existing backups get a timestamp suffix |

## Development

### Prerequisites

- Rust 2024 edition (MSRV 1.85+)

### Commands

```sh
# Build
cargo build

# Run tests
cargo test

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
| **CI** | push / PR | `cargo check` + `cargo test` (Windows / Linux / macOS) |
| **Build & Release** | tag `v*` / manual | check → test → `cargo build --release` → download HDiffPatch → package → publish to GitHub Release |

### TODO

- [x] Provide pre-built binary downloads

## Technical Stack

| Area | Choice |
|------|--------|
| Language | Rust (edition 2024) |
| CLI framework | clap (derive) |
| Serialization | serde + serde_json |
| Hashing | SHA-256 (ring crate, assembly-optimized) |
| Directory walk | walkdir |
| Time handling | chrono |
| TTY detection | atty |
| Error handling | anyhow |
| Build deps | cc (C/C++ compile), reqwest + zip (auto-download HDiffPatch) |
| Patch engine | [HDiffPatch](https://github.com/sisong/HDiffPatch) (FFI static link) |

## License

Licensed under the [Mozilla Public License 2.0](LICENSE).

## Acknowledgements

- [HDiffPatch](https://github.com/sisong/HDiffPatch) — the binary diff / patch engine
- The original [binary_patcher](https://github.com/100pangci/binary_patcher) Python project

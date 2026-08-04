# Binary Patcher

[English](README.en.md) | [日本語](README.ja.md)

---

一个用于生成和应用二进制补丁的工具，支持整目录补丁工作流。
底层补丁引擎使用 [HDiffPatch](https://github.com/sisong/HDiffPatch)，通过 FFI 静态链接 C 库，构建时自动下载编译。

## 功能

- **单文件补丁** — 对两个文件生成/应用补丁
- **整目录打包** — 对比 `Old/` 与 `New/`，自动生成 `manifest.json` + 补丁文件 + 新增文件
- **一键应用** — `apply_patch` 读取清单、校验 SHA256、备份原文件、执行补丁
- **一键回滚** — `rollback_patch` 恢复备份、删除新增文件
- **自适应内存/流式** — `--mode auto` 优先内存模式，OOM 时按文件自动回退流式
- **低内存流式** — `--mode stream` 强制流式模式，降低内存占用，适合大文件或内存受限环境
- **安全保障**：
  - 路径穿越防护（拒绝 `../` 逃逸）
  - 补丁前后 SHA256 校验
  - 校验失败自动回滚
  - 备份文件使用时间戳后缀（不静默覆盖）
  - Manifest 格式校验

## 二进制文件

| 文件 | 用途 |
|------|------|
| `binary_patcher` | 创建补丁（单文件和整目录打包） |
| `apply_patch` | 将补丁包应用到目标目录 |
| `rollback_patch` | 回滚已应用的补丁包 |

## 安装

### 从源码编译

```sh
git clone https://github.com/100pangci/binary_patcher.git
cd binary_patcher
cargo build --release
```

编译自动下载 HDiffPatch C 库并静态链接，无需额外依赖。可执行文件位于 `target/release/`。

### 预编译包

运行 `scripts/build.ps1` 可一键构建并打包为 `Releases/binary_patcher_toolkit.zip`：

```powershell
.\scripts\build.ps1
```

## 快速开始

### 1. 生成整目录补丁

准备目录结构：

```
Old/          ← 放入旧版本
New/          ← 放入新版本
Patch/        ← 自动生成
```

**首次运行：**

```sh
binary_patcher
```

程序自动创建 `Old/`、`New/`、`Patch/` 目录。将旧版本文件放入 `Old/`，新版本文件放入 `New/`。

**再次运行：**

```sh
binary_patcher
```

程序扫描 `Old/` 和 `New/`，计算每个文件的 SHA256，对比后生成：

- `Patch/manifest.json` — 变更清单
- `Patch/**/*.patch` — 变更文件的二进制补丁
- `Patch/**/*.new` — 新增文件的副本
- `Patch/README.txt` — 使用说明

### 2. 应用整包补丁

```
旧版本根目录/
├── apply_patch
├── Patch/
│   ├── manifest.json
│   ├── ... .patch
│   └── ... .new
```

```sh
./apply_patch
```

程序会：

1. 校验每个文件是否匹配 `old_sha256`
2. 将原文件备份为 `*.backup_before_patch`
3. 通过 HDiffPatch 引擎应用补丁
4. 验证输出是否匹配 `new_sha256`
5. 复制新增文件，删除已移除的文件

### 3. 回滚补丁

```sh
./rollback_patch
```

恢复 `*.backup_before_patch` 备份文件，删除补丁新增的文件。

## CLI 参考

### `binary_patcher`

| 命令 | 说明 |
|------|------|
| （无参数） | 工作区模式：初始化 `Old/`/`New/`/`Patch/`，然后打包 |
| `create <旧文件> <新文件> <补丁文件>` | 对两个文件创建单个补丁 |
| `apply <旧文件> <补丁文件> <输出文件>` | 应用单个补丁 |
| `bundle --base-dir <路径>` | 指定工作目录执行打包 |
| `--no-compress` | 禁用补丁压缩（默认启用 zlib 压缩） |
| `--mode auto/stream/memory` | 补丁创建模式：`auto` 自动选择（默认）、`stream` 流式低内存、`memory` 全加载最优 |
| `--format precise/fast` | 差分算法：`precise` suffix-string（补丁更小，默认）、`fast` hash（速度更快） |

### `apply_patch`

| 参数 | 说明 |
|------|------|
| `--base-dir <路径>` | 旧版本根目录，默认为当前目录（需包含 `Patch/`） |

### `rollback_patch`

| 参数 | 说明 |
|------|------|
| `--base-dir <路径>` | 旧版本根目录，默认为当前目录（需包含 `Patch/`） |

## 项目结构

```
.
├── build.rs                 # 构建入口（委托 build_script/）
├── build_script/            # 构建模块
│   ├── mod.rs               # 构建编排
│   ├── download.rs          # 自动下载 HDiffPatch / zlib
│   └── compile.rs           # C/C++ 编译
├── e2e.ps1                  # 端到端 CLI 冒烟测试
├── LICENSE                  # MPL-2.0 许可证
├── README.md                # 中文说明
├── README.en.md             # English
├── README.ja.md             # 日本語
├── .github/workflows/
│   ├── ci.yml               # CI: cargo check（Linux）+ test（多平台）
│   └── build.yml            # Release: 构建 → 打包 → GitHub Release
├── scripts/
│   ├── build.ps1            # Windows 一键构建 + 打包
│   └── gen_test_data.ps1    # 测试数据生成脚本
├── vendor/
│   └── hdiffpatch-sys/      # HDiffPatch C/C++ 包装代码
├── Cargo.toml               # 含 [lints] 配置（clippy/rustc 检查）
├── src/
│   ├── lib.rs               # 库入口，公开所有模块
│   ├── main.rs              # binary_patcher 入口
│   ├── backup.rs            # 文件备份与恢复
│   ├── bin/
│   │   ├── apply_patch.rs   # apply_patch 入口
│   │   └── rollback_patch.rs# rollback_patch 入口
│   ├── cli.rs               # 命令行参数解析（clap）
│   ├── ffi.rs               # HDiffPatch C 库 FFI 绑定
│   ├── fmt.rs               # 格式化工具（文件大小、终端暂停）
│   ├── fs.rs                # 文件系统遍历与映射
│   ├── hash.rs              # SHA256 哈希计算
│   ├── hdiffpatch.rs        # 补丁创建/应用调用封装
│   ├── manifest.rs          # Manifest 类型、JSON 序列化、校验
│   ├── path.rs              # 安全路径解析与穿越防护
│   ├── bundle.rs            # 整目录打包（Old/New → Patch）
│   ├── apply.rs             # 补丁应用逻辑
│   └── rollback.rs          # 补丁回滚逻辑
└── tests/
    ├── common/mod.rs       # 测试公共辅助（工作区构建、文件遍历、目录拷贝）
    ├── unit_fmt.rs         # format_size 单元测试
    ├── unit_hash.rs        # SHA256 单元测试
    ├── unit_path.rs        # 安全路径解析单元测试
    ├── unit_fs.rs          # 文件系统遍历与映射单元测试
    ├── unit_manifest.rs    # Manifest 校验/加载单元测试
    ├── unit_backup.rs      # 备份/恢复单元测试
    └── workflow.rs         # 端到端集成测试（39 项）
```

## 安全

| 特性 | 说明 |
|------|------|
| **路径穿越防护** | 所有 manifest 中的路径均经过校验，拒绝 `../` 逃逸 |
| **Manifest 校验** | 加载时验证字段完整性和类型，拒绝格式错误的清单 |
| **SHA256 校验** | 补丁前后均校验文件完整性，失败自动回滚 |
| **安全备份** | 备份文件使用 `.backup_before_patch` 后缀，已存在时追加时间戳 |

## 开发

### 环境要求

- Rust 2024 edition（最低支持 1.85+）

### 常用命令

```sh
# 构建
cargo build

# 运行所有测试（单元 + 集成）
cargo test

# 仅运行端到端集成测试（输出详细日志）
cargo test --test workflow -- --nocapture

# 发布构建
cargo build --release
```

### Windows 一键构建

```powershell
.\scripts\build.ps1
```

脚本自动：
1. `cargo build --release` 编译三个二进制文件（构建时自动下载编译 HDiffPatch C 库）
2. 将可执行文件及 HDiffPatch 工具收集到 `Releases/binary_patcher_toolkit.zip`

### CI / CD

本项目使用 GitHub Actions：

| 工作流 | 触发条件 | 内容 |
|--------|---------|------|
| **CI** | push / PR | `cargo check`（Linux）+ `cargo test`（Windows / Linux / macOS） |
| **Build & Release** | tag `v*` / 手动 | `cargo build --release` → 下载 HDiffPatch 工具 → 打包 → 发布到 GitHub Release |

### TODO

- [x] 提供预编译二进制下载

## 技术栈

| 领域 | 选型 |
|------|------|
| 语言 | Rust（edition 2024） |
| CLI 框架 | clap（derive 模式） |
| 序列化 | serde + serde_json |
| 哈希 | SHA-256（ring + hex，汇编优化） |
| 目录遍历 | walkdir |
| 时间处理 | chrono |
| 终端检测 | std::io::IsTerminal（标准库） |
| 错误处理 | anyhow |
| 构建依赖 | cc（编译 C/C++）、reqwest + zip（自动下载 HDiffPatch） |
| 补丁引擎 | [HDiffPatch](https://github.com/sisong/HDiffPatch)（FFI 静态链接） |
| Hex 编码 | hex |

## 许可证

本项目基于 [Mozilla Public License 2.0](LICENSE) 开源。

## 致谢

- [HDiffPatch](https://github.com/sisong/HDiffPatch) — 二进制差异/补丁引擎
- 原 [binary_patcher](https://github.com/100pangci/binary_patcher) Python 项目

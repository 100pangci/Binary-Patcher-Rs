# Binary Patcher

[English](README.en.md) | [日本語](README.ja.md)

---

一个用于生成和应用二进制补丁的工具，支持整目录补丁工作流。
底层补丁引擎使用 [HDiffPatch](https://github.com/sisong/HDiffPatch)，通过 FFI 静态链接 C 库，构建时自动下载编译。

## 功能

- **单文件补丁** — 对两个文件生成/应用补丁
- **整目录打包** — 对比 `Old/` 与 `New/`，自动生成 `manifest.json` + 补丁文件 + 新增文件
- **文件名映射** — 通过可选 `file-map.json` 显式声明 `Old/A -> New/B`，对改名文件生成二进制差分而非删除 + 新增
- **命名补丁包** — 使用 `--patch-name` 生成 `Patch_<名称>/`，可在同一目录保存多个版本补丁
- **补丁选择** — `apply_patch` 检测多个命名补丁后引导选择，避免误应用错误版本
- **一键应用** — `apply_patch` 读取清单、校验 SHA256、备份原文件、执行补丁
- **可追踪回滚** — 应用成功后写入应用标识，`rollback_patch` 自动定位最近一次应用的补丁；回滚完成后删除标识
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

如需在同一个工作目录生成多个可选补丁，可以为补丁指定名称：

```sh
binary_patcher --patch-name v1.4.0
# 或者使用显式的 bundle 子命令
binary_patcher bundle --base-dir . --patch-name v1.4.0
```

以上命令会生成 `Patch_v1.4.0/`。传入 `Patch_v1.4.0`（小写 `patch_v1.4.0` 亦可）也会得到同样的目录名；名称只能作为当前目录的单级目录名使用。

### 2. 应用整包补丁

```
旧版本根目录/
├── apply_patch
├── Patch/                         ← 没有命名补丁时使用
│   ├── manifest.json
│   ├── ... .patch
│   └── ... .new
```

如果目录中同时存在多个命名补丁，例如：

```text
旧版本根目录/
├── apply_patch
├── Patch_v1.4.0/
│   └── manifest.json
└── Patch_security_hotfix/
    └── manifest.json
```

运行 `apply_patch` 时会显示：

```text
要应用哪个补丁？
1 - security_hotfix
2 - v1.4.0
0 - 退出
```

这里只会列出名称以 `Patch_` 开头、且直接包含 `manifest.json` 的目录。输入 `0` 不会修改目标目录并立即退出。没有命名补丁时，程序继续使用传统的 `Patch/` 目录。

```sh
./apply_patch
```

程序会：

1. 校验每个文件是否匹配 `old_sha256`
2. 将原文件备份为 `*.backup_before_patch`
3. 通过 HDiffPatch 引擎应用补丁
4. 验证输出是否匹配 `new_sha256`
5. 复制新增文件，删除已移除的文件

应用成功后，程序会在实际使用的补丁目录中生成隐藏文件 `.applied_patch.json`。其中包含本次应用的唯一标识符和补丁目录名；`rollback_patch` 会优先读取最近一次有效标识，自动回滚对应补丁，不需要再次猜测。回滚成功后，该标识文件会被删除；如果没有可用标识，`rollback_patch` 会使用与 `apply_patch` 相同的补丁选择菜单。

### 3. 回滚补丁

```sh
./rollback_patch
```

恢复 `*.backup_before_patch` 备份文件，删除补丁新增的文件。

### 4. 文件名映射（rename-aware diff）

当同一个逻辑文件在旧版本和新版本中使用不同的文件名或扩展名时，默认行为会把它们识别为
「删除旧文件 + 新增新文件」，新文件会被完整复制进补丁包。如果希望程序把两个不同路径当作
同一个逻辑文件来生成二进制差分，可以在工作目录根目录放置可选的 `file-map.json`，
手动声明映射关系：

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

这会让 Binary Patcher 将两个不同路径的文件作为同一个逻辑文件进行二进制差分，
而不是把旧文件识别为删除、新文件识别为新增。

> 文件映射只指定**差分基础文件**，源文件默认会保留，并不是破坏性的文件重命名。
> （mapping source is preserved; this is a cross-name diff base mapping, not a
> destructive filesystem rename.）

生成的清单条目形如：

```json
{
  "path": "data/package_v2.dat",
  "source_path": "data/package.bin",
  "old_sha256": "...",
  "new_sha256": "...",
  "patch_file": "data/package_v2.dat.patch"
}
```

默认情况下（`delete_source: false`）：

- 应用补丁时：映射源文件只作为差分输入，**保持不变**（不备份、不修改、不删除）；目标文件由差分生成（若目标已存在，先备份再覆盖），校验 SHA256。
- 回滚时：删除目标文件；如果应用前目标文件已存在，则恢复其原内容。源文件始终不受影响。
- 映射涉及的源/目标路径会从普通扫描中**整体排除**：`New/` 中保留的映射源同名文件视为保持不变，不会作为新增文件全量复制进补丁包，应用后也仍然存在。

如果确实需要在应用成功后删除源文件（传统重命名效果），可以对该映射显式设置：

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

- `delete_source: true` 时，Apply 先用源文件完成差分并校验目标 SHA256，**确认成功后才删除源文件**；删除前源文件会先备份，失败自动回滚与 rollback 都能完整恢复。
- `delete_source: true` 要求 `New/` 中不存在同名源文件（否则应用结果无法与 `New/` 一致，扫描阶段直接报错）。
- 缺省或未指定时按 `false` 处理，兼容旧 `file-map.json` 与旧 manifest。
- 普通同路径变更条目不允许 `delete_source: true`。

其他规则：

- 如果映射目标在 `Old/` 中已存在，其内容会被映射结果覆盖（应用前先备份，回滚时恢复），不会被误判为删除。
- 内容完全相同、仅文件名不同的情况同样支持（生成最小差分补丁）。
- 映射**不会自动猜测**，必须由用户明确指定：程序不会根据文件名、扩展名、大小或哈希推断关系。
- 路径相对于 `Old/` 和 `New/`，统一使用 `/` 分隔符（Windows 的 `\` 会自动规范化；`./` 与重复分隔符会被折叠）。
- 校验严格：旧/新文件必须存在；同一个旧文件不能映射到多个新文件，多个旧文件也不能映射到同一个新文件；`old` 与 `new` 相同（含规范化后相同、Windows 下仅大小写不同）会报错并要求删除该条目；不支持链式映射（A→B 且 B→C）。
- 如果 `New/` 中存在映射源同名文件，其内容必须与 `Old/` 完全一致，否则报错（映射源是保持不变的差分基础文件）。
- 路径安全复用现有机制：拒绝 `../`、绝对路径、符号链接与路径逃逸。
- 不存在 `file-map.json` 时，行为与旧版本完全一致（opt-in 功能）。

## CLI 参考

### `binary_patcher`

| 命令 | 说明 |
|------|------|
| （无参数） | 工作区模式：初始化 `Old/`/`New/`/`Patch/`，然后打包 |
| `create <旧文件> <新文件> <补丁文件>` | 对两个文件创建单个补丁 |
| `apply <旧文件> <补丁文件> <输出文件>` | 应用单个补丁 |
| `bundle --base-dir <路径>` | 指定工作目录执行打包 |
| `--patch-name <名称>` | 自定义目录补丁名称，例如 `v1.4.0`，输出为 `Patch_v1.4.0/`；可用于无子命令模式或 `bundle` |
| `--mode auto/stream/memory` | 补丁创建模式：`auto` 自动选择（默认）、`stream` 流式低内存、`memory` 全加载最优 |
| `--format precise/fast` | 差分算法：`precise` suffix-string（补丁更小，默认）、`fast` hash（速度更快） |

### `apply_patch`

| 参数 | 说明 |
|------|------|
| `--base-dir <路径>` | 旧版本根目录，默认为当前目录；可包含 `Patch/` 或 `Patch_<名称>/` |

### `rollback_patch`

| 参数 | 说明 |
|------|------|
| `--base-dir <路径>` | 旧版本根目录，默认为当前目录；可包含 `Patch/` 或 `Patch_<名称>/` |

## 项目结构

```
.
├── build.rs                 # 构建入口（委托 build_script/）
├── build_script/            # 构建模块
│   ├── mod.rs               # 构建编排
│   ├── download.rs          # 自动下载 HDiffPatch / zlib（仅解压）
│   └── compile.rs           # C/C++ 编译
├── e2e.ps1                  # 端到端 CLI 冒烟测试（Windows）
├── e2e.sh                   # 端到端 CLI 冒烟测试（Linux）
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
│   ├── file_map.rs          # file-map.json 加载与显式映射校验
│   ├── fmt.rs               # 格式化工具（文件大小、终端暂停）
│   ├── fs.rs                # 文件系统遍历与映射
│   ├── hash.rs              # SHA256 哈希计算
│   ├── hdiffpatch.rs        # 补丁创建/应用调用封装
│   ├── manifest.rs          # Manifest 类型、JSON 序列化、校验
│   ├── path.rs              # 安全路径解析与穿越防护
│   ├── patch.rs             # 命名补丁发现、交互选择和应用标识
│   ├── bundle.rs            # 整目录打包（Old/New → Patch）
│   ├── apply.rs             # 补丁应用逻辑
│   └── rollback.rs          # 补丁回滚逻辑
└── tests/
    ├── common/mod.rs       # 测试公共辅助（工作区构建、文件遍历、目录拷贝）
    ├── unit_fmt.rs         # format_size 单元测试
    ├── unit_hash.rs        # SHA256 单元测试
    ├── unit_path.rs        # 安全路径解析单元测试
    ├── unit_fs.rs          # 文件系统遍历与映射单元测试
    ├── unit_file_map.rs    # file-map.json 加载与映射校验单元测试
    ├── unit_manifest.rs    # Manifest 校验/加载单元测试
    ├── unit_patch.rs       # 命名补丁发现、命名校验和标识测试
    ├── unit_backup.rs      # 备份/恢复单元测试
    ├── workflow.rs         # 端到端集成测试（工作流与安全回归）
    └── workflow_mapping.rs # 文件名映射端到端集成测试
```

## 安全

| 特性 | 说明 |
|------|------|
| **路径穿越防护** | 所有 manifest 中的路径均经过校验，拒绝 `../` 逃逸 |
| **符号链接防护** | 目标路径、补丁资源、manifest、备份、日志和输出路径逐组件检查，拒绝通过预先存在的符号链接重定向文件操作 |
| **Manifest 校验** | 加载时验证字段完整性和类型，拒绝格式错误的清单，并拒绝重复的补丁资源路径 |
| **映射校验** | `file-map.json` 严格校验一对一映射、文件存在性与路径安全，不自动猜测映射关系 |
| **SHA256 校验** | 补丁前后均校验文件完整性，失败自动回滚 |
| **安全备份** | 备份文件使用 `.backup_before_patch` 后缀，已存在时追加时间戳 |
| **命名安全** | 自定义名称拒绝路径分隔符、控制字符和 Windows 保留字符，只能生成目标目录下的单级目录 |
| **补丁目录安全** | 命名补丁必须是目标目录的直接子目录，补丁目录本身及其 manifest 不能是符号链接 |
| **应用标识** | 应用标识使用临时文件 + rename 写入，回滚完成后删除，避免残留错误状态 |

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

# 仅运行命名补丁相关测试
cargo test --test unit_patch

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

# Changelog

## [v1.2.0] — 2026-07-27

### Added
- 多语言国际化 (i18n) 系统，支持中文 / English / 日本語，自动检测系统语言
- 多语言 `--help`：CLI 帮助文本随 `--lang` 切换语言
- `--lang` / `--lang-dir` 参数：手动指定语言和自定义语言文件
- `--format fast/precise` 单文件补丁模式支持
- Manifest 版本号使用 `Cargo.toml` 中的 semver 格式
- 事务性文件操作：补丁应用过程中任何步骤失败自动回滚所有变更
- `cargo deny` 安全审计配置 (`deny.toml`)
- 构建缓存：`version.txt` 缓存 HDiffPatch 最新版本号，避免每次构建请求 GitHub API
- 14 个 i18n 模块单元测试
- 自动回滚集成测试

### Changed
- **大型重构**: 301 行 `src/utils.rs` 拆分为 5 个职责单一的模块（`hash` / `path` / `backup` / `fs` / `fmt`）
- **构建脚本重构**: 415 行 `build.rs` 拆分为 `build_script/` 目录下的 3 个模块（`mod.rs` 编排、`download.rs` 下载、`compile.rs` 编译）
- `ffi.rs` / `hdiffpatch.rs` 职责清晰化：新增 `apply_patch_auto` 统一处理 OOM 降级，`apply.rs` 不再直接调用 FFI
- `apply_bundle`（155 行）拆分为 6 个聚焦函数
- `build_patch_bundle` 中 `match mode` 三路重复提取为 3 个独立的 `process_changed_*` 函数
- 补丁创建/应用全面 OOM 自动检测 + 流式降级
- SHA256 实现从 `sha2` 更换为 `ring`（汇编优化，~2x 速度提升）
- SHA256 缓冲区改用 `thread_local!` 复用，避免反复 1MB 分配
- `try_read_old_new` 返回类型从 `Result<_, ffi::PatchError>` 改为 `anyhow::Result`
- `cleanup_empty_dirs` 抽取到 `fs.rs` 消除 `apply.rs` / `rollback.rs` 的代码重复
- 所有 `.unwrap()` / `.ok()` 静默忽略改为 `expect()` / `unwrap_or_else(|| panic!(...))`
- C++ wrapper 中 4 处 `catch (...)` 改为 `catch (const std::exception& e)` + `fprintf(stderr, ...)`

### Fixed
- 内存泄漏：FFI 返回路径均正确调用 `hdiffpatch_free`
- 路径遍历：ZIP 提取和运行时均增加路径穿越检测
- TOCTOU 竞态条件
- 未定义行为 (UB)：OOM 检测机制重构
- 补丁文件读取的 `format!` 嵌套 clippy lint
- SHA256 缓冲区从栈分配改为堆分配，防止 1MB 栈溢出
- e2e.ps1 stdin 挂起问题
- clap `mut_arg` 使用错误 arg ID 导致程序崩溃
- 测试改用 `CARGO_PKG_VERSION`，删除死代码

### Security
- `rollback.rs` 符号链接防护：`remove_dir_all` 前检查备份目录在 Patch 目录内
- 补丁应用失败自动回滚 + SHA256 校验
- 备份文件不覆盖：`create_new(true)` + 时间戳重试
- C 堆内存安全释放：空指针/零长度双重检查

## [v1.1.0] — 2026-07-26

### Added
- `--format fast` (v2 hash-based diff) 快速差分算法
- `--mode auto/stream/memory` 三种补丁创建模式（替代 `--stream` 标志）
- 多线程补丁应用，失败自动降级单线程
- 备份文件集中到 `Patch/.backup_before_patch/` 目录
- 三语 README（中文 / English / 日本語）

### Changed
- 差分线程上限提升至 32
- 运行时安全检查默认关闭（性能收益 15-20%）
- `build.rs` 版本缓存机制，缓存 HDiffPatch 最新版本号
- 升级 HDiffPatch 依赖版本自动检测（GitHub API + HTML 回退）

### Fixed
- Linux/macOS 跨平台兼容性
- 构建脚本 API 限流回退机制

## [v1.0.8] — 2026-07-26

### Added
- `--format fast` (v2 hash-based diff) 作为快速差分算法

### Changed
- 差分线程上限从 5 提升至 32
- 运行时安全检查默认关闭

## [v1.0.7] — 2026-07-26

### Added
- `--mode auto/stream/memory` 三种模式（替代 `--stream`）

### Changed
- 文档同步更新三语 README

## [v1.0.6] — 2026-07-26

### Changed
- SHA256 实现从 `sha2` 更换为 `ring`（汇编优化，~2x 速度提升）
- 流式模式支持：`--stream` 标志，大文件低内存处理
- 单次文件读取（SHA256 + 差分合并）

### Fixed
- CI 中 Linux 编译依赖（build-essential, nasm）

## [v1.0.5] — 2026-07-25

同 v1.0.1。

## [v1.0.2] — 2026-07-22

### Added
- `--no-compress` 标志，禁用 zlib 补丁压缩
- zlib 压缩支持（HDiffPatch 补丁引擎）
- CI/CD：tag 触发自动构建 + GitHub Release

### Fixed
- CI 重复触发问题

## [v1.0.1] — 2026-07-25

### Added
- 多线程补丁应用（失败自动降级单线程）
- 流式回退模式（内存不足时自动降级文件流）
- 错误诊断信息改进
- 构建脚本自动下载 HDiffPatch 和 zlib，零手动依赖
- 三平台构建支持（Windows / Linux / macOS）
- 端到端冒烟测试脚本 `e2e.ps1`
- 测试数据生成脚本 `scripts/gen_test_data.ps1`

### Changed
- 补丁引擎从 bsdiff 迁移至 HDiffPatch（FFI 静态链接 C/C++ 库）
- Rust 完整重写，替代原 Python 版本
- 构建脚本版本缓存机制 + HTML scraping 回退

### Fixed
- SHA256 缓冲区栈溢出（1MB 改为堆分配）
- 跨平台测试兼容性

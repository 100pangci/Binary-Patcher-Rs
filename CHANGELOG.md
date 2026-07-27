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
- 测试改用 `CARGO_PKG_VERSION`，删除死代码

### Security
- `rollback.rs` 符号链接防护：`remove_dir_all` 前检查备份目录在 Patch 目录内
- 补丁应用失败自动回滚 + SHA256 校验
- 备份文件不覆盖：`create_new(true)` + 时间戳重试
- C 堆内存安全释放：空指针/零长度双重检查
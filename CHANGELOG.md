# Changelog

## [dev] — 

### Changed
- Windows 文件路径改用宽字符 I/O：显式启用 HDiffPatch 自带的 `_IS_USED_WIN32_UTF8_WAPI` 宏
  （UTF-8→UTF-16 + `_wfsopen`），绕开窄字符 `fopen` 的 ANSI 代码页（中文系统 GBK）问题；
  MSVC 默认已启用（无行为变化），MinGW 需显式声明（此前的真正缺口）
  - MinGW 下 `_wfsopen` 的 `_SH_DENYNO` 常量在 `<share.h>` 中，C 构建强制 include（MSVC 幂等）
  - `src/ffi.rs` 路径注释同步更新：Windows 上以 UTF-8 传入 C 侧，仅未配对代理项 lossy
- e2e 测试集新增非 ASCII（中文）文件名用例，覆盖 bundle create/stream/apply/rollback 全链路

## [v1.3.0] — 2026-08-17

### Added
- `[lints]` 配置：crate 级声明 rustc/clippy 检查（含 pedantic 选择性启用），不再依赖 CI 参数
- 测试按模块拆分：`tests/integration_test.rs`（919 行）拆分为 `common/` + 6 个单元测试文件 + `workflow.rs`
- 持久化应用日志 `Patch/.apply_journal.json`：apply 中断（断电/进程终止）后再次运行可检测并回滚未完成的更改，支持 `[R]ollback / [A]bort` 选择
- `e2e.sh`：Linux 端到端 CLI 冒烟测试（对应 Windows 的 e2e.ps1，8 段全流程 + 计数断言）
- 崩溃恢复集成测试：journal 四类条目恢复、路径穿越拒绝、损坏 JSON 处理

### Changed
- `build_script/compile.rs` 重构：`compile_all` 拆分为 `compile_c` / `compile_cpp` / `new_build` / `includes_for`
- `build_script/download.rs` 修复 5 处 clippy pedantic 告警（manual_assert、uninlined_format_args 等）
- 修复 25 处 clippy pedantic 告警：FFI bool→i32 改用 `i32::from`、`match` 改为 `let...else` / `is_err()`、`map_or` 替代 `map().unwrap_or()` 等
- bundle 模式 `process_changed_stream/mem/auto` 三函数合并为按模式分派的一个函数，移除 `create_patch_mem` / `create_patch_stream` / `try_read_old_new` 重复代码
- auto 模式按文件大小阈值决策：old + new 超过 1 GiB 直接走流式，避免先整体读入内存再 OOM 回退（OOM 回退保留兜底）
- apply 日志条目在修改文件前落盘（临时文件 + rename 原子写入），崩溃时最多丢失一条记录
- FFI 路径参数从 `&str` 改为 `&Path`：Unix 上按原始字节转换，非 UTF-8 文件名无损支持；Windows 保持 lossy（窄字符 API 限制）
- 补丁大小/线程数输出改由调用处控制缩进，i18n 值移除 `"  - "` 前缀，`print_patch_result` 统一四种模式的输出格式
- **OOM 降级链路修复**（`ulimit -v` 实测模拟内存不足）：
  - wrapper 将 `std::bad_alloc` 映射为 OOM 码 -8，OOM 可触发流式降级而非普通错误
  - OOM 流式回退前释放已读入的文件数据（此前残留内存会导致流式再次 OOM，且 HDiffPatch 内部线程池异常展开时直接 terminate）
  - apply 接口改为两段式（解析补丁头 → Rust 分配输出缓冲 → C 库填充），消除 C 分配 + Rust 复制造成的双缓冲峰值内存
  - apply 侧新增大小阈值决策：old + new 超过 1 GiB 直接流式，避免 Rust 分配输出缓冲时 OOM（Rust 分配失败不可捕获，直接 abort）
  - **流式模式强制 fast 格式**：fast（window matcher）流式内存可控且实测可靠，precise 仅内存路径可用
- **移除补丁压缩（zlib deflate）**：实测 zlib 对随机二进制差异（真实补丁场景）几乎无收益（64M 随机差异数据 0% 压缩率）。`--no-compress` 参数移除；build.rs 的 zlib 仅保留 inflate 部分（兼容旧版压缩补丁的 apply 侧解压），create 侧不再压缩

> **上游问题（待 HDiffPatch 修复）**：TMT 多线程 diff 在内存临界时线程创建失败，`std::vector<std::thread>` 异常展开时 joinable 线程析构触发 `std::terminate`（官方 hdiffz 自编二进制实测偶发，rc=134；作者在 Linux/Windows 均复现）。复现脚本 `scripts/reproduce-hdiffpatch-oom.sh` 已随仓库提交，issue 草稿仅本地留存（`docs/hdiffpatch-oom-issue.md`，未入库），待上游确认后关闭。
>
> **勘误**：早前实测的"流式生成损坏/截断补丁（exit 0 无报错）"经排查为**本项目 wrapper 的 bug**——`hdiff_TMTSets_s` 的 `newDataIsMTSafe`/`oldDataIsMTSafe` 误传 `true`（实际传入非 MT 安全的文件流），TMT 多线程并发读同一 FILE* 产生数据竞争，偶发损坏补丁；与官方 hdiffz 的 `false/false` 对齐后 30 次运行零损坏（见 f4418cb）。上游 issue #455 症状 1 相应撤回。

### Fixed
- bundle 模式补丁信息中 `{0}` 占位符未替换，`print_patch_result` 和 `process_changed_auto` 未传递参数导致占位符字面输出
- 单文件 apply（`apply_single_patch`）中 `apply.output-generated` / `main.patch-size` 未传参数导致 `{0}` 字面输出，以及标签前缀导致的重复缩进
- 移除 `--copy-scripts` 死代码参数（`cli.rs`）
- 流式 apply 分支未释放 `old_data`，内存受限时额外失败

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

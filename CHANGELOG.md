# Changelog

## [1.1.0] — 2026-07-27

### Added
- 自适应内存/流式模式（`--mode auto`），OOM 时按文件自动回退流式
- 低内存流式模式（`--mode stream`），适合大文件或内存受限环境
- Fast/Precise 两种差分算法选择（`--format fast/precise`）
- 多线程补丁应用（失败时自动降级单线程）
- 端到端冒烟测试脚本 `e2e.ps1`
- GitHub Actions CI/CD（三平台测试 + Release 自动构建）
- 三语 README（中文 / English / 日本語）

### Changed
- Rust 重写，替代原 Python 版本
- 补丁引擎升级为 HDiffPatch（原 bsdiff），支持 zlib 压缩
- 模块化架构：`hash` / `path` / `backup` / `fs` / `fmt` 等职责单一模块

### Security
- 路径穿越防护（拒绝 `../` 逃逸）
- 补丁前后 SHA256 完整性校验
- 校验失败自动回滚
- 备份文件使用时间戳后缀，不静默覆盖
- Manifest 格式校验 + 版本兼容检查

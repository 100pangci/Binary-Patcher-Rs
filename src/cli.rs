use clap::{Command, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// 为 clap Command 的各个参数应用 i18n 帮助文本。
pub fn apply_arg_help(cmd: Command) -> Command {
    cmd.mut_arg("no_compress", |a| {
        a.help(crate::i18n::load_help_text("cli.no-compress"))
    })
    .mut_arg("patch_mode", |a| {
        a.help(crate::i18n::load_help_text("cli.mode"))
    })
    .mut_arg("patch_format", |a| {
        a.help(crate::i18n::load_help_text("cli.format"))
    })
    .mut_arg("lang", |a| a.help(crate::i18n::load_help_text("cli.lang")))
    .mut_arg("lang_dir", |a| {
        a.help(crate::i18n::load_help_text("cli.lang-dir"))
    })
}

/// 为 apply_patch 和 rollback_patch 共用参数应用 i18n 帮助文本。
pub fn apply_base_arg_help(cmd: Command, base_key: &str) -> Command {
    cmd.mut_arg("base_dir", |a| {
        a.help(crate::i18n::load_help_text(base_key))
    })
    .mut_arg("lang", |a| a.help(crate::i18n::load_help_text("cli.lang")))
    .mut_arg("lang_dir", |a| {
        a.help(crate::i18n::load_help_text("cli.lang-dir"))
    })
}

#[derive(Parser)]
#[command(name = "binary_patcher")]
pub struct Cli {
    #[arg(
        long = "no-compress",
        default_value_t = false,
        help = "禁用补丁压缩（默认启用 zlib 压缩）"
    )]
    pub no_compress: bool,

    #[arg(
        long = "mode",
        default_value = "auto",
        help = "补丁创建模式: auto（自动判断）、stream（流式低内存）、memory（全加载到内存，补丁更小）"
    )]
    pub patch_mode: PatchMode,

    #[arg(
        long = "format",
        default_value = "precise",
        help = "差分算法: precise（suffix-string，补丁更小，默认）、fast（hash匹配，速度更快）"
    )]
    pub patch_format: PatchFormat,

    #[arg(
        long = "lang",
        default_value = "",
        help = "语言代码（如 en, zh-CN, ja），不指定时自动检测系统语言"
    )]
    pub lang: String,

    #[arg(
        long = "lang-dir",
        help = "自定义语言文件目录（包含 {lang}.json 文件）"
    )]
    pub lang_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Clone, ValueEnum)]
pub enum PatchMode {
    Auto,
    Stream,
    Memory,
}

#[derive(Clone, ValueEnum)]
pub enum PatchFormat {
    Fast,
    Precise,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 比较两个文件并创建一个补丁文件
    Create {
        old_file: String,
        new_file: String,
        patch_file: String,
    },
    /// 将补丁应用到旧文件以生成新文件
    Apply {
        old_file: String,
        patch_file: String,
        output_file: String,
    },
    /// 按 Old/New/Patch 目录工作流生成整包补丁
    Bundle {
        #[arg(long = "base-dir", default_value = ".")]
        base_dir: String,
    },
}

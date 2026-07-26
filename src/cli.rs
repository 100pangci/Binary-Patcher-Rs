use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "binary_patcher")]
#[command(about = "一个用于创建和应用二进制文件补丁的工具", long_about = None)]
pub struct Cli {
    #[arg(
        long = "copy-scripts",
        default_value_t = false,
        help = "（兼容选项，Rust 版本无效）"
    )]
    pub copy_scripts: bool,

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

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Clone, ValueEnum)]
pub enum PatchMode {
    /// 自动判断：内存够用全加载，不够用流式
    Auto,
    /// 强制流式模式：低内存占用，补丁体积可能更大
    Stream,
    /// 强制内存模式：全部文件加载到内存，补丁最优
    Memory,
}

#[derive(Clone, ValueEnum)]
pub enum PatchFormat {
    /// 快速模式：hash 匹配，速度快，补丁体积较大
    Fast,
    /// 精确模式：suffix-string 匹配，补丁更小（默认）
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

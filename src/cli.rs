use clap::{Parser, Subcommand};

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
        long = "stream",
        default_value_t = false,
        help = "强制使用流式模式创建补丁（降低内存占用，但补丁体积可能更大）"
    )]
    pub force_stream: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
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

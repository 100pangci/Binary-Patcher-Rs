use binary_patcher::apply;
use binary_patcher::utils::pause_if_needed;
use clap::Parser;
use std::path::PathBuf;

/// apply_patch CLI：应用整包补丁到目标目录。
#[derive(Parser)]
#[command(name = "apply_patch")]
#[command(about = "应用整包补丁")]
struct Cli {
    #[arg(
        long = "base-dir",
        default_value = ".",
        help = "旧版本根目录（包含 Patch 文件夹）"
    )]
    base_dir: PathBuf,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = apply::apply_bundle(&cli.base_dir) {
        eprintln!("错误: {e}");
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

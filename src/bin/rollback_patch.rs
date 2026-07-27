use binary_patcher::rollback;
use binary_patcher::utils::pause_if_needed;
use clap::Parser;
use std::path::PathBuf;

/// rollback_patch CLI：回滚已应用的整包补丁。
#[derive(Parser)]
#[command(name = "rollback_patch")]
#[command(about = "回滚整包补丁")]
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

    if let Err(e) = rollback::rollback_bundle(&cli.base_dir) {
        eprintln!("错误: {e}");
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

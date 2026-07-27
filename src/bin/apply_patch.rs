use std::path::PathBuf;
use clap::Parser;
use binary_patcher::apply;
use binary_patcher::utils::pause_if_needed;

#[derive(Parser)]
#[command(name = "apply_patch")]
#[command(about = "应用整包补丁")]
struct Cli {
    #[arg(long = "base-dir", default_value = ".", help = "旧版本根目录（包含 Patch 文件夹）")]
    base_dir: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    let result = apply::apply_bundle(&cli.base_dir);

    if let Err(e) = result {
        eprintln!("错误: {e}");
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

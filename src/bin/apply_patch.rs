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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match apply::apply_bundle(&cli.base_dir) {
        Ok(()) => {
            pause_if_needed();
            Ok(())
        }
        Err(e) => {
            eprintln!("错误: {e}");
            pause_if_needed();
            Err(e)
        }
    }
}

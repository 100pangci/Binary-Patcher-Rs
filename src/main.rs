use std::path::Path;
use clap::Parser;
use binary_patcher::cli::{Cli, Commands};
use binary_patcher::bundle::{self, init_workspace};
use binary_patcher::apply;
use binary_patcher::hdiffpatch;
use binary_patcher::utils::{format_size, ensure_parent_dir};

fn create_single_patch(old_file: &str, new_file: &str, patch_file: &str, use_compression: bool) -> anyhow::Result<()> {
    let old_path = Path::new(old_file);
    let new_path = Path::new(new_file);
    let patch_path = Path::new(patch_file);

    ensure_parent_dir(patch_path)?;
    let old_size = std::fs::metadata(old_path)?.len();
    let new_size = std::fs::metadata(new_path)?.len();

    println!("正在读取旧文件: {old_file}");
    println!("正在读取新文件: {new_file}");
    println!("正在调用 HDiffPatch 生成补丁...");
    let thread_count = hdiffpatch::run_hdiffz(old_path, new_path, patch_path, use_compression)?;
    let patch_size = std::fs::metadata(patch_path)?.len();

    println!("{}", "-".repeat(30));
    println!("补丁创建成功！");
    println!("  - 使用线程数: {thread_count}");
    println!("  - 旧文件大小: {}", format_size(old_size));
    println!("  - 新文件大小: {}", format_size(new_size));
    println!("  - 补丁文件大小: {}", format_size(patch_size));
    println!("{}", "-".repeat(30));

    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let use_compression = !cli.no_compress;

    let result = match cli.command {
        Some(Commands::Create { old_file, new_file, patch_file }) => {
            create_single_patch(&old_file, &new_file, &patch_file, use_compression)
        }
        Some(Commands::Apply { old_file, patch_file, output_file }) => {
            apply::apply_single_patch(&old_file, &patch_file, &output_file)
        }
        Some(Commands::Bundle { base_dir }) => {
            bundle::build_patch_bundle(Path::new(&base_dir), use_compression, cli.patch_mode.clone())
        }
        None => {
            // No-arg workspace mode
            let base_dir = std::env::current_dir().unwrap_or_default();
            match init_workspace(&base_dir) {
                Ok(true) => bundle::build_patch_bundle(&base_dir, use_compression, cli.patch_mode.clone()),
                Ok(false) => {
                    pause_if_needed();
                    return;
                }
                Err(e) => Err(e),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("错误: {e}");
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

fn pause_if_needed() {
    if !atty::is(atty::Stream::Stdin) {
        return;
    }
    println!("\n按 Enter 键退出...");
    let _ = std::io::stdin().read_line(&mut String::new());
}

use binary_patcher::apply;
use binary_patcher::bundle::{self, init_workspace};
use binary_patcher::cli::{Cli, Commands, PatchFormat};
use binary_patcher::fmt::{format_size, pause_if_needed};
use binary_patcher::hdiffpatch;
use binary_patcher::path::ensure_parent_dir;
use binary_patcher::t;
use clap::{CommandFactory, FromArgMatches};
use std::path::Path;

fn create_single_patch(
    old_file: &str,
    new_file: &str,
    patch_file: &str,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<()> {
    let old_path = Path::new(old_file);
    let new_path = Path::new(new_file);
    let patch_path = Path::new(patch_file);

    ensure_parent_dir(patch_path)?;
    let old_size = std::fs::metadata(old_path)?.len();
    let new_size = std::fs::metadata(new_path)?.len();

    println!("{}", t!("main.reading-old", old_file));
    println!("{}", t!("main.reading-new", new_file));
    println!("{}", t!("main.calling-hdiff"));
    let thread_count =
        hdiffpatch::run_hdiffz(old_path, new_path, patch_path, use_compression, fast_format)?;
    let patch_size = std::fs::metadata(patch_path)?.len();

    println!("{}", "-".repeat(30));
    println!("{}", t!("main.patch-created"));
    println!("{}", t!("main.threads-used", thread_count));
    println!("{}", t!("main.old-size", format_size(old_size)));
    println!("{}", t!("main.new-size", format_size(new_size)));
    println!("{}", t!("main.patch-size", format_size(patch_size)));
    println!("{}", "-".repeat(30));

    Ok(())
}

fn main() {
    let about = binary_patcher::i18n::load_help_text("cli.about-bp");
    let cmd = Cli::command().about(about);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    let lang_dir = cli.lang_dir.as_deref();
    let lang = if cli.lang.is_empty() {
        binary_patcher::i18n::detect_language()
    } else {
        cli.lang.clone()
    };
    binary_patcher::i18n::init(&lang, lang_dir);

    let use_compression = !cli.no_compress;

    let result = match cli.command {
        Some(Commands::Create {
            old_file,
            new_file,
            patch_file,
        }) => {
            let fast_format = matches!(cli.patch_format, PatchFormat::Fast);
            create_single_patch(
                &old_file,
                &new_file,
                &patch_file,
                use_compression,
                fast_format,
            )
        }
        Some(Commands::Apply {
            old_file,
            patch_file,
            output_file,
        }) => apply::apply_single_patch(&old_file, &patch_file, &output_file),
        Some(Commands::Bundle { base_dir }) => bundle::build_patch_bundle(
            Path::new(&base_dir),
            use_compression,
            cli.patch_mode.clone(),
            cli.patch_format.clone(),
        ),
        None => {
            let base_dir = match std::env::current_dir() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{}", t!("main.cwd-failed", e));
                    std::path::PathBuf::from(".")
                }
            };
            match init_workspace(&base_dir) {
                Ok(true) => bundle::build_patch_bundle(
                    &base_dir,
                    use_compression,
                    cli.patch_mode.clone(),
                    cli.patch_format.clone(),
                ),
                Ok(false) => {
                    pause_if_needed();
                    return;
                }
                Err(e) => Err(e),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("{}", t!("error.generic", e));
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

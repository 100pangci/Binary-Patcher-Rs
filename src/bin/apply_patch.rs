use binary_patcher::apply;
use binary_patcher::fmt::pause_if_needed;
use binary_patcher::t;
use clap::{CommandFactory, FromArgMatches, Parser};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "apply_patch")]
struct Cli {
    #[arg(
        long = "base-dir",
        default_value = ".",
        help = "旧版本根目录（包含 Patch 文件夹）"
    )]
    base_dir: PathBuf,

    #[arg(
        long = "lang",
        default_value = "",
        help = "语言代码（如 en, zh-CN, ja），不指定时自动检测系统语言"
    )]
    lang: String,

    #[arg(
        long = "lang-dir",
        help = "自定义语言文件目录（包含 {lang}.json 文件）"
    )]
    lang_dir: Option<PathBuf>,
}

fn main() {
    let about = binary_patcher::i18n::load_help_text("cli.about-apply");
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

    if let Err(e) = apply::apply_bundle(&cli.base_dir) {
        eprintln!("{}", t!("error.generic", e));
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

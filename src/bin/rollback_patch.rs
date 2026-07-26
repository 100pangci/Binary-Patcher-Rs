use std::path::Path;
use binary_patcher::rollback;
use binary_patcher::utils::pause_if_needed;

fn main() {
    let result = rollback::rollback_bundle(&Path::new("."));

    if let Err(e) = result {
        eprintln!("错误: {e}");
        pause_if_needed();
        std::process::exit(1);
    }

    pause_if_needed();
}

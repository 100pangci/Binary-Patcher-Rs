use crate::t;

pub fn pause_if_needed() {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return;
    }
    println!("{}", t!("fmt.pause-prompt"));
    let _ = std::io::stdin().read_line(&mut String::new());
}

pub fn format_size(size_bytes: u64) -> String {
    if size_bytes < 1024 {
        format!("{size_bytes} B")
    } else if size_bytes < 1024 * 1024 {
        format!("{:.2} KB", size_bytes as f64 / 1024.0)
    } else if size_bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", size_bytes as f64 / (1024.0 * 1024.0))
    } else if size_bytes < 1024u64 * 1024 * 1024 * 1024 {
        format!("{:.2} GB", size_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!(
            "{:.2} TB",
            size_bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0)
        )
    }
}

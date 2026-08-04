// ===========================================================================
// format_size
// ===========================================================================

#[test]
fn test_format_size_bytes() {
    assert_eq!(binary_patcher::fmt::format_size(512), "512 B");
}

#[test]
fn test_format_size_kb() {
    assert_eq!(binary_patcher::fmt::format_size(1024), "1.00 KB");
    assert_eq!(binary_patcher::fmt::format_size(1536), "1.50 KB");
}

#[test]
fn test_format_size_mb() {
    assert_eq!(binary_patcher::fmt::format_size(1024 * 1024), "1.00 MB");
    assert_eq!(binary_patcher::fmt::format_size(2 * 1024 * 1024), "2.00 MB");
}

#[test]
fn test_format_size_gb() {
    assert_eq!(
        binary_patcher::fmt::format_size(1024 * 1024 * 1024),
        "1.00 GB"
    );
}

#[test]
fn test_format_size_tb() {
    assert_eq!(
        binary_patcher::fmt::format_size(1024u64 * 1024 * 1024 * 1024),
        "1.00 TB"
    );
}

#[test]
fn test_format_size_zero() {
    assert_eq!(binary_patcher::fmt::format_size(0), "0 B");
}

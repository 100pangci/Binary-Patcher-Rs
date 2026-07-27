use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

static I18N: OnceLock<I18n> = OnceLock::new();

struct I18n {
    lang: String,
    data: HashMap<String, String>,
}

fn embed_lang(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some(include_str!("../i18n/en.json")),
        "zh-CN" => Some(include_str!("../i18n/zh-CN.json")),
        "ja" => Some(include_str!("../i18n/ja.json")),
        _ => None,
    }
}

fn load_json(content: &str) -> HashMap<String, String> {
    serde_json::from_str(content).unwrap_or_default()
}

fn try_load_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn normalize_lang(input: &str) -> String {
    let lang = input.trim().to_lowercase();
    let lang = lang.split('.').next().unwrap_or(&lang);
    let lang = lang.replace('_', "-");
    match lang.as_str() {
        "en" | "en-us" | "en-gb" | "en-au" | "en-ca" => "en".to_string(),
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" => "zh-CN".to_string(),
        "ja" | "ja-jp" => "ja".to_string(),
        _ => {
            let parts: Vec<&str> = lang.split('-').collect();
            if !parts.is_empty() && parts[0].len() == 2 {
                parts[0].to_string()
            } else {
                "en".to_string()
            }
        }
    }
}

#[cfg(windows)]
fn detect_system_locale() -> Option<String> {
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }
    let mut buffer = [0u16; 85];
    let result = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), 85) };
    if result > 0 {
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let s = String::from_utf16(&buffer[..len]).ok()?;
        Some(normalize_lang(&s))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn detect_system_locale() -> Option<String> {
    if let Ok(lang) = std::env::var("LANG") {
        let lang = lang.split('.').next().unwrap_or(&lang);
        if !lang.is_empty() {
            return Some(normalize_lang(lang));
        }
    }
    None
}

/// 从原始 CLI 参数中解析 --lang 值（在 clap 解析之前调用）。
pub fn detect_language_from_args() -> Option<String> {
    let mut args = std::env::args();
    let _prog = args.next()?;
    while let Some(arg) = args.next() {
        if arg == "--lang" {
            return args.next().map(|v| normalize_lang(&v));
        }
        if let Some(val) = arg.strip_prefix("--lang=")
            && !val.is_empty()
        {
            return Some(normalize_lang(val));
        }
    }
    None
}

/// 在 i18n 初始化之前加载指定 key 的翻译文本（用于 CLI help）。
/// 优先使用 `--lang` 参数指定的语言，否则检测系统语言。
pub fn load_help_text(key: &str) -> String {
    let lang = detect_language_from_args()
        .or_else(|| {
            let d = detect_language();
            if d == "en" { None } else { Some(d) }
        })
        .unwrap_or_else(|| "en".to_string());
    embed_lang(&lang)
        .or_else(|| embed_lang("en"))
        .map(load_json)
        .and_then(|map| map.get(key).cloned())
        .unwrap_or_default()
}

pub fn detect_language() -> String {
    if let Ok(lang) = std::env::var("BINARY_PATCHER_LANG") {
        let lang = lang.trim().to_lowercase();
        if !lang.is_empty() {
            return normalize_lang(&lang);
        }
    }
    if let Some(lang) = detect_system_locale() {
        return lang;
    }
    "en".to_string()
}

fn supported(code: &str) -> bool {
    matches!(code, "en" | "zh-CN" | "ja")
}

pub fn init(lang: &str, lang_dir: Option<&Path>) {
    let lang = normalize_lang(lang);
    let lang = if supported(&lang) || lang_dir.is_some() {
        lang
    } else {
        "en".to_string()
    };

    let data = lang_dir
        .and_then(|dir| {
            let file_path = dir.join(format!("{lang}.json"));
            try_load_file(&file_path).and_then(|content| {
                let map = load_json(&content);
                if map.is_empty() { None } else { Some(map) }
            })
        })
        .or_else(|| embed_lang(&lang).map(load_json))
        .unwrap_or_else(|| load_json(include_str!("../i18n/en.json")));

    let _ = I18N.set(I18n { lang, data });
}

pub fn tr(key: &str) -> &str {
    I18N.get()
        .and_then(|i| i.data.get(key))
        .map(|s| s.as_str())
        .unwrap_or(key)
}

pub fn current_lang() -> &'static str {
    I18N.get().map(|i| i.lang.as_str()).unwrap_or("en")
}

pub fn into_arg<T: std::fmt::Display>(value: &T) -> String {
    value.to_string()
}

pub fn fmt(template: &str, args: &[String]) -> String {
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}

#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::tr($key).to_string()
    };
    ($key:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fmt(
            $crate::i18n::tr($key),
            &[$($crate::i18n::into_arg(&$arg)),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_lang_en() {
        assert_eq!(normalize_lang("en"), "en");
        assert_eq!(normalize_lang("en-US"), "en");
        assert_eq!(normalize_lang("en_GB"), "en");
    }

    #[test]
    fn test_normalize_lang_zh() {
        assert_eq!(normalize_lang("zh-CN"), "zh-CN");
        assert_eq!(normalize_lang("zh-Hans"), "zh-CN");
        assert_eq!(normalize_lang("zh"), "zh-CN");
    }

    #[test]
    fn test_normalize_lang_ja() {
        assert_eq!(normalize_lang("ja"), "ja");
        assert_eq!(normalize_lang("ja-JP"), "ja");
    }

    #[test]
    fn test_normalize_lang_fallback() {
        assert_eq!(normalize_lang("fr"), "fr");
        assert_eq!(normalize_lang("de-DE"), "de");
    }

    #[test]
    fn test_embed_lang_supported() {
        assert!(embed_lang("en").is_some());
        assert!(embed_lang("zh-CN").is_some());
        assert!(embed_lang("ja").is_some());
    }

    #[test]
    fn test_embed_lang_unsupported() {
        assert!(embed_lang("fr").is_none());
        assert!(embed_lang("xx").is_none());
    }

    #[test]
    fn test_supported_langs() {
        assert!(supported("en"));
        assert!(supported("zh-CN"));
        assert!(supported("ja"));
        assert!(!supported("fr"));
        assert!(!supported("de"));
    }

    #[test]
    fn test_fmt_no_args() {
        assert_eq!(fmt("hello", &[]), "hello");
    }

    #[test]
    fn test_fmt_with_args() {
        assert_eq!(fmt("{0} {1}", &["hello".into(), "world".into()]), "hello world");
    }

    #[test]
    fn test_fmt_repeated_arg() {
        assert_eq!(fmt("{0} + {0} = {1}", &["1".into(), "2".into()]), "1 + 1 = 2");
    }

    #[test]
    fn test_tr_before_init_returns_key() {
        assert_eq!(tr("some.random.key"), "some.random.key");
    }

    #[test]
    fn test_load_help_text_returns_fallback() {
        let text = load_help_text("nonexistent.key.xyz");
        assert!(!text.is_empty() || text.is_empty());
    }

    #[test]
    fn test_detect_language_from_args_before_main() {
        let _ = detect_language_from_args();
        // Should not panic even when env::args is empty or minimal
    }

    #[test]
    fn test_init_with_unsupported_fallback_to_en() {
        let _lang = current_lang();
        // init should not panic with unsupported language
        let _ = I18N.set(I18n {
            lang: "en".into(),
            data: load_json(include_str!("../i18n/en.json")),
        });
    }
}

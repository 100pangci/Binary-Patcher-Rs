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

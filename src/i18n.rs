//! Interface language: English by default, Italian when Open CAD Studio runs
//! in Italian.
//!
//! The host does not pass its UI language to plugins, so it is read from the
//! host's own settings file, `<config>/OpenCADStudio/settings.json`, key
//! `settings.language` (`"it-IT"`, `"en-US"`, … or `"system"`). With
//! `"system"` (or no file) the operating system's locale decides. Any
//! language other than Italian gets English.
//!
//! `RT_LANG IT|EN|AUTO` overrides the choice; the override is kept in
//! `<config>/OpenCADStudio/rastertools-lang.txt`. The ribbon is built once
//! when the plugin loads, so its labels follow a change after a restart;
//! messages and prompts follow it at once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

/// Pick the Italian or the English form. Both arms are expressions, so
/// `format!` works in either and only the chosen one is evaluated.
#[macro_export]
macro_rules! tr {
    ($it:expr, $en:expr $(,)?) => {
        if $crate::i18n::italian() {
            $it
        } else {
            $en
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    It,
    En,
}

/// The user's explicit choice from `RT_LANG`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Auto,
    It,
    En,
}

const UNSET: u8 = 0;
const IT: u8 = 1;
const EN: u8 = 2;

/// Resolved language, cached until [`refresh`].
static CURRENT: AtomicU8 = AtomicU8::new(UNSET);

/// Whether messages should be in Italian.
pub fn italian() -> bool {
    current() == Lang::It
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        IT => Lang::It,
        EN => Lang::En,
        _ => {
            let l = resolve();
            store(l);
            l
        }
    }
}

/// Re-read the settings (called before each command, so a language changed
/// in the CAD is picked up without restarting).
pub fn refresh() {
    store(resolve());
}

fn store(l: Lang) {
    CURRENT.store(if l == Lang::It { IT } else { EN }, Ordering::Relaxed);
}

/// The current choice and the language it leads to.
pub fn status() -> (Choice, Lang) {
    (read_choice(), current())
}

/// Save a `RT_LANG` choice and apply it.
pub fn set_choice(c: Choice) -> std::io::Result<()> {
    let path = choice_path().ok_or_else(|| std::io::Error::other("config folder not found"))?;
    match c {
        Choice::Auto => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        },
        Choice::It | Choice::En => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, if c == Choice::It { "it\n" } else { "en\n" })?;
        }
    }
    refresh();
    Ok(())
}

fn resolve() -> Lang {
    match read_choice() {
        Choice::It => Lang::It,
        Choice::En => Lang::En,
        Choice::Auto => {
            let tag = host_language().filter(|t| !t.eq_ignore_ascii_case("system")).or_else(os_locale);
            match tag {
                Some(t) if is_italian_tag(&t) => Lang::It,
                _ => Lang::En,
            }
        }
    }
}

/// `"it"`, `"it-IT"`, `"it_CH.UTF-8"`, … — but not `"ita"`-less strings.
pub fn is_italian_tag(tag: &str) -> bool {
    let t = tag.trim().to_ascii_lowercase();
    t == "it" || t.starts_with("it-") || t.starts_with("it_") || t.starts_with("it.")
}

/// Open CAD Studio's per-user config folder (same rules as the host).
fn config_dir() -> Option<PathBuf> {
    // Unset and empty variables are treated alike.
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let base: PathBuf = if cfg!(target_os = "windows") {
        var("APPDATA").map(PathBuf::from)?
    } else if cfg!(target_os = "macos") {
        let mut p = PathBuf::from(var("HOME")?);
        p.push("Library");
        p.push("Application Support");
        p
    } else if let Some(d) = var("XDG_CONFIG_HOME") {
        PathBuf::from(d)
    } else {
        let mut p = PathBuf::from(var("HOME")?);
        p.push(".config");
        p
    };
    Some(base.join("OpenCADStudio"))
}

fn choice_path() -> Option<PathBuf> {
    Some(config_dir()?.join("rastertools-lang.txt"))
}

fn read_choice() -> Choice {
    let Some(p) = choice_path() else { return Choice::Auto };
    match std::fs::read_to_string(p) {
        Ok(s) => parse_choice(&s).unwrap_or(Choice::Auto),
        Err(_) => Choice::Auto,
    }
}

pub fn parse_choice(s: &str) -> Option<Choice> {
    match s.trim().to_ascii_uppercase().as_str() {
        "IT" | "ITA" | "ITALIANO" | "ITALIAN" => Some(Choice::It),
        "EN" | "ENG" | "ENGLISH" | "INGLESE" => Some(Choice::En),
        "AUTO" | "" => Some(Choice::Auto),
        _ => None,
    }
}

/// `settings.language` from the host's settings.json.
fn host_language() -> Option<String> {
    let text = std::fs::read_to_string(config_dir()?.join("settings.json")).ok()?;
    language_from_settings(&text)
}

pub fn language_from_settings(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("settings")?.get("language")?.as_str().map(str::to_string)
}

/// The operating system's UI locale, e.g. `"it-IT"`.
fn os_locale() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85];
        // SAFETY: the buffer is valid for 85 UTF-16 units, the documented maximum.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return Some(String::from_utf16_lossy(&buf[..(n as usize - 1)]));
        }
        None
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn italian_tags() {
        for t in ["it", "it-IT", "it_CH.UTF-8", "IT-it"] {
            assert!(is_italian_tag(t), "{t}");
        }
        for t in ["en-US", "de-DE", "system", "", "iterate"] {
            assert!(!is_italian_tag(t), "{t}");
        }
    }

    #[test]
    fn reads_host_settings() {
        let j = r#"{"settings":{"osmode":4,"language":"it-IT","spell":{"language":"en-US"}},"theme":{}}"#;
        assert_eq!(language_from_settings(j).as_deref(), Some("it-IT"));
        assert_eq!(language_from_settings(r#"{"settings":{"language":"system"}}"#).as_deref(), Some("system"));
        assert_eq!(language_from_settings(r#"{"settings":{}}"#), None);
        assert_eq!(language_from_settings("not json"), None);
    }

    #[test]
    fn choices() {
        assert_eq!(parse_choice("it"), Some(Choice::It));
        assert_eq!(parse_choice(" English\n"), Some(Choice::En));
        assert_eq!(parse_choice("auto"), Some(Choice::Auto));
        assert_eq!(parse_choice("klingon"), None);
    }
}

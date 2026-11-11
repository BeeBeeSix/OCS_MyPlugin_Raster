//! A small diagnostic log, `rastertools-log.txt` in Open CAD Studio's
//! settings folder: the commands the plugin receives, the steps of the
//! longer ones and any panic. It helps find out where a command stops when
//! the command line shows nothing. Kept under about 1 MB (one old copy).
//!
//! Off by default: `RT_DIAG ON` turns it on (a marker file,
//! `rastertools-diag.txt`, keeps the choice), `RT_DIAG OFF` turns it off.

use std::io::Write;
use std::sync::Once;

pub fn path() -> Option<std::path::PathBuf> {
    Some(crate::i18n::config_dir()?.join("rastertools-log.txt"))
}

fn marker() -> Option<std::path::PathBuf> {
    Some(crate::i18n::config_dir()?.join("rastertools-diag.txt"))
}

/// Whether the log is on.
pub fn enabled() -> bool {
    marker().is_some_and(|m| m.is_file())
}

/// Turn the log on or off.
pub fn set_enabled(on: bool) -> std::io::Result<()> {
    let m = marker().ok_or_else(|| std::io::Error::other("config folder not found"))?;
    if on {
        if let Some(d) = m.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&m, "on\n")
    } else {
        match std::fs::remove_file(&m) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Append one line (with time and process id) when the log is on. Never
/// fails loudly.
pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    let Some(p) = path() else { return };
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::rename(&p, p.with_extension("old.txt"));
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let (h, m, s, ms) = ((secs / 3_600_000) % 24, (secs / 60_000) % 60, (secs / 1000) % 60, secs % 1000);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(
            f,
            "{h:02}:{m:02}:{s:02}.{ms:03} UTC [{} v{}] {msg}",
            std::process::id(),
            env!("CARGO_PKG_VERSION")
        );
    }
}

/// Log panics too (once per process), keeping the default report.
pub fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log(&format!("PANIC: {info}"));
            previous(info);
        }));
    });
}

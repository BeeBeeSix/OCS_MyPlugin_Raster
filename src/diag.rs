//! A small diagnostic log, `rastertools-log.txt` in Open CAD Studio's
//! settings folder: the commands the plugin receives, the steps of the
//! longer ones and any panic. It helps find out where a command stops when
//! the command line shows nothing. Kept under about 1 MB (one old copy).

use std::io::Write;
use std::sync::Once;

fn path() -> Option<std::path::PathBuf> {
    Some(crate::i18n::config_dir()?.join("rastertools-log.txt"))
}

/// Append one line (with time and process id). Never fails loudly.
pub fn log(msg: &str) {
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

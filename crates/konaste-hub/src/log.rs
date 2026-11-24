//! `hub.log` in the data folder: one line per notable failure, for support.
//!
//! Lines carry error codes, stage names, and numbers only. Never a token,
//! cookie, account field, or page body.
use std::{io::Write, path::PathBuf, sync::Mutex};

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX_BYTES: u64 = 256 * 1024;

pub fn init(directory: &std::path::Path) {
    let path = directory.join("hub.log");
    // Keep one previous file when this one grows past the cap.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, directory.join("hub.old.log"));
    }
    *FILE.lock().unwrap() = Some(path);
}

pub fn line(text: impl AsRef<str>) {
    let Some(path) = FILE.lock().unwrap().clone() else { return };
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let text: String = text.as_ref().chars().filter(|c| !c.is_control()).take(400).collect();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {text}", stamp(seconds));
    }
}

/// UTC `YYYY-MM-DD HH:MM:SS`.
fn stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamps_are_utc_calendar_dates() {
        assert_eq!(super::stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::stamp(1_795_132_800 + 3_723), "2026-11-20 01:02:03Z");
    }
}

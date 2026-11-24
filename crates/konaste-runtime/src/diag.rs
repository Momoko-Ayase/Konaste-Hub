//! Secret-free failure notes for the SDK worker.
//!
//! The worker records the stage it reached and the numeric SDK getters at a
//! failure, then prints one ASCII line on stderr before it exits. The parent
//! keeps the last line so the host can log why an attempt failed. Notes hold
//! static stage names and integers only: never a token, path, URL, or response.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Worker side: the last stage entered and any SDK values seen there.
static NOTE: Mutex<String> = Mutex::new(String::new());

/// Parent side: (spawned, finished, last line).
static LAST: Mutex<(u64, u64, String)> = Mutex::new((0, 0, String::new()));

const MAX_LINE: usize = 240;

/// Worker: entering `name`. Clears earlier values.
pub(crate) fn stage(name: &'static str) {
    let mut note = NOTE.lock().unwrap_or_else(|e| e.into_inner());
    note.clear();
    note.push_str(name);
}

/// Worker: numeric values observed in the current stage.
pub(crate) fn detail(values: &[(&'static str, i64)]) {
    let mut note = NOTE.lock().unwrap_or_else(|e| e.into_inner());
    for (key, value) in values {
        if note.len() < MAX_LINE {
            note.push_str(&format!(" {key}={value}"));
        }
    }
}

/// Worker: print the failure line. Called once, just before a failed exit.
pub(crate) fn report(error: &crate::RuntimeError) {
    use std::io::Write;
    let note = NOTE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut line = format!("{error:?} at {note}");
    line.truncate(MAX_LINE);
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

/// Parent: a worker was spawned and its stderr is being read.
pub(crate) fn spawned() -> u64 {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.0 += 1;
    last.0
}

/// Parent: the reader for worker `id` reached end of stream.
pub(crate) fn finished(id: u64, bytes: &[u8]) {
    let text = clean(bytes);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if id >= last.1 {
        last.1 = id;
        last.2 = text;
    }
}

/// The last line in this module's own format: `<Error> at <stage>` plus
/// `key=<integer>` pairs. Anything else the official libraries print (their
/// own logs may carry URLs or request data) is dropped.
fn clean(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines().rev().map(str::trim).find(|line| ours(line)).unwrap_or("").to_owned()
}

fn ours(line: &str) -> bool {
    let mut words = line.split(' ');
    let error = words.next().unwrap_or("");
    let at = words.next();
    let stage = words.next().unwrap_or("");
    line.len() <= MAX_LINE
        && !error.is_empty()
        && error.bytes().all(|b| b.is_ascii_alphabetic())
        && at == Some("at")
        && !stage.is_empty()
        && stage.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        && words.all(|pair| {
            pair.split_once('=').is_some_and(|(key, value)| {
                let digits = value.strip_prefix('-').unwrap_or(value);
                !key.is_empty()
                    && key.bytes().all(|b| b.is_ascii_lowercase())
                    && !digits.is_empty()
                    && digits.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

/// The last worker's failure line, waiting briefly for its stderr to close.
pub fn last_worker_note() -> Option<String> {
    let deadline = Instant::now() + Duration::from_millis(800);
    loop {
        {
            let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if last.1 >= last.0 || Instant::now() >= deadline {
                return (!last.2.is_empty()).then(|| last.2.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_this_modules_own_line_is_kept() {
        let output =
            b"[avs] connect https://example.invalid/?tk=SECRET\r\nRuntime at boot-wait state=2 result=2 error=2004 detail=-1\r\n\x00\xff";
        assert_eq!(super::clean(output), "Runtime at boot-wait state=2 result=2 error=2004 detail=-1");
        assert_eq!(super::clean(b"Runtime at boot-wait tk=abc\n"), "");
        assert_eq!(super::clean(b"Runtime at https://x\n"), "");
        assert_eq!(super::clean(b"log line only\n"), "");
    }
}

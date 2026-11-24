//! Native-only transfer sampling. Retained content is not network throughput.
use konaste_updater::{SessionPhase, TransferSnapshot};
use serde::Serialize;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub bytes_received: u64,
    pub bytes_total: u64,
    pub files_completed: u64,
    pub files_total: u64,
    pub bytes_per_second: Option<u64>,
    pub files: Vec<File>,
    /// The engine is waiting out a network outage. No speed is reported and
    /// progress is frozen on purpose, so the UI shows a waiting state.
    pub waiting_for_network: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub files_completed: u64,
    pub files_total: u64,
    pub bytes_completed: u64,
    pub bytes_total: u64,
    pub current_file: Option<String>,
}

impl From<&konaste_updater::WorkSnapshot> for Work {
    fn from(value: &konaste_updater::WorkSnapshot) -> Self {
        Self {
            files_completed: value.files_completed.min(value.files_total),
            files_total: value.files_total,
            bytes_completed: if value.bytes_total == 0 { value.bytes_completed } else { value.bytes_completed.min(value.bytes_total) },
            bytes_total: value.bytes_total,
            current_file: value.current.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub name: String,
    pub bytes_received: u64,
    pub bytes_total: u64,
    pub state: FileState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileState {
    Active,
    Retrying,
}

impl Snapshot {
    pub fn stopped(value: &TransferSnapshot) -> Self {
        Self {
            bytes_received: value.progress.bytes_received.min(value.progress.bytes_total),
            bytes_total: value.progress.bytes_total,
            files_completed: value.progress.files_completed.min(value.progress.files_total),
            files_total: value.progress.files_total,
            bytes_per_second: None,
            files: Vec::new(),
            waiting_for_network: value.waiting_for_network,
        }
    }

    pub fn stop(&mut self) {
        self.bytes_per_second = None;
        self.files.clear();
    }
}

#[derive(Default)]
pub struct Meter {
    samples: VecDeque<(Instant, u64)>,
}

impl Meter {
    pub fn sample(&mut self, value: &TransferSnapshot, now: Instant) -> Snapshot {
        let mut snapshot = Snapshot::stopped(value);
        if value.progress.phase != SessionPhase::Downloading || value.progress.paused || value.waiting_for_network {
            self.samples.clear();
            return snapshot;
        }
        if self.samples.back().is_some_and(|&(time, bytes)| now < time || value.network_bytes < bytes) {
            self.samples.clear();
        }
        self.samples.push_back((now, value.network_bytes));
        while self.samples.len() > 2 && now.saturating_duration_since(self.samples[1].0) >= Duration::from_secs(3) {
            self.samples.pop_front();
        }
        if let Some(&(first, bytes)) = self.samples.front() {
            let elapsed = now.saturating_duration_since(first).as_millis();
            snapshot.bytes_per_second = (u128::from(value.network_bytes.saturating_sub(bytes)) * 1000)
                .checked_div(elapsed)
                .map(|rate| rate.min(u128::from(u64::MAX)) as u64);
        }
        snapshot.files = value
            .files
            .iter()
            .take(8)
            .map(|file| File {
                name: file.relative.clone(),
                bytes_received: file.bytes_received.min(file.bytes_total),
                bytes_total: file.bytes_total,
                state: if file.retrying { FileState::Retrying } else { FileState::Active },
            })
            .collect();
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use konaste_updater::{FileProgress, Progress};

    fn transfer() -> TransferSnapshot {
        TransferSnapshot {
            progress: Progress {
                phase: SessionPhase::Downloading,
                files_total: 10,
                files_completed: 4,
                bytes_total: 10_000,
                bytes_received: 4_000,
                paused: false,
            },
            network_bytes: 0,
            waiting_for_network: false,
            files: vec![FileProgress { relative: "game/data/file.bin".into(), bytes_received: 200, bytes_total: 1_000, retrying: false }],
        }
    }

    #[test]
    fn local_work_keeps_measured_bytes_when_the_total_is_unknown() {
        let work = konaste_updater::WorkSnapshot {
            phase: SessionPhase::Preparing,
            files_completed: 1,
            files_total: 2,
            bytes_completed: 7,
            bytes_total: 0,
            current: Some("game/data/file.bin".into()),
        };
        let snapshot = Work::from(&work);
        assert_eq!(snapshot.bytes_completed, 7);
        assert_eq!(snapshot.bytes_total, 0);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["currentFile"], "game/data/file.bin");
        assert_eq!(json["bytesCompleted"], 7);
        assert!(json.get("bytesPerSecond").is_none());
    }

    #[test]
    fn speed_counts_actual_network_writes_not_retained_or_restarted_bytes() {
        let now = Instant::now();
        let mut meter = Meter::default();
        let mut value = transfer();
        assert_eq!(meter.sample(&value, now).bytes_per_second, None);
        value.network_bytes = 200;
        value.progress.bytes_received = 4_200;
        assert_eq!(meter.sample(&value, now + Duration::from_millis(500)).bytes_per_second, Some(400));
        // Another attempt can transfer bytes without increasing retained content.
        value.network_bytes = 400;
        assert_eq!(meter.sample(&value, now + Duration::from_secs(1)).bytes_per_second, Some(400));
        value.network_bytes = 0;
        assert_eq!(meter.sample(&value, now + Duration::from_secs(2)).bytes_per_second, None);
    }

    #[test]
    fn rate_decays_to_zero_on_stall_and_is_never_live_outside_downloading() {
        let now = Instant::now();
        let mut meter = Meter::default();
        let mut value = transfer();
        meter.sample(&value, now);
        value.network_bytes = 1_000;
        assert_eq!(meter.sample(&value, now + Duration::from_secs(1)).bytes_per_second, Some(1_000));
        meter.sample(&value, now + Duration::from_secs(3));
        assert_eq!(meter.sample(&value, now + Duration::from_secs(5)).bytes_per_second, Some(0));
        for phase in [
            SessionPhase::Paused,
            SessionPhase::Failed,
            SessionPhase::Verifying,
            SessionPhase::Installing,
            SessionPhase::Recovering,
            SessionPhase::Idle,
        ] {
            value.progress.phase = phase;
            let snapshot = meter.sample(&value, now + Duration::from_secs(6));
            assert_eq!(snapshot.bytes_per_second, None);
            assert!(snapshot.files.is_empty());
            assert_eq!(snapshot.bytes_received, 4_000);
        }
        value.progress.phase = SessionPhase::Downloading;
        assert_eq!(meter.sample(&value, now + Duration::from_secs(7)).bytes_per_second, None);
        value.progress.paused = true;
        assert!(meter.sample(&value, now + Duration::from_secs(8)).files.is_empty());
    }

    #[test]
    fn a_network_wait_has_no_speed_and_keeps_the_frozen_byte_marks() {
        let now = Instant::now();
        let mut meter = Meter::default();
        let mut value = transfer();
        meter.sample(&value, now);
        value.network_bytes = 1_000;
        value.progress.bytes_received = 5_000;
        assert_eq!(meter.sample(&value, now + Duration::from_secs(1)).bytes_per_second, Some(1_000));
        value.waiting_for_network = true;
        let snapshot = meter.sample(&value, now + Duration::from_secs(2));
        assert!(snapshot.waiting_for_network);
        assert_eq!(snapshot.bytes_per_second, None);
        assert!(snapshot.files.is_empty());
        // Bytes already retained are still shown; nothing is fabricated.
        assert_eq!(snapshot.bytes_received, 5_000);
        assert_eq!(snapshot.bytes_total, 10_000);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["waitingForNetwork"], true);
        // Once the wait ends, sampling resumes normally.
        value.waiting_for_network = false;
        assert!(!meter.sample(&value, now + Duration::from_secs(3)).waiting_for_network);
    }

    #[test]
    fn ipc_exposes_only_bounded_relative_files_and_typed_retry_state() {
        let mut value = transfer();
        value.files[0].retrying = true;
        value.files = vec![value.files[0].clone(); 12];
        let snapshot = Meter::default().sample(&value, Instant::now());
        assert_eq!(snapshot.files.len(), 8);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["files"][0]["name"], "game/data/file.bin");
        assert_eq!(json["files"][0]["state"], "retrying");
        assert!(json["bytesPerSecond"].is_null());
        let mut stopped = snapshot;
        stopped.stop();
        assert!(stopped.files.is_empty());
        assert_eq!(stopped.bytes_received, 4_000);
    }
}

use crate::{hub::Hub, state::HubError};
use serde::Serialize;
use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const LOGIN_URL: &str = "https://p.eagate.573.jp/game/konasteapp/API/login/login.html";
const PROBE_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    None,
    Scheduled,
    Service,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub kind: Kind,
    pub ends_at: Option<u64>,
}

#[derive(Default)]
struct Observation {
    service: bool,
    revision: u64,
}
impl Observation {
    fn confirm_503(&mut self) {
        self.service = true;
        self.revision = self.revision.saturating_add(1);
    }

    fn receive(&mut self, revision: u64, status: Option<u16>) -> bool {
        if status == Some(503) {
            self.confirm_503();
        } else if matches!(status, Some(200..=299)) && revision == self.revision && revision != u64::MAX {
            self.service = false;
            return true;
        }
        false
    }

    fn snapshot(&self, now: u64) -> Snapshot {
        // An observed outage may extend past the published recurring window.
        if self.service {
            return Snapshot { kind: Kind::Service, ends_at: None };
        }
        match scheduled_end(now) {
            Some(end) => Snapshot { kind: Kind::Scheduled, ends_at: end.checked_mul(1000) },
            None => Snapshot { kind: Kind::None, ends_at: None },
        }
    }
}

pub struct Maintenance {
    clock: fn() -> u64,
    observed: Mutex<Observation>,
    probe: tokio::sync::Mutex<()>,
    client: Option<reqwest::Client>,
}
impl Default for Maintenance {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(6))
            .user_agent("Konaste Hub/0.1.0")
            .build()
            .ok();
        Self { clock: unix_seconds, observed: Mutex::new(Observation::default()), probe: tokio::sync::Mutex::new(()), client }
    }
}
impl Maintenance {
    pub fn snapshot(&self) -> Snapshot {
        self.observed.lock().unwrap().snapshot((self.clock)())
    }

    #[cfg(test)]
    pub(crate) fn with_clock(clock: fn() -> u64) -> Self {
        Self { clock, ..Self::default() }
    }

    pub fn ensure_allowed(&self) -> Result<(), HubError> {
        if self.snapshot().kind != Kind::None {
            return Err(HubError { code: "maintenance" });
        }
        Ok(())
    }

    pub fn confirm_503(&self) {
        self.observed.lock().unwrap().confirm_503();
    }

    pub async fn refresh(&self) -> Result<(), HubError> {
        let _probe = self.probe.lock().await;
        let revision = self.observed.lock().unwrap().revision;
        let status = match &self.client {
            Some(client) => probe_status(client, LOGIN_URL).await,
            None => None,
        };
        let healthy = self.observed.lock().unwrap().receive(revision, status);
        // Check the current clock again: a maintenance window may begin during I/O.
        self.ensure_allowed()?;
        if !healthy {
            return Err(HubError { code: "availabilityUnavailable" });
        }
        Ok(())
    }
}

async fn probe_status(client: &reqwest::Client, url: &str) -> Option<u16> {
    // Fixed anonymous GET: no cookies, authorization, redirect following, or body reads.
    client
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .send()
        .await
        .ok()
        .map(|response| response.status().as_u16())
}

pub fn start_monitor(app: Hub) {
    let hub = app.clone();
    hub.spawn(async move {
        let mut previous = None;
        let mut last_probe = None;
        loop {
            let state = app.state();
            if last_probe.is_none_or(|last: Instant| last.elapsed() >= PROBE_INTERVAL) {
                let _ = state.maintenance.refresh().await;
                last_probe = Some(Instant::now());
            }
            let current = state.maintenance.snapshot();
            if previous.as_ref() != Some(&current) {
                app.publish();
                previous = Some(current);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

// All current catalog titles use this schedule. The medal-title weekend exception
// does not apply to them. Source: docs/research/maintenance.md (2026-10-15).
fn scheduled_end(now: u64) -> Option<u64> {
    const DAY: u64 = 86_400;
    const HOUR: u64 = 3_600;
    let jst = now.checked_add(9 * HOUR)?;
    let time = jst % DAY;
    if !(2 * HOUR..7 * HOUR).contains(&time) {
        return None;
    }
    // Unix day zero was Thursday; Sunday is zero.
    let tuesday = (jst / DAY + 4) % 7 == 2;
    let extended = tuesday && (15..=21).contains(&day_of_month(jst / DAY));
    let start = if extended { 2 * HOUR } else { 5 * HOUR };
    if time < start {
        return None;
    }
    now.checked_add(7 * HOUR - time)
}

fn day_of_month(days_since_epoch: u64) -> u64 {
    // Gregorian leap years repeat every 400 years, bounding the conversion even
    // for an unusually large clock value. No host timezone or DST rules are used.
    let mut days = days_since_epoch % 146_097;
    let mut year = 1970;
    let leap = |year: u64| year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    for length in [31, if leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] {
        if days < length {
            return days + 1;
        }
        days -= length;
    }
    unreachable!("day within a Gregorian year")
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

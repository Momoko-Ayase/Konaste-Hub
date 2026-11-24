//! Per-player e-amusement play data for the selected title.
//!
//! Only titles with a first-party per-player page are covered. Pages are read
//! with the signed-in native client (`account::Session`), parsed inertly, and
//! cached per session. A title with no data, or a page whose layout is not the
//! expected one, yields nothing.
use crate::{catalog::GameId, hub::Hub};
use scraper::{Html, Selector};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Play data changes only after a session, so one read per ten minutes is enough.
const TTL: Duration = Duration::from_secs(600);
/// A failed read is retried sooner, in case the failure was the network.
const RETRY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Volforce,
    Skill,
    PlayCount,
    LastPlay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub kind: Kind,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayData {
    pub name: Option<String>,
    pub stats: Vec<Stat>,
}

impl PlayData {
    fn is_empty(&self) -> bool {
        self.name.is_none() && self.stats.is_empty()
    }

    fn push(&mut self, kind: Kind, value: Option<String>) {
        if let Some(value) = value
            && !self.stats.iter().any(|stat| stat.kind == kind)
        {
            self.stats.push(Stat { kind, value });
        }
    }
}

/// Titles with a real per-player page. The rest show nothing.
///
/// SOUND VOLTEX keeps a Konaste profile page whose layout matches the arcade
/// site build it ships from. DDR has no Konaste page; while a basic course is
/// active the Konaste data is the arcade WORLD data, so the arcade status page
/// is the same player's. NOSTALGIA, GITADORA, pop'n Lively and Bombergirl
/// keep separate data and expose no page read here.
fn source(id: GameId) -> Option<&'static str> {
    Some(match id {
        GameId::Sdvx => "https://p.eagate.573.jp/game/eacsdvx/vi/playdata/profile/index.html",
        GameId::Ddr => "https://p.eagate.573.jp/game/ddr/ddrworld/playdata/index.html",
        _ => return None,
    })
}

pub fn supported(id: GameId) -> bool {
    source(id).is_some()
}

struct Entry {
    at: Instant,
    data: Option<PlayData>,
    fetching: bool,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<GameId, Entry>,
    epoch: u64,
}

/// Session-scoped cache. A sign-in change clears it and drops in-flight reads.
#[derive(Default)]
pub struct Cache {
    inner: Mutex<Inner>,
}

impl Cache {
    /// `Some(epoch)` when the caller should fetch; `None` when fresh or busy.
    pub fn begin(&self, id: GameId) -> Option<u64> {
        if !supported(id) {
            return None;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner
            .entries
            .get(&id)
            .is_some_and(|entry| entry.fetching || (entry.data.is_some() && entry.at.elapsed() < TTL) || entry.at.elapsed() < RETRY)
        {
            return None;
        }
        let data = inner.entries.get(&id).and_then(|entry| entry.data.clone());
        inner.entries.insert(id, Entry { at: Instant::now(), data, fetching: true });
        Some(inner.epoch)
    }

    /// Store a read. A result from before the last sign-in change is dropped.
    pub fn finish(&self, id: GameId, epoch: u64, data: Option<PlayData>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.epoch != epoch {
            return;
        }
        inner.entries.insert(id, Entry { at: Instant::now(), data: data.filter(|d| !d.is_empty()), fetching: false });
    }

    pub fn data(&self, id: GameId) -> Option<PlayData> {
        self.inner.lock().unwrap().entries.get(&id).and_then(|entry| entry.data.clone())
    }

    pub fn invalidate(&self, id: GameId) {
        self.inner.lock().unwrap().entries.remove(&id);
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.entries.clear();
        inner.epoch = inner.epoch.wrapping_add(1);
    }
}

/// Read one title's page. Any redirect, error page, or unexpected layout is `None`.
pub async fn fetch(hub: &Hub, id: GameId) -> Option<PlayData> {
    let url = source(id)?;
    let (final_url, body) = hub.state().session.page(url).await.ok()?;
    let expected = url::Url::parse(url).ok()?;
    if final_url.scheme() != "https" || final_url.host_str() != Some("p.eagate.573.jp") || final_url.path() != expected.path() {
        return None;
    }
    match id {
        GameId::Sdvx => parse_sdvx(&body),
        GameId::Ddr => parse_ddr(&body),
        _ => None,
    }
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("fixed selector")
}

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text_of(node: scraper::ElementRef<'_>) -> String {
    clean(&node.text().collect::<String>())
}

fn bounded(value: String, max: usize) -> Option<String> {
    let placeholder = value.chars().all(|c| matches!(c, '-' | '−' | '―' | '—' | '…' | '?'));
    (!value.is_empty() && !placeholder && value.chars().count() <= max && !value.chars().any(char::is_control)).then_some(value)
}

fn number(value: &str) -> Option<u64> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 9 || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    digits.parse().ok()
}

/// First `YYYY-MM-DD` / `YYYY/MM/DD` token of a timestamp.
fn day(value: &str) -> Option<String> {
    let first = value.split_whitespace().next()?;
    let bytes = first.as_bytes();
    let shaped = bytes.len() >= 8
        && bytes.len() <= 19
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && matches!(bytes[4], b'-' | b'/')
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && matches!(bytes[7], b'-' | b'/');
    shaped.then(|| first.to_owned())
}

/// Decimal like `18.742`, bounded to four integer digits.
fn decimal(value: &str) -> Option<String> {
    let value = clean(value);
    let mut seen_dot = false;
    let shaped = !value.is_empty()
        && value.len() <= 8
        && value.bytes().all(|b| {
            if b.is_ascii_digit() {
                true
            } else if b == b'.' && !seen_dot {
                seen_dot = true;
                true
            } else {
                false
            }
        })
        && value.bytes().any(|b| b.is_ascii_digit());
    shaped.then_some(value)
}

/// SOUND VOLTEX profile. Labels are read by their Japanese text so both the
/// arcade and Konaste variants of the same layout resolve.
fn parse_sdvx(html: &str) -> Option<PlayData> {
    let document = Html::parse_document(html);
    let mut data = PlayData::default();

    // The first paragraph is the profile comment; the second is the name.
    let names: Vec<_> = document.select(&selector("#player_name p")).collect();
    data.name = names.get(1).map(|node| text_of(*node)).and_then(|value| bounded(value, 64));

    let volforce = document.select(&selector("#force_point")).next().and_then(|node| decimal(&text_of(node)));
    data.push(Kind::Volforce, volforce);

    for item in document.select(&selector("#profile_li li")).take(24) {
        let Some(label) = item.select(&selector(".profile_col")).next() else { continue };
        let label = text_of(label);
        let value = item
            .select(&selector(".profile_skill"))
            .next()
            .or_else(|| item.select(&selector(".profile_cnt")).next())
            .map(text_of)
            .unwrap_or_default();
        match label.as_str() {
            "プレー回数" => data.push(Kind::PlayCount, number(&value).map(|n| n.to_string())),
            "スキルLV" => data.push(Kind::Skill, bounded(value, 32)),
            "最終プレー日時" => data.push(Kind::LastPlay, day(&value)),
            _ => {}
        }
    }

    (!data.is_empty()).then_some(data)
}

/// DDR WORLD status block: label/value pairs. Konaste DDR shares this data
/// while a basic course is active.
fn parse_ddr(html: &str) -> Option<PlayData> {
    let document = Html::parse_document(html);
    let section = document.select(&selector(".status-section")).next()?;
    let mut data = PlayData::default();
    for item in section.select(&selector(".status-item")).take(32) {
        let (Some(label), Some(value)) = (item.select(&selector(".status-label")).next(), item.select(&selector(".status-data")).next())
        else {
            continue;
        };
        let label = text_of(label);
        let value = text_of(value);
        match label.as_str() {
            "DANCER NAME" => {
                if data.name.is_none() {
                    data.name = bounded(value, 64);
                }
            }
            "総プレー回数" => data.push(Kind::PlayCount, number(&value).map(|n| n.to_string())),
            "最終プレー日時" => data.push(Kind::LastPlay, day(&value)),
            _ => {}
        }
    }
    (data.name.is_some() || !data.stats.is_empty()).then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDVX: &str = r#"
    <div id="player_id">1234-5678</div>
    <div id="player_name"><p></p><p>TEST PLAYER</p></div>
    <div id="force"><div class="force_class" id="force_class_s">A</div><div id="force_point">18.742</div></div>
    <ul id="profile_li">
      <li><div class="profile_col">プレー回数</div><div class="profile_cnt">1,234回</div></li>
      <li><div class="profile_col">スキルLV</div><div class="profile_skill">11</div></li>
      <li><div class="profile_col">最終プレー日時</div><div class="profile_cnt">2026-11-19 21:03:11</div></li>
      <li><div class="profile_col">最終プレー店舗</div><div class="profile_cnt">TEST STORE</div></li>
    </ul>"#;

    #[test]
    fn sdvx_reads_headline_fields_only() {
        let data = parse_sdvx(SDVX).unwrap();
        assert_eq!(data.name.as_deref(), Some("TEST PLAYER"));
        assert_eq!(
            data.stats,
            vec![
                Stat { kind: Kind::Volforce, value: "18.742".into() },
                Stat { kind: Kind::PlayCount, value: "1234".into() },
                Stat { kind: Kind::Skill, value: "11".into() },
                Stat { kind: Kind::LastPlay, value: "2026-11-19".into() },
            ]
        );
        assert!(parse_sdvx("<div id=\"error\">ERROR</div>").is_none());
        assert!(parse_sdvx("<div id=\"player_name\"><p>only</p></div>").is_none());
        // A profile with only a placeholder comment and no data shows nothing.
        assert!(parse_sdvx("<div id=\"player_name\"><p>-</p><p>---</p></div>").is_none());
    }

    const DDR: &str = r#"
    <div class="status-section"><div class="section-data">
      <div class="status-item wide"><div class="status-label">DANCER NAME</div><div class="status-data">TEST DANCER</div></div>
      <div class="status-item"><div class="status-label">DDR-CODE</div><div class="status-data">1234-5678</div></div>
      <div class="status-item"><div class="status-label">総プレー回数</div><div class="status-data">1,380回</div></div>
      <div class="status-item"><div class="status-label">最終プレー日時</div><div class="status-data">2026-10-17 15:59:32</div></div>
    </div></div>"#;

    #[test]
    fn ddr_reads_status_pairs() {
        let data = parse_ddr(DDR).unwrap();
        assert_eq!(data.name.as_deref(), Some("TEST DANCER"));
        assert_eq!(
            data.stats,
            vec![Stat { kind: Kind::PlayCount, value: "1380".into() }, Stat { kind: Kind::LastPlay, value: "2026-10-17".into() },]
        );
        assert!(parse_ddr("<div class=\"no-play\">NO PLAY DATA</div>").is_none());
    }

    #[test]
    fn cache_is_cleared_by_a_sign_in_change() {
        let cache = Cache::default();
        let epoch = cache.begin(GameId::Sdvx).unwrap();
        cache.finish(GameId::Sdvx, epoch, Some(PlayData { name: Some("A".into()), stats: Vec::new() }));
        assert!(cache.data(GameId::Sdvx).is_some());
        assert!(cache.begin(GameId::Sdvx).is_none());
        cache.clear();
        assert!(cache.data(GameId::Sdvx).is_none());
        let stale = cache.begin(GameId::Sdvx).unwrap();
        cache.clear();
        cache.finish(GameId::Sdvx, stale, Some(PlayData { name: Some("B".into()), stats: Vec::new() }));
        assert!(cache.data(GameId::Sdvx).is_none());
        assert!(cache.begin(GameId::BomberGirl).is_none());
    }

    #[test]
    fn values_are_bounded() {
        assert_eq!(number("01"), None);
        assert_eq!(number("1234567890"), None);
        assert_eq!(day("2026-10-17 15:59:32").as_deref(), Some("2026-10-17"));
        assert_eq!(day("15:59"), None);
        assert_eq!(decimal("18.742").as_deref(), Some("18.742"));
        assert_eq!(decimal("1.2.3"), None);
        assert_eq!(bounded("a".repeat(65), 64), None);
    }
}

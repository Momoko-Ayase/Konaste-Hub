//! Native readers for the signed-in e-amusement pages.
//!
//! Selectors are the ones mapped from the requester-supplied signed-in layout
//! (docs/research/eamusement-profile.md). Each field is taken only when every
//! read page agrees on one value.
use scraper::{Html, Selector};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountProfile {
    pub name: Option<String>,
    /// The KONAMI ID the account signs in with.
    pub konami_id: Option<String>,
    /// The memo the owner gave the shown card on the pass page.
    pub card_memo: Option<String>,
    pub avatar_url: Option<String>,
    pub card_number: Option<String>,
    pub paseli_balance: Option<u64>,
    pub paseli_points: Option<u64>,
}

/// What a portal page says about the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginFlag {
    SignedIn { pass: bool },
    SignedOut,
    Unknown,
}

/// `ea_common_template.userstatus.state` is inlined in every portal page.
pub fn login_flag(html: &str) -> LoginFlag {
    let Some(start) = html.find("\"state\"") else { return LoginFlag::Unknown };
    let tail = &html[start..html.len().min(start + 600)];
    let flag = |key: &str| -> Option<bool> {
        let at = tail.find(&format!("\"{key}\""))?;
        let rest = tail[at + key.len() + 2..].trim_start().strip_prefix(':')?.trim_start();
        if rest.starts_with("true") {
            Some(true)
        } else if rest.starts_with("false") {
            Some(false)
        } else {
            None
        }
    };
    match flag("login") {
        Some(true) => LoginFlag::SignedIn { pass: flag("eapass") == Some(true) },
        Some(false) => LoginFlag::SignedOut,
        None => LoginFlag::Unknown,
    }
}

#[derive(Default)]
pub struct Collector {
    names: BTreeSet<String>,
    avatars: BTreeSet<String>,
    cards: BTreeSet<String>,
    balances: BTreeSet<u64>,
    points: BTreeSet<u64>,
}

impl Collector {
    /// Read one page. `base` resolves a relative portrait URL.
    pub fn read(&mut self, html: &str, base: &url::Url) {
        let pass = matches!(login_flag(html), LoginFlag::SignedIn { pass: true });
        let document = Html::parse_document(html);
        let texts = |selector: &str| -> Vec<String> {
            let selector = Selector::parse(selector).expect("fixed selector");
            let nodes: Vec<_> = document.select(&selector).collect();
            if nodes.len() > 8 {
                return Vec::new();
            }
            nodes
                .into_iter()
                .filter(|node| matches!(node.value().name(), "div" | "span"))
                .map(|node| node.text().collect::<String>().trim().to_owned())
                .filter(|text| !text.is_empty() && text.len() <= 256)
                .collect()
        };
        for name in texts(".cl_profile > div:nth-child(2) > div:first-child") {
            if let Some(name) = name_value(&name) {
                self.names.insert(name);
            }
        }
        if pass {
            for card in texts("#id_ea_common_content > div:first-child > div:first-child > div:first-child > a > span") {
                if let Some(card) = card_value(&card) {
                    self.cards.insert(card);
                }
            }
        }
        for value in texts(".cl_eapass_body .cl_paseli_info > .cl_paseli_info_paseli") {
            if let Some(value) = number_value(&value) {
                self.balances.insert(value);
            }
        }
        for value in texts(".cl_eapass_body .cl_paseli_info > .cl_paseli_info_point") {
            if let Some(value) = number_value(&value) {
                self.points.insert(value);
            }
        }
        let images = Selector::parse(".cl_profile > div:first-child > img").expect("fixed selector");
        let images: Vec<_> = document.select(&images).collect();
        if images.len() <= 8 {
            for image in images {
                if let Some(url) = image.value().attr("src").and_then(|src| official_url(src, base)) {
                    self.avatars.insert(url);
                }
            }
        }
    }

    pub fn finish(self) -> AccountProfile {
        fn one<T: Clone>(set: BTreeSet<T>) -> Option<T> {
            if set.len() == 1 { set.into_iter().next() } else { None }
        }
        AccountProfile {
            name: one(self.names),
            konami_id: None,
            card_memo: None,
            avatar_url: one(self.avatars),
            card_number: one(self.cards),
            paseli_balance: one(self.balances),
            paseli_points: one(self.points),
        }
    }
}

/// The KONAMI ID from the signed-in header templates
/// (`/gate/p/common/multi/ea_common_multi_lang.html`): the text of the
/// `eavd_header_konamiid` entry. Entries are `@`-separated `key:html` blocks.
pub fn konami_id(templates: &str) -> Option<String> {
    let block = templates.split('@').find_map(|part| part.trim_start().strip_prefix("eavd_header_konamiid:"))?;
    let document = Html::parse_fragment(block);
    let selector = Selector::parse("span").expect("fixed selector");
    let ids: BTreeSet<String> = document
        .select(&selector)
        .map(|node| node.text().collect::<String>().trim().to_owned())
        .filter(|text| konami_id_value(text))
        .collect();
    if ids.len() == 1 { ids.into_iter().next() } else { None }
}

/// KONAMI IDs are 8-32 of lowercase ASCII letters, digits, `-`, `_`, `.`.
fn konami_id_value(value: &str) -> bool {
    (8..=32).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
}

/// The memo of `card` in the pass list (`/gate/eapass/api/getlist.html`).
pub fn card_memo(list: &str, card: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct List {
        status: i64,
        data: Data,
    }
    #[derive(serde::Deserialize)]
    struct Data {
        card: Vec<Card>,
    }
    #[derive(serde::Deserialize)]
    struct Card {
        cardnumber: String,
        #[serde(default)]
        cardmedia_memo: Option<String>,
    }
    let list: List = serde_json::from_str(list).ok()?;
    if list.status != 0 || list.data.card.len() > 16 {
        return None;
    }
    let memo = list.data.card.into_iter().find(|entry| card_value(&entry.cardnumber).as_deref() == Some(card))?.cardmedia_memo?;
    let memo = memo.trim();
    (!memo.is_empty() && memo.chars().count() <= 64 && !memo.chars().any(char::is_control)).then(|| memo.to_owned())
}

fn name_value(value: &str) -> Option<String> {
    (value.chars().count() <= 64 && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

fn card_value(value: &str) -> Option<String> {
    if value.len() > 24 || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-') {
        return None;
    }
    let compact: String = value.chars().filter(char::is_ascii_alphanumeric).collect();
    (compact.len() == 16).then(|| compact.to_ascii_uppercase())
}

fn number_value(value: &str) -> Option<u64> {
    let mut text = value.trim().replace('，', ",");
    for suffix in ["PASELI", "ポイント", "points", "point", "円", "P"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            text = stripped.trim().to_owned();
        }
    }
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b',') || text.starts_with(',') {
        return None;
    }
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    if digits.len() > 9 || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    digits.parse().ok()
}

fn official_url(value: &str, base: &url::Url) -> Option<String> {
    if value.len() > 2048 {
        return None;
    }
    let url = base.join(value).ok()?;
    (url.scheme() == "https"
        && url.host_str() == Some("p.eagate.573.jp")
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then(|| url.to_string())
}

/// Card numbers are shown masked by default.
pub fn mask_card(card: &str) -> String {
    let groups: Vec<String> = card
        .as_bytes()
        .chunks(4)
        .enumerate()
        .map(|(index, chunk)| if index == 3 { String::from_utf8_lossy(chunk).into_owned() } else { "••••".into() })
        .collect();
    groups.join(" ")
}

pub fn format_card(card: &str) -> String {
    card.as_bytes().chunks(4).map(|chunk| String::from_utf8_lossy(chunk).into_owned()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MYPAGE: &str = r##"<html><script>ea_common_template.userstatus = {"region":"jp","state":{"course":{},"eapass":true,"login":true,"playdata":true}};</script>
    <div id="id_ea_common_content"><div><div><div><a href="/x"><span>E004 0100 1234 5678</span></a></div></div></div>
    <div class="cl_profile"><div><img src="/gate/p/profile/img.html?x=1"></div><div><div>合成テスト</div><div class="cl_sp"><a>edit</a></div></div></div></div></html>"##;
    const MENU: &str = r#"<html><script>ea_common_template.userstatus = {"state":{"eapass":true,"login":true}};</script>
    <div class="cl_eapass_body"><div class="cl_paseli_info"><div class="cl_paseli_info_paseli">1,234</div><div class="cl_paseli_info_point">56</div></div></div></html>"#;

    #[test]
    fn signed_in_pages_produce_one_value_per_field() {
        let base = url::Url::parse("https://p.eagate.573.jp/gate/p/mypage/index.html").unwrap();
        let mut collector = Collector::default();
        collector.read(MYPAGE, &base);
        collector.read(MENU, &base);
        let profile = collector.finish();
        assert_eq!(profile.name.as_deref(), Some("合成テスト"));
        assert_eq!(profile.card_number.as_deref(), Some("E004010012345678"));
        assert_eq!(profile.paseli_balance, Some(1234));
        assert_eq!(profile.paseli_points, Some(56));
        assert_eq!(profile.avatar_url.as_deref(), Some("https://p.eagate.573.jp/gate/p/profile/img.html?x=1"));
    }

    #[test]
    fn konami_id_comes_from_the_header_template() {
        let templates = "\neavd_side_login:\nログイン\n@\neavd_header_konamiid:\n    <a href=\"/gate/p/mypage/index.html\">\n    <span class=\"cl_nav_menu_span\"><img src=\"/x.jpg\" /></span>\n    </a>\n    <span class=\"cl_nav_menu_span cl_pc_dsp\">\n      synthetic_id01\n    </span>\n@\neavd_lgfm_advice_konamiid_invalid:\nKONAMI IDは半角英小文字\n";
        assert_eq!(konami_id(templates).as_deref(), Some("synthetic_id01"));
        // Signed-out templates have no entry; a malformed value is refused.
        assert_eq!(konami_id("\neavd_side_login:\nログイン\n"), None);
        assert_eq!(konami_id("@eavd_header_konamiid:<span>Not An ID!</span>"), None);
    }

    #[test]
    fn card_memo_matches_the_shown_card() {
        let list = r#"{"data":{"card":[{"cardnumber":"E004010012345678","acccode":"0","is_active":true,"paseli_state":1,"playdata_memo":"","cardmedia_memo":"Aime - synthetic"},{"cardnumber":"E004000000000001","cardmedia_memo":"other"}]},"fail_code":0,"status":0,"max_stash":2}"#;
        assert_eq!(card_memo(list, "E004010012345678").as_deref(), Some("Aime - synthetic"));
        assert_eq!(card_memo(list, "E004000000000009"), None);
        assert_eq!(card_memo(&list.replace("Aime - synthetic", " "), "E004010012345678"), None);
        assert_eq!(card_memo(&list.replace("\"status\":0", "\"status\":1"), "E004010012345678"), None);
        assert_eq!(card_memo("<html>", "E004010012345678"), None);
    }

    #[test]
    fn login_flag_reads_the_inline_state() {
        assert_eq!(login_flag(MYPAGE), LoginFlag::SignedIn { pass: true });
        assert_eq!(login_flag(r#"{"state":{"eapass":null,"login":false}}"#), LoginFlag::SignedOut);
        assert_eq!(login_flag("<html></html>"), LoginFlag::Unknown);
    }

    #[test]
    fn card_is_hidden_without_an_active_pass_and_conflicts_drop_a_field() {
        let base = url::Url::parse("https://p.eagate.573.jp/").unwrap();
        let mut collector = Collector::default();
        collector.read(&MYPAGE.replace("\"eapass\":true", "\"eapass\":false"), &base);
        collector.read(&MENU.replace("1,234", "100"), &base);
        collector.read(MENU, &base);
        let profile = collector.finish();
        assert!(profile.card_number.is_none());
        assert!(profile.paseli_balance.is_none());
        assert_eq!(profile.paseli_points, Some(56));
    }

    #[test]
    fn values_are_bounded() {
        assert_eq!(number_value("1,234円"), Some(1234));
        assert_eq!(number_value("0"), Some(0));
        assert_eq!(number_value("01"), None);
        assert_eq!(number_value("-5"), None);
        assert_eq!(number_value("1234567890"), None);
        assert_eq!(card_value("E004-0100-1234-5678").as_deref(), Some("E004010012345678"));
        assert_eq!(card_value("1234"), None);
        let base = url::Url::parse("https://p.eagate.573.jp/").unwrap();
        assert!(official_url("https://evil.test/a.png", &base).is_none());
        assert_eq!(mask_card("E004010012345678"), "•••• •••• •••• 5678");
    }
}

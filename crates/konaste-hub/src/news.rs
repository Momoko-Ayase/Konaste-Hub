//! Official news and banners, read from each title's public top page.
use crate::catalog::GameId;
use scraper::{ElementRef, Html, Selector};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewsItem {
    pub date: String,
    pub title: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub image: String,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct News {
    pub items: Vec<NewsItem>,
    pub banners: Vec<Banner>,
}

pub fn official_site(id: GameId) -> Option<&'static str> {
    Some(match id {
        GameId::BomberGirl => "https://p.eagate.573.jp/game/eacbg/bg/index.html",
        GameId::Sdvx => "https://p.eagate.573.jp/game/eacsdvx/vi/index.html",
        GameId::Ddr => "https://p.eagate.573.jp/game/eacddr/konaddr/index.html",
        GameId::Popn => "https://p.eagate.573.jp/game/eacpopn/lively/index.html",
        GameId::Gitadora => "https://p.eagate.573.jp/game/eacgitadora/konagt/index.html",
        GameId::Nostalgia => "https://p.eagate.573.jp/game/eacnostalgia/op3/index.html",
        GameId::Infinitas => "https://p.eagate.573.jp/game/infinitas/2/index.html",
    })
}

pub async fn fetch(client: &reqwest::Client, id: GameId) -> Option<News> {
    let site = official_site(id)?;
    let response = client.get(site).timeout(Duration::from_secs(10)).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    let base = url::Url::parse(site).ok()?;
    Some(parse(&String::from_utf8_lossy(&bytes), &base))
}

fn clean(text: &str) -> String {
    let joined: Vec<&str> = text.split_whitespace().collect();
    let mut line = joined.join(" ");
    if line.chars().count() > 90 {
        line = line.chars().take(88).collect::<String>() + "…";
    }
    line
}

fn absolute(base: &url::Url, href: &str) -> Option<String> {
    let url = base.join(href).ok()?;
    (url.scheme() == "https").then(|| url.to_string())
}

fn first_text_line(element: ElementRef<'_>) -> String {
    // The first meaningful line of a news block: up to the first <br>.
    let mut out = String::new();
    for node in element.children() {
        if let Some(child) = ElementRef::wrap(node) {
            if child.value().name() == "br" {
                if !out.trim().is_empty() {
                    break;
                }
                continue;
            }
            if child.value().name() == "a" && out.trim().is_empty() {
                // The block starts with a link; its text is the headline.
                return clean(&child.text().collect::<String>());
            }
            out.push_str(&child.text().collect::<String>());
        } else if let Some(text) = node.value().as_text() {
            out.push_str(text);
        }
    }
    clean(&out)
}

pub fn parse(html: &str, base: &url::Url) -> News {
    let document = Html::parse_document(html);
    let sel = |s: &str| Selector::parse(s).expect("fixed selector");
    let mut items = Vec::new();

    // Layout used by Bombergirl, DDR, pop'n, GITADORA, and NOSTALGIA.
    for block in document.select(&sel(".news_list")).take(40) {
        let Some(head) = block.select(&sel(".news_head")).next() else { continue };
        let date: String = head.text().next().unwrap_or_default().trim().to_owned();
        let Some(body) = block.select(&sel(".news_link")).next() else { continue };
        let anchors: Vec<_> = body.select(&sel("a")).collect();
        // A block that starts with its own text uses the anchor only as a "details" link.
        let leading = first_text_line(body);
        let leading_text = !leading.is_empty() && !anchors.iter().any(|a| clean(&a.text().collect::<String>()) == leading);
        if leading_text && body.select(&sel(".bold_blue")).next().is_none() {
            let link = anchors.first().and_then(|a| a.value().attr("href")).and_then(|h| absolute(base, h));
            items.push(NewsItem { date: date.clone(), title: leading, link });
        } else if !anchors.is_empty() && body.select(&sel(".bold_blue")).next().is_none() {
            for anchor in anchors.into_iter().take(4) {
                let title = clean(&anchor.text().collect::<String>());
                if !title.is_empty() {
                    let link = anchor.value().attr("href").and_then(|h| absolute(base, h));
                    items.push(NewsItem { date: date.clone(), title, link });
                }
            }
        } else {
            let title = body
                .select(&sel(".bold_blue"))
                .next()
                .map(|b| clean(&b.text().collect::<String>()))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| first_text_line(body));
            if !title.is_empty() {
                items.push(NewsItem { date: date.clone(), title, link: None });
            }
        }
    }
    // SOUND VOLTEX layout.
    if items.is_empty() {
        for entry in document.select(&sel("ul.news > li")).take(20) {
            let Some(date) = entry.select(&sel("strong")).next() else { continue };
            let raw_date = date.text().collect::<String>().trim().to_owned();
            let date = raw_date.replace('.', "/");
            let mut text = String::new();
            let mut seen_date = false;
            for node in entry.children() {
                if !seen_date {
                    seen_date = ElementRef::wrap(node).is_some_and(|c| c.value().name() == "strong");
                    continue;
                }
                if let Some(child) = ElementRef::wrap(node) {
                    if child.value().name() == "br" {
                        if !text.trim().is_empty() {
                            break;
                        }
                        continue;
                    }
                    text.push_str(&child.text().collect::<String>());
                } else if let Some(t) = node.value().as_text() {
                    text.push_str(t);
                }
            }
            let title = clean(&text);
            if !title.is_empty() {
                let link = entry.select(&sel("a")).next().and_then(|a| a.value().attr("href")).and_then(|h| absolute(base, h));
                items.push(NewsItem { date, title, link });
            }
        }
    }
    items.truncate(12);

    let mut banners = Vec::new();
    for selector in ["#main_area li a", ".swiper-slide a", ".img_box a", ".detail_box.img_box a"] {
        for anchor in document.select(&sel(selector)).take(8) {
            let Some(image) = anchor.select(&sel("img")).next().and_then(|img| img.value().attr("src")) else { continue };
            let Some(image) = absolute(base, image) else { continue };
            let lower = image.to_ascii_lowercase();
            if !(lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png")) {
                continue;
            }
            if banners.iter().any(|b: &Banner| b.image == image) {
                continue;
            }
            let link = anchor.value().attr("href").and_then(|h| absolute(base, h));
            banners.push(Banner { image, link });
        }
        if !banners.is_empty() {
            break;
        }
    }
    banners.truncate(6);
    News { items, banners }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_common_and_sdvx_layouts() {
        let base = url::Url::parse("https://p.eagate.573.jp/game/eacbg/bg/index.html").unwrap();
        let common = r#"<ul id="main_area"><li><a href="/x.html"><img src="https://eacache.s.konaminet.jp/a.jpg"></a></li></ul>
        <li><div class="news_list"><div class="news_head">2026/11/10</div><div class="news_link">
        <a href="/game/eacbg/bg/news/a.html">Event A</a><br><a href="/b.html">Map update</a><br></div></div></li>
        <li><div class="news_list"><div class="news_head">2026/11/17<span> NEW !!</span></div><div class="news_link">
        <span class="bold_blue">Songs added</span><br>Long text</div></div></li>"#;
        let news = parse(common, &base);
        assert_eq!(news.items.len(), 3);
        assert_eq!(news.items[0].date, "2026/11/10");
        assert_eq!(news.items[0].link.as_deref(), Some("https://p.eagate.573.jp/game/eacbg/bg/news/a.html"));
        assert_eq!(news.items[2].title, "Songs added");
        assert_eq!(news.banners.len(), 1);
        let sdvx = r#"<ul class="news"><li><strong>2026.10.2</strong>
        First line here<br>second line<br><a href="/c.html">link</a></li></ul>"#;
        let news = parse(sdvx, &base);
        assert_eq!(news.items[0].date, "2026/10/2");
        assert_eq!(news.items[0].title, "First line here");
    }
}

#[cfg(test)]
mod live_samples {
    /// Reads pages saved to KONASTE_NEWS_SAMPLES (not committed). Skipped when unset.
    #[test]
    fn saved_official_pages_parse() {
        let Some(dir) = std::env::var_os("KONASTE_NEWS_SAMPLES") else { return };
        for (file, site) in [
            ("bg", "eacbg/bg"),
            ("sdvx", "eacsdvx/vi"),
            ("ddr", "eacddr/konaddr"),
            ("popn", "eacpopn/lively"),
            ("gita", "eacgitadora/konagt"),
            ("nost", "eacnostalgia/op3"),
        ] {
            let html = std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{file}.html"))).unwrap();
            let base = url::Url::parse(&format!("https://p.eagate.573.jp/game/{site}/index.html")).unwrap();
            let news = super::parse(&html, &base);
            println!("{file}: {} items, {} banners", news.items.len(), news.banners.len());
            for item in news.items.iter().take(3) {
                println!("  {} | {}", item.date, item.title);
            }
            for banner in news.banners.iter().take(2) {
                println!("  [img] {}", banner.image);
            }
            assert!(!news.items.is_empty(), "{file}");
        }
    }
}

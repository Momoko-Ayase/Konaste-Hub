# Konaste regular maintenance

Research dates: **2026-10-15–16**. Anonymous public GETs only. No login, browser profile, credentials, or authenticated game page. The public launch-login endpoint was checked while it served a generic maintenance shell; no launch token was obtained. Cookie and challenge-token values from responses were not retained.

`smart-search` 0.1.25 `fetch` returned empty Tavily content for both official URLs (`error_type: network_error`, no fallback). Claim text below comes from anonymous HTTPS fallback, except the two Konaste pages that were already in maintenance; those two are cited from the `exa-search` page text and marked as such.

## Verified schedule

Times are **Japan Standard Time (UTC+9)**, not the viewer's local zone. The live maintenance shell parses `YYYY/MM/DD HH:MM` with `setUTC*` and then subtracts 9 hours (`getTime` in the 503 HTML). Its hidden clock read `2026/10/15 6:39:32` while the checking machine's clock read `2026-10-14T21:39:06Z` (UTC).

| Window | When | Effect stated by the source |
|---|---|---|
| Daily Konaste maintenance | **05:00–07:00 JST every day** | Play cannot start. For コナステ メダルコーナー download titles, Saturday and Sunday 05:00–07:00 is skipped for a named title list. |
| Monthly long maintenance | **02:00–07:00 JST on the third Tuesday of each month** | Replaces that morning's 05:00 start. Konaste play cannot start, and the notice says the site cannot be logged in during 02:00–07:00. |
| Site-wide gate maintenance | **02:00–07:00 JST on the third Tuesday of each month** | Login is unavailable. This is the same clock window, from the e-amusement notice, not a second Konaste-only window. |
| Arcade-linked content | **05:00–07:00 JST every day** | Arcade-linked content and services that reference an e-amusement pass are unavailable. |

Half-open for implementation: **start inclusive, end exclusive**. `02:00:00` and `05:00:00` are inside; `07:00:00` is outside. The notices write `02:00～07:00` and `05:00～07:00` and do not define the exact end second. The shell clock above was still serving 503 at 06:39 JST inside a window labeled through 07:00.

Weekday rule: "毎月第3火曜日" / "毎月第三火曜日" means the **third Tuesday of the calendar month** (day 15–21), not the third Tuesday on or after the 1st in any other counting. March 2023 is the documented exception: that month's Tuesday window was moved one day later to Wednesday 22 March 2023, 02:00–07:00 JST. No standing rule says later months moved.

## What to disable

- **Scheduled:** show the banner and disable Play for the current seven-title catalog from 05:00 to 07:00 JST every day, and from 02:00 to 07:00 JST on the third Tuesday. None of those seven titles is in the medal-title exception list below.
- **Live:** also show the banner and disable Play when `GET https://p.eagate.573.jp/game/konasteapp/API/login/login.html` returns **HTTP 503**. On 2026-10-15 at 06:39 JST, that exact URL (no `game_id`, no redirects) returned `503`, `Content-Type: text/html;charset=UTF-8`, 5642 bytes, and the same maintenance shell as other `/game/` URLs. The shell says the service is unavailable because maintenance is in progress and links the regular-maintenance notice `mid=18736`.
- A missing `game_id` is **not** what produced this 503. The shell is generic: it does not mention `konasteapp`, `login.html`, or `game_id`. The same 503 shell was returned for `https://p.eagate.573.jp/game/eacloud/p/common/info/longmainte.html` and `https://p.eagate.573.jp/game/eacloud/p/common/top.html`.
- A later anonymous, header-only GET of that exact no-`game_id` URL returned **HTTP 200 on 2026-10-16 at 03:27:05 JST**, outside the recurring window. Redirects were disabled; no cookies/authorization were supplied and no body was read. This establishes one healthy anonymous status baseline, not account entitlement or GUI recovery. The 2026-10-15 probes were both inside the daily window. Gate pages returned 200 even during that maintenance, so they are not a substitute probe.

## Title exception

Saturday and Sunday 05:00–07:00 JST does **not** apply to every title. The コナステ メダルコーナー notice lists download titles that can keep running through that weekend window. The third-Tuesday 02:00 start is not included in that exception.

List as of the notice's 2026-02-21 update (all ダウンロードコンテンツ):

- アニマロッタ 夢のアニマランド
- カラコロッタ まぼろしの桃源郷
- GI-Classic
- GRANDCROSS GOLD
- FORTUNE TRINITY 精霊の至宝祭 コナステ
- フォーチュントリニティ 時空のダイヤモンド
- フィーチャープレミアム TwinkleDrop RUSH!
- フィーチャープレミアム TwinkleDrop DINNER
- フィーチャープレミアム TwinkleDrop JUKE!
- フィーチャープレミアム TENGU KING

The notice says more days and titles may be added later. Do not hard-code this list as complete beyond that date. The page itself returned 503 during this research, so the list is from fetched `exa-search` page text, not a fresh body.

## Sources

1. `https://p.eagate.573.jp/gate/p/site_info.html?msgtype=19&mid=18736` — anonymous GET, HTTP 200, `Date: Wed, 14 Oct 2026 21:37:50 GMT`. Inline notice `param: "18736"`, title 「定期メンテナンスのお知らせ」, undated. It states 毎月第3火曜日 午前2:00～7:00 for the whole site, with login unavailable, and 毎日 朝5:00～7:00 for arcade-linked content and e-amusement pass services. It also says the time can change. This notice is not Konaste-specific.
2. Live 503 shell on `https://p.eagate.573.jp/game/konasteapp/API/login/login.html` and `https://p.eagate.573.jp/game/eacloud/p/common/info/longmainte.html` — HTTP 503, no redirect. Hidden fields: `id_range_begin=2026/10/15 05:00`, `id_range_end=2026/10/15 07:00`, `id_current_clock=2026/10/15 6:39:32`. Visible text links source 1. The script's minus-9-hours conversion is the timezone evidence.
3. `https://p.eagate.573.jp/game/eacloud/p/common/info/longmainte.html` — `exa-search` text, 2026-10-15. Title 「毎月第3火曜日の長時間メンテナンスについて」. States 毎月第三火曜日 02:00～07:00, play cannot start, and login is unavailable during that window. Direct GET of this URL returned the 503 shell instead of the article, so this paragraph was not re-read from the origin body in this session.
4. `https://p.eagate.573.jp/game/medal/eacloud/p/news/medal2026/news_medal_0129dla.html` — `exa-search` text. States コナステ メダルコーナー maintenance is daily 5:00～7:00, and the third Tuesday starts at 2:00. It names the weekend exception list above as of 2026-02-21. Direct GET returned HTTP 503 (5642 bytes, same shell size) and the body was not the article.
5. `https://p.eagate.573.jp/game/eacloud/p/common/info/news.html?news_id=news2023/20230315_konast_ns` — `exa-search` text, dated 2023-03-16. One-off move of the March 2023 long maintenance from Tuesday 21 March to Wednesday 22 March, both 02:00～07:00. Not fetched as its own body in this session.
6. `https://ja-support1.konami.com/hc/ja/articles/32579392187289` — `exa-search` text, updated 2026-03-06. KONAMI support answer for コナステメダルコーナー: every day except Saturday and Sunday, 05:00–07:00; third Tuesday starts at 02:00. Anonymous GET from this network returned HTTP 403 with a Cloudflare challenge page, so the article body was not fetched.
7. `https://p.eagate.573.jp/game/konasteapp/API/login/login.html` — anonymous header-only GET on 2026-10-16 at 03:27:05 JST returned HTTP 200. Python `urllib` with an explicit no-redirect handler, six-second deadline, `Konaste Hub/0.1.0`, and `Cache-Control: no-cache, no-store`. No body, cookies, authorization, account session, or TLS changes. This is a healthy-status observation, not a live browser or entitlement test.

## Caveats

- The daily 05:00 start for Konaste is not in notice `mid=18736`. That notice only has the monthly site window and the daily arcade-linked window. The 05:00 Konaste start comes from the Konaste long-maintenance page, the medal notice, and the support article.
- Sources 3–6 were not available as fresh origin bodies while `/game/` was returning 503 or the support host was challenging the client. Their wording is from `exa-search` extracted text.
- Windows can be extended or moved without a new standing notice. March 2023 already did that. A 503 probe is the only source here for an unscheduled or extended window.
- The base login URL has one observed outside-window anonymous HTTP 200 baseline. Redirects, timeouts, and unrelated failures still must not be inferred healthy. Real WebView/banner/recovery acceptance remains unobserved.
- PASELI's own support text gives a different monthly window (03:00–07:00). It is not the Konaste play window and is not part of this schedule.

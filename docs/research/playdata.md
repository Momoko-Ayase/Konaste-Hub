# e-amusement play data per title

Research date: **2026-11-20**. Anonymous GETs of public pages, plus signed-in GETs of the requester's own read-only play data pages with the hub's saved session. No forms, settings, purchases, or cookies/tokens were read into the repository. Sample HTML is not committed; test fixtures are synthetic.

The accounts checked during this research had an e-amusement pass with **no per-title play data** for any Konaste title (`state.playdata` false on the My Page, and every title's play page returned its "プレーデータがありません" error). Authenticated success markup could therefore not be observed for every title below. Where the layout is asserted it comes from the official page's own scripts, CSS, or a first-party-verified community parser; those sources are named.

## Summary

| Title | Page | Fields | Status |
|---|---|---|---|
| SOUND VOLTEX EXCEED GEAR (Konaste) | `https://p.eagate.573.jp/game/eacsdvx/vi/playdata/profile/index.html` | name, VOLFORCE, play count, skill LV, last play | Implemented |
| DDR GRAND PRIX (Konaste) | `https://p.eagate.573.jp/game/ddr/ddrworld/playdata/index.html` | name, play count, last play | Implemented (arcade-shared data, see below) |
| pop'n music Lively | `https://p.eagate.573.jp/game/eacpopn/lively/playdata/index.html` | — | Skipped: layout unverified |
| GITADORA (Konaste) | none found | — | Skipped: no Konaste page |
| NOSTALGIA (Konaste) | none found | — | Skipped: arcade-only page, data not shared |
| Bombergirl (Konaste) | none found | — | Skipped: no per-player page |
| beatmania IIDX INFINITAS | out of current launch scope | — | Skipped |

## SOUND VOLTEX

- **URL:** `https://p.eagate.573.jp/game/eacsdvx/vi/playdata/profile/index.html`. The landing page's `プレーデータ` menu links it (public GET, 200, 2026-11-20). Anonymous GETs and GETs with a signed-in session that has no SDVX play data both redirect to `/game/eacsdvx/vi/error/index.html?err=4` (`『SOUND VOLTEX EXCEED GEAR』のプレーデータが必要デス`).
- **Fields and selectors**, from the community profile extractor `Bamuel/SDVX-Profile-Extractor` (`cypress/e2e/sdvx.cy.js`, reads the same `p.eagate.573.jp` page) and `katagaki/DJDX` (`SDVXProfileHeaderView.swift`), both agreeing on the element ids used by the official page's own naming:
  - Player name: `#player_name p` second paragraph. The first paragraph is the profile comment.
  - VOLFORCE: `#force_point` text (decimal, e.g. `18.742`).
  - Play count: `#profile_li li` row whose `.profile_col` is `プレー回数`, value in `.profile_cnt`.
  - Skill level: the `スキルLV` row, value in `.profile_skill`.
  - Last play: the `最終プレー日時` row, value in `.profile_cnt`; read as the leading `YYYY-MM-DD`.
- **Not read:** `#player_id` (card/rival id), `apcard` image, store name, PCB count, arena values, per-song tables. These are identifiers or not headline numbers.

## DDR GRAND PRIX

- **URL:** `https://p.eagate.573.jp/game/ddr/ddrworld/playdata/index.html`. The Konaste landing page has no play data menu; this is the arcade WORLD status page. The Konaste FAQ (`/game/eacddr/konaddr/info/faq.html`, public GET) states that with a basic course the arcade and Konaste play data are shared, so this is the same player's data while subscribed.
- **Fields and selectors**, from `sh1ggy/ddr-md` (`lib/models/eamuse.dart` and `test/eamuse_test.dart`, which read `.../playdata/index.html`) and confirmed by `fohte/ddr-score-manager`:
  - `.status-section` contains `.status-item` rows, each with `.status-label` and `.status-data`.
  - `DANCER NAME`: the name.
  - `総プレー回数`: total play count (e.g. `1380回`).
  - `最終プレー日時`: e-amusement Japan-time `YYYY-MM-DD HH:MM:SS`; the date is shown.
- **Not read:** `DDR-CODE` (rival code), per-style flare values, per-song tables.

## pop'n music Lively

- Page exists: `https://p.eagate.573.jp/game/eacpopn/lively/playdata/index.html` (public GET, 200; the page's own menu calls it `ステータス`). Anonymous and no-data requests return the page's own error shell (`プレーデータのあるe-amusement passに切り替えを…`).
- The signed-in status layout is **not** documented by any first-party asset read here: the page loads no play-data script beyond `/game/eacpopn/lively/js/menu.js`, and its CSS (`common.css`, `content.css`, `menu.css`) contains no status classes. The only public parsers found (`r3coder/popnTable`, `matonix/pomiziku`) target the arcade `popn/unilab` site, not Lively.
- **Decision:** skipped until a signed-in layout can be observed and its selectors recorded.

## GITADORA / NOSTALGIA / Bombergirl

- **GITADORA:** the Konaste landing page links only `download`, `info`, `serial`; no `playdata` path exists under `/game/eacgitadora/konagt/` (the guessed `playdata/index.html` is 404). The arcade page `/game/gfdm/gitadora_galaxywave_delta/p/playdata/profile.html` exists but the Konaste FAQ states the arcade data is not linked, so it would show a different player.
- **NOSTALGIA:** the arcade portal `/game/nostalgia/op3/` has a play data page at `/game/nostalgia/op3/playdata/entrance.html` (public GET, 200). Its player block is loaded from `/game/nostalgia/op3/json/pdata_getdata.html` (POST `service_kind=player_info&pdata_kind=player_info`). The Konaste FAQ (`/game/eacnostalgia/op3/info/faq.html`) states the arcade data is not linked, and no Konaste play data path exists under `/game/eacnostalgia/op3/` (all guessed paths 404).
- **Bombergirl:** no per-player page on either the Konaste landing or the arcade portal `/game/bombergirl/bg/p/`; only news, how-to, and character pages.

All three are skipped: no page was found that shows the Konaste player's own data.

## Session and error handling

Reads use `account::Session` (the same reqwest client, cookie jar, redirect allowlist, and `503 → maintenance` mapping as the profile reader). A response that redirects away from the exact expected path (sign-in expiry or a title's "no play data" error page) is treated as no data; nothing is shown. The result is cached per session with a 10-minute success TTL and a 1-minute retry after a failure, and the cache is cleared on sign-in adoption and sign-out.

## Evidence limits

- The stored sample of the signed-in DDR status page (`sh1ggy/ddr-md` fixtures, 2026-10) uses `.status-section`; the live page's error shell for a no-data account does not contain that class, so the parser correctly yields nothing for such an account.
- SOUND VOLTEX selectors are from two independent community parsers that target the live page; they were not confirmed against a signed-in body with data during this research because no such account was available.

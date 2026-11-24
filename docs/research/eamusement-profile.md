# e-amusement profile and session markup

Research date: **2026-10-15**. Anonymous GETs of public first-party pages and scripts only. No login, no credential or cookie reuse, no browser profile, no game or launcher, and no authenticated document. Cookie values, OAuth `state`, and `nonce` from those responses are omitted.

The initial anonymous-research sections are not signed-in profile samples. Name, avatar bytes, active card number, PASELI balance, and PASELI points were **not** present in those fetched bodies. The later requester-provided structural report establishes markup without exposing personal values; see the final section.

## Verified session flag

Public HTML on `https://p.eagate.573.jp/` (HTTP 200, 2026-10-14 20:21 UTC) inlines:

```text
ea_common_template.userstatus = {
  "1":  { maintxt, path, linktxt },          // login required
  "2":  { maintxt, path, linktxt, reserve_url }, // basic course
  "3":  { maintxt, path, linktxt, reserve_url }, // premium course
  "4":  { maintxt, path, linktxt, reserve_url }, // no active e-amusement pass
  "5":  { maintxt, path, linktxt, reserve_url }, // no play data
  "99": { maintxt, path, linktxt },          // system error
  "region": "notlogin",
  "state": {
    "course": {},
    "eapass": null,
    "login": false,
    "playdata": false,
    "sg": {},
    "subscription": false
  }
}
```

The same anonymous `state` / `"region":"notlogin"` shape was on:

- `https://p.eagate.573.jp/etc/help/index.html` (200)
- `https://p.eagate.573.jp/gate/p/site_info.html` (200)
- `https://p.eagate.573.jp/gate/k/login_error.html` (200)
- `https://p.eagate.573.jp/payment/p/select_course.html` (503 shell, still inlined)

`state.login === false` is the only verified signed-out flag. The signed-in value of `state.login`, and any extra `state` keys, are **unknown**. `eapass: null` is the anonymous value only; its logged-in type is **unknown**.

`M573SSID` is set `HttpOnly; Secure; Path=/` on anonymous responses as well, so the cookie name is not proof of sign-in.

`GET https://p.eagate.573.jp/gate/k/API/common/reserveurl.html` returned `{"success":false,"status":"not login"}`. The public caller in `ea_common_header.js` uses **POST** to that URL with `reserve_url` and `message`. It is not a profile endpoint. No other profile or balance URL returned JSON; these guesses were HTTP 404: `/gate/k/API/common/userstatus.html`, `/gate/k/API/paseli/balance.html`, `/gate/p/profile/api/profile.html`, `/gate/p/common/header/user.html`.

## Login redirect, not a home-page widget

`https://p.eagate.573.jp/gate/p/common/tk/ea_common_header.js?v20260826` does not render a user, card, or balance. On ready it:

1. `GET /gate/p/common/multi/ea_common_multi_lang.html` (optional `lang`, `add_multi_lang_key` from `#id_ea_add_multi_lang_key`).
2. Splits the body on `@`, then `id:html`, and writes that HTML into `.cl_ea_variable_document[data-id]`.
3. Reads only `ea_common_template.userstatus.region` for `.cl_ea_region` visibility.

The homepage defines `ea_common_template.login.show_loginform(path)` inline. It assigns `location.href` to `/gate/p/login.html` plus optional `?path=`. It does not submit credentials.

Anonymous `GET /gate/p/login.html` and `GET /gate/p/login.html?path=/` are HTTP 302 to:

```text
https://my.konami.net/api/authorizations
  scope=openid read
  response_type=code
  prompt=login
  redirect_uri=https://p.eagate.573.jp/gate/k/login_exec.html
  client_id=KRXaB58UFTtra9ZWUERRHMziPUXrFhwH
  claims.userinfo = address.country, konamiId,
                    loginConsent.kam, profile.birthdate,
                    profile.gender, consent.kam
```

Those claims do **not** request a display name or picture. `GET /gate/k/login_exec.html` without a code is 302 to `/gate/k/login_error.html`, whose visible text is 「ログイン中に想定外の遷移を検知しました。」 `my.konami.net` did not answer from this network (timeout); `https://account.konami.net/` returned 403. No sign-in page DOM was fetched.

## Empty slots versus logged-in markup

Homepage slots that stay empty until the multi-lang response:

| `data-id` | Anonymous template (`add_multi_lang_key=top`, also `en`/`ja`/`zh`/`ko`) |
|---|---|
| `eavd_header_konamiid` | **key absent** |
| `eavd_side_mypage`, `eavd_side_logout`, `eavd_side_setting`, `eavd_side_mycharge` | **keys absent** |
| `eavd_side_login` | text `ログイン` |
| `eavd_top_login_box` | login link `javascript:ea_common_template.login.show_loginform();`, register `/gate/k/newcomer.html` |
| `eavd_top_login_img` | empty `<div class="cl_ea_top_login_img_a">` |

Logged-in HTML for `eavd_header_konamiid` is **unknown**. No public script names a child selector, image URL, or text node for a display name or avatar.

## What is gated, and what a card number is

Anonymous 302 to `/gate/p/login.html` (then the OAuth URL above):

| URL | Role from public help text |
|---|---|
| `/gate/p/mypage/` | also redirects with `path=/gate/p/mypage/`. Authenticated body **not fetched**. |
| `/gate/p/profile/index.html` | help `id_profile`: login, then this page, to view or change profile text, プロフィール画像, and visibility. Field names and image URL **unknown**. |
| `/gate/eapass/menu.html` | active-pass switcher. Help `id_eapass_04` describes a switch button, swipe, or pulldown. No class names. |
| `/gate/eapass/register.html` | new-pass registration. Form fields **not fetched**. |

`https://p.eagate.573.jp/gate/eapass/aboutcardnum.html` (200) defines the card number as the **16-character alphanumeric** id printed in-game or on older card backs. Amusement IC backs show a **20-digit access code** that is not the same string; either may be entered in official forms. This page has no account data and no balance.

PASELI, from `https://www.konami.com/arcadegames/paseli/` and `/point/index.html`: the spendable balance is a KONAMI ID wallet; PASELI points are a separate counter (200 yen of use to 1 point, exchange from 10 points). No balance or points JSON, and no e-amusement header selector, was in those pages.

## Why a My Page URL watch can miss sign-in

Verified public behavior that affects the current home-only `show_loginform()` flow:

1. The helper leaves `https://p.eagate.573.jp/` immediately for `/gate/p/login.html`, then `my.konami.net`.
2. The OAuth return path is `/gate/k/login_exec.html`, not `/gate/p/mypage/`. Whether a successful `login_exec` then honors `path` is **not established**.
3. An anonymous request for `/gate/p/mypage/` never stays there. Only a session that does **not** redirect away would match that URL.
4. Even on the homepage, identity is not in the first HTML. `state.login` is. The header name slot is filled later, and only if the session-specific multi-lang body contains `eavd_header_konamiid`.

A verified read of signed-out state is `ea_common_template.userstatus.state.login === false` on a `p.eagate.573.jp` document that includes the inline assignment. A verified signed-in profile response does not exist in this note.

## Requester feedback and next evidence

On **2026-10-15**, the requester confirmed the revised launcher recognizes login but **does not read or show the account details**. Generic HTML-label matching is not a verified authenticated-page contract. Do not claim those fields work based on synthetic fixtures.

A debug-only **Settings → Capture account layout** action now opens the official My Page in the app-owned session. It reads the current document plus fixed My Page/profile/pass/translation pages and saves `account-layout-<timestamp>-<generation>.json` in the app's local-data directory, then opens that folder. This is a diagnostic step, not a profile-reader fix, and has not been run against a real account by the agent.

The report contains bounded element trees, scalar type categories, HTTP outcomes, and property-name/type schemas for a fixed account-field vocabulary. Element IDs/classes/names are retained only if they also occur in anonymously fetched official HTML/static sources. No text values, form values, image URLs, cookie values, authorization codes, credentials, or launch tokens are included. Source HTML stays in detached template contents and is never inserted into the live page. Unexpected fields are refused natively. The action must be clicked explicitly, does not upload a report, is hidden in production frontend builds, and the native command refuses release execution.

Use the redacted report to establish the actual fixed selectors, card-selection scope, avatar transport, and available wallet data before replacing the generic reader. A redirect or missing structure must remain visible as missing evidence, not trigger additional guessed endpoints.

## Requester-provided authenticated layout, 2026-10-15

The requester supplied the generated structural report. Only that explicitly supplied report was read; no browser profile, cookies, other local-data files, raw authenticated page bodies, or personal values were inspected. The report is not copied into the repository.

Observed page outcomes:

| Page | Result | Nodes |
|---|---|---:|
| Current rendered portal document | Captured; no HTTP status recorded | 553 |
| My Page `/gate/p/mypage/` | HTTP 200 | 250 |
| My Page `/gate/p/mypage/index.html` | HTTP 200 | 250 |
| Profile `/gate/p/profile/index.html` | HTTP 200 | 178 |
| Pass menu `/gate/eapass/menu.html` | HTTP 200 | 292 |
| Multi-lang document | HTTP 200; no parsed nodes | 0 |

`state.login` and `state.eapass` are booleans; the captured property schema has no account name, image, card-number, or wallet-value properties. The old reader's expected labels and form-field names do not occur. This is a DOM-contract mismatch, not evidence of an HTTP failure.

### Mapped display fields

| Field | Captured structure used by the revised reader |
|---|---|
| Configured profile name | `.cl_profile > div:nth-child(2) > div:first-child`. My Page nodes 92/95/96 show the profile, second column, and plain-text name div. Sibling `.cl_sp` contains mobile edit links and is deliberately excluded. |
| Profile portrait | `.cl_profile > div:first-child > img`. My Page nodes 92/93/94 show an unlabelled same-origin JPEG. No alt/id label is required. |
| Selected-pass header | `#id_ea_common_content > div:first-child > div:first-child > div:first-child > a > span`. My Page nodes 72–77 locate the card-like text outside the global KONAMI ID header and pass templates. Read only with `state.eapass === true`, and accept only a full 16-character alphanumeric value. |
| PASELI balance | `.cl_eapass_body .cl_paseli_info > .cl_paseli_info_paseli`. The pass-menu report marks the value as numeric; the My Page wallet placeholder is empty. |
| PASELI points | `.cl_eapass_body .cl_paseli_info > .cl_paseli_info_point`, likewise populated on the pass menu. |

The global `[data-id="eavd_header_konamiid"]` contains a hydrated text span and portrait on the rendered document but not the fetched shells. It is **not** used as the configured profile name. The pass-menu `#id_eapass_cmn_data` contains empty list/card-number templates, a numeric `.cl_eapass_acccode`, hidden inputs, and wallet-setting radios. None are read as the selected card number; no radio/button is clicked and no pass is switched.

The reader now fetches only the two observed useful documents (My Page index and pass menu), parses them inertly, and also reads matching elements in the current document. It no longer fetches the empty profile/translation shells or uses generic label matching. Avatar URLs resolve against the source document, remain HTTPS same-origin, and are validated by image media type rather than filename extension. `mode: same-origin` permits same-origin redirects while blocking cross-origin redirect hops. Launcher image data remains bounded, decoded, and re-encoded as PNG.

The report proves structural locations and value categories, **not the private values or completed end-to-end display**. Tests reconstruct those selectors with synthetic values, check ambiguous/missing fields, active-pass gating, access-code exclusion, relative/extensionless portraits, media/dimension bounds, and no credential/storage/log access. The requester still needs to run the revised reader via restart/Refresh to confirm the actual fields display.

## KONAMI ID and pass memo (2026-11-22)

Both read with a signed-in session, GET only:

- **KONAMI ID:** `GET /gate/p/common/multi/ea_common_multi_lang.html`. Signed in, the `@`-separated `key:html` blocks include `eavd_header_konamiid`; its text `<span>` holds the ID. Accepted only as 8-32 lowercase letters, digits, `-`, `_`, `.` (the portal's own rule). Signed out, the key is absent.
- **Pass memo:** `GET /gate/eapass/api/getlist.html` returns JSON `{"data":{"card":[{"cardnumber","acccode","is_active","paseli_state","playdata_memo","cardmedia_memo"}]},"fail_code","status","max_stash"}`. The Hub shows `cardmedia_memo` for the entry whose `cardnumber` matches the card already read from My Page. `acccode` is never read into the profile.

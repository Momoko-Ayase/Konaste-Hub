# Konaste platform research

Research/access date: **2026-10-14**. This record separates public documentation, local static evidence, and the limited live experiment. It is not a completed launcher implementation or a legal determination.

No official launcher, updater, game, or MSI was executed. Package inspection and native-library probes used isolated temporary artifacts; existing installations and project files were not modified by those experiments. No real credentials/session tokens, purchases, subscription changes, TLS interception, new certificates, or Windows URI registration were used.

## 1. Official downloadable PC catalog

The official [video catalog](https://p.eagate.573.jp/game/eacloud/p/video/video_top.html) distinguishes **PC専用 ダウンロードコンテンツ** (each title's own installed software) from shared-client streaming. It currently lists seven downloadable PC titles.

| Title | Official landing/download | Current stated Windows requirement |
|---|---|---|
| SOUND VOLTEX EXCEED GEAR | [Landing](https://p.eagate.573.jp/game/eacsdvx/vi/index.html), [download](https://p.eagate.573.jp/game/eacsdvx/vi/download/index.html) | Windows 10 64-bit / 11 — [environment](https://p.eagate.573.jp/game/eacsdvx/vi/info/env.html) |
| ボンバーガール / Bombergirl | [Landing](https://p.eagate.573.jp/game/eacbg/bg/index.html), [download](https://p.eagate.573.jp/game/eacbg/bg/download/index.html), [installer](https://p.eagate.573.jp/game/eacbg/bg/download/installer.html) | Windows 11 Japanese edition — [environment](https://p.eagate.573.jp/game/eacbg/bg/info/env.html) |
| DanceDanceRevolution GRAND PRIX | [Landing](https://p.eagate.573.jp/game/eacddr/konaddr/index.html), [download](https://p.eagate.573.jp/game/eacddr/konaddr/download/index.html) | Windows 11 — [environment](https://p.eagate.573.jp/game/eacddr/konaddr/info/env.html) |
| beatmania IIDX INFINITAS | [Landing](https://p.eagate.573.jp/game/infinitas/2/index.html), [download](https://p.eagate.573.jp/game/infinitas/2/download/index.html) | Windows 10 64-bit / 11 — [system](https://p.eagate.573.jp/game/infinitas/2/startup/system.html) |
| pop'n music Lively | [Landing](https://p.eagate.573.jp/game/eacpopn/lively/index.html), [download](https://p.eagate.573.jp/game/eacpopn/lively/download/index.html) | Windows 10 64-bit / 11 — [environment](https://p.eagate.573.jp/game/eacpopn/lively/info/env.html) |
| GITADORA | [Landing](https://p.eagate.573.jp/game/eacgitadora/konagt/index.html), [installer](https://p.eagate.573.jp/game/eacgitadora/konagt/download/installer.html) | Windows 11 — [environment](https://p.eagate.573.jp/game/eacgitadora/konagt/info/env.html) |
| ノスタルジア / NOSTALGIA | [Landing](https://p.eagate.573.jp/game/eacnostalgia/op3/index.html), [download](https://p.eagate.573.jp/game/eacnostalgia/op3/download/index.html) | Windows 10 / 11; bitness not explicitly stated — [environment](https://p.eagate.573.jp/game/eacnostalgia/op3/info/env.html) |

These are snapshots, not permanent support promises. Launcher support for Windows 10/11 does not override a game's narrower official requirements.

Streaming mahjong/quiz/shogi, medal offerings, mobile-only titles, and discontinued services are outside the agreed downloadable-PC scope. The [PC FAQ](https://p.eagate.573.jp/game/eacloud/p/common/faq_pc.html) describes the separate shared streaming client; its `kdecloud` handoff must not be mistaken for downloadable games' URI contracts.

## 2. Official account flow: documented versus unknown

The [beginner page](https://p.eagate.573.jp/game/eacloud/p/common/beginner.html) describes KONAMI ID and e-amusement pass requirements. Title setup pages describe launching a web flow to confirm the account/pass, accept terms, and start the game. Public download pages use e-amusement login entry points on `p.eagate.573.jp`.

Static strings in the two inspected launchers contain:

- `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=bomber-girl`
- `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=sdvx`

Bounded anonymous GETs returned HTTP 302 to the e-amusement login page. No credentials were submitted and response cookies were not retained/reused. This proves an entry/redirect, not embedded-login compatibility or successful token issuance.

An earlier private Bombergirl research corpus (the "BG notes", not in this repository) describes the `konaste.bomber-girl://login?tk=…` handoff and subsequent `-t` child argument. These notes are research inputs supplied with this work, not bundled software or live session captures. The current static comparison also finds `konaste.sdvx` and title-specific IDs/paths.

**Native sign-in follow-up (2026-11-22):** An anonymous request to `https://p.eagate.573.jp/gate/p/login.html` returned a redirect to `https://my.konami.net/api/authorizations` with its callback set to `https://p.eagate.573.jp/gate/k/login_exec.html`. [KONAMI documents passkeys for My KONAMI](https://ja-support1.konami.com/hc/ja/articles/38853566892057), but no separate client registration or callback for the Hub was found. Opening the official page in the default browser alone does not return an e-amusement session to the Hub. The existing official sign-in window remains in place; no credentials or passkeys were used for this check.

**Not established by this investigation:** a third-party Konaste authorization contract, embedded-WebView policy, end-to-end passkey behavior, or the remaining five titles' callbacks. Do not copy an OAuth flow belonging to a different KONAMI product or assume a fabricated callback/state parameter is accepted.

## 3. Public package acquisition experiment

Public official pages linked two MSIs. Both were fetched anonymously into a new temporary directory: HTTP **200**, no redirect. Read-only Windows MSI APIs and static cabinet tooling were used; **no `msiexec` or PE execution**.

| Package | Official URL | Size | SHA-256 |
|---|---|---:|---|
| Bombergirl | [BomberGirl_Installer.msi](https://d1q1iflay6y5jj.cloudfront.net/Installer/2021122200_b37bc853/BomberGirl_Installer.msi) | 38,649,856 bytes | `977f60c5d05828ca5263b7367cd0e153ed330fc134a5a141bb0ca32f98355aae` |
| SDVX | [sdvx_installer_2022011800.msi](https://dks1q2aivwkd6.cloudfront.net/vi/installer/sdvx_installer_2022011800.msi) | 32,337,920 bytes | `af3f96f91832a81882a44c9d5232163aaf0aa379df221b68541ee27e12e33894` |

`WinVerifyTrust` returned **0** for both packages and the inspected extracted launcher executables/DLLs. Signer display name: **Konami Amusement Co., Ltd.** Both MSI product versions are `1.0.0`; their public links and embedded files date from 2020–2022. A public bootstrap package is not proof of the current game build.

| Component | Bombergirl package | SDVX package |
|---|---|---|
| `libeacnet.dll` | `1.3.2.101`, 8,768,488 bytes | `1.3.3.104`, 8,742,352 bytes |
| `avs2-core.dll` | `2.17.3.0`, 1,635,400 bytes | `2.17.3.0`, 1,633,232 bytes |
| `launcher.exe` | `0.1.0.1` | `2.0.0.2` |
| `afp-core.dll` / `afp-utils.dll` | Present | Present |
| `settings.exe` / `libsettings.dll` | Not present | Present |

Neither package contains an external AVS XML configuration; the launcher builds its configuration tree in memory. Static imports show that `libeacnet` depends on launcher-side `avs2-core` and WinHTTP. The graphical launcher additionally imports AFP and legacy DirectX components; that does not establish they are needed by a metadata-only helper.

Game executables are absent from these bootstrap packages. That is **not** proof the hub needs the official launcher: official setup normally downloads game content afterward. Standalone service/manifest discovery and initial content installation remain the prerequisite.

For **DDR, INFINITAS, pop'n, GITADORA, and NOSTALGIA**, the inspected public pages presented login-gated download flows. No anonymous package equivalence was established and no gated download/login was attempted. Do not invent public URLs for them.

## 4. Shared implementation evidence (two titles only)

Local static comparison of the official Bombergirl and SDVX launcher/updater/library sets found:

- Matching launcher/updater process flow, eacnet import names, and AVS ordinal usage.
- Updater boot/download call sites at matching addresses and same-sized code sections; differences primarily in immediates/UI resources.
- All 50 `libeacnet` export RVAs match. Its code has a small version difference, not evidence of an unrelated engine.
- Launcher-side `avs2-core` code/data sections match; its size/hash difference is the Authenticode certificate. AFP code also matches with certificate-only differences.
- Title-specific game ID, scheme, registry name, executable/digest path, window/mutex name, and log filename.
- SDVX has an optional settings UI absent from the Bombergirl package.

**Conclusion:** use a shared engine plus descriptors for these two titles. There is no demonstrated need for separate update strategies. Do not generalize this evidence to all seven or swap arbitrary libraries solely because their displayed version numbers match. The game-side AVS library is a different binary from the launcher-side pair.

The BG notes also describe whole-install hashing, serial downloading, verification after replacement, discarded partials, and manifest commitment only at completion. They describe the proprietary service bootstrap and a proposed replacement. Some live-capture conclusions can no longer be rechecked from those artifacts because sensitive captures were deliberately removed; the proposal to drive the library is not itself proof of a correct ABI.

## 5. Limited native-library experiment

The authorized helper work used trusted library copies, a new temporary working/resource directory, and no game/official launcher/updater execution. AVS itself created temporary nvram/log files there. No existing installation was changed.

### Bootstrap facts and failures

The official bootstrap allocates **`0x08000000` = 134,217,728 bytes = 128 MiB**, then passes both the buffer and that size to AVS. An initial null-buffer attempt crashed. It is an initialization error, not evidence about authentication.

A corrected attempt used the required buffer and an in-memory AVS property/configuration tree. AVS returned `1`; a standard object was non-null and the later library exchange was reached. The official launcher does not test that return at this point. This is partial initialization evidence, not a documented guarantee that every AVS subsystem is ready.

The eacnet boot blob is 0xA0 bytes. Static official callers establish:

| Field | Official source |
|---|---|
| Mode at `+0` | Launcher **1**; updater **2** |
| Game ID | Title-specific literal |
| Client version at `+0x45` | ProductVersion of the process's **own executable**, obtained through `GetModuleFileNameA(NULL)` and the Japanese version-resource block |
| Token | Parsed `tk` field |
| Byte at `+0x9B` | The `dn` command-line/URI flag, not a version field |

The current game executable ProductVersion does **not** fill this boot-version field; its separate digest/version check is a different operation. Substituting the newest game build is not a verified remedy.

### Clean executed exchange

The executed exchange used:

- **Mode 0**, unlike the official launcher/updater modes.
- The Bombergirl launcher ProductVersion `QBG:J:B:A:2021122100`.
- A non-empty placeholder token, not a real account session.
- `dn` flag 0.

Observed result:

| Observation | Result |
|---|---|
| Initial library boot call | Returned 0 |
| Service/bootstrap progression | Reached the version-response step |
| Version response fields | Status **1**, error **1**, permission **0** |
| Library error | **1021** (`0x3FD`) |
| Copied version/resource strings | Empty |
| `GetResourceInfo` | **Not called** |
| Heartbeat/account stages | **Not entered** |
| Check-game-start/entitlement call | **Not used** |

The worker was shut down after this result. A clean empty-token mode-0 attempt produced client error **1004** before a network worker. A later mode-2 empty-token attempt reused an already-initialized process and crashed; **it is not a valid mode-2 result**.

The actual successful exchange was inline ctypes, retained as an experiment record rather than a runnable saved script. An abandoned C probe did not perform that exchange. Reproduction requires constructing a new, lifecycle-correct harness; do not treat the abandoned executable as verification.

### What this does and does not establish

A placeholder was accepted far enough to obtain a version response. It does **not** establish anonymous manifest discovery or anonymous updating. Permission 0 cannot be attributed independently to the token, version, or initialization mode because those variables were not isolated. In particular, **the correct official updater-mode path was not tested**.

No fresh real token, authenticated launch, complete installation, SDVX live bootstrap, or other-title bootstrap was tested. CDN object GETs being anonymous after a valid URL is known does not prove that service discovery returns that URL anonymously.

## 6. Terms and distribution caveat

The title download pages state that downloading constitutes agreement and contain restrictive copying, modification, reproduction, and reverse-engineering wording. The broader [PC terms](https://p.eagate.573.jp/game/eacloud/p/common/tos_pc.html), especially articles 9 and 12, restrict copying/alteration/unauthorized tools/reverse engineering and reserve rights to KONAMI or its licensors. The linked [KONAMI ID agreement](https://my.konami.net/agreement.html) was not successfully extracted during this research.

Nothing inspected grants redistribution or expressly resolves a third-party hub downloading a package for an individual user's local extraction. That is a **licensing uncertainty**, not a conclusion that the proposed use is permitted or unlawful. Technical access and a valid signature do not settle rights. Do not bundle proprietary artifacts; resolve the component-use/extraction and artwork questions before public distribution.

## 7. Gates for the next native milestone

1. Implement a lifecycle-correct, bounded helper using the verified official calling contract; establish the updater-mode metadata path without guessed version strings.
2. Verify anonymous resource discovery, or document the actual required official sign-in flow without weakening the strict launch gate.
3. Test official in-app authentication, pass/consent steps, callback capture, and explicit browser fallback in WebView2.
4. Complete first-run game installation from valid manifests while preserving integrity and recoverability.
5. Verify package/runtime contracts and real launch behavior for the remaining five titles.
6. Resolve public-distribution permission and asset rights.

The UI prototype simulates the desired experience while these gates remain open. Its versions, artwork, transfer speed, and recovery journal are not new evidence about the native service.

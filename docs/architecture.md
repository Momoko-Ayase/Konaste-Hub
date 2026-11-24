# Architecture

Konaste Hub is one Windows executable (`crates/konaste-hub`) built on library crates (the other `crates/konaste-*`). The UI uses gpui through `gpui-kit`. Everything that touches files, processes, or the network runs outside the UI thread.

## Crates

Dependency order, lowest first.

| Crate | Role |
|---|---|
| `konaste-manifest` | Decodes the official binary resource index (`ri.bin`). The caller supplies the expected SHA-256 and a host allowlist. No I/O. |
| `konaste-bootstrap` | Downloads a pinned official launcher MSI per title family, checks size, SHA-256, and Authenticode, and statically extracts `libeacnet.dll` and `avs2-core.dll` into an app-owned cache. Never runs the installer. Pins live in `src/catalog.rs`. |
| `konaste-runtime` | The pinned runtime worker. A child process (`konaste-hub.exe --konaste-runtime-worker`) loads the extracted DLLs. Mode 2 with a fixed placeholder performs discovery. Mode 1 with a fresh token performs launch verification (`checkGameStart`). Also holds per-title data (`title.rs`), installation pinning, game creation, settings-program preparation, and graphics arguments. |
| `konaste-discovery` | Read-only current-version check: runs the worker in mode 2, gets the resource index URL and digest, fetches and decodes the index. |
| `konaste-updater` | Recoverable staged updater for one installation: resumable range downloads, staged verification of the full changed delta, write-ahead journal under `<game root>\.konaste-hub\`, per-file backup and replace through pinned no-follow handles, and `ri.bin` published last. |
| `konaste-install` | Binds fresh discovery to an updater plan for one title. |

INFINITAS-specific modules (`konaste-bootstrap/src/infinitas.rs`, `konaste-runtime/src/infinitas*.rs`) are kept as reference. The Hub does not use them; INFINITAS is unsupported.

## `crates/konaste-hub` modules

| Module | Role |
|---|---|
| `main.rs` | Entry point. Dispatches the runtime worker and the hidden `--headless-install` / `--headless-index` development commands, otherwise starts the UI. |
| `catalog.rs` | The seven catalog titles, their registry keys, executables, and support flags. |
| `state.rs` | Hub state, preferences (`settings.json`), and the library. |
| `hub.rs` | Shared handle between background tasks and the UI thread. |
| `instance.rs` | Exclusive `hub-instance.lock` per data folder; a second start asks the first to show itself. |
| `browser.rs` | Hidden WebView2 that keeps the official sign-in page loaded. Never shown. |
| `signin.rs`, `passkey.rs` | Native sign-in form. Each step is sent from inside the hidden page. Passkeys go through `webauthn.dll`. |
| `account.rs`, `jar.rs` | Native HTTP session after sign-in. Cookies are saved with DPAPI only when Stay signed in is on. |
| `profile.rs`, `playdata.rs`, `news.rs` | Parsers for the signed-in e-amusement pages, per-title play data, and public news. |
| `launch.rs` | Background checks and Play. |
| `update.rs`, `transfer.rs` | Install and update queue on a worker thread; throughput sampling. |
| `maintenance.rs` | Scheduled maintenance windows (JST) and HTTP 503 detection. |
| `game_settings.rs`, `official_setup.rs` | Opens each title's official settings program, after writing the registry values and DLL copy it needs. |
| `platform.rs` | Windows helpers: registry detection, DPAPI, shell open, free space, process checks. |
| `self_update.rs` | Hub self-update: checks the latest GitHub release, downloads and verifies its setup, and starts it. |
| `log.rs` | `hub.log`: error codes and stage names only. |
| `i18n.rs` | Strings for en, ja, zh-CN, zh-TW, ko. |
| `ui/` | `view.rs` (window, rail, stage, overlays), `theme.rs` (colors), `motion.rs` (animations), `art.rs` (images), `assets.rs` (embedded icons), `tray.rs`. |

## Data folder

`%LOCALAPPDATA%\konaste-hub` (`platform::data_directory`).

| Path | Contents |
|---|---|
| `settings.json` | Preferences and library. |
| `session.dat` | Cookies, DPAPI-encrypted for the current user. Absent when Stay signed in is off. |
| `runtimes\` | Extracted official runtime pairs and the package cache. |
| `backgrounds\` | Stage backgrounds downloaded from this repository. |
| `hub.log` | Failure log. |
| `update\` | A verified Hub setup while a self-update runs; deleted on the next start. |
| `hub-instance.lock`, `hub-activate.request` | Single-instance lock and activation signal. |

Removing a game from the library deletes none of this and no game files. The uninstaller asks, defaulting to No, whether to delete this folder; game folders are never removed.

## Main flows

**Sign-in.** The official my.konami.net page loads in a hidden WebView2, because KONAMI's bot check only accepts requests from its own page. The Hub draws its own form and sends each step from inside that page. After sign-in the cookies move to a `reqwest` client; profile, PASELI, news, and launch tokens are then fetched natively. See [native sign-in](research/native-signin.md).

**Check.** At startup each installed title runs fresh discovery through the runtime worker and pins the installed `ri.bin` and main module against it. An outdated install shows Update.

**Install and update.** One title at a time from a queue. Every start or resume fetches fresh metadata and binds it to the durable journal. The full changed delta is downloaded and verified before any installed file changes. Interrupted runs recover from the journal. See [updater integration](research/updater-integration.md) and [download reliability](research/download-reliability.md).

**Play.** Reuses the last check. Re-pins the two files locally, probes maintenance once, takes a fresh launch token, runs mode-1 verification in the worker, then creates the game process directly with ` -t <token>`. Tokens live only in zeroizing memory and private pipes, never on disk or in logs. See [game creation](research/game-creation.md) and [launch verification](research/launch-verification.md).

**Hub self-update.** `GET api.github.com/repos/Momoko-Ayase/Konaste-Hub/releases/latest` returns the newest published release; drafts and prereleases are never offered. The release must carry `KonasteHub-<version>-setup.exe` and `SHA256SUMS.txt` from this repository. On Update the Hub downloads both, and the setup must match its line in `SHA256SUMS.txt`, its size, and GitHub's recorded digest when present. The verified setup goes to `<data>\update\` and starts with `/SILENT /SUPPRESSMSGBOXES /NORESTART /RESTARTHUB`, and the Hub quits. Setup waits up to 30 s for the `KonasteHub` mutex to clear, installs over the same folder, and starts the Hub. The next start deletes `update\`.

**Official setup.** A title's settings program needs `InstallDir`/`ResourceDir` registry values and a verified `avs2-core.dll` copy. For pop'n, GITADORA, and NOSTALGIA the Hub writes HKCU. DDR reads HKLM only, so it gets one elevated `reg.exe` write. Existing values are never overwritten and nothing is removed later.

## Asset sources

| Asset | Source |
|---|---|
| App icon, UI icons, fallback title art (original SVG), pass card | Embedded in the binary from `crates/konaste-hub/assets`. |
| PASELI and e-amusement logos | Embedded in the binary from `crates/konaste-hub/assets/services`. |
| Stage backgrounds | `assets/backgrounds/<game>.jpg` in this repository. The Hub downloads them from `https://raw.githubusercontent.com/Momoko-Ayase/Konaste-Hub/main/assets/backgrounds/` into the data folder, refreshes weekly, and falls back to the embedded SVG. |
| Game icons | Read from the user's installed launcher or game executable. |
| Games, launcher MSIs, official DLLs | Downloaded from KONAMI's servers on the user's machine. Never committed or bundled. |

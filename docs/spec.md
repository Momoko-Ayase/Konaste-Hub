# Product specification

What Konaste Hub does and the rules it keeps. Evidence for each rule is in [research](research/README.md).

## Scope

An unofficial, open-source launcher for KONAMI's downloadable コナステ PC games.

- Windows 10/11 x64. Distributed as a per-user installer only; no portable build.
- The Hub updates itself from GitHub releases. It checks at startup and every six hours; a newer version shows an Update button in the title bar, and nothing is downloaded until the user clicks it. Updating is refused while a game, its settings, or a download is active. There is no switch to turn checks off.
- MIT license. Clearly unofficial; not affiliated with KONAMI.
- Languages: en, ja, zh-CN, zh-TW, ko. Dark theme only.
- Uses real accounts, official downloads, and normal entitlement checks. No subscription, protection, or anti-tamper bypass.
- Game OS and language requirements are separate from the Hub's; never imply every game supports every Windows version.

### Titles

| Title | Install / update | Play | Game settings |
|---|---|---|---|
| Bombergirl | Yes | Yes | No separate program |
| SOUND VOLTEX EXCEED GEAR | Yes | Yes | Yes |
| DanceDanceRevolution GRAND PRIX | Yes | Yes | Yes (one administrator prompt on first use) |
| pop'n music Lively | Yes | Yes | Yes |
| GITADORA | Yes | Yes, after choosing GuitarFreaks or DrumMania | Yes |
| NOSTALGIA | Yes | Yes | Yes |
| beatmania IIDX INFINITAS | No | No | No |

INFINITAS is listed as unsupported. Its official package has no `libeacnet.dll` and uses a different client; see [INFINITAS current contract](research/infinitas-current.md). Do not add detection, install, or Play for it without a new decision.

## Library

- Add a game by installing into an empty folder or locating an existing installation. Official-launcher installs are detected from the registry (HKCU, then HKLM).
- A new install cannot overlap an existing installation. Install elsewhere keeps the old files and journal. Cancel install drops the attempt from the Hub without deleting files.
- Removing a game from the library deletes no game files and no registry values.

## Checks and the launch gate

Checks are per title; one failure does not block other titles.

1. Installed titles are checked at startup with fresh official discovery.
2. Play is unavailable while checking.
3. A required update replaces Play with Update.
4. A failed check shows the error and Retry. There is no offline bypass.
5. Play reuses the latest check, re-pins the installed index and main module locally, and does not hash the whole install.
6. Every Play takes a fresh launch token and passes official verification (`checkGameStart`) before the game starts.

| State | Primary action |
|---|---|
| Checking | Disabled |
| Current | Play |
| Update required | Update |
| Paused or failed download | Resume / Retry |
| Installing or recovering | Disabled |
| Check failed | Retry |

### Maintenance

- Play is blocked during scheduled maintenance, evaluated in JST regardless of the PC's time zone: 05:00 to 07:00 daily, and 02:00 to 07:00 on the third Tuesday of each month.
- HTTP 503 from the official launch-login page counts as maintenance, including unscheduled outages. Timeouts and other errors do not.
- Maintenance never stops a running game, changes an installation, or clears the session. Downloads continue.

See [maintenance](research/maintenance.md).

## Accounts

- Sign-in is global, not per game. The Hub shows a native form; requests run inside the official page in a hidden WebView2. No web window is ever shown, including on Play.
- Supported steps: ID and password with the picture check, email and SMS codes, two-step, and passkeys through Windows Hello. Steps the Hub cannot show send the user to the website.
- Stay signed in (on by default) saves eligible cookies with DPAPI. Launch tokens are never saved or logged.
- Official launch terms and the "previous game still running" prompt are shown as native dialogs.
- Account fields (name, avatar, card number, PASELI) stay in memory. The card number is masked until revealed. Missing values show as unavailable, never zero.
- Never request credentials or tokens in issues or chat.

## Game launch

- The Hub creates the game process itself with the title's proven arguments and ` -t <token>`. It does not run the official launcher or error reporter and does not upload crash logs.
- No developer or trial flags, parent spoofing, hooks, or patches.
- The Hub never terminates a game. Closing the window while a game runs minimizes to the tray.
- After a game exits, a normal exit triggers a fresh check; an abnormal exit shows the exit code.

## Official components

- Runtime DLLs come from pinned official MSIs, verified by size, SHA-256, and Authenticode, then statically extracted. The installer is never executed.
- No KONAMI DLL, installer, game file, or artwork is committed to the repository or bundled in a release, except the PASELI and e-amusement service marks.
- The settings program of each title runs unchanged. For pop'n, GITADORA, and NOSTALGIA the Hub writes `InstallDir`/`ResourceDir` to HKCU and copies the verified `avs2-core.dll` into `launcher\modules`. DDR needs one elevated HKLM write. Existing values are never overwritten. Any other registry write needs a new decision.

## Updater

Priority 1 is finishing reliably on unstable networks. Priority 2 is speed, through bounded parallel downloads.

- Partial downloads and staged files survive interruptions and restarts. Range responses and validators are checked before appending.
- Transient failures retry with backoff; network loss waits and resumes on its own. A stalled connection is reconnected from the staged byte.
- The full changed delta is staged and verified before any installed file changes.
- Free space is checked first.
- A durable journal and per-file backups make replacement recoverable. Multi-file replacement is not atomic; recovery handles interruption between files.
- The official `ri.bin` is written last.
- Play is blocked during install or recovery, and running game files are never modified.
- Unknown files are never deleted. Verify files is an explicit action, separate from the launch check.
- An interrupted transaction whose official source changed is not migrated automatically; the user is offered Install elsewhere.

Downloads shows speed, bytes and files ready out of total, active files with retry state, and queue order. Rates are measured from real network bytes, never inferred.

## Open before a public release

- Installer run on clean Windows 10 and 11 machines.
- Clean first-run Play for every title, including SDVX.
- A real interrupted update of an older installed version.
- Complete transitive license notices and code signing.
- Permission to use and extract official components, and rights for the background images.

See [release](release.md).

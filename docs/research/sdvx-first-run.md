# SDVX first run and game settings

Date: **2026-10-17**. Static comparison and synthetic tests only. No official launcher, reporter, settings program, authenticated SDK operation, or game was executed for this follow-up. No account, browser credentials, or private game log was inspected. No existing installation was used as a write-test target, and no registry value was changed.

## Reported sequence and limits

The requester reported that the first SDVX start entered a check and then crashed, subsequent Hub starts were rejected by the server, and moving the installation followed by a successful official launch also allowed Hub to launch it. This is requester-reported eventual SDVX launch success, with an unresolved first-run failure. It is not a clean fresh-install acceptance result.

The exact first check, the crashing process (Hub or game), exit status, and official-service result were not captured. Moving the folder and running another launcher change multiple variables. The sequence does not identify a cause, and no red-capable reproduction of that failure is available under the current no-game/no-authenticated-test restrictions.

The Hub already requests a fresh real token on every Play. Nothing in this report establishes token reuse. Do not weaken entitlement checks, automatically retry a denial, run the crash reporter, or add registry writes as a guessed fix.

## Confirmed comparison

- The official reporter starts `InstallDir + game\\modules\\sv6c.exe` with ` -t <token>` and the install root as the working directory. The Hub's SDVX profile uses the same game path, token-only arguments, and install-root working directory. See [SDVX contract](sdvx-contract.md) and `crates/konaste-runtime/src/title.rs`, `game.rs`.
- The official launcher starts `launcher\\modules\\errorreporter.exe`; the reporter retains and waits on the **game** handle. The launcher does not retain the reporter handle. The Hub creates the game directly and does not execute the reporter or upload crash logs. See [SDVX contract](sdvx-contract.md) and [game creation](game-creation.md).
- The official launcher resolves `HKCU\\SOFTWARE\\KONAMI\\SOUND VOLTEX EXCEED GEAR\\InstallDir`, falling back to HKLM if opening HKCU fails. Hub Play uses the selected imported root and does not publish an official `InstallDir`. Whether SDVX itself needs that value on a fresh first run is not established.

These differences are not confirmed causes. A useful comparison would change one factor at a time on an explicitly authorized isolated installation, not on the requester's working installation.

## Evidence for a recurrence

Start with the exact Hub error, approximate time, whether Hub or the game closed, and any displayed game exit code. The Hub now retains the observed numeric game exit code in memory and displays it alongside a game-crash error. An official-service denial is shown separately and must not inherit a prior game exit code. Those values do not contain tokens, cookies, account data, paths, or launch arguments and are not saved as entitlement or preferences.

A safe next investigation would use those details to choose a narrow reproduction. Registry values or before/after file hashes may become useful later, but they are not required upfront and have not been read here. Never share a token-bearing command line or authentication URL; redact tokens and personal paths before sharing an artifact.

## Official launcher reference, not the Hub settings target

The retained current official `launcher.exe` is 1,918,328 bytes, SHA-256 `4e9f9a2cfd99d9f24f3a9c3c13396a94e5d5be816e4855598990a3521ef64acf`. Static `SettingsAppCaller` at `0x140028040` calls shared `AppCaller` at `0x14005E090` with flag 1. The single settings-path reference is at `0x140028152`.

| Field | Official value |
|---|---|
| File | `InstallDir + launcher\\modules\\settings.exe` |
| Parameters | Empty |
| Working directory | Install root |
| Verb / display | `open` / `SW_SHOWNORMAL` |
| Mask | `SEE_MASK_NOASYNC` (`0x100`); reporter/game wait behavior is not used |

The retained independently verified index has a `launcher/modules/settings.exe` entry, revision `2026012101`, size 1,580,136, SHA-256 `e16b779e234194085e8fc01c68a7cd746d1f573abf5cbdf2368d79511bd2a323`. This is historical static evidence for the official launcher's copy only, **not the Hub target or a hardcoded production pin**. The separate `game/modules/settings.exe` has different bytes; that game-content executable is the corrected Hub target. The old launcher disassembly does not establish the game copy's dependencies or working-directory requirements. The older 2022 MSI settings executable is different again.

### Target and native command correction

The requester corrected the first implementation: **the installed game settings file is `game/modules/settings.exe`**, not the official launcher's separate copy. The original code and constant-driven fixtures mistakenly refused a game-only settings layout. Hub now pins and targets exactly `<selected install root>/game/modules/settings.exe`; it does not fall back to `launcher/modules/settings.exe` or install the launcher as a workaround. SDVX's empty content prefix means an index-relative `game/modules/settings.exe` maps directly under the selected root.

A second defect, a missing command permission in the retired Tauri shell, caused the immediate generic error. It does not apply to the native Hub.

**Details → Game settings** targets the unchanged installed SDVX settings program. Its UI remains Japanese. No binary translation, patch, or settings-file reimplementation is included. The current handoff keeps empty arguments and the install root as cwd; this is not a new static proof of the different game copy's runtime behavior.

Each user request obtains fresh official metadata and derives the exact fixed-target size/hash from the authenticated original index bytes, not the caller-editable decoded projection or installed `ri.bin`. Native no-follow directory/file pins, hardlink rejection, size/hash checks, fixed empty arguments, and a final native identity/running/update guard precede process creation. Missing or modified settings fail closed; no other settings executable is substituted.

Unlike the official fire-and-forget call, Hub retains its process handle and verified-file pins until the settings process exits. An in-memory Configuring phase excludes checks, imports, Play, and updates for that title; the launch admission guard remains held. Closing Hub while its settings window is open minimizes it. After a normal settings exit, the installation is checked again before Play can become available. Process cleanup does not terminate settings. Play and update-mutation guards also reject an externally started SDVX settings process at the selected root.

After the target correction the workspace tests and strict lint for the runtime and discovery crates passed. Retained-file tests remain ignored and were not rerun.

An actual settings window and the reported first-run failure still need requester-side native acceptance; source validation and synthetic tests do not establish them. No settings/game/official launcher was executed and no existing installation was modified.

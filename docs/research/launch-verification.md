# Bombergirl launch-verification contract

Date: **2026-10-16**. This records static first-party executable evidence and the source-implemented, synthetic-tested read-only verifier. The requester subsequently reported **successful launch verification** in their development launcher and supplied its success/no-game-start toast. That is requester-reported interactive acceptance, not a maintainer-run authenticated probe or raw SDK capture. No game was started for the verification increment. Static research used no real token, authenticated library request, purchase, subscription change, installation write, TLS interception, or browser credential-store read.

## Primary inputs

The already-retained official Bombergirl MSI was checked before parsing:

- Official source: <https://d1q1iflay6y5jj.cloudfront.net/Installer/2021122200_b37bc853/BomberGirl_Installer.msi>.
- 38,649,856 bytes; SHA-256 `977f60c5d05828ca5263b7367cd0e153ed330fc134a5a141bb0ca32f98355aae`.
- Authenticode: `WinVerifyTrust` 0, with no UI/revocation network lookup. SignTool `/pa /v` also passed.
- Static MSI mapping: embedded `#Sample.cab`, `File` key `launcher_modules_launcher.exe`, long name `launcher.exe`, sequence 65 of 88.
- Only that cabinet member was decoded into a fresh research directory. No MSI custom action or executable was run.

Extracted launcher:

| Field | Value |
|---|---|
| Bytes | 1,873,384 |
| SHA-256 | `2e5ccec5c14797c8cda662eb4eebe89086ad761d1a8ff34a53713a65322f66bc` |
| MD5 | `12d2a212c2168b1f9c3b70d2b03b10e1` |
| Authenticode | `WinVerifyTrust` 0; SignTool passed; Konami Amusement Co., Ltd. |
| Japanese ProductVersion | `QBG:J:B:A:2021122100` |
| Fixed ProductVersion | `2021.12.21.0` |
| FileVersion | `0.1.0.1` |

The paired `libeacnet.dll` / `avs2-core.dll` are the independently pinned bootstrap pair in [updater-mode evidence](updater-mode-bootstrap.md). No proprietary executable or IDA database is included in the repository.

## Three versions and two boot modes

Do not substitute one field for another:

- Launcher client profile: `QBG:J:B:A:2021122100`.
- Installed game `modules/bomber.exe` ProductVersion: read from that file's Japanese `041104b0` resource; not guessed from the index revision.
- Resource-index `ri_ver`: a numeric content revision, not a PE ProductVersion.

`launcher.exe` `sub_140014D20` builds the 0xA0 eacnet record with **mode 1**, `bomber-girl`, its own client ProductVersion, the command's token, and `dn`. The updater uses mode 2. The old experimental mode-0 probe was not the official launcher mode and must not be used as its contract.

For verification, use a fresh legitimate callback token and `dn = 0`. Do not carry developer/trial exceptions into this implementation.

Static `libeacnet.dll` boot worker `sub_18001A920` establishes:

1. Mode 1 obtains services, public information, server state, and clock.
2. Unlike mode 2, it executes version-check cases 8–9. A non-successful response or permission other than the normal allowed branch must not become success.
3. Case 10 starts heartbeat naturally. All nonzero modes then go to case 20; subscription/account-list cases 11–19 are the mode-0 branch, **not the official mode-1 launcher sequence**.
4. `OA043711000001 == 2` means boot-worker exit. Error getter `000003` must be 0. Normal result getter `000009` is 1, not 2. Heartbeat exit can change that result to 2.

Boot input lifetime was also checked statically. The export copies the mode at `0x18001A639`–`0x18001A63F`, game/token/client strings through `0x18001A6D5`, and reads `dn` at `0x18001A6E8`–`0x18001A6F4`, before creating the worker at `0x18001A76F`–`0x18001A782`. That worker receives a null parameter and uses library-owned copied fields, not the caller's record pointer. There is no whole-record `0xA0` memcpy; it is not needed to establish this lifetime. The caller keeps the record alive through the synchronous boot call and zeroizes it only after that call returns. Its `dn` byte is already zero.

Official launcher `sub_140015280` waits for worker exit and checks the error getter, then obtains resource metadata. The hub retains its independently validated fresh index/local-installation prerequisites; no cached-index or dummy-token launch bypass is introduced.

## Module verification

Primary launcher sites: `sub_140016930`, `sub_140016E60`, version reader `sub_140044D60`.

- The launcher reads **`<install>/modules/bomber.exe`**, not its own version for this stage.
- `sub_140044D60` queries exactly `\\StringFileInfo\\041104b0\\ProductVersion` and copies to a 21-byte output buffer.
- `OA043711000030` receives that game ProductVersion string. It sends request type 5 (`EacnetRequestGetSum`, method `getHash`, request field `name`). It does not receive a filesystem path or a caller-supplied hash.
- `000031` polls for completion. Once it returns 2, the request has been consumed/released. It returns 2 on both successful and failed imports, so require `000003 == 0` and never poll again after 2.
- `000032(char *out, int capacity)` copies the returned C string. The official caller supplies 65 bytes.
- The official caller SHA-256s `modules/bomber.exe`, formats the 32 digest bytes as 64 lowercase hexadecimal characters, and compares against that returned hash. A mismatch blocks the normal path.

Relevant pinned-library addresses:

| Export | Address | Checked contract |
|---|---|---|
| `OA043711000030` | `0x18001BF20` | `(const char *game_product_version)`; 0 enqueued, otherwise error |
| `OA043711000031` | `0x18001C010` | `()`; 0 pending, 2 consumed; then require error 0 |
| `OA043711000032` | `0x18001C1D0` | `(char *out, int cap)`; 0 copied; cap 65 |

The library uses the version string as a formatting argument internally. Accept only the observed bounded ASCII QBG ProductVersion grammar, with no format directives, truncation, NULs, or control characters. Reject malformed/non-hex/empty server digests rather than treating a successful copy call as module approval.

The hub's read-only installation prerequisite additionally compares installed `ri.bin` against the fresh discovered index digest and the game module against that index entry's final stored size/SHA-256. This does not require game-content transport or executing a game. No installation directory is mounted into AVS; the parent owns read-only, no-write/no-delete file leases and supplies only the checked version/digest to the worker.

## Official game-start authorization

Primary launcher sites: `sub_1400174A0`, `sub_1400179B0`. Pinned library `OA043711000040` at `0x18001C430`, `000041` at `0x18001C560`.

| Export | Arguments / return |
|---|---|
| `OA043711000040` | Pointer to the fresh callback-token string. The caller treats a nonzero return as sent. This pinned implementation returns **1 on its paths**, including request failure; therefore also require error getter 0 before polling. A zero-style enqueue convention from other exports is wrong here. |
| `OA043711000041` | No arguments. 0 pending, **2 consumed/released**, including failed responses. Stop polling after 2, then require error getter `000003 == 0`. |

`EacnetRequestCheckGameStart` serializes method `checkGameStart` and `data/token`. The library prepares a 256-byte token buffer; the hub uses a zeroed bounded buffer, not an unbounded pointer. The boot record's token capacity is 65 bytes, so oversized tokens must be rejected, never truncated.

The library maps nonzero service errors to errors including 2002–2005. The official launcher reads **the error getter after poll 2**, not an authorization value from the poll itself. Error 0 takes the game-creation branch. Codes 1 and 2005 enter a bounded official retry path; 2004 takes a non-launch status path. Their business meanings are not guessed here. The initial read-only verifier can fail closed on any nonzero result rather than inventing an entitlement or retry policy.

Earlier `BG` notes conflated poll results with error-getter values and elsewhere claimed the launcher never calls `checkGameStart`. Both interpretations are contradicted by these pinned call sites. Normal module and server authorization checks must both succeed. A callback token by itself grants no permission.

## Verification without game creation

The development action must be explicit and explain that no game starts. It may obtain a fresh callback through the app-owned official browser only after successful fresh metadata/local-file checks and maintenance checks. It must recheck installation/request identity and maintenance after authentication and after native verification.

Token-bearing data stays in native, zeroizing memory and private worker pipes. No token belongs in command-line arguments, environment, diagnostics, settings, snapshots, UI state, or research captures. Do not reuse a token from a successful verification attempt for a later Play attempt.

The worker retains the same paired-DLL pins, strict load search, scratch-only AVS configuration, process/memory/deadline limits, and single-consumption polling as discovery. It does not call the official downloader or create any game/errorreporter process. A successful verification receipt is a result for that one attempt, not persistent entitlement or a general game-spawn capability.

## Interactive acceptance

On **2026-10-16**, the requester reported “Verification pass” and supplied a launcher toast reading “启动验证已通过。游戏未启动。” This accepts the explicit read-only verification path in their app. Its native success branch requires the fresh local-file, mode-1, module-hash, and `checkGameStart` checks described above. No account details or token values were supplied or collected. The requester then explicitly asked to proceed to game launching.

No maintainer-run authenticated probe or raw official response capture was performed. The reported Verify success alone does not establish game creation, game lifetime, behavior for other subscriptions/titles, or a reusable grant. The requester subsequently reported successful actual Bombergirl launch on the same date; that separate acceptance is recorded in [the game-creation evidence](game-creation.md). Every Play attempt still needs a fresh token and normal verification. Any further interactive authentication must stay on the official website, never in chat.

A separate anonymous, header-only GET of the exact base launch-login endpoint returned **HTTP 200 at 2026-10-16 03:27 JST**, outside the recurring maintenance window. Redirects were disabled, no cookies/authorization were supplied, no body was read, and normal TLS verification remained enabled. This closes the previously missing anonymous healthy-status baseline; it proves neither entitlement nor WebView/GUI recovery behavior.

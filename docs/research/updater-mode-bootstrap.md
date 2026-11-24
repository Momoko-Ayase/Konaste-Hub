# Updater-mode metadata discovery

Research dates: **2026-10-14** (historical breakpoint experiment) and **2026-10-15** (normal public-export completion). Scope: one pinned Bombergirl launcher-library pair. No official launcher, updater, installer, or game executable was started. No installation was changed, account session used, browser store read, or TLS verification changed.

The earlier note incorrectly interpreted two completion results and the mode-2 branch. Those claims are corrected below. The successful normal probe does not use the earlier debugger mechanism.

## Provenance

The historical research directory contains copies from `launcher/modules` with these identities. The two DLLs also match the independently pinned files statically extracted from the official MSI by `konaste-bootstrap`.

| File | Bytes | SHA-256 | `WinVerifyTrust` |
|---|---:|---|---|
| `updater.exe` | 1,877,480 | `e3a4885be89bd05968f045a3f21a73669f75b5ee6216ecd7d696bdde9303b341` | 0 |
| `libeacnet.dll` | 8,768,488 | `a8cdc52b3eec6002011b2742f4cea84c01770e3d4ee7bb12ea7cacb3f21e2203` | 0 |
| `avs2-core.dll` | 1,635,400 | `2ac717d929f368d199a3fc4cb85f8066e79377db3675e66f8050bbf61277887e` | 0 |

The 2026-10-15 probes rechecked the DLL hashes and Authenticode. Trust checks used `WTD_UI_NONE`, no revocation check, and `WTD_CACHE_ONLY_URL_RETRIEVAL`; no online revocation result is claimed.

`updater.exe` was only read as a version resource and static image. Its `041104b0` ProductVersion is **`QBG:J:C:A:2021122100`**, FileVersion `2.0.1.100`, description `BOMBERGIRL Updater`. This literal identifies the recorded updater profile. It is not the installed game's PE version or the resource-index revision. The prior mode-0 research used launcher ProductVersion `QBG:J:B:A:2021122100` and is a different experiment.

## AVS initialization

Primary call sites: `updater.exe` `sub_140017D70`. Both libraries are x64; calls use the Windows x64 convention. AVS ordinals have export base 1.

| Ordinal | Name | Arguments / checked result |
|---|---|---|
| 145 | `XCgsqzn0000090` | `(0x17, descriptor, 0xD18)` creates a property; descriptor is a zeroed 0xD18-byte buffer |
| 163 | `XCgsqzn00000A2` | root `(property, NULL, type, name, value)`; child `(NULL, parent, type, name, value)` |
| 171 | `XCgsqzn00000AA` | `(node)` after a non-null string or attribute add |
| 159 | `XCgsqzn000009E` | `(property)` must return 0 |
| 162 | `XCgsqzn00000A1` | `(property, NULL, "/config")` must return non-null |
| 298 | `XCgsqzn0000129` | `(config, real buffer, 0x08000000, 0, 0, 0)` must return 1 |
| 146 | `XCgsqzn0000091` | `(property)` after boot |

The AVS buffer is a real **128 MiB** allocation. A null pointer is known to crash. Keep the allocation and loaded libraries alive until worker-process exit: native threads may still own them even after the eacnet shutdown call.

Node types `0x0B` and `0x2E` take a C string as the fifth argument; types `1`, `5`, and `0x34` take an immediate 32-bit value. The verified tree is:

- `/config/fs/root/device` = `.`, `/config/fs/root/option` = `posix=0`.
- `/config/fs/mounttable`: two `vfs` children, each with `name@=boot`:
  - `fstype@=fs`, `src@=<fresh scratch>/work/updater/dev/raw`, `dst@=/dev/raw`, `opt@=vf=0,posix=0`.
  - `fstype@=nvram`, `src@=<fresh scratch>/work/updater/dev/nvram`, `dst@=/dev/nvram`, `opt@=sz_block=512,nr_block=32,nr_mirror=4`.
- `/config/sntp/ea_on` = 0 and `/config/net/enable_raw` = 0, type `0x34`.
- `/config/fs/nr_mountpoint`, `/config/thread/nr_mutex`, `/config/fs/nr_filedesc` = `0x100`, type `5`.
- `/config/log/level` = `DISABLE`; `/config/log/enable_file` = 0, type `0x34`.

No imagefs, `/InstallDir`, or `/TEMP` mount is required. AVS creates its own scratch NVRAM/clock files; this is not a filesystem-pure operation. The historical experiment also produced scratch logs despite its disabled logging configuration. No installed content or installed NVRAM is supplied to this operation.

## Fixed eacnet boot record

Primary sites: `updater.exe` `sub_1400132E0`, `libeacnet.dll` `OA043711000000` at `0x18001A4E0`. One pointer to a zeroed 0xA0-byte record:

| Offset | Capacity | Value |
|---|---:|---|
| `+0x00` | u32 | mode **2** |
| `+0x04` | 0x41 | `bomber-girl` |
| `+0x45` | 0x15 | `QBG:J:C:A:2021122100` |
| `+0x5A` | 0x41 | fixed non-identity placeholder `public-metadata` |
| `+0x9B` | byte | `dn` = 0 |

The original updater reads its own `041104b0` ProductVersion (`sub_14003FC00`), not the game executable. The boot validator permits an empty client-version field only in mode 2. It rejects an empty game ID with `0x3EB` and an empty token with `0x3EC`, before starting a thread. Empty-token mode 2 was not executed. No real token is needed by the verified discovery path or accepted by the new discovery interface.

## Corrected state and completion contract

Primary static evidence: `libeacnet.dll` `sub_18001A920`, heartbeat thread `sub_180027A40`, getter `OA043711000009` at `0x18001DCC0`; official updater wait `sub_140013540` and resource-enqueue caller `sub_140013790`.

- Boot returns 0 after starting its worker.
- Steps 0–7 obtain the service list, server state, and clock.
- **Case 7 sends mode 2 directly to step 10. It skips version-check cases 8 and 9.** The earlier interpretation that the breakpoint's step 10 proved a permission-1 version response was wrong.
- Case 10 starts the heartbeat thread. There is no mode test inside that startup function. Mode 0 then goes to subscription/account cases 11–19; mode 2 goes to terminal case 20.
- Case 20 stores result **1**, the normal completion result.
- Case 24 stores result **2**, a failure result. Heartbeat-thread exit also stores 2.
- `OA043711000001 == 2` means the boot worker exited. It is not a success result.
- `OA043711000009` returns the result above: normal is **1**, not 2. The earlier note's claim that the official updater waits for both exports to equal 2 was wrong.
- The official updater waits for `000001 == 2`, checks `000003 == 0`, then calls `000026`. It does not test `000009` there. The new probe additionally required result 1 immediately after boot.

Mode 2 therefore does **not** mean no auth-class traffic: it naturally starts a heartbeat whose method is in the `need_auth` class. No probe inspected or decrypted its request/response body, established exact rejection/retry behavior, or treated it as entitlement proof. The public resource request is separately validated. No subscription, account-list, download, or game-start export is called by the helper.

## Resource-info exports

These exports use the one boot context; they do not accept arbitrary request bodies.

| Export | Address | Contract |
|---|---|---|
| `OA043711000026` | `0x18001CA50` | `()`; enqueue type 4 `EacnetRequestResourceInfo`; 0 means enqueued. Clears previous URL/hash and error. |
| `OA043711000027` | `0x18001CB20` | `()`; 0 while pending; **2 on completion, including failure**. It imports the result and releases the request. Never call again after 2. |
| `OA043711000003` | reads error dword | Must be 0 after completion; 2 from the poll alone does not prove success. |
| `OA043711000028` | `0x18001CD70` | `(char *url, int url_cap, char *hash, int hash_cap)`; capacities `0x800`, `0x41`; 0 means copied. URL first, hash second. |
| `OA043711000002` | `0x18001A820` | `()`; stop/join boot worker. Does not guarantee cancellation of an already-entered native call. |

The method is `getResourceInfo`; fields copied out are `eacnet/response/url` and `eacnet/response/hash`. Calling `000027` a second time after it released its request is a harness error. The prior note also incorrectly claimed 2 was returned only on successful import.

`OA043711000033` (download) and `OA043711000040` (`checkGameStart`) are never called. Neither are account/subscription/goods/user-id exports. The helper does not set private state, patch code, or manufacture success.

## Normal-mode execution, 2026-10-15

Three fresh Python/ctypes research processes used the pinned pair after hash/Authenticode checks, one 128 MiB allocation, strict DLL-directory/System32 search, and a Windows job limited to 512 MiB and one active process with kill-on-close. Each had a new scratch directory. No debugger/private RVA was used. Job process limits are **not a filesystem sandbox**; the native operation's verified fixed configuration supplies no installation path.

1. `konaste-discovery-normal-sxq2r__2`: AVS 1, boot 0, state 2, result 1, error 0, shutdown 0. The harness rejected the old, incorrect `result == 2` assumption. Not a library boot failure.
2. `konaste-discovery-normal-8khcn__t`: the harness called `000027` again after completion and saw error 1. Static inspection confirmed the first call releases the request. No success is claimed for that attempt.
3. `konaste-discovery-normal-evvl9bw7`: corrected, single-consumption result:

| Field | Value |
|---|---|
| AVS boot / eacnet boot | 1 / 0 |
| Boot worker / result / error | 2 / **1** / 0 |
| Resource enqueue / completion / error / copy | 0 / 2 / 0 / 0 |
| URL | `https://d1q1iflay6y5jj.cloudfront.net/ResourceInfo/2026091500_ktdr8wre/qbg_2026091500_pro.xml` |
| SHA-256 | `205bb4a45e1b38ad421ea8b0ab5c41214b99c7cc5714bbdecf8d15dd3e881e9a` |
| Shutdown / process exit | 0 / 0 |

Only the returned public metadata strings and numeric export results were inspected. No content GET was made by these Python probes. The heartbeat was allowed to start normally, not intercepted; its response contents are unknown. The result does not authorize Play.

The URL is a current discovery response, **not a stable endpoint to hardcode**. A digest computed from a fetched body is not a substitute for the separately discovered digest.

## Historical breakpoint experiment, 2026-10-14

The earlier helper stopped before heartbeat startup by a write breakpoint on a private step dword, then setting a private stop flag. The observed writes `4 5 6 7 10` agree with the corrected mode-2 skip; they do not prove a version-check response. The boot-result field remained 0 because normal terminal case 20 was not reached. The breakpoint had an arming race after boot and must not ship.

That experiment returned the same URL/hash. A separate anonymous HTTPS GET returned HTTP 200, 2,681,096 bytes, SHA-256 `205bb4a45e1b38ad421ea8b0ab5c41214b99c7cc5714bbdecf8d15dd3e881e9a`, MD5 `a5af15398bdf46e95c1de2c8aef84fec`. It was not copied from installed `ri.bin`. The proprietary body is not in the repository. The later [manifest contract](manifest-contract.md) establishes its pure structural decoder.

## Native implementation and remaining limits

`konaste-runtime` now owns the fresh pinned-runtime worker; `konaste-discovery` separately validates/fetches/decodes the returned index. The fixed discovery operation accepts no token or installation root. The shared worker additionally has one fixed verification operation, documented in [launch verification](launch-verification.md); it is not a generic export, download, or process-spawn interface. The Hub executable dispatches `--konaste-runtime-worker` before any UI starts. Native integration is tested separately and is not inferred from these research probes.

Keep the updater's ProductVersion, installed game PE ProductVersion, global `ri_ver`, and per-file revisions distinct. A successfully discovered resource index does not prove local installation integrity, content-transfer identity, subscription, or entitlement. Do not infer a runnable game or executable update plan from it.

Still unverified here: SDVX/other-title discovery, empty-token mode 2, real-account token behavior, exact heartbeat payload behavior, game-content transfer identity, download/install/resume/launch, public-distribution component-use rights. Existing installations remain unchanged.

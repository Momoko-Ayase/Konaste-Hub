# Multi-title native integration

Date: **2026-10-16**. Scope: SOUND VOLTEX first, followed by the remaining downloadable PC titles. Existing installations were not used as write-test targets. No authenticated verification, game launch, installer execution, registry write, purchase, subscription change, TLS change, or browser credential access was used for this increment.

## Supported operations

| Title | Bootstrap / fresh metadata | Import, check, install, update, pause/resume | Normal Play implementation | Interactive launch acceptance |
|---|---|---|---|---|
| Bombergirl | Yes | Yes | Yes | Requester reported success in the earlier increment |
| SOUND VOLTEX EXCEED GEAR | Yes, current pinned SDK | Yes | Yes, current normal caller | Not run |
| DanceDanceRevolution GRAND PRIX | Yes | Yes | Yes, typed graphics settings / `game` cwd | Not run |
| pop'n music Lively | Yes, current pinned SDK | Yes | Yes, current normal caller / `game` cwd | Not run |
| GITADORA | Yes, signed compatible SDK | Yes | Yes, explicit GuitarFreaks/DrumMania choice | Not run |
| NOSTALGIA | Yes | Yes | Yes, current normal caller / `game` cwd | Not run |
| beatmania IIDX INFINITAS | Separate embedded protocol not implemented | Not enabled | Not enabled | Not run |

“Implementation” is not authenticated acceptance. NOSTALGIA now also has a complete isolated content-install check (below); the other newly added titles have bounded anonymous content checks, not full-install acceptance. No real older-version update acceptance is claimed. The earlier complete Bombergirl isolated installation remains separate evidence.

Installation and launch are independent native capabilities. `Title` owns verified file layout, product-code grammar, and HTTPS namespaces; its optional launch contract is the only source of creation permission. `GameSnapshot.supported`, `bootstrap`, and `launchSupported` are derived, not saved grants. A title without a proven launch contract stays Blocked / `launchUnsupported`, even when its installation is current. The six currently admitted titles now have launch implementations, but GITADORA additionally requires an explicit saved instrument choice. Completed installation clears pending tracking and releases its reservation; it never starts a game. Required updates remain available. Neither a Ready-looking frontend value nor a fresh account callback is entitlement or an executable creation grant.

## Fresh anonymous indexes

These locations and expected digests came from the normal official mode-2 worker, not guessed build stamps. Production discovery independently fetched and checked each body through its HTTPS/identity/bounds/decoder seam. The table contains declared stored bytes, not a claim that all of them were downloaded.

| Title | Revision | Files | Declared stored bytes | Index SHA-256 |
|---|---:|---:|---:|---|
| SDVX | 2026070300 | 19,311 | 9,003,326,204 | `97697900e0ea87faf6caaf8d55b024f5204b867a613857c55db7d00540beb3b5` |
| DDR | 2026091600 | 4,938 | 23,666,014,036 | `7600f0633bd7c2808fd684095b42cb402d775dc2a1ed664fa902aaf549c7bd83` |
| pop'n | 2026080500 | 4,031 | 5,568,649,986 | `9a94d58a8d3d23708ad4110978a2f4925d7d815230bd5c15ae4ed8e94ee42a0c` |
| GITADORA | 2026062300 | 1,597 | 4,504,906,876 | `95c09deae96b790f69001fee6ee54b7abf84b318d49df13cef0ac162f7192ce7` |
| NOSTALGIA | 2023112800 | 3,663 | 1,009,711,542 | `5256582a366d2028da7488f1c1fde8ab04f50e727eb82af1631a6403faccf1c0` |

Verified index locations:

- SDVX: `https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource_info/resource_info_pro_2026070300.xml`
- DDR: `https://d2el0dli9l0x2p.cloudfront.net/list/resource_info_pro_2026091600_7600f06.xml`
- pop'n: `https://d1twxdn5j8sbm3.cloudfront.net/ResourceInfo/U39JAA2026080500_resource_info_pro.xml`
- GITADORA: `https://d1omkh45tn6edw.cloudfront.net/data/list/resource_info_2026062300pro_dkju9p4a.xml`
- NOSTALGIA: `https://d26kzmeiv4899f.cloudfront.net/data/list/resource_info_2023112800pro.xml`

Each host and resource namespace is independently restricted by its fixed profile. Content uses its own verified prefix: SDVX `/vi/downloads/resource/`, DDR `/update/`, pop'n `/Updater/`, and GITADORA/NOSTALGIA `/data/`. File URLs must also end with the exact index-relative path. No arbitrary URL or manifest enters frontend IPC.

## Index encoding and paths

SDVX's independently verified index is **6,158,980 bytes**, with header `A0 45 00 FF`. The initial 4 MiB limit correctly refused it. The decoder now has a bounded 8 MiB limit and supports the verified raw-ASCII node-name form alongside Bombergirl's packed `A0 42` form. A plain name's length byte is `0x40 | (length - 1)`, for 1–64 characters. Unknown encodings, malformed names, padding, nesting, schema fields, duplicates, unsafe Windows paths, bad digests, and trailing bytes remain errors. The single root `ri_ver` may occur between file records; file order is preserved.

All six admitted installation descriptors publish the **exact original** `ri.bin` at the selected root, last. They do not insert an extra `game/` directory into the index paths. In particular pop'n uses singular `game/module/`.

| Title | Main index path | Actual checked PE ProductVersion |
|---|---|---|
| SDVX | `game/modules/sv6c.exe` | `QCV:J:B:A:2026070300` |
| DDR | `game/modules/ddr-konaste.exe` | `VGP:J:A:A:2026091600` |
| pop'n | `game/module/popnLively.exe` | `U39:J:A:A:2026080500` |
| GITADORA | `game/modules/gitadora.exe` | `U32:J:A:A:2026062300` |
| NOSTALGIA | `game/modules/nostalgia.exe` | `UAN:J:B:A:2023112800` |

The game modules were anonymously downloaded into new temporary directories, checked against the fresh indexes' independent size/SHA-256, and read with the Windows version-resource reader. None was executed. Versions were not synthesized from `ri_ver` or copied from launcher resources. The production read-only installation check also accepted the retained SDVX index/module in an isolated fixture and retained its write-denying leases.

## Current SDVX runtime

The MSI still supplies the pinned launcher AVS. Its 2022 eacnet is not silently treated as the current normal caller. The fresh official index identifies the current eacnet, launcher, and updater; static inspection established the current contract before the new SDK was loaded.

- MSI: the unchanged `sdvx_installer_2022011800.msi` pin.
- AVS: MSI `avs2-core.dll`, 1,633,232 bytes, SHA-256 `92132343f29b28dec9f9e513c34230435ee14658c988031f42498dcf699d6a85`.
- Current `libeacnet.dll`: 8,742,720 bytes, SHA-256 `84564b3f804dd66e6d7008895502b7d4caab17bdb19317590c0450cf2447df2c`, official URL `https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource/2023011200/launcher/modules/libeacnet.dll`.
- Current mode-2 profile: `QCV:J:C:A:2023021500`. Current mode-1 profile: `QCV:J:A:A:2023021500`.
- Record: exactly `0x89C` bytes. The normal services slot `[0x9C, 0x89C)` is entirely zero; it selects the SDK's built-in normal endpoint. No services URL, `usta`, developer, or trial input is exposed.
- Existing export conventions and AVS ordinals/descriptor/buffer remain verified. The private worker protocol is **4**. Tokens still use only zeroizing native storage/private pipes and the unavoidable normal game's `-t` argument, never worker argv/environment, frontend state, settings, or logs.

With explicit component-download consent, bootstrap verifies the MSI and extracted old member, downloads and verifies the separately pinned current eacnet (including Authenticode), then publishes the final pair atomically. An existing old SDVX pair is stale. Offline helpers require the retained current DLL and never download it implicitly. Cache provenance includes the overlay URL/hash and is checked against the fixed descriptor rather than trusted by itself.

Fresh discovery and content probes also passed with the **current** SDK and the extended normal record, not only the older 2022 research pair. Details and static addresses are in [SDVX contract](sdvx-contract.md).

## Other current caller profiles and SDK trust

| Title | Mode-1 client profile | Mode-2 client profile | SDK boot shape |
|---|---|---|---|
| DDR | `VGP:J:B:A:2026012800` | `VGP:J:C:A:2026012800` | Legacy `0xA0` |
| pop'n | `U39:J:B:A:2026031100` | `U39:J:D:A:2026031100` | Services `0x89C`, zero normal services slot |
| GITADORA | `U32:J:C:A:2026062300` | `U32:J:D:A:2026062300` | Legacy `0xA0` |
| NOSTALGIA | `UAN:J:A:A:2023042500` | `UAN:J:C:A:2023042500` | Legacy `0xA0` |

These client profiles come from the current official callers' PE resources and their object-to-boot copies, not from the game PE or resource revision. Fresh anonymous discovery passed with these profiles. No real account token or authenticated mode-1 check was used to obtain that result.

Pop'n now also acquires a separately pinned **signed** current eacnet from `https://d1twxdn5j8sbm3.cloudfront.net/Updater/2026080500/launcher/modules/libeacnet.dll`: 8,753,008 bytes, SHA-256 `119bfb79688e8ea323c183c319e0ec665fc0c27cc3587d5edc32d4ce0b6b60ae`. The MSI's old member must first match `59f5ab7f6151b111c9686e5d52a1fc9b737398cee1d85037598773da8d2c8f6c` / 8,724,912 bytes. Its AVS remains the independently pinned MSI image, byte-identical to the current index entry. The same consent, signature, hash, stage, paired-publication, and stale-cache rules as SDVX apply. Current-SDK pop'n mode 2 also passed the strict normal-terminal check (`boot result == 1`) and production content samples. See [current pop'n](popn-current-launch.md).

**GITADORA's advertised newer SDK is unsigned. It was not loaded.** The current index's `42360c07…` image has an empty PE security directory and returns `TRUST_E_NOSIGNATURE`. No hash-only exception, certificate change, or disabled trust check was added. The hub instead uses the separately verified, signed MSI pair (`libeacnet` `f016c1f6…`, FileVersion `1.1.0.84`; AVS `c380e64b…`) whose required record, module/start methods, return conventions, and current caller arguments match the normal legacy interface. Fresh anonymous discovery with the current 2026 client profile also passes using that signed pair. Actual authenticated compatibility remains an interactive acceptance gate; any denial fails closed. The unsigned update is normal game-tree content, not a privileged hub runtime-cache input.

DDR's launcher eacnet is still exactly its signed MSI pin. NOSTALGIA's fresh index has no replacement launcher SDK entry; its signed MSI pair matches the current caller's required methods. See [current DDR](ddr-current-launch.md) and [current GITADORA/NOSTALGIA](gitadora-nostalgia-current-launch.md).

## Normal game arguments and settings

All Play attempts still repeat fresh metadata and pinned local-module evidence, acquire a new token, require the normal module digest and game-start result, hold a live healthy SDK through the final native identity/maintenance/running-game check, and retain the normal game process without killing it on drop.

- Bombergirl and SDVX: install-root working directory, normal ` -t <fresh token>` only.
- Pop'n and NOSTALGIA: install-root `game` working directory, normal token argument only. The hub does not read or write pop'n `setting.bin` to create a game. The odd quote in pop'n's reporter path literal is not copied into the independently verified executable path.
- DDR: `game` working directory. A missing settings file yields ` --display0 --fps60 -t <fresh token>`. Display, FPS, and window values come from the fixed `HKLM\SOFTWARE\KONAMI\DanceDanceRevolution\ResourceDir` plus `config/settings_cf.xml`; values are parsed as signed C-style base-0 integers, display is monitor-clamped, FPS is only 60/120, and window flags are only fullscreen/borderless.
- GITADORA: `game` working directory. Default graphics prefix is ` -display0 -window`; positive typed settings can select fullscreen, full-HD, and exclusive audio. Ordering is display, window, resolution, audio. The freshly checked token is followed by exactly **one** explicit `-gf` or `-dm`. The normal reporter preserves these flags from its raw-command branch; the URI reconstruction branch is not the normal handoff. No instrument is guessed on first use.

`Arguments` prepares these closed flags **before** SDK preparation, not through filesystem work after the final creation guard. Prefix/suffix text cannot come from frontend argv or raw configuration values. The UTF-16 parameter buffer reserves its full size before token insertion and is zeroizing. No `dn`, trial, services override, or developer flag is accepted.

The read-only settings module accepts the verified `SettingDatas/Parameters/KeyValuePair` text-XML schema (optional UTF-8 BOM), with known key/value strings, last-duplicate-wins behavior, and normal missing-key defaults. XML predefined/numeric escapes are decoded without recursion or an external resolver. DOCTYPE, custom/external entities, malformed input, and unproven binary/JSON configuration bodies fail with `gameSettingsInvalid`; no format is guessed. Accepted data is bounded at 256 KiB with depth/event/string limits. Paths must be local fixed-drive paths. Ancestors are pinned before directory listing or file reads, and reparse points, hardlinks, and concurrent writable file handles are refused. No registry or settings file is written.

For GITADORA, `SOFTWARE\KONAMI\GITADORA` is opened under HKCU first, with HKLM tried only when that key open fails. An opened HKCU key with no `ResourceDir` does not fall through to machine settings. DDR's reporter reads HKLM only. This distinction is tested through the production lookup policy with a fake registry; no host key is read by a test. Exactly one `config/*_cf.xml` is selected under GITADORA's resulting fixed `ResourceDir`. No matching file uses normal defaults. Multiple matching files are deliberately refused until the official per-user RID fallback is mapped; the hub does not guess the first file or inspect other account stores. Those configuration-format/RID cases are known compatibility limits, not permission to bypass checks.

The small GITADORA selector persists only `guitarFreaks`, `drumMania`, or null. It does not authorize a launch. Check stamps bind the instrument and installation identity; mode changes while Starting/Running are refused before settings persistence, and a superseded attempt cannot create a game with a different choice. The mode is not sent in worker argv, environment, or SDK requests.

## Production updater samples

Each row is a fresh-metadata source admitted by `UpdateSource::from_metadata`, followed by the real updater downloading into an owned temporary root. Each main module plus one small content object matched the exact stored length and SHA-256. No full official index was published for these sample-only trees.

| Title | Samples | Verified stored bytes |
|---|---:|---:|
| SDVX, current SDK | 2 | 16,338,075 |
| DDR | 2 | 10,562,907 |
| pop'n | 2 | 8,528,517 |
| GITADORA | 2 | 4,245,101 |
| NOSTALGIA | 2 | 12,399,213 |

The content objects remain opaque. They were not decrypted, unpacked, patched, or renamed into inferred logical names. Full changed-delta verification, same-source fresh rebinding, real response validators, protected native writes, backups, small durable progress records, running-image guards, unknown-file preservation, and index-last publication remain shared implementation behavior. Maintenance blocks Play, not installation.

## Full isolated NOSTALGIA installation

The production `konaste-install --example install -- --game nostalgia <retained-pinned-MSI> --fresh-in <scratch folder>` check completed in a newly created `konaste-install-check-*` tree. It planned **3,663 files / 1,009,711,542 stored bytes**, paused at **4,334,988 bytes** with no early official index, released the entire updater session, rediscovered fresh metadata, reopened and rebound the transaction, resumed, and verified every installed file. The real pinned module read check returned `UAN:J:B:A:2023112800`; exact original `ri.bin` was published last. The process exited 0. No existing installation, registry, app import, account session, or game process was touched. The retained tree is not a game-launch or fresh-machine prerequisite acceptance claim.

## Remaining work and acceptance

Normal launch implementations now exist for six titles. New-title authenticated verification, actual game creation/gameplay/exit behavior, GUI acceptance, fresh-machine prerequisites, and full installs other than the isolated Bombergirl/NOSTALGIA checks remain unverified. DDR/GITADORA's unproven configuration encodings and GITADORA's ambiguous multi-RID fallback remain fail-closed compatibility limits. No cached entitlement, trial exception, or developer path is used to skip those gates.

INFINITAS's official package contains no `libeacnet.dll`; its launcher/updater embed a different WinHTTP/AVS client and use a different login endpoint. The normal service/resource request fields have been located, but the transport verb/headers/body encoding/signature connection and anonymous admission are not proven. It needs a separately verified metadata/authentication adapter, not a guessed family entry or execution of a private embedded function. See [the adapter evidence and concrete blockers](infinitas-adapter.md).

No proprietary body, file list, DLL, installer, or game executable was added to the repository.

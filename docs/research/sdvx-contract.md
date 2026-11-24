# SDVX contract

Research date: **2026-10-16**. Static inspection of the retained official bootstrap MSI and its extracted launcher, updater, error reporter, and pinned runtime DLLs. No official MSI, launcher, updater, installer, or game was executed. No account, cookie, credential, existing installation, or TLS configuration was used.

IDA was opened only on the extracted updater and error reporter, then closed. The created databases are beside that temporary extraction, not in this repository.

## Package

The retained file matches the pin in [platform research](konaste-platform.md) §3. This session recomputed its SHA-256 and the extracted module hashes. It did not repeat `WinVerifyTrust`; the earlier check reported 0 for the MSI and inspected PE files.

| Field | Value |
|---|---|
| Official URL | `https://dks1q2aivwkd6.cloudfront.net/vi/installer/sdvx_installer_2022011800.msi` |
| Retained path | `%TEMP%\konaste-research-bootstrap-rqgvytya\sdvx_installer_2022011800.msi` |
| Static extraction | `%TEMP%\konaste-research-bootstrap-rqgvytya\x\sdvx_installer_2022011800\` |
| Bytes | 32,337,920 |
| SHA-256 | `af3f96f91832a81882a44c9d5232163aaf0aa379df221b68541ee27e12e33894` |
| Allowed package host | `dks1q2aivwkd6.cloudfront.net` only |

Pinned runtime pair, hashed from that extraction:

| File | Version resource | Bytes | SHA-256 |
|---|---|---:|---|
| `libeacnet.dll` | `1.3.3.104` | 8,742,352 | `8f12689e3693065c837a13315fa066467bbc6b24461a1b1e4b299edcf227e438` |
| `avs2-core.dll` | `2.17.3 release (r8311)` | 1,633,232 | `92132343f29b28dec9f9e513c34230435ee14658c988031f42498dcf699d6a85` |

`avs2-core.dll` code and data sections are byte-identical to the pinned Bombergirl launcher AVS. Its larger Bombergirl size is the Authenticode overlay. AFP modules are the same kind of certificate-only difference and are not part of metadata discovery.

The bootstrap cabinet contains launcher modules only. It does not contain `sv6c.exe`, `ri.bin`, or game content. A public package pin is not a current game build.

## Title identity

PE version resources and launcher strings agree on the product name **SOUND VOLTEX EXCEED GEAR**. That is the catalog title recorded in [platform research](konaste-platform.md). The internal game id is the literal `sdvx`, not the display name and not `sv6c`.

| Use | Value | Evidence |
|---|---|---|
| Login entry | `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=sdvx` | Launcher string. Anonymous redirect behavior remains the earlier platform observation, not a new login test. |
| Callback scheme | `konaste.sdvx://login?tk=` | Launcher, updater, and error reporter |
| Registry key | `SOFTWARE\KONAMI\SOUND VOLTEX EXCEED GEAR` | Error reporter `sub_14000A6E0`; the same literal is present in the launcher and updater |
| Registry value | `InstallDir`, `REG_SZ` | Error reporter registry reader |
| Registry lookup | `HKCU`, then `HKLM` only if the first open fails; `KEY_READ` (`0x20019`) | Error reporter `sub_14000A6F0` |
| Registry limit | Copy only when the queried size is `<= 260`; no separator is added | Same reader |
| Module tail | `game\modules\sv6c.exe` (21 bytes, `0x15`) | Launcher hash join and error reporter creation |
| Virtual module path | `/id/game/modules/sv6c.exe` | Launcher string, used by the hash mount rather than process creation |

`sv6c.exe` is the relative game image. It is not a substitute for the eacnet game id.

## Three launcher/updater profiles, no game product code

These are PE `041104b0` ProductVersion resources. They are not interchangeable, and none is the installed game module's ProductVersion.

| PE | FileVersion | ProductVersion | Role |
|---|---|---|---|
| `launcher.exe` | `2.0.0.2` | `QCV:J:A:A:2022011800` | Mode-1 launcher profile |
| `updater.exe` | `2.0.0.2` | `QCV:J:C:A:2022011800` | Mode-2 discovery profile |
| `errorreporter.exe` | `2.0.0.2` | `QCV:J:D:A:2022011800` | Reporter profile, not the boot client version |
| `settings.exe` | `2.0.0.3` | `QCV:J:K:A:2021122200` | Optional settings UI, absent from Bombergirl; not a discovery or launch input |

The game image is absent from this package. Its PE product code is **unknown**. Do not derive it from any `QCV` string. It has to come from the installed or freshly downloaded `sv6c.exe` version resource after a real index identifies that file.

## Runtime ABI

Both pinned `libeacnet.dll` images are x64 PE32+. All 50 named exports have the same ordinals and RVAs. The raw bytes of these functions are equal, so this is not only an export-name match:

| Export | RVA | Compared bytes |
|---|---:|---:|
| `OA043711000000` | `0x1A4E0` | 796 |
| `OA043711000002` | `0x1A8A0` | 117 |
| boot worker | `0x1A920` | 5,488 |
| `OA043711000026` | `0x1CA50` | 201 |
| `OA043711000027` | `0x1CB20` | 115 |
| `OA043711000028` | `0x1CD70` | 140 |
| `OA043711000030` | `0x1BF20` | 235 |
| `OA043711000031` | `0x1C010` | 435 |
| `OA043711000032` | `0x1C1D0` | 66 |
| `OA043711000040` | `0x1C430` | 289 |
| `OA043711000041` | `0x1C560` | 306 |

`OA043711000001` at `0x1A800`, `OA043711000003` at `0x1DCA0`, and `OA043711000009` at `0x1DCC0` are short getter entries. Their immediate bytes also match the Bombergirl image. Because the boot worker and resource/start functions are byte-identical, the mode-1 and mode-2 terminal, error, enqueue, and poll behavior documented for the Bombergirl pair applies to this pair without a second derivation:

- Boot success is worker-finished (`000001 == 2`), error getter 0, and normal result `000009 == 1`.
- Resource poll `000027 == 2` consumes the request on both success and failure. Require error 0 and do not poll again.
- Copy order is URL then hash, capacities `0x800` and `0x41`.
- `000040` uses the nonzero sent convention already established for the identical Bombergirl body.

The SDVX updater independently shows the caller side. `sub_1400132E0` builds the record; `OA043711000000` is called with that pointer and must return 0. The following state polls `OA043711000001`, checks `OA043711000003`, and only then calls `OA043711000026`. `sub_140013AD0` calls `OA043711000028` with URL capacity `0x800` and hash capacity `0x41`.

`libeacnet.dll` is not a whole-file match. The first code section differs by 142 bytes. The only differing exception-directory function is `0x25B50`–`0x25DE7`: SDVX calls shared helper `0x1A490` before the following calls, while Bombergirl calls it afterward. Those bytes are outside the exports and worker above. Do not treat the DLLs as interchangeable files, but the listed ABI does not require a separate implementation.

AVS ordinals 145, 146, 159, 162, 163, 171, and 298 have the same names and RVAs in both pinned AVS images. Their implementation bytes match because the AVS code sections match. SDVX updater `sub_140017D70` calls them as follows:

| Call | Immediate |
|---|---|
| Ordinal 145, property create | `(23, descriptor, 3352)`; `3352 == 0xD18` |
| Ordinal 163, node create | root `(property, NULL, type, name, value)`; child `(NULL, parent, type, name, value)` |
| Node types | `0x0B` and `0x2E` carry C strings; type `1` is structural; types `5` and `0x34` carry 32-bit immediates |
| Ordinal 171 | called after a non-null string or attribute add |
| Ordinal 159 | property query must return 0 |
| Ordinal 162 | search `(property, NULL, "/config")` |
| Ordinal 298 | `(config, real buffer, 0x08000000, 0, 0, 0)` |
| Allocation | `malloc(0x08000000)`, 128 MiB, before boot; a null buffer is not used |
| Ordinal 146 | destroys the property after boot |

The mount tree in that function matches the Bombergirl updater tree: `/dev/raw`, `/dev/nvram`, SNTP and raw-net off, `0x100` mount/mutex/file counts, logging disabled, then imagefs mounts and `/InstallDir`.

## Mode-2 discovery record

Updater `sub_1400132E0` zeroes `0x98` bytes at record `+0x04`, which covers the last dn byte, then fills:

| Offset | Capacity | Value |
|---|---:|---|
| `+0x00` | u32 | mode **2** |
| `+0x04` | `0x41` | literal `sdvx` |
| `+0x45` | `0x15` | updater's own ProductVersion, `QCV:J:C:A:2022011800` |
| `+0x5A` | `0x41` | token from the command object |
| `+0x9B` | byte | `dn`, from command vtable slot `+0x18` |

The version reader is updater `Version::getProductVersion`: `GetModuleFileNameA(NULL)`, then `\StringFileInfo\041104b0\ProductVersion`, copied with length `0x15`. It does not read `sv6c.exe`.

Launcher `sub_140014D20` uses the same offsets with mode **1** and the launcher profile object. Its own version resource is `QCV:J:A:A:2022011800`. Discovery must use the updater profile, not the launcher profile.

No SDVX mode-2 request was executed. Empty-token mode 2 remains untested. The Bombergirl non-identity placeholder `public-metadata` is the existing tested token convention; it is not a new SDVX observation.

## Discovery URL policy

The bootstrap MSI is pinned to `dks1q2aivwkd6.cloudfront.net`. The resource URL is not pinned: fresh mode-2 discovery returns it. The Bombergirl host `d1q1iflay6y5jj.cloudfront.net` and `/Updater/` prefix are not an SDVX content policy. Do not reuse this observed URL after it changes.

## Index and install layout

Proven from the updater, without an SDVX index body:

- `/InstallDir` is a filesystem mount of the registry `InstallDir` value (`sub_140017D70`).
- The index file is `/InstallDir/ri.bin`, or `/InstallDir/rid.bin` when the command flag selects it (`sub_14001C3F0`, publish site in `sub_14001DCA0`). It is not under `game/`.
- Each manifest child supplies `path` and `tmp`. Install joins `/old/` with `path` only for the backup name, then renames `tmp` to `path` (`sub_14001DCA0`). There is no inserted `game/` destination prefix and no caller-side content transform at that rename.
- Download initialization converts `/TEMP` and `.` for disk checks. It does not rewrite content paths (`sub_14001D130`).
- The updater image contains no `game/`, `modules/`, or `sv6c` path string.

The decoded index confirms that contract. Its paths are already relative to the install root, including the `game/` prefix, so the title uses an empty content root. The local module tail `game\modules\sv6c.exe` is the same entry as the index path `game/modules/sv6c.exe`.

## Launch creation and working directory

Official launcher creation does not open `sv6c.exe` directly.

1. After `OA043711000041` returns 2 and `OA043711000003` is 0, launcher `sub_1400182A0` calls `sub_14001B750` with flag 1.
2. That flag selects `ShellExecuteExA` mask `0x100` (`SEE_MASK_NOASYNC`) in `sub_1400456F0`. The process handle is not retained. Verb is `open`, show is `SW_SHOWNORMAL`.
3. `lpFile` is `InstallDir` concatenated with `launcher\modules\errorreporter.exe` (34 bytes). `lpDirectory` is the raw `InstallDir`. Parameters are the rebuilt command string.

The error reporter then creates the game in `CApplication::MainLoop`:

| `ShellExecuteExA` field | Value |
|---|---|
| `lpFile` | raw `InstallDir` plus `game\modules\sv6c.exe` |
| `lpParameters` | ` -t ` plus the raw `tk` value, then ` --dev` only if `dn` is set and ` --trial` only if `trial` is set |
| `lpDirectory` | the same raw `InstallDir` registry value, not `InstallDir\game` |
| `fMask` | `0x40` (`SEE_MASK_NOCLOSEPROCESS`) |
| `hProcess` | retained |
| Verb / show | `open` / `SW_SHOWNORMAL` |

The reporter waits with `MsgWaitForMultipleObjectsEx` and treats a signaled process as normal completion. No `CreateProcess` import exists in the launcher, updater, or reporter. No token is written to an environment variable at these calls. The official code does not repair a missing trailing separator on `InstallDir`; a root without `\` or `/` produces a bad joined file path while the working directory remains that raw root.

The updater's own handoff is different: it opens `launcher\modules\launcher.exe` with `lpDirectory` equal to `InstallDir` and mask `0x100`. That is not the game-process call.

Token flow is the same shape as Bombergirl: callback query key `tk`, rebuilt as the child parameters ` -t <token>`. Boot stores that token at record `+0x5A` with capacity `0x41`. Game creation uses the rebuilt parameter string, not the callback URI. This session did not execute either call.

## Fresh anonymous discovery, 2026-10-16

The parent ran the admitted production mode-2 profile against the pinned runtime: mode 2, game id `sdvx`, client version `QCV:J:C:A:2022011800`, the existing non-account placeholder, and `dn` 0. The SDK located and returned:

| Field | Value |
|---|---|
| URL | `https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource_info/resource_info_pro_2026070300.xml` |
| SDK SHA-256 | `97697900e0ea87faf6caaf8d55b024f5204b867a613857c55db7d00540beb3b5` |
| HEAD | HTTP 200, `Content-Length` 6,158,980 |
| Stored body | `%TEMP%\konaste-sdvx-index-o0afwnfd\ri.bin` |
| Stored bytes | 6,158,980 |
| Stored SHA-256 | `97697900e0ea87faf6caaf8d55b024f5204b867a613857c55db7d00540beb3b5` |
| First four bytes | `a0 45 00 ff` |

The anonymous identity GET matched the SDK digest exactly. This is one current discovery response, not a URL to hardcode. No game content was downloaded and no installation was written.

The body is not the Bombergirl packed form (`a0 42`). Its uncompressed name section starts `a0 45 00 ff`. The first strict decoder rejected the body on its former size cap. That cap was not bypassed. The production decoder, extended to 8 MiB, then decoded this verified body.

## Decoded index

The production decoder reports:

| Field | Value |
|---|---|
| Entries | 19,311 |
| `ri_ver` | `2026070300` |
| Main path | `game/modules/sv6c.exe` |
| Main stored size | 16,338,056 bytes |
| Main SHA-256 | `20ccfd6c763f67d71d794636de8293ddffdb49dc083643624dc1ac95a6adb11d` |
| Main URL | `https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource/2026070300/game/modules/sv6c.exe` |
| Path roots | `game/` 19,296 entries; `launcher/` 15 entries |
| Content URL shape | all 19,311 URLs use `dks1q2aivwkd6.cloudfront.net` and `/vi/downloads/resource/<revision>/` plus the exact entry path |

Paths are install-root-relative as written. No additional destination prefix is applied. The title mapping is therefore `content_root=""`, index file `ri.bin`, and main path `game/modules/sv6c.exe`. `ri_ver` remains a content revision, not the game PE product code. The main URL is the current entry location, not a hardcoded endpoint.

## Main module, static only

An anonymous identity GET of that one main URL was stored at `%TEMP%\konaste-sdvx-module-rx3t_utb\sv6c.exe`. This session re-read it and did not execute it.

| Field | Value |
|---|---|
| Bytes | 16,338,056, matching the index and the download |
| SHA-256 | `20ccfd6c763f67d71d794636de8293ddffdb49dc083643624dc1ac95a6adb11d` |
| `041104b0` ProductVersion | `QCV:J:B:A:2026070300` |
| FileVersion | `2,1,64,2286` |
| FileDescription / ProductName | `SOUND VOLTEX EXCEED GEAR` |
| OriginalFilename / InternalName | `sv6c.exe` / `sv6c` |

This is the game-module product code. It is not the launcher profile `QCV:J:A:A:2022011800`, the updater profile `QCV:J:C:A:2022011800`, or the numeric `ri_ver` `2026070300`. The date stamp agrees with the index revision, but the full string is a separate PE field.

## Current launcher components, 2023

The fresh index names newer launcher components than the 2022 bootstrap package. These three files, plus the current library, were fetched anonymously into `%TEMP%\konaste-sdvx-current-pe-k4m2\` and hash-checked. None was executed or loaded. The pinned 2022 runtime was not replaced.

| Path | Index revision | Bytes | SHA-256 | ProductVersion |
|---|---|---:|---|---|
| `launcher/modules/launcher.exe` | `2023021500` | 1,918,328 | `4e9f9a2cfd99d9f24f3a9c3c13396a94e5d5be816e4855598990a3521ef64acf` | `QCV:J:A:A:2023021500` |
| `launcher/modules/updater.exe` | `2023021500` | 1,928,056 | `6e553e2f1dfea7fa2f94e085829f0c0bbc4f87f13a45f99f51984370cd8751a6` | `QCV:J:C:A:2023021500` |
| `launcher/modules/errorreporter.exe` | `2023011200` | 1,110,336 | `10dddac4201a449de04589b07c11b32c2c8eaaa8883b88e4511ae5074657caf1` | `QCV:J:D:A:2023011200` |
| `launcher/modules/libeacnet.dll` | `2023011200` | 8,742,720 | `84564b3f804dd66e6d7008895502b7d4caab17bdb19317590c0450cf2447df2c` | not used as the client profile |

All four URLs use `dks1q2aivwkd6.cloudfront.net` and `/vi/downloads/resource/<revision>/`.

The current launcher and updater read their own `041104b0` ProductVersion into the `+0x45` field, with capacity `0x15`. Their normal profiles are therefore the 2023 strings above. A newer file and profile do not by themselves prove that the server rejects the 2022 profile; no rejection was observed.

### Current boot shape

Current `libeacnet.dll` exports 53 names, not the pinned library's 50. `OA043711000000` moved from RVA `0x1A4E0` to `0x1A500`, and its body is not byte-identical. It still validates and copies:

| Offset | Current use |
|---|---|
| `+0x00` | mode; mode 2 may omit client version, every other mode requires it |
| `+0x04` | game id, capacity `0x41`; empty is invalid |
| `+0x45` | client version, capacity `0x15` |
| `+0x5A` | token, capacity `0x41`; empty is invalid |
| `+0x9B` | `dn` byte |
| `+0x9C` | optional services C string |

The services pointer is passed to a selector that returns it when its first byte is nonzero. When it is empty, the selector returns one of the two built-in `GetServices` URLs. Empty services are explicitly accepted there. This is not a required credential and there is no static evidence that an empty value crashes boot. The current launcher and updater both copy it from command-object field `+0x68` with capacity `0x800`.

The launcher parser stores that field from the value following `-s`. It appends ` -s <value>` to the game command only when the field is nonempty, and rebuilds `&services=<value>` in the reverse direction. It also adds a `usta.sdvx://login` mode, selected by `-m usta`, which appends ` -m usta` and sets a mode flag. Normal `konaste.sdvx://login` does not set that flag or require services.

The current launcher still builds mode 1, game id `sdvx`, its own 2023 ProductVersion, the token, `dn`, and services. The current updater does the same with mode 2 and its own 2023 ProductVersion. The current error reporter still opens `InstallDir + game\modules\sv6c.exe`, passes the command string, uses the raw install root as `lpDirectory`, and retains the process with mask `0x40`.

### Current record extent and retained library

Both current callers zero `0x898` bytes starting at record `+0x04`. That is the game id through the last byte of the `0x800` services field at `+0x9C`. The mode dword occupies `+0x00` through `+0x03`, so the full record is **`0x89C` bytes (2,204)**. It is not `0x8A0`. The original 2022 record remains `0xA0` and stops at the `dn` byte. A current caller must allocate and zero at least `0x89C`; zeroing the services field is the normal empty default. No caller supplies a services URL.

The current library is retained only as `%TEMP%\konaste-sdvx-current-pe-k4m2\libeacnet.dll`, filename `libeacnet.dll`, 8,742,720 bytes, SHA-256 `84564b3f804dd66e6d7008895502b7d4caab17bdb19317590c0450cf2447df2c`. `WinVerifyTrust` returned **0** using `WTD_UI_NONE`, `WTD_REVOKE_NONE`, and `WTD_CACHE_ONLY_URL_RETRIEVAL`. It was not loaded. The current index has no launcher `avs2-core.dll`, so the official current pair is this library over the pinned MSI AVS image, SHA-256 `92132343f29b28dec9f9e513c34230435ee14658c988031f42498dcf699d6a85`.

### ABI check before loading

The current library adds exports `OA043711000050`, `000051`, and `000052`. All 13 previously used exports keep the same ordinals. Their RVAs move by `0x30`. Normalized instruction comparison shows relocated calls and globals, not changed return or poll conventions:

- `000001` still returns 2 when its state byte equals 2.
- `000009` still returns the result dword.
- `000003` is still the error getter.
- Resource and start poll bodies keep the same comparisons, completion value 2, error check, and one-time consumption. `000040` keeps its nonzero sent convention.
- `000000` is the one semantic addition: after copying `dn` at `+0x9B`, it passes the services string at `+0x9C` to the empty-default selector.

The current library imports the same 67 AVS ordinals as the pinned library, including 145, 146, 159, 162, 163, and 171. Ordinal 298 is still called by the updater, not imported by `libeacnet`. The current updater text contains both `malloc(0x08000000)` and property-create size `0xD18` (`3352`). The 128 MiB buffer and descriptor size are unchanged.

The 2022 profile therefore does not represent the current normal caller. It lacks the current client version and the services field at `+0x9C`. Whether the current server accepts the old profile was not tested. A current-mode caller must use the 2023 profile and a record that includes `+0x9C`, which may be empty for the normal path. Do not silently substitute the 2022 profile or load the newer DLL in place of the pinned pair without a separate gate.

## Verified content samples

The production updater's bounded sample check passed **16,338,075 stored bytes**. That is a separate result from the static identity downloads in this research.

This research separately fetched and hash-checked `game/modules/sv6c.exe` (16,338,056 bytes) and the current `launcher/modules/launcher.exe` (1,918,328 bytes). Those two static downloads total 18,256,384 bytes. Do not add that total to, or substitute it for, the production sample total. Neither number is a full fresh installation.

## What this admits

The SDVX title mapping is admitted: empty `content_root`, index `ri.bin` at the install root, main path `game/modules/sv6c.exe`, and game working directory equal to the install root. The source and UI may expose that path.

The 2022 launcher and updater profiles are historical package facts, not the current normal client profiles. Current normal mode 1 uses `QCV:J:A:A:2023021500`; current mode 2 uses `QCV:J:C:A:2023021500`. The current boot record adds optional services at `+0x9C`. Keep a caller pinned to the 2022 profile and DLL fail-closed for current Play, or explicitly gate a caller that supplies the 2023 profile, the services field, and the separately pinned current library. Do not infer server rejection from the newer file alone.

That is not a completed SDVX installation or launch. Production updater bounded samples are the next check. A full fresh SDVX installation is not claimed. Authenticated module verification, `checkGameStart`, and actual game creation remain untested. Empty-token mode 2 was not the discovery probe. Do not treat the static module digest or its product code as entitlement.

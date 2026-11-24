# GITADORA and NOSTALGIA current normal launch

Date: **2026-10-16**. Static only. Current modules and both fresh settings DLLs were hash-checked before parsing. Nothing was loaded or executed. Shared IDA was not opened.

`title.rs` still has `launch: None`, legacy layout, and the 2020–2022 versions.

## Current modules

GITADORA base `https://d1omkh45tn6edw.cloudfront.net/data/2026062300/launcher/modules/`. NOSTALGIA base `https://d26kzmeiv4899f.cloudfront.net/data/2023042500/launcher/modules/`. All are PE32+ x64, image base `0x140000000`. ProductVersion below is `\StringFileInfo\041104b0\ProductVersion` except both settings binaries, whose version block is `000004b0`.

| Title | File | Bytes | ProductVersion |
|---|---|---:|---|
| GITADORA | `launcher.exe` | 1,801,320 | `U32:J:C:A:2026062300` |
| GITADORA | `updater.exe` | 1,802,856 | `U32:J:D:A:2026062300` |
| GITADORA | `errorreporter.exe` | 1,132,136 | `U32:J:E:A:2026062300` |
| GITADORA | `settings.exe` | 1,512,552 | `U32:J:F:A:2022022800` |
| NOSTALGIA | `launcher.exe` | 1,789,544 | `UAN:J:A:A:2023042500` |
| NOSTALGIA | `updater.exe` | 1,793,640 | `UAN:J:C:A:2023042500` |
| NOSTALGIA | `errorreporter.exe` | 1,107,048 | `UAN:J:D:A:2032042500` |
| NOSTALGIA | `settings.exe` | 1,499,240 | `UAN:J:K:A:2023042500` |

The reporter string `2032042500` is the literal version resource. It was not corrected. Game ProductVersions remain the already checked `U32:J:A:A:2026062300` and `UAN:J:B:A:2023112800` at `game/modules/*.exe`. Those are not launcher or updater versions.

## Normal boot record

Both callers zero a `0x98`-byte tail after the mode dword, so `OA043711000000` receives `0xA0` bytes. Neither writes a services URL nor imports `OA043711000009`.

| Field | GITADORA updater `0x1400132E0` | GITADORA launcher `0x140015078` | NOSTALGIA launcher `0x140014E8C` |
|---|---|---|---|
| Mode | dword 2 at `[rsp+0x20]` | dword 1 at `[rbp-0x70]` | dword 1 at `[rbp-0x70]` |
| Id, cap `0x41` | `gitadora` | `gitadora` | `nostalgia` |
| Version, cap `0x15` | object `+0xD0` | object `+0xD0` | object `+0xD0` |
| Token, cap `0x41` | command slot `+0x10` | command slot `+0x10` | command slot `+0x10` |
| `dn` | command slot `+0x18`, one byte at `+0x9B` | same | same |

NOSTALGIA updater `0x1400132E0` matches the GITADORA updater instruction sequence, with id `nostalgia` and getter `0x140042490`.

GITADORA updater `0x1400128E7` uses `GetModuleFileNameA(NULL)`; `0x14003FD81` copies `0x15` bytes of its own `041104b0\ProductVersion`. Launcher `0x1400144FA` copies the same self-read to `+0xD0`, and NOSTALGIA launcher does so at `0x14001443A`. Normal mode 1 is `U32:J:C:A:2026062300` and `UAN:J:A:A:2023042500`. Normal mode 2 is `U32:J:D:A:2026062300` and `UAN:J:C:A:2023042500`.

Accepted caller tests are boot 0, worker state 2, error 0, resource send 0, and resource poll 2 followed by error 0. The updater at `0x140013581` accepts error 0; `0x3F8` is an error path. Current launchers additionally call module send/poll/copy and start send/poll. Start poll 2 followed by error 0 reaches creation. `OA043711000040` is tested with `test eax, eax; setne`: zero is failure and any nonzero value continues. That is the opposite of a zero-success send test.

## Current GITADORA SDK

Fresh index rows under `https://d1omkh45tn6edw.cloudfront.net/data/2025010600/launcher/modules/` matched:

| File | Bytes | SHA-256 |
|---|---:|---|
| `libeacnet.dll` | 8,735,744 | `42360c07d6f6a2fc83b3e62e05ffa70ec9e97a4f01977b4896256e3a9e763f7f` |
| `avs2-core.dll` | 1,626,624 | `9ff12a33732a9778da3d1bb374253ccf23cdc5c8a3de51f0b251e1d10f124f75` |

`avs2-core.dll` matches the retained AVS in all seven section hashes and all 392 export RVAs, including ordinals 145, 146, 159, 162, 163, 171, and 298. ProductVersion remains `2.17.3 release (r8311)`. The launcher imports 61 AVS ordinals and no names.

Current `libeacnet.dll` is FileVersion `1.3.3.104` with 50 exports. It adds `000047` through `000049`; no current launcher, updater, or reporter imports them or `000009`.

Current `42360c07…` has an empty security directory and must not be loaded. Signed MSI `f016c1f6…`, FileVersion `1.1.0.84`, has a 7,512-byte security directory and the same 47 names. Boot `0x18001A230` returns 0 after thread creation and 1 after storing terminal 2. State returns 2 or 0, and `000009` returns one dword. Module send returns 0 or 1, copy returns 0 or 1 for null or zero capacity, start send returns 1 on every path, and its poll returns 2 after release or 0 while pending. The caller imports none of `000047` through `000049`.

`0x140016D43` reads `game\modules\gitadora.exe`; `0x1400456D1` copies `0x15` bytes of that ProductVersion to module send. `0x1400179D6` passes command slot `+0x10`, the fresh token, to start send. The signed pair matches this legacy ABI. Mode-1 acceptance remains unproven.

NOSTALGIA has no fresh SDK row. Retained `0ceebfbb…` is `1.0.1.10` and not byte-identical, but its returns match tests `0x140016A9D`, `0x140016EC6`, `0x140016F07`, `0x140017651`, `0x140017A03`, and error 0 at `0x140017A1B`: state 2/0, module 0/1, copy 0 or 1, start 1, and poll 2 or 0. Neither image creates a process, pipe, mapping, or job.

## Reporter create

Both launchers open `HKCU\SOFTWARE\KONAMI\<TITLE>` (`0x80000001`) and then `HKLM` (`0x80000002`), access `0x20019`. GITADORA `0x140018837` copies object `+0x153` to the selector. Callbacks `0x140013150` and `0x140019630` store 1; `0x140013160` and `0x140019650` store 0. No startup write was found, so neither value is a default. `0x14001BF77` selects `-gf` when that byte is zero and `-dm` when nonzero, then formats `%s %s` with command slot `+0x38`. The launcher therefore emits one instrument flag. Settings uses an empty parameter and flag 1.

Launcher wrapper `0x140045E10` uses size `0x70`, verb `open`, show 1, and raw `InstallDir` as its directory. Nonzero flag selects mask `0x100`. NOSTALGIA passes its command directly and has no instrument formatter.

Correction: the reporter does not drop `-gf` or `-dm` on the normal path. `WinMain` `0x140006A4C` calls vtable `+8` with argument 0. That branch of `0x1400088D0` copies the whole raw `lpCmdLine` to object `+0x28`. It recognizes `-t`, `--dev`, `--trial`, and `-c`, but does not rebuild or strip `+0x28`. Argument 1 is the URI branch, whose `0x140008CB0` rebuild is token-only; normal startup does not use it. Getter `+56` at `0x140009190` returns `+0x28`, and `MainLoop` joins it after the settings prefix. The instrument flag passes unchanged. The caller chooses one mode and includes it.

`0x140001F7E` appends `game\` to `InstallDir`; `0x14000D1C0` uses that buffer as `lpDirectory` with mask `0x40`. The file argument appends `modules\gitadora.exe`. The call waits. NOSTALGIA joins the same directory, passes tail `modules\nostalgia.exe` and the raw command, and has no instrument flag or settings prefix.

No separator is inserted at either join. `SetCurrentDirectoryA` is startup code, not `lpDirectory`.

## Settings file

Reporter registry detail independently checked after the original pass: current GITADORA `sub_14000D8D0` first opens `HKEY_CURRENT_USER` and falls back to `HKEY_LOCAL_MACHINE` only if that open fails. A missing or invalid value on an opened user key does not trigger machine-key fallback. Its string query is `REG_SZ`, bounded at 260 ANSI bytes. The hub's wide-character read is bounded at 260 UTF-16 units, and its shared private lookup policy is regression-tested with fake keys. This is different from current DDR `sub_14000D870`, which opens HKLM only. No host user setting was read during this verification.

`GetSettingFile` joins the resource path, `\config`, the RID, and `_cf.xml`. GITADORA exposes display, audio, window, and resolution; NOSTALGIA exposes window only. UI bounds are 0 through 1. Static defaults are 3 for GITADORA display and audio, otherwise 2. Neither executable writes XML.

GITADORA `0x140001400` accepts exactly one `/ResourceDir/config/*_cf.xml`. Otherwise it calls `GetUserRID` and uses `/ResourceDir/config/%s_cf.xml`. Multiple matches are not selected here. Values are parsed base 10 (`r8d = 10`), not base 0. A missing key inserts 0 at `0x140003E6E`. The prefix order at `0x1400014C0` is display, window, resolution, audio. Display is clamped to `GetSystemMetrics(80) - 1`, then raised to 0. Positive display formats ` -display%d`; zero or missing leaves ` -display0`. Zero or missing window leaves ` -window`; a positive value replaces it with ` -fullscreen`. Resolution appends ` -fhd` only when positive. Audio appends ` -exclusive` only when positive. The empty-file prefix is ` -display0 -window`. NOSTALGIA has no reader.

Fresh settings DLLs match: GITADORA 122,368 bytes, `f2ffcd83…`; NOSTALGIA 132,712 bytes, `5936fb53…`. `GetResourcePath` formats `ResourceDir` into 256 bytes. `0x180002C30` reads that `REG_SZ` from HKCU, then HKLM, at most `0x104` bytes. The file is `<ResourceDir>\config\<RID>_cf.xml`, with no inserted separator. NOSTALGIA’s token-only path does not call this DLL.

GITADORA `0x14001CC70` opens `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=gitadora`. NOSTALGIA’s launcher has no `https`, host, or `game_id`. Its scheme is lowercase `konaste.nostalgia://login?tk=`; uppercase `NOSTALGIA` is absent. The catalog id is `nostalgia`. The login URL is not in this image.

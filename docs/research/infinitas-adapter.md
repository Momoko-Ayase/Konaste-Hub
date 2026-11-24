# INFINITAS adapter contract

Research date: **2026-10-16**. Static read of the retained 2022 bootstrap only. No launcher, updater, installer, or game was executed. No private function was loaded. No service, auth, or game endpoint was contacted. IDA was not opened. Capstone 5.0.7 disassembled the extracted updater and launcher.

This is not a `Title`. The package has no `libeacnet.dll`. Do not reuse the SDVX/Bombergirl `0xA0` record, mode 2, or `OA0437110000xx` sequence. Do not treat a metadata read as entitlement, and do not bypass login.

## Package

| Field | Value |
|---|---|
| MSI | `%TEMP%\konaste-remain-vBJRq9\pkg\infinitas.msi` |
| Bytes / SHA-256 | 44,212,224 / `9de019339a24e36ae54283bab6cb5602922a4ceae7d1a29d847c28f01e195dc2` |
| Cabinet | `MSCF` at file offset 20480, 27 members, one folder |
| Extraction | `%TEMP%\konaste-remain-vBJRq9\x\infinitas` |

Cabinet names are flat. The MSI `Directory` and `File` tables were not recovered, so the installer destination layout is not proven from the database. There is no `libeacnet.dll` member.

| Member | Bytes | Role |
|---|---:|---|
| `launcher_modules_bm2dx_updater.exe` | 8,782,728 | Discovery caller. ProductVersion `P2D:J:C:A:2020122300` |
| `launcher_modules_bm2dx_launcher.exe` | 8,910,728 | Launcher. ProductVersion `P2D:J:A:A:2020122300` |
| `launcher_modules_avs2_core.dll` | 1,634,184 | Imported by ordinal. Not the eacnet DLL |

Updater image base `0x140000000`. Code is `.text1` at RVA `0x1000`. Strings are `.sxdata` at RVA `0x139000`.

## What is proven

### Service URL

`0x14004ff70` returns one of two literals. Nonzero `dl` selects the dev host. Zero selects production.

- Dev: `https://p.eagate.dev.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices`
- Production: `https://p.eagate.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices`

Updater boot `0x14004e545` calls it with the byte at `[rbx]`. The normal services call does not pass a URL override. No caller was shown supplying an account token to this selector.

### Two different requests

Both are built by `eacnet_upd_boot` / `eacnet_resource_info_request`, then `EacnetCom::request` at `0x140054290`. A type byte selects the C++ object (`0x140054fe0`).

| Step | Type | Constructor | Object size | `dword [obj+8]` | Setup argument |
|---|---:|---|---:|---:|---|
| Services | 0 | `0x14005f8a0` | `0x48220` | 0 | Address of one zeroed stack byte (`0x14004e96b`) |
| Resource info | 7 | `0x140064070` | `0x260` | 7 | Address of stack byte `1` (`0x14004f1dd`) |

Services setup `0x14005f980` copies `getServices` into `obj+0x110` and stores that one byte at `obj+0x210`. Resource-info setup `0x140064150` does the same with `getResourceInfo`. The byte is not shown to be an HTTP header or a credential. `getResourceInfo` here is an embedded method name, not `libeacnet` export `OA043711000026`.

`EacnetRequestGetSum` is a different setup (`0x1400644e0`). It is not this discovery call.

### Property fields that are written

Both vtables put `0x1400650e0` at slot `+0x60`. That function writes these property names through `0x140067a90`:

| Name | Value source |
|---|---|
| `protocol_version` | Literal `P2D:2015091800` |
| `client_version` | NUL-terminated string at BSS `0x1401aa776` |
| `signature` | String at `obj+0xa8` |
| `retry_count` | Decimal of `dword [obj+0x10]` |
| `request` | String at `obj+0x88` |

`0x140060ce0` is a different three-field writer (`protocol_version`, `client_version`, `request`). It is not the slot used by these two objects.

`client_version` is the updater's own version resource. `0x1400141cf` calls `GetModuleFileNameA` on a null module, then `0x14006ee50` (`Version::getProductVersion`) queries `\StringFileInfo\041104b0\ProductVersion` and copies with `edx = 0x15`. `0x1400142cf` stores that copy at `obj+0xd0`. The updater ProductVersion is `P2D:J:C:A:2020122300`, which is 21 bytes plus NUL, so it fits `0x15`. The launcher profile `P2D:J:A:A:2020122300` is the wrong input. No code copies `P2D:J:C:A:2020122300` as an immediate; the version resource is the source.

This is not SDVX mode 1 or mode 2. No mode dword, game id, or `dn` byte is written by this path. `bm2dxinf://login?tk=` and `--trial` exist as launcher/updater strings. They were not found on this discovery write.

### Local index URL and hash

After a resource-info response, `0x140064260` reads `p2d/error_code` and continues only when that value and `[obj+0x108]` are both zero. It then reads node `p2d/result`. `0x140064314` opens a property named `resource_info`.

The index walk is separate: `0x14001f6c0` formats `resource_info/file#%d` and reads:

| Node | Type immediate | Capacity |
|---|---:|---:|
| `path` | `0x0B` | `0x104` |
| `version` | `6` | `4` |
| `size` | `6` | `4` |
| `sum` | `0x0B` | `0x41` |
| `url` | `0x0B` | `0x800` |

`sum` is the per-file hash field. Width `0x41` can hold 64 hex characters plus NUL. The hash algorithm for that field is not named at this read. `eacnet_util_sha256sum_calc` exists elsewhere and was not tied to this node. `url` is a string of at most `0x7FF` bytes. These are index-entry fields, not the discovery HTTP body.

Updater strings select `/InstallDir/ri.bin`, or `/InstallDir/rid.bin` when a flag is set (`0x14001f494`). The launcher has `ri.bin`, `rid.bin`, and `resource_info/ri_ver`, without the `/InstallDir/` prefix. No CloudFront host or `ResourceInfo/` URL is a literal in this updater.

### Registry and module strings

`0x14007a6f0` returns `SOFTWARE\KONAMI\beatmania IIDX INFINITAS`. `0x14007a700` opens that key with access `0x20019`: hive `0xFFFFFFFF80000001` first, then `0xFFFFFFFF80000002` if the first open fails. Callers pass value name `InstallDir` (`0x14006edf0`) or `ResourceDir` (`0x14006ee11`). The queried size is rejected unless it is `<= 0x104`. The value type must be `1`. The hive constants match `HKCU` then `HKLM`, but the import slot itself was not decoded to an API name.

Launcher `0x14002ce72` appends `game\app\bm2dx.exe` with length `0x12`. The launcher also contains `launcher\modules\bm2dx_updater.exe`. Those are path strings, not an installed tree.

### Shared HTTP behavior, not yet tied to these two calls

`EacnetWinHttpRequest::exec` at `0x140067320` builds `name=value` pairs separated by `&` and `=`. Object byte `+0x40` selects wide `POST` plus header `Content-Type: application/x-www-form-urlencoded`; otherwise it selects `GET` and may append `?`. It refuses a non-SSL URL. `EacnetRequestPost` logs say its body path calls `property_mem_write`, `EacnetLZ77::encode`, `eacnet_util_calc_signature`, and `utils_base64_encode`, then checks `protocol_version` and `signature` on the response. The response body log names an LZ77 decode and `property_mem_read`.

## Blockers

These are unknown. They are not safe to guess.

1. The wire verb, header list, and body encoding for `getServices` and `getResourceInfo`. The POST/GET flag and the LZ77/base64 path were not traced from these two objects to `WinHttpSendRequest`. No field order, charset, or signature input is proven.
2. Whether `client_version` at `0x1401aa776` is the same string stored at `obj+0xd0`. Both exist. The store that joins them was not found.
3. What the services response must contain before type 7 runs, and which URL type 7 uses. The constructor stores two BSS pointers (`0x1400640c0`). Their writers were not found. The production `GetServices` literal is only the fallback selector.
4. The discovery response shape that yields the index URL and its SHA. The `url` / `sum` table above is the local `ri.bin` reader.
5. Whether a normal anonymous call is accepted. The zero argument byte is not a tested login. The earlier anonymous GET of `https://p.eagate.573.jp/game/infinitas/2/api/login/login.html` was a 302 to e-amusement login and was not followed. No token was read from this discovery path.
6. The current installer. This MSI is the 2022-06-08 bootstrap. It does not prove the current public build, and its `Directory` table was not decoded.

No Rust client, request builder, or installer should be written until items 1 and 3 are closed from this updater, not from another title's `libeacnet.dll`.

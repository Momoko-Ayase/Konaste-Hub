# Current pop'n music Lively launch contract

Research date: **2026-10-16**. Static only. The six current helper binaries were downloaded anonymously, checked, and parsed with pefile, dnfile, and local Capstone 5.0.7. None was executed. No account, token, browser credential, existing installation, or TLS setting was used. Shared IDA session `a36e739ffe1be5d00` was not opened. The main game PE was not downloaded again.

`crates/konaste-runtime/src/title.rs` still describes the 2020–2022 caller: `BootLayout::Legacy`, updater `U39:J:A:A:2020110400`, launcher `U39:J:B:A:2021020900`, and `launch: None`. That historical caller is [older title contracts](older-title-contracts.md). It is not this SDK. The fresh index and main module in [multi-title integration](multi-title-integration.md) remain the content identity.

## 1. Verified helper set

Base: `https://d1twxdn5j8sbm3.cloudfront.net/Updater/2026080500/launcher/modules/`

Each GET returned HTTP 200 and `Content-Length` equal to the pinned size. The body was hashed before any parse and kept at `%TEMP%\konaste-popn-current-mgpetm9z`. All six matched. Cache-only `WinVerifyTrust` (`WTD_CACHE_ONLY_URL_RETRIEVAL`) returned 0 for the retained `libeacnet.dll` and `avs2-core.dll`. No revocation network request was made. The files were not executed.

| File | Bytes | SHA-256 | ProductVersion |
|---|---:|---|---|
| `launcher.exe` | 1,880,704 | `be64995cdc6f12f46e616eca6dde35ba19aef1a47286b31552a7181711896ad1` | `U39:J:B:A:2026031100` |
| `updater.exe` | 1,882,752 | `e3aa54fee653e0e1e93607129d8d3535b7f52077e4f24d8ac718ce2b4e9df9e0` | `U39:J:D:A:2026031100` |
| `errorreporter.exe` | 1,231,488 | `aae6a5948a0cc47fd9ac781c31282e13f91d2edfb1798b770749f055076c75ed` | `U39:J:E:A:2026031100` |
| `settings.exe` | 1,572,480 | `ebb7dd69d7db4b3811adc11660410eb43dec1e37805f1004a3de07dc45fe1f42` | `U39:J:C:A:2026031100` |
| `libeacnet.dll` | 8,753,008 | `119bfb79688e8ea323c183c319e0ec665fc0c27cc3587d5edc32d4ce0b6b60ae` | `1.4.0.110` |
| `avs2-core.dll` | 1,634,224 | `01f18b206d7cc1b7af40187daf5ea766ceeed026adfafe22431ce3912e9f803a` | `2.17.3 release (r8311)` |

All four executables and both DLLs are PE32+ x64. Image base is `0x140000000` for executables and `0x180000000` for DLLs. Version strings are `StringFileInfo\041104b0\ProductVersion`, except `settings.exe`, whose block is `000004b0`.

These three profiles are distinct and none is the game:

| Role | ProductVersion | Use |
|---|---|---|
| Mode 1 | `U39:J:B:A:2026031100` | launcher client version |
| Mode 2 | `U39:J:D:A:2026031100` | updater client version |
| Game | `U39:J:A:A:2026080500` | checked `game/module/popnLively.exe` only |

`settings.exe` is `U39:J:C:A:2026031100` and `errorreporter.exe` is `U39:J:E:A:2026031100`. Neither is a boot client version.

The current `avs2-core.dll` is byte-identical to the historical pop'n MSI member, including all seven sections. Ordinals 145, 146, 159, 162, 163, 171, and 298 remain at RVAs `0x1D180`, `0x1D480`, `0x193C0`, `0x1B600`, `0x19C00`, `0x1C930`, and `0x2F300`. The shared descriptor `0xD18` and 128 MiB boot buffer in `official.rs` still match that AVS. The current eacnet does not.

## 2. Boot shape

Current boot is the services layout, not legacy `0xA0`.

`OA043711000000` at RVA `0x1A4E0` reads mode at `[rcx]`, the game id at `+0x04`, the token at `+0x5A`, and the client version at `+0x45`. After copying those fields it loads `dl` from `[rcx+0x9B]` and `r8` from `[rcx+0x9C]`, then calls `0x180011EC0`.

That helper returns the pointer at `+0x9C` when its first byte is nonzero. When that byte is zero, `dn == 0` selects `https://p.eagate.573.jp/game/konasteapp/APIEX/servletex/pre_process/GetServices` at VA `0x1800880A0`. Nonzero `dn` selects `https://p.eagate.dev.573.jp/game/konasteapp/APIEX/servletex/pre_process/GetServices` at `0x180088040`. The selected pointer is passed to `0x180011CC0`. Empty therefore means the built-in normal URL. This function writes no caller URL.

Both current callers build that slot:

| Caller | Zero | Mode | Record |
|---|---|---|---|
| Launcher `0x140014D60` | `0x140014EB2`, `r8d=0x898` at `[rbp-0x6C]` | dword 1 at `0x140014EC3` | pointer `[rbp-0x70]` |
| Updater `0x1400132E0` | `0x140013310`, `r8d=0x898` at `[rsp+0x24]` | dword 2 at `0x140013322` | pointer `[rsp+0x20]` |

`0x898 + 4 = 0x89C`. The fields are:

| Offset | Capacity | Value |
|---|---:|---|
| `+0x00` | u32 | mode 1 or 2 |
| `+0x04` | `0x41` | literal `popn-music` |
| `+0x45` | `0x15` | that executable's own ProductVersion |
| `+0x5A` | `0x41` | command object slot `+0x10` |
| `+0x9B` | byte | command object slot `+0x18` (`dn`) |
| `+0x9C` | `0x800` | command object slot `+0x30` |

The version source is `[record_object+0xD0]`, filled from `GetModuleFileNameA(NULL)` and `VerQueryValueA` of `\StringFileInfo\041104b0\ProductVersion`. It is not read from `popnLively.exe`.

Launcher copies are `0x140014EE7` for the version, `0x140014F17` for the token, `0x140014F89` for `dn`, and `0x140014F9C` for services. Updater copies are `0x140013341`, `0x140013374`, `0x1400133E8`, and `0x140013400`. Both pass the services string through `%s` with capacity `0x800`. There is no separate padding between `+0x9B` and `+0x9C`.

The command parser rebuilds `konaste.popn-music://login?tk=`, `&dn=`, `&trial=`, and `&services=`. Those literals are launcher `0x14012ED30` and updater `0x140131038`. The exact vtable field numbers were not recovered. Empty-token mode 2 was not executed.

This is the same record size as current SDVX, but not the same eacnet image. Section and export bytes differ from the current SDVX DLL, the Bombergirl DLL, and the 2020 pop'n DLL. The return behavior below was derived from this image.

## 3. Thirteen exports

`libeacnet.dll` has 50 named exports. The thirteen used by the shared worker:

| Export | RVA | Body | Return / poll |
|---|---:|---:|---|
| `OA043711000000` | `0x1A4E0` | 803 | `0` after the worker thread is created (`0x18001A7EC`); `1` on every checked failure (`0x18001A7E5`) |
| `OA043711000001` | `0x1A810` | 17 | `2` when the finished flag is nonzero, otherwise `0` |
| `OA043711000002` | `0x1A8B0` | 117 | `0` after shutdown |
| `OA043711000003` | `0x1DCB0` | 5 | tail-calls `0x180008990`; the stored code was not re-derived |
| `OA043711000009` | `0x1DCD0` | 7 | the dword at RVA `0x9F870`; boot stores `2` there on failure (`0x18001A7D6`) |
| `OA043711000026` | `0x1CA60` | 201 | enqueues type `4`; `0` when enqueue succeeds, `1` when it fails |
| `OA043711000027` | `0x1CB30` | 115 | `2` after the response is consumed, `0` while pending; does not return the error |
| `OA043711000028` | `0x1CD80` | 140 | `(url, urlCap, hash, hashCap)`; `0` when both pointers are non-null and both caps are `> 0`, otherwise `1` |
| `OA043711000030` | `0x1BF30` | 235 | enqueues type `5`; `0` on success, `1` on failure |
| `OA043711000031` | `0x1C020` | 435 | `2` after release, `0` while pending |
| `OA043711000032` | `0x1C1E0` | 66 | `(out, cap)`; `0` when the pointer is non-null and cap is `> 0`, otherwise `1` |
| `OA043711000040` | `0x1C440` | 289 | enqueues type `7`; returns `1` on the null, enqueue-success, and enqueue-failure paths |
| `OA043711000041` | `0x1C570` | 306 | `2` after release, `0` while pending |

`000001 == 2` means the worker finished. It does not by itself mean success. `000009` reads the dword at `0x18009F870`.

The boot entry creates one thread at `0x18001A930` (`0x18001A778`). That worker is shared by mode 1 and mode 2. It reads the mode dword copied to `0x18009F7D4` and compares it at three sites. None of those compares selects a different terminal:

| Site | Compare | Effect |
|---|---|---|
| `0x18001AB2E` | mode `== 1` | log code 2 instead of 4 |
| `0x18001B1F9` | mode `== 2` | one log code becomes 6 instead of 8 |
| `0x18001B501` | mode `== 0` | log code 11 instead of 20 |

All three continue into the same request loop. The loop tail is `0x18001A9B0`. It jumps to `0x18001BDA1` when the finished flag is set. `0x18001BDA6` then stores `r13d` to `0x18009F870` and `0x18001BDCC` sets the finished flag. The failed boot entry stores 2 at `0x18001A7D6` before returning 1. Initialization clears the same dword at `0x18001A2EB`.

`r13d` is set to 1 at `0x18001BCA5`, which then jumps to the common pre-store block `0x18001BD8B`. The bytes are a real instruction on an instruction boundary inside the worker, but no decoded branch or immediate in this image addresses `0x18001BCA5`. The store of a successful 1 is therefore the register value that reaches `0x18001BDA6`, not a second terminal. Which response path assigns that register on a live mode-2 boot is what the existing production wait observes. It is not a second ABI and it is not an authentication proof. A mode-2 discovery that returns boot 0, state 2, error 0, and `000009 == 1` is the normal terminal for this worker.

Poll value 2 is terminal for resource, module, and start. Each poll function returns 2 after it has released the request, then the caller must read `000003`. The copy exports do not enqueue another request. `000028` copies URL first and hash second. The launcher passes caps `0x800` and `0x41` at `0x140015FBB`; the export itself only tests that the caps are positive.

`000040` is not a zero-success export. Its three paths all return 1. The launcher at `0x140017701` treats any nonzero return as sent (`setne`) and continues only when `000003` is later 0.

`000041` maps a nonzero response field to error `0x7D2` through `0x7D5` for response values 1 through 4, but it still returns 2. The discrimination is `000003`, not the poll value. The launcher accepts only poll 2 followed by error 0 (`0x140017AB9`, `0x140017ACB`).

Launcher compares, all on this current image:

| Call | Address | Accepted |
|---|---|---|
| `000000` | `0x140015004` | `eax == 0` |
| `000001` | `0x140015393` | `eax == 2`, then `000003 == 0` and `000009` is read |
| `000026` | `0x140015BCF` | `eax == 0` |
| `000027` | `0x140015F36` | `eax == 2`, then `000003 == 0` |
| `000030` | `0x140016B4D` | `eax == 0` |
| `000031` | `0x140016F76` | `eax == 2`, then `000003 == 0` |
| `000040` | `0x140017701` | `eax != 0`, then `000003 == 0` |
| `000041` | `0x140017AB3` | `eax == 2`, then `000003 == 0` |

The updater uses the same boot, state, error, resource-send, resource-poll, and resource-copy exports. It does not import `000009`, `000030`, `000040`, or `000041`. Metadata discovery therefore has no `000009 == 1` test in this updater. The accepted discovery chain is boot 0, state 2, error 0, send 0, poll 2, error 0, then one copy.

AVS compatibility is the file identity above plus the existing call shape: ordinal 145 `(0x17, descriptor, 0xD18)`, ordinal 298 `(config, buffer, 0x08000000, 0, 0, 0)`. This session did not re-disassemble the current updater's mount tree.

## 4. Login and callback

The current launcher contains both literals as C strings:

| Use | VA | Exact bytes |
|---|---|---|
| Login page | `0x14012BC50` | `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=popn-music` |
| Callback prefix | `0x14012ED28` | `konaste.popn-music://login?tk=` |

`0x14001C54E` passes the login URL to `ShellExecuteA` with show 1. The callback builder at `0x140042026` copies `0x1E` bytes from `0x14012ED28` and appends the raw token. `0x1E` is the full 30-byte prefix, including `tk=`. The same image has the separate suffixes `&dn=`, `&trial=`, and `&services=` at `0x14012ED48`, `0x14012ED50`, and `0x14012ED58`. There is no second game id in the login URL. The reporter and updater carry the same callback prefix; they do not contain the HTTPS login URL.

## 5. Game creation

Normal creation is indirect.

1. Launcher `0x1400184BD` writes byte 1 and calls `0x14001B750` with the handle slot at scene `+0x158`.
2. `0x14001B750` appends `launcher\modules\errorreporter.exe`, 34 bytes (`0x22`) at `0x14001B82A`, to the raw `InstallDir`.
3. `0x140046080` calls `ShellExecuteExA`. The flag byte is the fifth stack argument. Zero selects mask `0x40`. Nonzero selects mask `0x100`. This caller passes 1, so the mask is `0x100` (`SEE_MASK_NOASYNC`), `hProcess` is not retained, verb is `open`, and show is 1. `lpDirectory` is the raw `InstallDir` (`0x14001B8F8`). `lpParameters` is command slot `+0x40`.

The reporter then creates the game from `0x140001690`, called at `0x1400097E0`. Its only `ShellExecuteExA` is `0x14000D1D0`:

| Field | Value | Site |
|---|---|---|
| `cbSize` | `0x70` | `0x14000D226` |
| `fMask` | `0x40` | `0x14000D22E` |
| `lpVerb` | `open` | `0x14000D23A` |
| `lpFile` | separate file buffer | `rdx` at `0x14000D249` |
| `lpParameters` | rebuilt command | `r8` at `0x14000D245` |
| `lpDirectory` | mutable directory buffer | `r9` at `0x14000D24D` |
| `nShow` | 1 | `0x14000D251` |
| `hProcess` | stored | `0x14000D309` |

The call is `0x1400025D0`. `rdx` is `[rbp+0x1B8]`, `r8` is `[rbp+0xC8]`, and `r9` is `[rbp+0x198]`.

`lpDirectory` starts as `InstallDir` (`0x14000D160` argument 0). At `0x140001CDB` the function appends four bytes, `game`, from `0x1400B1FF4`. It does not append `game\`. The joined directory is therefore the raw registry bytes plus `game`.

`lpFile` is a different buffer. It starts as `InstallDir`, then `0x140001779` appends `0x1B` bytes from `0x1400B1FA8`. That literal is `game\module\popnLively.exe"` and is 27 bytes including the final quote. The quote is the byte at the end of the C string and the length passed to the copy. It is not a disassembler artifact. The launcher's own module string, `game\module\popnLively.exe` at `0x140127C08`, has no quote; the reporter's create tail does. The virtual string `/id/game/module/popnLively.exe` is not the ShellExecute path.

Arguments are rebuilt at `0x140009D80` into command field `+0x28`:

| Condition | Bytes | Literal |
|---|---:|---|
| always | 4 | ` -t ` plus the raw token |
| byte `+0x68` nonzero | 6 | ` --dev` |
| byte `+0x69` nonzero | 8 | ` --trial` |
| services length at `+0x80` nonzero | 11 | ` -services ` plus the raw services string |

There is no quoting or escaping. The reporter imports `ShellExecuteExA` and not `CreateProcess`.

Registry reads are `0x14000D8E0`. `0x14000D933` opens `HKCU` (`0x80000001`) with `KEY_READ` (`0x20019`). Only a failed open continues to `HKLM` (`0x80000002`) at `0x14000D95A`. The value must be `REG_SZ` (type 1). `0x14000DA93` rejects a size greater than `0x104`. The copy adds no separator. `InstallDir` and `ResourceDir` are read by `0x14000D100`; argument 0 of `0x14000D160` returns `InstallDir`. The key is `SOFTWARE\KONAMI\pop'n music Lively` at `0x1400B2DA0`.

`SetCurrentDirectoryA` is a separate startup import. It is not `lpDirectory`.

Nothing between the reporter's path construction and `ShellExecuteExA` is another entitlement or protection exchange. The reporter imports no WinHTTP, Winsock, crypto, pipe, file-mapping, or device-control API. Its eacnet imports are boot, state, shutdown, error, and exports `000036` through `000039`; the game-create function does not call them. `CreateMutexA` and `CreateEventW` are present, but they are not arguments of `0x14000D1D0` and are not invoked on this path. Registry reads happen before the join, only to obtain `InstallDir`. With `dn`, trial, and services empty, `0x140009D80` appends only ` -t ` plus the token: the `+0x68`, `+0x69`, and `+0x80` tests fail and their suffixes are not copied. The ShellExecute parameters are that command string. There is no inherited-handle list, environment block, or second process.

## 6. Settings file

`settings.exe` is a .NET Framework 4.0.30319 WPF executable. It references `libsettings.dll`. The current index row is allowed and was fetched separately:

| Field | Value |
|---|---|
| URL | `https://d1twxdn5j8sbm3.cloudfront.net/Updater/2026080500/launcher/modules/libsettings.dll` |
| Bytes | 129,968 |
| SHA-256 | `c9b2b321cfc4e8787d4ae3afc6ed97bb2c1f1071970ec4a614174d7f50659ead` |
| Retained at | `%TEMP%\konaste-popn-libsettings-cfzjavs_\libsettings.dll` |

It was not executed. Its exports are display and key helpers plus `GetResourcePath`, `InitSettingLib`, `FinishSettingLib`, and `GetUserRID`. `GetResourcePath` at `0x180001730` formats `%s` into a caller buffer. The registry strings in this DLL are `InstallDir`, `ResourceDir`, and `SOFTWARE\KONAMI\pop'n music Lively`. It contains no `setting.bin` string. The DLL does not create or parse the settings file. The old MSI copy was not used.

The path is a user string, `game\\dev\\setting.bin`, token `0x700001D3`. The static constructor at RVA `0x2965` executes `ldc.i4.0`, `stsfld isThisReady`, `ldstr` that path, `stsfld path`, `ret`. The path is relative. No drive, profile, or registry value is added in this constructor.

`popnSave` at `0x2378`:

1. `Path.GetDirectoryName(path)`.
2. `Directory.CreateDirectory` when that directory does not exist.
3. `new FileStream(path, FileMode.Create)` (`Create` is 2).
4. Writes the static `saveData` byte array with `BinaryWriter.Write(byte[])`.

`popnLoad` at `0x2480` returns immediately unless `File.Exists(path)`. It opens `FileMode.Open` (`Open` is 3), constructs a `BinaryReader`, and calls `Read(byte[], int, int)` into `saveData` at offset 0 for 4 bytes.

`saveData` is field 19, signature `06 1D 05`: a byte array. `popnLoad` allocates it with `newarr uint8` of 8 at IL `+0x28` (`1A` is `ldc.i4.4`; the array length operand is 8). The load reads only the first 4 bytes. The save writes the whole array. Bytes 4 through 7 are not assigned in `popnSave`.

The four bytes are UI state, not a text schema:

| Index | Write | Load effect |
|---|---|---|
| 0 | `1` when `RadioButton_window` is checked; otherwise `0` when `RadioButton_Monitor0` is checked | `1` calls `SetWindow`; any other loaded value calls `SetMonitor0` |
| 1 | `1` when `ChatteringFilterChekBox` is checked, otherwise `0` | a nonzero byte checks that box; zero clears it |
| 2 | not written by this method | not read |
| 3 | not written by this method | not read |

`SetWindow` checks the window button and clears monitor 0. `SetMonitor0` does the opposite. `SetMonitor1` and `SetMonitor2` clear both when their display entry is enabled; otherwise they call `SetMonitor0`. `SetMultiMon` only enables `RadioButton_Monitor0`. No numeric default for the file bytes is stored. A missing file leaves the window button unchecked and selects monitor 0.

The displayed options are window, monitor, and the chattering-filter checkbox. `DisplaySettingsData`, `AudioPlaySettingsData`, and `FPSSettingsData` are empty types in this assembly. They are not fields of `setting.bin`.

The reporter also copies `game\dev\setting.bin`, 21 bytes including NUL, from `0x1400B1FC8` into a stack constant at `0x140001795`. That constant is a candidate path, not a second parser. The launcher and updater contain no `setting.bin` string. The game PE was already identified and was not parsed again. Its private interpretation of this file is not required for the caller. Normal creation passes the token on the reporter command line and does not read or write `setting.bin`. The hub must not open that file merely to launch. A later game-side settings test is separate from this caller contract.

## 7. Admission boundary

A current mode-1 call can use the services record: mode 1, game id `popn-music`, launcher version `U39:J:B:A:2026031100`, `dn` 0, and an empty `0x800` services field. A current mode-2 call uses updater version `U39:J:D:A:2026031100` in the same record. The pinned AVS ordinals and buffer sizes remain compatible because the AVS file is unchanged.

That does not make the current pair interchangeable with the historical pop'n eacnet or with SDVX. `title.rs` still has the legacy layout and the 2020–2022 versions, and `POPN.launch` is `None`. No live mode-1 or mode-2 call was made in this session. The retained pair is the one a later public mode-2 run can load: current profile `2026031100`, eacnet SHA-256 `119bfb79688e8ea323c183c319e0ec665fc0c27cc3587d5edc32d4ce0b6b60ae`, MSI-identical AVS SHA-256 `01f18b206d7cc1b7af40187daf5ea766ceeed026adfafe22431ce3912e9f803a`, and a zeroed services slot. `000009` stays enabled. A public mode-2 result of boot 0, state 2, error 0, and `000009 == 1` is the normal terminal already required by `official.rs`; it does not require an account.

The reporter path remains blocked by its own literal. Copying 27 bytes from `game\module\popnLively.exe"` puts a quote in `lpFile`. The directory join is `InstallDir` bytes plus `game`, while the file join uses the same unnormalized `InstallDir`. Neither join repairs a missing separator. The command can also carry ` -services `, which the 2020 reporter did not add. No implementation change was made from this evidence.

## 8. Game command line (2026-11-21 correction)

The section 5 note that the reporter "appends only ` -t ` plus the token" is wrong for the current reporter (`errorreporter.exe` U39:J:E:A:2026031100). Its `sub_140001690` builds the whole game parameter string:

`dev/LocalProp/bootstrap.local.xml debug_localstrap "<InstallDir>x" "<ResourceDir>work" [-full ][-cf] -t <token>`

- Both registry values come from `SOFTWARE\KONAMI\pop'n music Lively` (HKCU, else HKLM), with every `\` replaced by `/` (`sub_1400044D0` with the `/` constant at `0x1400B1FE8`) and joined without a separator. The stored trailing backslash supplies it.
- `game\dev\setting.bin`, read relative to the install root: four bytes. Byte 0 nonzero is window mode and omits ` -full `. Byte 1 nonzero appends `-cf` (`0x1400B1FE0`). A missing file or a failed 4-byte read keeps window mode and no filter.
- `popnLively.exe` (32-bit) `sub_5A5400` returns 515900000, shown as `ERROR CODE : 5-1590-0000` "popnLively.exe からは起動出来ません", when `__argc < 5`. A token-only command line therefore always fails. `sub_5A4F40` takes `argv[1]` as the bootstrap path and `argv[2]` as a mode name.

Konaste Hub's `ArgumentProfile::PopnBootstrap` (`crates/konaste-runtime/src/arguments.rs`) builds this prefix. The other four titles' reporters contain no such prefix.

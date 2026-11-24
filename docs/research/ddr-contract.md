# DDR contract evidence

Date: **2026-10-16**. Static only. No official launcher, updater, installer, or game was executed. IDA was not used. Capstone decoded the extracted PE images. INFINITAS stays unsupported: its package has no `libeacnet.dll` and its service URL is a different embedded client. Do not map it onto this DLL worker.

Reporter creation and the updater's local index mapping are pinned below. The launcher boot record and the metadata ABI are unchanged. DDR `libeacnet` poll/return constants were not re-derived.

## Package pin

Anonymous historical MSI, not linked by the current login-gated download page. HTTP 200, no redirect. `WinVerifyTrust` 0, cache-only, no revocation. Signer: Konami Amusement Co., Ltd.

| Field | Value |
|---|---|
| URL | `https://d2el0dli9l0x2p.cloudfront.net/installer/ddr_installer_2022012601.msi` |
| Bytes | 32,956,416 |
| SHA-256 | `c938081a9eab7d0a35b14a36d392a8a56a10f379dd3f8f62d2b4a906deef729e` |
| Last-Modified | Mon, 31 Jan 2022 01:38:14 GMT |
| ProductVersion | `1.0.0` |
| ProductCode | `{41796B7B-A1DF-47D8-8839-021EC45959EE}` |
| UpgradeCode | `{6AD2A3EB-6204-49A6-A5EC-F2998545816F}` |
| Package code | `{42B292B0-53D4-421F-8FE2-F29B304D7506}` |
| Cabinet | `#Sample.cab`, disk 1, last sequence 30 |
| Retained at | `%TEMP%\konaste-remain-vBJRq9\pkg\ddr.msi` |

Not copied into the repo. A 2022 bootstrap is not the current game build.

## Extraction schema

`File` long names and sequences. Target directory is `Games\DanceDanceRevolution\launcher\modules` for the runtime files.

| Sequence | File key | Long name | Bytes |
|---:|---|---|---:|
| 23 | `launcher_modules_avs2_core.dll` | `avs2-core.dll` | 1,633,304 |
| 25 | `launcher_modules_launcher.exe` | `launcher.exe` | 1,796,096 |
| 26 | `launcher_modules_libeacnet.dll` | `libeacnet.dll` | 8,742,400 |
| 29 | `launcher_modules_updater.exe` | `updater.exe` | 1,800,704 |
| 24 | `launcher_modules_errorreporter.exe` | `errorreporter.exe` | 1,128,960 |

Registry root 2 (`HKLM`): `Software\KONAMI\DanceDanceRevolution`, values `InstallDir` = `[INSTALLDIR]`, `ResourceDir` = `[RESOURCEDIR]`. Root 0 (`HKCR`): `konaste.ddr` command `"[INSTALLDIR]launcher\modules\launcher.exe" "%1"`.

Extracted pair, both PE32+ x64, `WinVerifyTrust` 0:

| File | Bytes | SHA-256 | Version |
|---|---:|---|---|
| `launcher.exe` | 1,796,096 | `55e05f132d552f92da2c1629b5fb34191d0c202a23b8c152cec87ca8c53ad721` | Product `VGP:J:B:A:2022012600`, File `1.0.2.0` |
| `updater.exe` | 1,800,704 | `6d6b9c568ff6a249b660d0eb65c27c256afc8dc6691b631d3a377e08f48fc0a6` | Product `VGP:J:C:A:2022012600`, File `1.0.2.0` |
| `libeacnet.dll` | 8,742,400 | `aa91b712f2c2ffc63eda73efc53544199299670640ca6b49463c693790f494e0` | `1.3.3.104`, 50 exports `OA043711000000`–`000049` |
| `avs2-core.dll` | 1,633,304 | `8a1473ae12064edf92228ed9c306346c1dc1e02031581e099a4a46d955b496c0` | `2.17.3 release (r8311)` |

`.text` of this AVS DLL matches the other four packages' AVS `.text` (`5206ffda34c8a860e526152cd7031fb77fd120efeaa2395a83f476742eb81192`, 709,120 bytes). Whole-file hashes differ.

## Launcher boot record

`launcher.exe` `0x140014D40`. At `0x140014E8E` it clears `0x98` bytes at `rbp-0x6C`. At `0x140014E9D` it stores dword **1** at `rbp-0x70`. The boot pointer passed to `OA043711000000` at `0x140014F6A` is `rbp-0x70`.

| Offset from that pointer | Instruction | Value |
|---|---|---|
| `+0x00` | `mov dword ptr [rbp-0x70], 1` | mode **1** |
| `+0x04` | copy capacity `0x41` from literal `ddr` | game id |
| `+0x45` | copy capacity `0x15` from object `rdi+0xD0` | client version slot |
| `+0x5A` | copy capacity `0x41` | token slot |
| `+0x9B` | `mov byte ptr [rbp+0x2B], al` | `dn` byte |

`0x140044F1C` reads `GetFileVersionInfoA` and queries `\StringFileInfo\041104b0\ProductVersion` into a `0x15`-byte buffer. The launcher PE ProductVersion is `VGP:J:B:A:2022012600`. The instruction does not itself show that this function's return is the `rdi+0xD0` value, so that link is not proven.

## AVS setup

`launcher.exe` `0x14001A160`:

- `ecx = 0x08000000` at `0x14001A1A5`, then a 128 MiB allocation.
- descriptor clear and ordinal **145** both use `r8d = 0xD18`.
- ordinal **298** is called with `r8d = 0x08000000`.
- ordinals used: 145, 163, 171, 159, 162, 298, 146.
- mounted strings are `work\launcher\dev\raw`, `work\launcher\dev\nvram`, `/dev/raw`, `/dev/nvram`, plus the same config names as the Bombergirl tree (`posix=0`, `vf=0,posix=0`, `sz_block=512,nr_block=32,nr_mirror=4`, `ea_on`, `enable_raw`, `nr_mountpoint`, `nr_mutex`, `nr_filedesc`, `DISABLE`). Numeric node values were not read from the registers.

## Caller sequence

All names are launcher imports. Return constants inside DDR `libeacnet` were **not** disassembled.

| Export | Call | Observed arguments |
|---|---|---|
| `OA043711000000` | `0x140014F6A` | pointer to the mode-1 record |
| `OA043711000003` | `0x140014F82` | no argument in the instruction |
| `OA043711000026` | `0x140015B1F` | no argument |
| `OA043711000027` | `0x140015E86` | no argument |
| `OA043711000028` | `0x140015F0B` | after an `0x800`-byte clear |
| `OA043711000030` | `0x140016A9D` | after loading `game\modules\ddr-konaste.exe` |
| `OA043711000031` | `0x140016EC6` | no argument |
| `OA043711000032` | `0x140016F01` | follows 000031 |
| `OA043711000040` | `0x140017651` | `rcx` = token string pointer |
| `OA043711000041` | `0x140017A03` | next function, no argument |

Index strings in `0x140015E40`: `ri.bin`, `rid.bin`, `%s%s`, `resource_info/ri_ver`, `%010d`. Module hash path: `game\modules\ddr-konaste.exe`. Digest path: `/id/game/modules/ddr-konaste.exe`. These are path strings, not a verified install root.

## Reporter, updater process, token

`0x14001B750` appends `launcher\modules\errorreporter.exe` and logs `GameAppCaller::call`. No `CreateProcess` import is in that window. `0x14001D830` formats `%slauncher\modules\updater.exe` and calls `ShellExecuteExA` with verb `runas`. `0x1400411C0` compares query keys `tk`, `dn`, and `trial`, and builds `konaste.ddr://login?tk=`. Login open is `ShellExecuteA` of `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=ddr`.

## Updater mode 2

`updater.exe` `0x140013540` stores dword **2** at the boot record. The next copies match the launcher layout: game id literal `ddr` (`0x14012B3A0`) through format `%s` at `0x14012B39C`, capacity `0x41`, destination at record `+0x04`; client version capacity `0x15` from the updater object `+0xD0` into record `+0x45`; token capacity `0x41` into `+0x5A`. `OA043711000000` receives that record. The updater PE ProductVersion is `VGP:J:C:A:2022012600`. The instruction copies the object string, not the resource bytes directly.

This is enough for an anonymous mode-2 discovery profile: game id `ddr`, version `VGP:J:C:A:2022012600`, the pinned DLL pair above. A decoder mismatch is not an ABI failure. DDR `libeacnet` poll/return constants are still unread. No live request was made.

## Admitted metadata profile

Compared with the verified SDVX `libeacnet.dll` (`8f12689e3693065c837a13315fa066467bbc6b24461a1b1e4b299edcf227e438`, 8,742,352 bytes). These RVA ranges are byte-identical, so SDVX poll semantics apply: boot `0x1A4E0` (796), state `0x1A800` (16), shutdown `0x1A8A0` (117), worker `0x1A920` (5488), error `0x1DCA0` (16), result `0x1DCC0` (16), module `0x1BF20`/`0x1C010`/`0x1C1D0`, start `0x1C430`/`0x1C560`, resource `0x1CA50`/`0x1CB20`/`0x1CD70`. Whole files differ by 48 bytes outside those ranges.

DDR and SDVX `avs2-core.dll` have identical `.text`, `.rdata`, `.data`, `.pdata`, `.data1`, `.rsrc`, and `.reloc`. Whole files differ (DDR 1,633,304 bytes, SDVX 1,633,232). The launcher clears and passes a `0xD18` descriptor and a `0x08000000` buffer.

Anonymous discovery profile:

| Field | Value |
|---|---|
| Mode | 2 |
| Game id | `ddr` |
| Client version | `VGP:J:C:A:2022012600` |
| `libeacnet.dll` | 8,742,400 bytes, `aa91b712f2c2ffc63eda73efc53544199299670640ca6b49463c693790f494e0` |
| `avs2-core.dll` | 1,633,304 bytes, `8a1473ae12064edf92228ed9c306346c1dc1e02031581e099a4a46d955b496c0` |

No live request was made. The version is the updater PE ProductVersion copied through the object field; the copy instruction does not load the resource bytes directly.

## Game create

`errorreporter.exe` has one `ShellExecuteExA`, `0x14000D248`, in `sub_14000D160`. `CApplication::MainLoop` (`0x140001EE0`) is the only caller. There is no `CreateProcess` import.

`WinMain` (`0x140006920`) parses its own argv with `CommonCommandLinesImpl` mode 0 (`[vtable+8]`, `edx = 0`). That mode copies the raw command line to object `+0x28` and does not rebuild it. `--dev` and `--trial` are recognized there but are not appended. The ` -t ` / ` --dev` / ` --trial` rebuild exists only in mode 1, which this `WinMain` does not call. A normal launcher handoff is therefore ` -t ` plus the raw token, with neither flag.

| `ShellExecuteExA` field | Value | Site |
|---|---|---|
| `cbSize` | `0x70` | `0x14000D1B6` |
| `fMask` | `0x40` (`SEE_MASK_NOCLOSEPROCESS`) | `0x14000D1BE` |
| `lpVerb` | `open` | `0x14000D1CA` |
| `lpFile` | raw `HKLM\SOFTWARE\KONAMI\DanceDanceRevolution\InstallDir` plus `game\modules\ddr-konaste.exe` (28 bytes, `0x1400ADF00`) | append `0x14000201B` |
| `lpParameters` | `%s %s` (`0x14000206C`): settings string, then the raw `+0x28` command | format `0x14000207C` |
| `lpDirectory` | that same `InstallDir` plus `game\` (5 bytes) | append `0x140002010` |
| `nShow` | `1` (`SW_SHOWNORMAL`) | `0x14000D1E1` |
| `hProcess` | stored to the caller slot | `0x14000D291` |

`InstallDir` is `REG_SZ`, read only when `cbData <= 260` (`0x14000D870`). No separator is inserted. The settings prefix comes from `sub_140001400`. With no positive `DisplaySettings` value it is the literal ` --display0`. ` --display%d`, ` --fps%d`, ` --fullscreen`, and ` --borderless` are added only from `ResourceDir\config\settings_cf.xml`. They are not `--dev` or `--trial`.

The wait is `sub_14000D2C0`: `MsgWaitForMultipleObjectsEx(1, &hProcess, INFINITE, 0x1DFF, 0)`. Exit code `259` (`STILL_ACTIVE`) returns to a 100 ms sleep. Any other code closes `hProcess`. The reporter does not terminate the game.

The fresh index module path `game/modules/ddr-konaste.exe` is this same file. The reporter writes backslashes; the working directory is `InstallDir\game`, not the install root.

## Local index mapping

Updater `dn` is command byte `+0x68`, returned by vtable slot `+0x18` (`0x140042750`). It is set only by query key `dn` or argv `--dev`. A normal `-t` launch leaves it clear.

All three index sites use that byte the same way: `0x14001C6F6`, `0x14001CF63`, and `0x14001E25C`. Set selects the literal `/InstallDir/rid.bin` (`0x14012E220`). Clear selects the literal `/InstallDir/ri.bin` (`0x14012E238`). AVS ordinal 258 copies that string into a `0x104`-byte buffer. Nothing is joined in front of it.

Install state `0x14001DFE0` reads each node's `path` and `tmp` (ordinal 176). The only prefix is the backup name: `%s/%s` of `/old` plus the raw `path` (`0x14001E0EF`). Ordinal 89 then renames `tmp` to that raw `path` (`0x14001E1AA`). No `game/` or other destination prefix is inserted. The current module therefore lands at install-root-relative `game/modules/ddr-konaste.exe`.

## Not proven

The registry value's trailing separator is not proven. A root without `\` joins a bad file path. The settings XML is not in this package, so a populated display, fps, or window suffix was not observed. `rid.bin` remains the `dn` path only. DDR `libeacnet` poll/return constants remain unread.

# DDR current launch, 2026012800

Date: **2026-10-16**. Static only. The three current launcher modules and `settings.exe` were fetched anonymously and hash-checked. None was executed. No account, installed tree, or user configuration was read. IDA was opened only for the current `launcher.exe`, then closed. The reporter and settings binaries were decoded locally.

The fresh index row is revision `2026091600`, module ProductVersion `VGP:J:A:A:2026091600`. Its launcher callers are revision `2026012800`. `libeacnet.dll` in that index is the old MSI image (`aa91b712…`, 8,742,400 bytes), so the metadata ABI is unchanged. This note does not repeat it.

## Fetched files

Host `d2el0dli9l0x2p.cloudfront.net`, base `/update/2026012800/launcher/modules/`. All four responses were HTTP 200 with `Last-Modified: Mon, 26 Jan 2026 06:26:57 GMT`. Stored at `%TEMP%\konaste-ddr-current-n1nsnqq4`.

| File | Bytes | SHA-256 | `041104b0` ProductVersion |
|---|---:|---|---|
| `launcher.exe` | 1,801,856 | `44a196c70991001629ea7e8d261444bb02fcc18f9c440aaa5beea9f663c65f69` | `VGP:J:B:A:2026012800` |
| `errorreporter.exe` | 1,132,672 | `9c3f0cc0c01c5a8aa952d3ad128cfcdaa897dcd6a7630e6343d6ef01e8f301a0` | `VGP:J:D:A:2026012800` |
| `updater.exe` | 1,804,416 | `e5a8ef8971ecb9d70ce0112b15a675bc666baef676b94a8c187b1027a2f32353` | `VGP:J:C:A:2026012800` |
| `settings.exe` | 1,523,328 | `aee6ef2020e5cc5105d51f54548207e27dc847a3aa123c4310fbf0a51f24227b` | `VGP:J:E:A:2026012800` |

FileVersion is `1.1.37.0` on all four. The fresh index places `libsettings.dll` at revision `2023012500`, not beside the 2026012800 launcher files. The anonymous GET of `https://d2el0dli9l0x2p.cloudfront.net/update/2023012500/launcher/modules/libsettings.dll` returned 166,768 bytes, SHA-256 `ca3cd978abced719ab77246ebd1259204a93b007d9d925b6211747c85d62585a`, `Last-Modified: Mon, 23 Jan 2023 06:23:08 GMT`. It was not loaded. `GetResourcePath` (`0x1800021F0`) formats the `ResourceDir` registry buffer at `0x180026ED4`. `0x1800041E1` reads that `REG_SZ`. The image has no `settings_cf`, `SettingDatas`, `KeyValuePair`, XML declaration, or property magic. `FinishSettingLib` (`0x180002080`) does not write a file.

## Normal mode-1 caller

Current `launcher.exe` `0x140023170` calls `GetModuleFileNameA(NULL)` and `Version::getProductVersion` (`0x140044E30`). That reads `\StringFileInfo\041104b0\ProductVersion` into `0x1407B76E0`, capacity 21 (`0x140044F7D`). The PE value is `VGP:J:B:A:2026012800`.

`0x140014424` calls the accessor `0x1400226F0` and copies that buffer to launcher object `+0xD0` (`0x14001443A`). Mode 1 at `0x140014D40` then copies object `+208` with capacity 21 into record `+0x45` (`0x140014EAA`). The record is otherwise the known layout: mode dword 1, game id `ddr` capacity `0x41`, token capacity `0x41`, and the `dn` byte. `OA043711000000` receives it at `0x140014F6A`. A zero return advances to `0x1400152A0`; nonzero formats `(003)-(004)` and goes to `0x140018BF0`.

This is the launcher profile, not the game module `VGP:J:A:A:2026091600` and not the updater `VGP:J:C:A:2026012800`.

## Check start

`btn_start` (`0x140016720`) enables `btn_start` and `btn_file_check`, then enters `0x1400168B0`. The start byte at object `+336` calls `0x140016950`.

That function joins `InstallDir` (`0x140044E10(0)`, buffer `0x1407B7800`) with `game\modules\ddr-konaste.exe` (28 bytes, `0x140016A33`). `Version::getProductVersion` on that joined path is only a presence test (`0x140016A8C`). Success calls `OA043711000030` with a zeroed 12-byte result. Nonzero then reads `OA043711000003` and `OA043711000004` into the status string `(003-004)`. Failure appends `(002-GFV)` and goes to `0x140018BF0`.

The next state, `0x140016E80`, calls `OA043711000031`. Return 2 calls `OA043711000032` into a 65-byte buffer. Zero from 032 mounts `InstallDir` at `/id` as `fs` (`0x140016F2B`), hashes `/id/game/modules/ddr-konaste.exe` with ordinal 326 into 32 bytes (`0x140016F3A`), and formats it with `%02x` thirty-two times. The compare is ordinal 229 against the 032 buffer, skipped when command slot `+0x18` is set and the first word of that buffer is `0x0030`. Mismatch appends `(001-GSF)`. A 031 result other than 2 waits while object `+188` is at most 3600, then appends `(000-STO)`.

There is no `getHash` string. The local hash is this ordinal-326 digest. Its equality with a particular SHA-256 construction is not proven from this caller.

## Reporter create

Current reporter `.text` is byte-identical to the 2022 reporter `.text` (614,912 bytes). `.rdata` differs in debug metadata, not these instructions. The create proof from the 2022 image therefore applies at the same addresses.

`WinMain` `0x140006920` parses argv with `CommonCommandLinesImpl` mode 0 (`[vtable+8]`, `edx = 0`). Mode 0 copies the raw command line to object `+0x28`. It does not rebuild ` -t `, `--dev`, or `--trial`. An empty parse result exits before the game.

`CApplication::MainLoop` `0x140001EE0` calls the only `ShellExecuteExA` at `0x14000D248`:

| Field | Value |
|---|---|
| `cbSize` / `fMask` / `nShow` | `0x70` / `0x40` / `1` |
| `lpVerb` | `open` |
| `lpFile` | raw `InstallDir` plus `game\modules\ddr-konaste.exe` |
| `lpDirectory` | raw `InstallDir` plus `game\` |
| `lpParameters` | `%s %s`: settings prefix, then raw object `+0x28` |
| `hProcess` | retained and waited with `MsgWaitForMultipleObjectsEx(1, INFINITE, 0x1DFF, 0)` |

Exit code 259 sleeps 100 ms and waits again. Any other code closes the handle. No separator is inserted at the join.

## Settings argument

The prefix is `sub_140001400`. Its file is the AVS path `/ResourceDir/config/settings_cf.xml` (`0x140001453`). `/ResourceDir` is mounted at `0x140006118` from `sub_14000D0F0(1)`, the `ResourceDir` registry buffer filled by `0x14000D090`. The value is `HKLM\SOFTWARE\KONAMI\DanceDanceRevolution\ResourceDir`, `REG_SZ`, accepted only when `cbData <= 260`. The filesystem location is therefore that registry value plus `config\settings_cf.xml`. No separator is inserted. The fresh index contains no XML entry for this file.

AVS ordinal 100 stats the path. A false result or a non-positive size skips the walk (`0x140001492`, `0x1400014A5`) but does not return from the flag builder. Ordinal 182 mode 17 reads a body of at least `0x350` bytes. The reader detects format 0 through `format_binary_check` (`0x180065010`): magic `0xA0450000` or `0xA0420000`, and the companion `0xA0460000` or `0xA0430000` when its mode argument is 1. Format 1 is an optional UTF-8 BOM (`EF BB BF`) followed by `<` (`0x180066C50`). Format 2 is `{` or `[` after spaces, tabs, CR, or LF (`0x180068890`). Formats 0, 1, and 2 each call a reader. Any other result, including format 4, is rejected. No emitter is proven. `settings.exe` is a .NET settings UI, and the pinned `libsettings.dll` writes neither XML nor a property image. The reader accepts only the detected bodies: binary property magic, `<`, or `{`/`[`. Any other body fails closed.

Ordinal 162 walks `/SettingDatas/Parameters/KeyValuePair`, and ordinal 167 mode 4 advances to the next child (`0x14000162A`). Ordinal 176 reads `/Key/string` and `/Value/string`, each capped at 64 bytes. The caller passes base 0 to ordinal 245. A leading `0x` or `0X` selects base 16, a leading `0` selects base 8, and any other text selects base 10. `+` and `-` are accepted. The signed 32-bit result is stored. Overflow behavior was not reduced.

The map starts empty (`0x1400031D0`). A missing key inserts integer 0 (`0x140003E0E`). An equal key returns the existing slot (`0x14000272B`), and `0x1400015EB` overwrites it, so the later child wins.

There is no return before the FPS branch. A missing file, a failed open, and an empty walk all reach the builder with an empty map:

| Case | Result |
|---|---|
| File missing, open failed, or no pairs | every key reads 0: ` --display0 --fps60`, no window text |
| Key absent from a parsed body | that key alone reads 0; stored keys still apply |
| Explicit `DisplaySettings` 0 | ` --display0` |
| Explicit `FPSSettings` 0 | ` --fps60` |
| Explicit `WindowSettings` 0 | no window text |

`DisplaySettings` at or above `GetSystemMetrics(80)` becomes monitor count minus one. A negative value becomes 0. Stored 0 emits literal ` --display0`; a positive value emits ` --display%d`. `FPSSettings` 0 emits ` --fps60`, 1 emits ` --fps120`, and every other value emits nothing. `WindowSettings` 1 emits ` --fullscreen`, 2 emits ` --borderless`, and every other value emits nothing. `%s%s%s` joins the pieces. The normal empty-settings parameters are therefore ` --display0 --fps60 -t <raw token>`. No current settings body was available.

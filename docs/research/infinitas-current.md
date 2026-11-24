# Current beatmania IIDX INFINITAS contract

Research date: **2026-11-21** (local time). The baseline is static: the 2022 bootstrap MSI was downloaded anonymously, hashed, and read through `msi.dll` (`MsiOpenDatabaseW` / `MsiViewFetch`). The executable copies were examined with pefile, dnfile/dncil, and Capstone 5.0.7. The follow-up used the official MSI, launcher, and updater on Windows with an approved signed-in trial session. [§5.1](#51-observed-cdn-transfers) separates captured CDN traffic from static evidence about service discovery. No replacement client sent a native protocol request.

**Status 2026-11-22:** INFINITAS is unsupported again. The Hub's INFINITAS detection, import, launcher setup, and settings editor were removed, so §9 describes code that no longer ships in `crates/konaste-hub`. The runtime-crate modules it links remain as reference.

This note supersedes the open items in [INFINITAS adapter contract](infinitas-adapter.md). That note did not have the MSI `Directory`/`File`/`Registry` tables, the launcher's settings reader, or the fact that the **error reporter**, not the launcher, creates the game process.

## 1. Conclusion

INFINITAS is not a `libeacnet` title. The WiX bootstrap installs the official launcher chain, writes HKLM paths, registers `bm2dxinf:`, and ships no game binary. The stock updater installed `game\app\bm2dx.exe`, replaced bootstrap modules, and published `ri.bin` at the game root. The Hub's existing packed binary index decoder accepted that file unchanged. All 3,302 listed files matched the installed lengths and SHA-256 values. Static analysis identifies `bm2dx_er.exe` as the process that starts the game; game startup was not tested here. Settings are the same `SettingDatas/Parameters` XML the Hub already parses, at `%ResourceDir%\config\<RID>_cf.xml`, where `<RID>` is the tail of the current user's SID.

The chosen direction is a **native Hub adapter**. Installed-root detection, current-user settings, and native trial/terms presentation are separable from update and launch authentication. The packed index and the stored-byte transfers both fit existing Hub components. An official launcher response advertised the SHA-256 of this exact index, but no supported way for a replacement client to obtain a fresh authenticated index or complete launch verification was established. Do not substitute a direct game invocation, local index, or captured URL list for those checks.

## 2. Verified bootstrap binaries and package

| File | Bytes | SHA-256 | ProductVersion |
|---|---:|---|---|
| `infinitas_installer_2022060800.msi` | 44,212,224 | `9de019339a24e36ae54283bab6cb5602922a4ceae7d1a29d847c28f01e195dc2` | — |
| `launcher_modules_bm2dx_launcher.exe` | 8,910,728 | `51977e18612c8e957fabec6cce8dfbdb03d4f9c706e8208939eaa4aefac8d6ee` | `P2D:J:A:A:2020122300` |
| `launcher_modules_bm2dx_updater.exe` | 8,782,728 | `21deff9644a998520e26b1e603fd4b287670cefcad3fe626a09b0c2898c2a59c` | `P2D:J:C:A:2020122300` |
| `launcher_modules_bm2dx_er.exe` | 8,039,304 | `0e758a4b6aacfafe575b249d5a33f97cf0a99327dee7a0a004fa8ef8290d9b4d` | `P2D:J:D:A:2020122300` |
| `launcher_modules_bm2dx_settings.exe` | 1,963,472 | `a277052cf5d0aa16f822e1fa3e227fd2fce6056c40d677149f32ff96b7fa6425` | `P2D:J:K:A:2021111700` |
| `launcher_modules_libsettings.dll` | 129,928 | `c8b70edac2b26f6316af00018716fd0ee6ac4f398a8e016008a373d5200c8746` | — |

The MSI URL `https://d1rc4pwxnc0pe0.cloudfront.net/v2/installer/infinitas_installer_2022060800.msi` returned **HTTP 200**, 44,212,224 bytes, on 2026-11-21, and the body hash equals the 2022 pin. No newer bootstrap is evidenced; no other CloudFront path was guessed.

Images are PE32+ x64, image base `0x140000000` for executables and `0x180000000` for `libsettings.dll`. The three native executables carry `\StringFileInfo\041104b0\ProductVersion`. `bm2dx_settings.exe` has no `041104b0` block (its MSI `File` row language is `0`), so its ProductVersion must be read from whichever language block the resource actually has; do not hard-code `041104b0` for it. The updater's own ProductVersion exists only as UTF-16 inside its version resource (file offset `0x85e3d4`), not as an ASCII literal.

## 3. Install: MSI database facts

Read with `MSIDBOPEN_READONLY`. `Directory` has 17 rows, `File` 27, `Component` 22, `Registry` 7, `Property` 19. There is no `Shortcut` table (`MsiDatabaseOpenViewW` returned 1615, "table not found"). The summary stream's PackageCode is `{0DC977F8-D420-4A0F-9DDF-714ED8C264BA}`, Template `x64;1041`, Subject `beatmania IIDX INFINITAS インストーラ`.

### 3.1 Directories

| Directory Id | Parent | Name |
|---|---|---|
| `TARGETDIR` | — | `SourceDir` |
| `WINDOWSVOLUME` | `TARGETDIR` | `.` |
| `GamesDir` | `WINDOWSVOLUME` | `Games` |
| `INSTALLDIR` | `GamesDir` | `sqc2h04c|beatmania IIDX INFINITAS` |
| `RESOURCEDIR` | `INSTALLDIR` | `Resource` |
| `Dir_launcher` | `INSTALLDIR` | `launcher` |
| `Dir_launcher_modules` | `Dir_launcher` | `modules` |
| `Dir_launcher_image` | `Dir_launcher` | `image` |
| `Dir_launcher_image_p0000` | `Dir_launcher_image` | `p0000` |
| `Dir_launcher_image_p1000` | `Dir_launcher_image` | `p1000` |
| `Dir_launcher_image_p2000` | `Dir_launcher_image` | `p2000` |
| `DX_REDIST_DIR` | `INSTALLDIR` | `skkfqtug|DirectX 9.0c Redist` |
| `ProgramMenuDir` | `ProgramMenuFolder` | `_qowz8pb|KONAMI` |
| `System64Folder_amd64_VC.BFF61907_AA2D_3A26_8666_98D956A62ABC` | `WindowsFolder_amd64_VC.…` | `System64` |

The `x|y` form is a short name plus long name. Default install root is therefore `<install drive>\Games\beatmania IIDX INFINITAS`, with resource data in `…\Resource\`.

### 3.2 Files and renames

| MSI File key | Installed name | Bytes | Version |
|---|---|---:|---|
| `inficon` | `beatmania IIDX INFINITAS.ico` | 360,158 | — |
| `launcher_image_d0000.ifs` | `d0000.ifs` | 580 | — |
| `launcher_image_p0000_d0000` | `d0000` | 28,031,408 | — |
| `launcher_modules_afp_core.dll` | `afp-core.dll` | 1,000,328 | 2.14.20.2 |
| `launcher_modules_afp_utils.dll` | `ws6favu9.dll|afp-utils.dll` | 182,152 | 1.2.21.2 |
| `launcher_modules_avs2_core.dll` | `q4r8nmsb.dll|avs2-core.dll` | 1,634,184 | 2.17.3.0 |
| `launcher_modules_bm2dx_er.exe` | `bm2dx_er.exe` | 8,039,304 | 2.0.1.110 |
| `launcher_modules_bm2dx_launcher.exe` | `8os2qish.exe|bm2dx_launcher.exe` | 8,910,728 | 2.0.1.120 |
| `launcher_modules_bm2dx_settings.exe` | `ompejjao.exe|bm2dx_settings.exe` | 1,963,472 | 2.0.1.110 |
| `launcher_modules_bm2dx_updater.exe` | `khakcefx.exe|bm2dx_updater.exe` | 8,782,728 | 2.0.1.110 |
| `launcher_modules_libsettings.dll` | `wkfps-io.dll|libsettings.dll` | 129,928 | — |
| `F_CENTRAL_msvcr100_x64.…` | `msvcr100.dll` → `System64Folder` | 829,264 | 10.0.40219.1 |
| `F_CENTRAL_msvcp100_x64.…` | `msvcp100.dll` → `System64Folder` | 608,080 | 10.0.40219.1 |

The DirectX 9.0c redistributable files (`DXSETUP.exe`, `DSETUP.dll`, `dsetup32.dll`, five `Jun2010_*` cabs) install to `DX_REDIST_DIR`. There is no game member in the MSI, so the bootstrap contains only the launcher chain. The live transfer of `game/app/bm2dx.exe` is recorded in [§5.1](#51-observed-cdn-transfers). The startup page says that content downloads may reach tens of GB.

### 3.3 Registry rows

| Row | Root | Key | Name | Value | Component |
|---|---:|---|---|---|---|
| `reg9E58D94F…` | 2 (HKLM) | `Software\KONAMI\beatmania IIDX INFINITAS` | `InstallDir` | `[INSTALLDIR]` | `RegistInstallDir` |
| `regA41DB3E8…` | 2 (HKLM) | same | `ResourceDir` | `[RESOURCEDIR]` | `RegistResourceDir` |
| `reg358870B9…` | -1 | `Software\[Manufacturer]\[ProductName]` | — | — | `ProgramMenuDir` |
| `regC106F9DC…` | 0 (HKCR) | `bm2dxinf\shell\open\command` | — | `"[INSTALLDIR]launcher\modules\bm2dx_launcher.exe" "%1"` | `CreateURLScheme` |
| `regB6134AF5…` | 0 (HKCR) | `bm2dxinf` | — | `URL:bm2dxinf Protocol` | `CreateURLScheme` |
| `regD4B27854…` | 0 (HKCR) | `bm2dxinf` | `URL Protocol` | — | `CreateURLScheme` |
| `reg7E4E0945…` | 0 (HKCR) | `bm2dxinf\DefaultIcon` | — | `[INSTALLDIR]beatmania IIDX INFINITAS.ico,0` | `CreateURLScheme` |

The two path values are written to HKLM (root 2). The `-1` root means "current user for a per-user install, machine otherwise"; with `ALLUSERS=1` it resolves to machine scope, so the installer writes no HKCU value. This matters because the launcher and `libsettings.dll` *read* HKCU first and HKLM second, so the normal installed state has both reads falling through to HKLM.

`Property`: `ALLUSERS=1`, `ProductCode={86BE9AFB-152B-4298-A9C9-D6C7A59C75CA}`, `UpgradeCode={A763CAF9-BCF4-4470-8573-1130EAA18934}`, `ProductVersion=1.0.0`, `WIXUI_INSTALLDIR=INSTALLDIR`, `WIXUI_RESOURCEDIR=RESOURCEDIR`, `Manufacturer=KONAMI`, `ProductLanguage=1041`, `ARPNOMODIFY=1`, `SecureCustomProperties=NEWERFOUND;SELFFOUND`. The PackageCode in the summary stream (`{0DC977F8-…}`) is a different field from the ProductCode above, matching [remaining-title-contracts](remaining-title-contracts.md).

`InstallExecuteSequence` has 42 rows. The notable conditionals are `InstallDirectX` (6595, `NOT REMOVE`), `DelDirectX` (6597, `NOT REMOVE`), and the upgrade guards `AlreadyUpdated` (26, `SELFFOUND`) and `NoDowngrade` (27, `NEWERFOUND`). There is a `CreateShortcuts` row (4500) even though the `Shortcut` table is absent, which is normal for WiX when no shortcut rows exist.

There is no `Shortcut` table, so the installer does not create a Start-menu or desktop shortcut; the scheme registration and the ARP entry are the only user-facing entry points.

**Live check:** `msiexec /qb` returned 1603, and the MSI log explicitly required full UI. The normal elevated wizard exited 0; the expected `InstallDir`, `ResourceDir`, and scheme registration were verified. The bootstrap went to a volume that met the published 60 GB free-space requirement. Bootstrap admission in that run used the pinned hashes. The subsequent installer implementation passed a full Windows Authenticode check on both the retained MSI and a fresh download of the pinned bytes. The new Hub-triggered wizard was not run against the existing installation.

## 4. Launch chain

### 4.1 Who starts the game

The launcher does not start `bm2dx.exe`. The chain is:

1. Shell executes the scheme handler `"[INSTALLDIR]launcher\modules\bm2dx_launcher.exe" "%1"` (Registry row above).
2. `bm2dx_launcher.exe` boots its embedded eacnet and calls `GameAppCaller` at `0x140032580` (from `0x14002e4d9`) to start `launcher\modules\bm2dx_er.exe`. The app path is built at `0x14003262e`–`0x14003266b` (`0x140178780`, length `0x1d`) prefixed with InstallDir from `0x140088bd0`; the helper at `0x1400222f0` is an append.
3. `bm2dx_er.exe` (the error reporter) boots its own eacnet, resolves InstallDir/ResourceDir, and is the process that creates the game.

The launcher's game-presence probe is separate: `0x14002d2a0` resolves `/id` + `fs` (`0x14002d33e`, `0x14002d345`) and checks `/id/game/app/bm2dx.exe` (`0x14002d355`, width `3`) via `0x140030110`, which returns `eax == 0x20` (`0x14002d34b`); a second use of the same string is at `0x14002d358`. `0x14002cecf` also calls a file-existence helper on the built path (`0x140088bf0`, `PathFileExistsA`-shaped).

The launcher's process-create helper `0x1400894b0` uses `SHELLEXECUTEINFOA` with `cbSize=0x70` (`0x140089512`), verb `open` (`0x140178730`), `nShow=1` (`0x140089547`), `lpFile=rdx`, `lpParameters=r8`, `lpDirectory=r9`, and `fMask` `0x100` when its fifth argument (stack byte) is nonzero, else `0x40` (`0x14008950d`–`0x14008951d`, `cmove`). `GameAppCaller::call` (`0x140032580`) passes the reporter path as `lpFile`, the ` -t …` command as `lpParameters`, InstallDir as `lpDirectory`, and a flag byte read from `[rdi]` (`0x14003274e`) as the fifth argument (`0x140032758`). The caller at `0x14002e4ca`–`0x14002e4d9` sets that flag to 1, so launcher→reporter is `fMask 0x100`.

The updater is started separately by `0x1400347e0` (callers `0x14002e7e5` with flag `dl=0`, `0x14002ea6d` with flag `dl=1`). InstallDir comes from `0x140088bd0` (`0x14003484a`): the working directory is formatted with `%s` (`0x1401794a0`, `0x140034888`–`0x14003489b`) and the application with `%slauncher\modules\bm2dx_updater.exe` (`0x1401794a8`, `0x1400348a4`–`0x1400348b4`). The verb is `runas` (`0x1401794d0`) when the flag is 0 and `open` (`0x140178730`) when it is 1 (`0x1400348ba`–`0x1400348ca`, `cmovne`). The ` -c ` argument (`0x1401794d8`) is appended only when the flag is 1, i.e. the `open` path (`0x1400348e2`–`0x1400348f7`). `fMask` is `0x100` when the flag is 0 and `0x40` when it is 1 (`0x140034921`–`0x140034938`), `nShow=1` (`0x140034972`), then `ShellExecuteExA` (`0x1400349e6`). So there are two shapes: `runas` with `fMask 0x100` and no `-c`, or `open` with `fMask 0x40` and ` -c `. The caller at `0x14002e7c4`–`0x14002e7e5` takes the `runas` shape when a version comparison returns 1 and otherwise jumps to `0x14002e877`; which context is the first run is not decoded, so "first run uses runas" is not a proven statement.

### 4.2 The exact game command line

`bm2dx_er.exe` main `0x140001130`:

| Step | Address | Value |
|---|---|---|
| Get InstallDir | `0x140001568` → `0x14002b630(0)` | pointer `0x14018f5b0`, filled by `0x14002b5d0` |
| Append game path | `0x140001651`–`0x140001663` | `game\app\bm2dx.exe` at `0x1400d2018`, length `0x12` |
| Get command | `0x140001668`–`0x140001677` | singleton `0x140028740`, vtable slot `+0x38` → `0x140028720` returns `obj+0x28` |
| Log | `0x1400016aa` | `CApplication::MainLoop cmd:%s` |
| Create | `0x1400016f4` | `lpFile=game path`, `lpParameters` = command (`obj+0x28`), `lpDirectory` = InstallDir |

`lpDirectory` is the InstallDir string built at `0x140001597`, not ResourceDir. The create helper `0x14002b6a0` sets `cbSize=0x70` (`0x14002b6f6`), `fMask=0x40` (`0x14002b6fe`), `lpVerb=open` (`0x1400db0ec`), `nShow=1` (`0x14002b721`), and calls `ShellExecuteExA` at `0x14002b788`. The wait helper `0x14002b800` uses `WaitForSingleObject` with timeout `0x1dff`, then `GetExitCodeProcess`; exit code `0x103` is logged as "app is still active".

The command string is built by the same parser that handles the scheme URI, `0x140027e70`:

| Input | Site | Result |
|---|---|---|
| `-t <token>` | `0x1400283eb`, `0x1400280bf` | token copied to `obj+0x48` |
| URI `tk=` | `0x14002808f`–`0x1400280c7` | same slot |
| `dn` | `0x1400280d8`… | `obj+0x68` |
| `trial` | `0x1400282d5` | `obj+0x69` |
| `-c` | `0x1400284cf` | `obj+0x6a` |
| build | `0x14002822e`–`0x140028308` | `" -t "` + token, then `" --dev"` if `+0x68`, then `" --trial"` if `+0x69` |
| rebuild URI | `0x140028531` | `bm2dxinf://login?tk=` + token + `&dn=` + `&trial=` |

So `lpParameters` is ` -t <64-hex token>[ --dev][ --trial]`. Graphics, audio, FPS, LED, and language settings are **not** in this command line.

### 4.3 What the launcher does between the URI and the reporter

`bm2dx_launcher.exe` imports `CommandLineToArgvW` (IAT slot `0x140edcd98`). Its callback parser `0x140085560` tests for a leading `"`, splits on `?`, `&`, `=`, and accepts either the URI as the first token or `-t` as a separate token (`0x140085adb`). It stores token at `+0x48`, `dn` at `+0x68`, `trial` at `+0x69`, `-c` at `+0x6a` (`0x140085779`–`0x140085bd3`) and rebuilds the same ` -t …` command (`0x1400858e9`–`0x1400859f8`) for the reporter. The exact argv-extraction call site was not pinned; the launcher's call at `0x140085a80` is a local helper, not the shell32 import.

It also re-reads the local index: `ri.bin` (`0x1401764e8`) or `rid.bin` (`0x1401764e0`) is selected by the `dn` getter at `0x14002c389` and joined with InstallDir using `%s%s` (`0x140176274`, `0x14002c3b8`), then opened at `0x14002c3d1`–`0x14002c3f0` with access `0x80000000`, share `3`, and open disposition `3` (a `CreateFileA`-shaped call; the IAT slot was not named). `resource_info/ri_ver` is at `0x1401764f0`.

Its embedded eacnet boot thread logs the fixed order at `0x14017cb50`–`0x14017d030`: services → get_information → server_status → server_clock → upd_check → log_check.

The launcher's own start verification is a `checkGameStart` request object:

| Part | Address |
|---|---|
| setup | `0x140075750`; copies `checkGameStart` (`0x14017ff88`) into `+0x110` cap `0x100`, copies 2×0x80 bytes to `+0x210` |
| property writer | `0x1400758b0`; `protocol_version` = `P2D:2015091800` (`0x140172c80`), `client_version`, `request` |
| getInformation | setup `0x140075b40`: `getInformation` (`0x140180140`), cap `0x100`, byte to `+0x210` |
| failure path | `0x14002dac0`: `failed to eacnet_lch_check_game_start` (`0x140176640`) |

The same property writer shape exists in the launcher's generic request object at `0x14007a13e`–`0x14007a1ef`: `protocol_version`, `client_version`, `signature`, `retry_count`, `request`. `client_version` is read from the BSS string at `0x1401d5f26` (`0x1400758f5`, `0x14007a04a`). That BSS is filled during boot at `0x140062059`–`0x14006206c` from the version resource pointer at `[rbx]`, cap `0x15`, immediately after the game id is copied with cap `0x41` at `0x140062036`–`0x14006204a`. The launcher's ProductVersion is `P2D:J:A:A:2020122300`, present only as UTF-16 in the version resource (file `0x87d7dc`), not as an ASCII literal.

The reporter repeats this pattern independently: `eacnet_er_boot` at `0x140004500`, boot-thread log order services → server_status → server_clock → log_check (`0x1400d2ab0`–`0x1400d2e10`). Its `.rodata` holds setup function pointers adjacent to the request name literals: `0x140018ac0` sits next to `checkGameStart` (`0x1400d66a8`), `0x140019b60` next to `gameEnd` (`0x1400d6dd8`), and `0x140019db0` next to `getGoodsList` (`0x1400d6ee8`). No call site outside those literals was found, so the table's dispatch use is inferred from layout, not proven.

### 4.4 Can the Hub reproduce this?

Three options:

| Option | Cost | What it needs | Status |
|---|---|---|---|
| A. Supported path: `ShellExecute` the official launcher with the URI | Low | Token + installed tree | Proven at the MSI/scheme level |
| B. Direct `game\app\bm2dx.exe` with ` -t <token>` | — | Token | Observed statically as the reporter's command line, not a verified alternative to the reporter |
| C. Native Hub adapter (selected) | High | Supported metadata and launch authentication; a verified index adapter | Transfer reuse supported by [§5.1](#51-observed-cdn-transfers); authentication remains unresolved |

Option A is what the MSI itself wires: the scheme handler is `bm2dx_launcher.exe "%1"`, so handing it `bm2dxinf://login?tk=<token>` is the vendor-supported path, and the launcher's own parser also accepts `-t <token>` as a bare argv token (`0x140085adb`). The Hub gives up visibility of the verification result and the exit code because the reporter owns the game process, and the updater still needs the signed `getResourceInfo` exchange on first run.

The stock client's logs and static strings show request signing, protocol checks, and response integrity verification in its embedded client. Reading sample payloads offline does not reproduce those checks or establish a supported way for the Hub to obtain a trusted index or complete a launch check.

### 4.5 Reuse check against the currently installed executables

A read-only PE inventory checked the installed copies of `bm2dx_launcher.exe`, `bm2dx_updater.exe`, and `bm2dx_er.exe`. All three have **zero PE exports**, no normal or delay import of `libeacnet.dll`, and no ASCII or UTF-16 literal for that DLL. All three import `avs2-core.dll` and WinHTTP and carry service-request markers in their own images. This supports the earlier finding that the client logic is embedded. It does not prove the absence of every private interface; a simple static scan did not resolve the current build's WinHTTP call sites, so exact control-flow details in §4.1–§4.3 remain bootstrap-version findings.

The installed INFINITAS `avs2-core.dll` has the same `.text` SHA-256 (`5206ffda34c8a860e526152cd7031fb77fd120efeaa2395a83f476742eb81192`) and ordinal/name export map as all six Hub-cached AVS copies, but a different whole-file SHA-256. This is direct evidence of the shared low-level AVS implementation, not of a shared service client. The Hub's [normal worker](../../crates/konaste-runtime/src/official.rs) binds resource, module, and start calls from **`libeacnet.dll`**, not from the AVS export map. No INFINITAS component supplying that interface was found. All six cached `libeacnet.dll` files contain the same high-level request names (`getServices`, `getResourceInfo`, `checkGameStart`), but none contains an INFINITAS endpoint, URI scheme, or protocol-version marker in ASCII or UTF-16. This is not a complete profile-table analysis. It rules out visible support in those binaries, not every possible configuration. Endpoint substitution alone would not establish that the server accepts the other titles’ client identity or preserves INFINITAS verification.

The [official setup guide](https://p.eagate.573.jp/game/infinitas/2/startup/index.html) documents the Windows installer followed by the launcher and KONAMI web sign-in/start flow. The [requirements](https://p.eagate.573.jp/game/infinitas/2/startup/system.html) describe game-data downloads during play; the [FAQ](https://p.eagate.573.jp/game/infinitas/2/startup/faq.html) discusses resuming an interrupted update. None of those pages documents a callable update-only interface or a native third-party launch API. The old internal updater process handoff is not such an interface.

## 5. Update path

`bm2dx_updater.exe` `eacnet_upd_boot` is at `0x14004e470`; boot-thread log order is services → server_status → server_clock → log_check (`0x140165d88`–`0x1401660c0`).

The request name table is at `0x1401a39f4`, stride `0x44`; the index in this table is the type value used by the earlier note:

| Type | Name |
|---:|---|
| 0 | `getServices` |
| 1 | `getServerState` |
| 2 | `getServerClock` |
| 3 | `getServerValues` |
| 4 | `checkVersion` |
| 5 | `checkSendLogAvailable` |
| 6 | `sendLog` |
| 7 | `getResourceInfo` |
| 8 | `getHash` |
| 9 | `uploadFile` |
| 10 | `checkGameStart` |
| 11 | `getInformation` |

`getServices` uses dev `https://p.eagate.dev.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices` (`0x1401666d0`) or production `https://p.eagate.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices` (`0x140166730`), selected at `0x14004ff70`.

The common property writer `0x1400650e0` writes `protocol_version` = `P2D:2015091800` (`0x140169180`), `client_version`, `signature` (`obj+0xa8`), `retry_count` (`obj+0x10`), `request` (`obj+0x88`). `client_version` is the updater's own ProductVersion read from the version resource, matching the launcher's pattern.

After a `getResourceInfo` response, the download-init state reads node `resource_info` (`0x140161de0`), `ri_ver` (`0x140161ffc`, type 6, cap 4), and per-file fields `path` / `version` / `size` / `sum` / `url` (`0x140161df0`–`0x140161e14`, types `0xB`/6/6/6/`0xB`/`0xB`, caps `0x104`/4/4/4/`0x41`/`0x800`). It then formats `%s/%s#%d` (`0x140161ff0`) and writes the local index to `/InstallDir/ri.bin` (`0x140161fc8`) or `/InstallDir/rid.bin` (`0x140161fb0`), chosen by the dev flag, then hands off to `LauncherAppCaller` with `launcher\modules\bm2dx_launcher.exe` (`0x1401612b8`, log `LauncherAppCaller::call cmd:%s` at `0x1401612e0`).

Static analysis indicates the same index *field shape* as the Hub's other titles, but a binary property image and a signed metadata exchange. Per-file `url` fields carry the content URLs. The updater contains `.hcache` (`0x14016d390`) and `.partial` (`0x14016d3a0`) cache names and `EacnetRequestPackageListGet` and `EacnetRequestServerValues` code. Those strings do not establish which cache or resume paths were exercised.

### 5.1 Observed CDN transfers

Source: passive WinHTTP event records from the approved first stock update, correlated with read-only checks of the install tree. The elevated updater was attached **after** it had started. This trace begins during CDN traffic; it contains no service requests. The separate launcher trace confirms the official client's use of WinHTTP and compressed binary XML for some service messages. It does not fill in the updater's early exchange or establish a supported discovery mechanism.

| Observation | Result |
|---|---|
| Content requests | 3,294 distinct HTTPS GET targets on the same CloudFront host as the pinned MSI, over about 23 minutes. No URL query strings were observed. One sent request at a time had an open handle in this trace. |
| HTTP results | 3,293 HTTP 200s. Each recorded byte count matched its queried `Content-Length`, totaling 2,988,772,249 bytes. One earlier request closed with zero bytes and no queried status; later requests still succeeded. The cause of that request's outcome is unknown. |
| Resume evidence | The updater queried a non-weak `ETag` for each of the 3,293 responses. No 206 or explicit `Range` request was captured. This does not prove interrupted-download resume works. The capture did not retain complete header sets. An ETag is a transfer validator, not the manifest's content digest. |
| Content layout | All captured paths contained a 10-digit directory segment immediately before the install-relative subtree: 3,288 under `game/`, six under `launcher/`. The trace is not a manifest; do not reconstruct targets or revisions from it. |
| On-disk comparison | 2,961 captured objects could be matched to an installed file with a safely resolved path and a retained payload sample. All 2,961 matched both the retained prefix and stored length, including `game/app/bm2dx.exe` (16,087,176 bytes). Two objects fit within the recorder's byte limit and matched in full. This supports fetching stored bytes as-is; it does not verify whole-file hashes for the larger objects. |
| Launcher self-update | The CDN delivered newer launcher and updater executables. The installed copies matched the downloaded lengths and sampled prefixes and carried revision `2026012800`; the corresponding `old/launcher/modules` copies carried `2020122300` and matched the MSI SHA-256 pins. The MSI hashes describe bootstrap binaries, not every valid updated copy. |

The original recorder labeled WinHTTP status `0x00400000` as `cb_error`; the Windows SDK identifies it as `SENDREQUEST_COMPLETE`. Actual `REQUEST_ERROR` is `0x00200000` and was not logged by that version of the recorder. Do not count the historical `cb_error` events as HTTP failures or infer that the absence of real error events proves a clean run.

During the capture the updater window remained responsive, but the process reached a point with no TCP endpoints and no `ri.bin` yet. This was an intermediate state, not a permanent failure. The completed tree was verified separately below.

### 5.2 Published `ri.bin` and full-tree verification

The official updater published `<InstallDir>/ri.bin`. A read-only follow-up checked the file and the installed content without starting the game, fetching any URLs, or writing to the installation.

| Property | Observed value |
|---|---|
| Exact file | 1,039,160 bytes; SHA-256 `314dfcab394868ff6655be43908a35f3c2e3179138e4778a8c027a2ae6598835` |
| Encoding | `A0 42 00 FF`, the same packed-name binary property format as Bombergirl; 128,800 node bytes, 910,348 data bytes, no trailing bytes |
| Schema | `resource_info`, repeated `file` with `path`, `version`, `size`, `sum`, and `url`, and one `ri_ver` |
| Revision | `ri_ver=2026111100`; 24 distinct per-file versions, 11 entries at `ri_ver`. A URL uses its file's version, not necessarily `ri_ver`. |
| Content | 3,302 files (3,288 under `game/`, 14 under `launcher/`), 3,021,076,215 stored bytes. Every listed file matched both its exact size and SHA-256 on disk. |
| Routing | Every URL identifies the pinned MSI's CloudFront host and has the exact path `/v3/resource/distribution/jp/<file-version>/<install-relative-path>`. All paths passed the existing case and Windows path validation. No URL had credentials, a query, or a fragment. The index does not list itself. |
| Trace cross-check | All 3,294 captured targets are present in the published index. The byte count agrees with the index for the 3,293 HTTP 200s. The one request without a result was also present and its installed file verified. Eight index entries were absent from the partial trace. |

An earlier stock launcher `getResourceInfo` exchange advertised the exact SHA-256 in its `result/hash` field. This confirms what the advertised digest covers: the complete raw bytes of the published `ri.bin`. The offline comparison did not independently verify the response signature, retrieve a fresh result, or establish replacement-client authentication.

The existing Rust [`konaste-manifest::decode`](../../crates/konaste-manifest/src/lib.rs) accepted this exact pinned input without production-code changes. The [opt-in offline regression test](../../crates/konaste-manifest/tests/offline_official.rs) uses `KONASTE_OFFLINE_INFINITAS_MANIFEST` to read the local file and check the decoded projection. The pin is for reproducing this check, not for authorizing a future update.

**Follow-up HAR (2026-11-21).** The user-provided HAR contained 13 native API exchanges, including three `getResourceInfo` responses: two hash-only responses and one containing the hash and full `resource_info` subtree. All three hash values matched the published `ri.bin`. The full response was 399,733 bytes on the wire; all 3,302 records decoded in the same order with identical path, version, size, SHA-256, and URL values. The response's decompressed binary XML wrapper has a different header from the disk file (`A0 42 A0 5F` versus `A0 42 00 FF`), but its manifest subtree's node bytes and data bytes matched exactly. Adding the standalone header, section lengths, end marker, and padding reproduced the published index byte for byte in this offline check. Do not hash the entire API response as though it were `ri.bin`, or loosen the standalone decoder to accept the API wrapper.

This HAR fills the earlier evidence gap about how the complete index arrives. Offline decoding and digest comparison did not verify the server response signature or establish a supported way for Hub to make fresh authenticated requests. The original HAR contains cookies and request bodies; keep it private and out of the repository.

### 5.3 Hub transfer boundary

The existing [`ManifestFile`](../../crates/konaste-updater/src/manifest.rs) already takes a safe relative path, exact HTTPS URL, stored size, SHA-256, and optional strong ETag; `OfficialManifest` can publish the exact accepted index bytes. Reuse these pieces once a supported source can obtain fresh authenticated index bytes and their digest. The title adapter must enforce the host, prefix, path/URL correspondence, per-file revision, executable presence, and index-path collision rules. [`UpdateSource::from_metadata`](../../crates/konaste-install/src/lib.rs) currently takes the other titles' discovery contract. A local `ri.bin` is useful for read-only diagnostics but is not proof of freshness or authority for writes. Keep launcher self-update separate from an initial game-content adapter.

## 6. Settings

### 6.1 Where settings live

`bm2dx_settings.exe` is .NET Framework 4.0 WPF and P/Invokes `libsettings.dll` for `GetResourcePath`, `GetUserRID`, `InitSettingLib`, `FinishSettingLib`, `InitSettingLibKey`.

`SettingsSaveLoad.GetSettingFile` (IL RVA `0x397c`):

1. `GetResourcePath().ToString()`
2. append `"\\config"`
3. `Directory.Exists` else `CreateDirectory`
4. `GetUserRID().ToString()`
5. return dir + `"\\"` + RID + suffix, with `fileNameSuffix = "_cf.xml"` (`SettingsSaveLoad..cctor`, RVA `0x39df`)

The RID is not the account name: `GetUserRID` calls `GetUserNameA`, then `LookupAccountNameA` (`0x180001000`, `0x180001060`), then `ConvertSidToStringSidA` (`0x1800017bd`), and returns the SID substring after its last `-` (`0x180001815`–`0x180001845`). For a normal local profile that is `1001`.

`libsettings.dll`:

| Export | RVA | Behaviour |
|---|---:|---|
| `GetResourcePath` | `0x1730` | returns `%s`-formatted output of `0x180002c10`, an empty template at `0x18001ea64`; the buffer is filled by `0x180002bb0` (called from `0x180001587` inside `InitSettingLib`), which reads `InstallDir` and `ResourceDir` from the registry, so the returned path is `ResourceDir` |
| `GetUserRID` | `0x1760` | `GetUserNameA` (`0x18000179f`) → `0x180001000` (`LookupAccountNameA`) → `ConvertSidToStringSidA` (`0x1800017bd`) → trims at the last `-` (`0x18000182b`–`0x180001840`) → formats `%s` |
| `InitSettingLib` / `FinishSettingLib` | `0x11f0` / `0x15c0` | display/device enumeration (`EnumDisplaySettingsA`, `EnumDisplayDevicesA`, `DirectInput8Create`) |
| `InitSettingLibKey` / `FinishSettingLibKey` | `0x1b10` / `0x1b80` | key-config binding, HINSTANCE argument |

`libsettings.dll` also contains `SOFTWARE\KONAMI\beatmania IIDX INFINITAS` (`0x180017a50`), `InstallDir` (`0x180017a30`), `ResourceDir` (`0x180017a40`), and opens HKCU `0xFFFFFFFF80000001` first, then HKLM `0xFFFFFFFF80000002` (`0x180002c83`–`0x180002cb1`). Value type must be `1` and size `<= 0x104`. The launcher duplicates this logic at `0x14008f430` (key), `0x14008f440` (read, hives `0x14008f493` / `0x14008f4ba`), with callers at `0x140088b90` (`InstallDir`) and `0x140088bb1` (`ResourceDir`); the reporter duplicates it again at `0x140031a00` with key `0x1400db9b8`.

So the settings file is `%ResourceDir%\config\<RID>_cf.xml`, where `<RID>` is the account SID tail (typically `1001`), not a fixed name.

### 6.2 Format and keys

`SettingsSaveLoad` serialises `SettingDatas` with `System.Xml.Serialization.XmlSerializer`: one `Parameters` array of `KeyValuePair` rows with string `Key` and `Value`. `LoadFromXmlFile` is IL `0x3870`, `SaveToXmlFile` `0x38f8`; `Load`/`Save` are one-line wrappers at `0x3861`/`0x3868`. Both use UTF-8 without BOM (`UTF8Encoding(false)`).

`Settings.get_DisplaySettings` / `get_AudioPlaySettings` / `get_FPSSettings` / `get_LEDSettings` / `get_LanguageSettings` (`IL 0x2c98`, `0x2cbe`, `0x2ce4`, `0x2d0a`, `0x2d30`) each read `GetParamAsInt32("<Key>", 0)`; the keys are exactly `DisplaySettings`, `AudioPlaySettings`, `FPSSettings`, `LEDSettings`, `LanguageSettings`. `KeyConfig1P_1`…`E4` and `KeyConfig2P_1`…`E4` are also fields. `MainWindow.Window_Loaded` (`IL 0x3ab0`) loads then binds; `SaveButton_Click` (`0x3bc8`) saves.

**Values verified in both settings executables.** The bootstrap copy (`P2D:J:K:A:2021111700`, pin in §2) and the installed copy (`P2D:J:K:A:2026012800`, SHA-256 `7dfd6138bc3be998f8ba5a9fe79f3ec0036c54dc366ecd482c8deec46466886a`, matching `ri.bin`) have the same settings mappings. Decompilation of `Setting.Settings`, the setting-specific data classes, and `mainwindow.baml` established the controls and persisted enum values. A separate .NET serializer fixture reproduced the `Parameters/KeyValuePair/Key/string` and `Value/string` layout without running the official settings program. Both revisions' `Settings.FPSSettings` getter and setter use the literal **`FPSSettings`**, correcting the earlier `FPPSettings` transcription.

| Hub setting | Key | Persisted choices |
|---|---|---|
| Audio output | `AudioPlaySettings` | `0` WASAPI Exclusive, `1` WASAPI Shared. The enum's `2` has no control and is not offered. |
| Frame rate | `FPSSettings` | `0` Automatic, `1` Fixed 60 fps |
| LED color | `LEDSettings` | `0` Blue, `1` Red, `2` Green, `3` Purple, `4` Light blue, `5` Orange, **`999`** Random |

All three missing-key defaults are `0`. `DisplaySettings` depends on the official monitor enumeration order and is not edited. `LanguageSettings` has no live control in the official UI and no confirmed game consumer, so it is left alone along with key bindings and other parameters. The game image contains the settings key table, but no runtime application of changed settings was tested.

The launcher and reporter both contain the same XML schema reader: `/SettingDatas/Parameters` (`0x1401845a8` launcher, `0x1400db270` er), `KeyValuePair#%d` (`0x140184598`, `0x1400db290`), `/Key/string` and `/Value/string`. The launcher builds the same `config\` + RID + `_cf.xml` path at `0x140083430`, fed with ResourceDir (`0x140088bd0(1)` at callers `0x14002edc6` / `0x140032fc0`). The reporter has a matching path builder at `0x14002be80` (base built by `0x14002c020`) and parser at `0x14002bc00`; the base that builder uses was not fully resolved, so treat the reporter's copy as pattern evidence only.

The launch command does not consume these keys. They are read by the launcher and reporter but no code path adds them to ` -t <token>`; the game presumably reads the XML itself. That last step is not proven.

## 7. Login page and token

| URL | Status (2026-11-21) | Result |
|---|---|---|
| `https://p.eagate.573.jp/game/infinitas/2/api/login/login.html` | 302 | `Location: https://p.eagate.573.jp/gate/p/login.html?path=https%3A%2F%2Fp.eagate.573.jp%2Fgame%2Finfinitas%2F2%2Fapi%2Flogin%2Flogin.html` |
| `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=infinitas` | 200, 11,751 B | generic Konaste login page; contains `通信エラー ERROR-03` and `ページを閉じて、ショートカットよりゲームを再起動してください`; not a token |
| `https://p.eagate.573.jp/game/infinitas/2/startup/system.html` | 200, 26,047 B | Windows 10 64-bit / 11, 8 GB RAM, 60 GB free, DirectX 9.0c redist; data downloads during play |
| `https://p.eagate.573.jp/game/infinitas/2/startup/index.html` | 200, 26,131 B | live nav, `ゲームを始める`, links to `djdata/*`, `howto/*`, `customize/*`, `download/index.html`; assets on `eacache.s.konaminet.jp` |
| `https://p.eagate.573.jp/game/infinitas/2/download/index.html` | 302 | gate login |
| `https://p.eagate.573.jp/game/infinitas/2/djdata/index.html`, `music.html`, `score_download.html` | 302 | gate login |

Neither the public startup page nor the generic Konaste login page contains a `bm2dxinf` literal (0 matches in both fetched bodies); the hrefs are ordinary navigation and a gate-login link. In the live check, the separate signed-in INFINITAS page first required title-specific terms. After acceptance, it offered only trial mode and said that full play requires the INFINITAS Basic Course. No subscription was changed. This verifies the trial presentation for this account, not the appearance or behavior of the full-play control.

The scheme is fixed: `bm2dxinf://login?tk=<token>`, per the registered handler and both parsers. The token slot accepts any string in the binaries; the third-party `mizztgc/infinitas-for-linux` `bin/konaste` (HTTP 200, 36,724 B) checks `[0-9a-f]{64}`, appends `&rel=` when no mode is given, and launches the same `launcher\modules\bm2dx_launcher.exe` under Wine. Cross-checked against the binaries: the MSI URL and hash, the scheme registration, the launch target, and the `bm2dxinf://login?tk=` shape all match. The `&rel=` requirement, the 64-hex shape, and the claim that the login page mints the token were not independently verified. The launcher parser splits on `?`, `&`, and `=` and tests keys `tk`, `dn`, `trial`, `-t`, `--dev`, `--trial`, `-c` (`0x140085779`–`0x140085bd3`); `rel` has no key at any site examined, so `&rel=` is probably ignored, but that is not proven.

## 8. Play data

`djdata/index.html`, `djdata/music.html`, `djdata/score_download.html`, `djdata/comment.html`, `djdata/rival_list.html`, and `music/index.html` are either 302 to the gate or require a session. No play-data contract can be written from anonymous pages. `startup/index.html` and `startup/system.html` are reachable but say nothing about play data formats.

## 9. Hub implementation

The GPUI Hub offers a registered INFINITAS copy in **Add game** when it finds `game/app/bm2dx.exe`. Adding it only changes library membership. **Game settings** reads the registered `ResourceDir` and the current Windows SID's RID, and saves only edited audio, frame-rate, and LED values. Settings never starts a vendor process. **Install launcher** starts the pinned, Authenticode-verified KONAMI MSI through the full Windows Installer wizard after confirmation. The MSI contains no game data and the Hub does not count launcher registration as a complete game installation. Full game-data install, check, repair, native update, and Play remain disabled; there is no automatic launcher or browser handoff.

### 9.1 Reuse

| Hub piece | File:line | Fit |
|---|---|---|
| Registry paths | [`arguments.rs`](../../crates/konaste-runtime/src/arguments.rs) | Reads both paths from the same key, opening HKCU first and HKLM second. |
| Local settings | [`infinitas.rs`](../../crates/konaste-runtime/src/infinitas.rs) | Uses the exact RID file, validates the bounded `SettingDatas/Parameters` XML, retains untouched rows, and checks the original bytes before replacement. |
| Download/verify engine | `docs/research/updater-integration.md` | Reusable for stored content once the source supplies fresh authenticated metadata; it does not discover or sign the index |
| Catalog row | [`crates/konaste-hub/src/catalog.rs`](../../crates/konaste-hub/src/catalog.rs) | Registry and game-presence marker are enabled. Shared runtime family, service login, and launch support stay disabled. |

### 9.2 Adapter scope and remaining pieces

1. **Install**: [Launcher setup](../../crates/konaste-runtime/src/infinitas_setup.rs) implements the pinned MSI and full UI. It checks Windows Installer and the official paths before starting, then checks launcher presence on successful completion. The vendor installer owns the registry and DirectX steps. First-time game-data installation still depends on the update path.
2. **Settings**: the native editor is implemented for a subset of settings. It never selects a different user’s file and preserves unknown keys on save.
3. **Native Play UI**: distinguish terms, trial-only access, and full play where evidenced by the official account flow. Native launch remains blocked on a supported way to complete the official launch and entitlement checks. The official scheme handler remains a reference, not an implicit Hub fallback.
4. **Update**: reuse the verified decoder and transfer engine. Native update remains blocked on a supported source of fresh authenticated metadata and a title-specific admission policy for its plan. Keep validation of a locally installed index read-only.

### 9.3 What stays unknown

The verification payloads and their order, the signature input, the property-image encoding, the `-c` flag's meaning, the `&rel=` parameter, and whether the game validates the token against launcher state.

## 10. Blockers / open questions

1. **Metadata and launch authentication.** The initial WinHTTP trace missed native metadata; the follow-up HAR contains both full-index and hash-only exchanges. No supported replacement-client mechanism for authenticated discovery or launch has been established. The launcher and reporter have their own server checks; a game command line alone does not establish equivalent verification.
2. **Freshness and admission.** The published index's format and hash coverage are proven for this revision. That does not prove the current update on a later date or determine how a native adapter would obtain its authenticated index.
3. **Resume.** The updater observed ETags, but this run did not demonstrate interrupted-transfer recovery or 206 handling. Keep the existing Hub resume validation; do not infer support from a non-weak ETag alone.
4. **Settings and launch.** Whether the installed game reads `_cf.xml` and whether the trial passes launch verification were not tested in the read-only follow-up.
5. **Newer bootstrap.** Only the pinned 2022 MSI was tested. Launcher module replacements are not evidence of a newer MSI.

## 11. Evidence handling

The working capture included account-bearing data. This report retains only aggregates and local file observations. Do not commit raw captures, page bodies, or launcher arguments. Any later observation aimed only at transfer reliability should record status, lengths, timing, and validator presence without recording credentials, process arguments, or response bodies.

## 12. What changed against the 2026-10-16 note

- MSI `Directory`/`File`/`Component`/`Registry`/`Property`/`InstallExecuteSequence` decoded; install layout, HKLM-only writes, and the absence of a game binary are now facts.
- The scheme handler and `"%1"` argument are proven from the MSI, not inferred.
- The launcher is confirmed not to start the game; `bm2dx_er.exe` creates it with `lpDirectory = InstallDir`.
- The game command line is confirmed token-only.
- Settings path and format are confirmed as `%ResourceDir%\config\<RID>_cf.xml` with the Hub's existing XML schema; the RID is the user's SID tail.
- The updater's `getResourceInfo` type, request table, and `ri.bin`/`rid.bin` selection are confirmed.
- The eacnet boot order is confirmed for launcher, updater, and reporter.
- The 2026-10-16 note's claim that `bm2dxinf://login?tk=` "exists as a launcher/updater string" is now tied to concrete parsers in both `bm2dx_launcher.exe` (`0x140085560`) and `bm2dx_er.exe` (`0x140027e70`).
- Its "no caller was shown supplying an account token to this selector" still holds for the services URL, but the token is now proven to flow from the scheme URI into the reporter and then the game command line.

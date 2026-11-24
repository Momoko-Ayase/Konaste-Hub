# Remaining title contracts

Research date: **2026-10-16**. Scope is the five downloadable PC titles other than SDVX and Bombergirl: beatmania IIDX INFINITAS, DanceDanceRevolution GRAND PRIX, pop'n music Lively, GITADORA, and NOSTALGIA.

No official launcher, updater, installer, or game was executed. No login, purchase, browser store, installed tree, or TLS change was used. Packages were downloaded anonymously into one new temporary directory and inspected with 7-Zip listing/extraction and static PE reads. `WinVerifyTrust` used `WTD_UI_NONE`, no revocation check, and `WTD_CACHE_ONLY_URL_RETRIEVAL`.

This does **not** admit any of the five into `konaste-runtime` `Title`. Package strings and a shared DLL version are not a caller ABI. Unknown contracts stay `Unsupported`.

## 1. Official pages are still login-gated

The catalog page still lists the same seven PC download titles, including these five. Each current download page shows the installer agreement and a login control. Anonymous HTML contains no MSI, EXE, or CloudFront URL.

| Title | Public download page observed | Login target in that HTML | Stated download size |
|---|---|---|---|
| INFINITAS | [download/index.html](https://p.eagate.573.jp/game/infinitas/2/download/index.html) | e-amusement login. Anonymous GET returned **302** to `https://p.eagate.573.jp/gate/p/login.html?path=https%3A%2F%2Fp.eagate.573.jp%2Fgame%2Finfinitas%2F2%2Fdownload%2Findex.html`. Not followed. | Not stated on the fetched system page |
| DDR | [download/index.html](https://p.eagate.573.jp/game/eacddr/konaddr/download/index.html) | `/gate/p/login.html?path=http%3A%2F%2Fp.eagate.573.jp%2Fgame%2Feacddr%2Fkonaddr%2Fdownload%2Findex.html` | Installer plus update, about 22 GB |
| pop'n | [download/index.html](https://p.eagate.573.jp/game/eacpopn/lively/download/index.html) redirects to [installer.html](https://p.eagate.573.jp/game/eacpopn/lively/download/installer.html) | `/gate/p/login.html?path=/game/eacpopn/lively/download/installer.html` | Installer plus update, about 6 GB |
| GITADORA | [installer.html](https://p.eagate.573.jp/game/eacgitadora/konagt/download/installer.html) | `/gate/p/login.html?path=http%3A%2F%2Fp.eagate.573.jp%2Fgame%2Feacgitadora%2Fkonagt%2Fdownload%2Finstaller.html` | Installer plus update, about 4 GB |
| NOSTALGIA | [download/index.html](https://p.eagate.573.jp/game/eacnostalgia/op3/download/index.html) | `/gate/p/login.html?path=/game/eacnostalgia/op3/download/index.html` | Size not stated on this page |

Official OS statements on the same date:

| Title | Page | Stated OS |
|---|---|---|
| INFINITAS | [system](https://p.eagate.573.jp/game/infinitas/2/startup/system.html) | Windows 10 64-bit and Windows 11. Storage: at least 60 GB free |
| DDR | [environment](https://p.eagate.573.jp/game/eacddr/konaddr/info/env.html) | Windows 11 only. Storage: at least 40 GB free |
| pop'n | [environment](https://p.eagate.573.jp/game/eacpopn/lively/info/env.html) | Windows 10 64-bit and Windows 11. Storage: at least 20 GB free |
| GITADORA | [environment](https://p.eagate.573.jp/game/eacgitadora/konagt/info/env.html) | Windows 11 only. Storage: 5 GB trial through at least 50 GB with added packs |
| NOSTALGIA | [environment](https://p.eagate.573.jp/game/eacnostalgia/op3/info/env.html) | Windows 10 and Windows 11. Bitness is not stated. Storage: at least 3 GB free |

The [2026-10-14 platform note](konaste-platform.md) correctly said these pages were login-gated. It is no longer true that no anonymous package URL exists: the URLs below answer anonymously. They were **not** linked by the pages fetched on 2026-10-16, and their file dates are 2020–2022. They are historical bootstrap packages, not proof of the current game build.

## 2. Anonymous packages that answered

The URLs were found in the public script [mizztgc/infinitas-for-linux `bin/konaste`](https://github.com/mizztgc/infinitas-for-linux/blob/main/bin/konaste), then fetched directly. Each ranged probe returned HTTP **206** with no redirect. Full GETs returned HTTP **200**, matching `Content-Length`, with no redirect. MSI summary subjects name KONAMI installers. `WinVerifyTrust` returned **0** for all five MSIs. Signer display name on each package: **Konami Amusement Co., Ltd.**

Retained only under `%TEMP%\konaste-remain-vBJRq9`. Not copied into the repository.

| Title | URL | Bytes | SHA-256 | Object `Last-Modified` |
|---|---|---:|---|---|
| INFINITAS | [infinitas_installer_2022060800.msi](https://d1rc4pwxnc0pe0.cloudfront.net/v2/installer/infinitas_installer_2022060800.msi) | 44,212,224 | `9de019339a24e36ae54283bab6cb5602922a4ceae7d1a29d847c28f01e195dc2` | Wed, 08 Jun 2022 05:06:04 GMT |
| DDR | [ddr_installer_2022012601.msi](https://d2el0dli9l0x2p.cloudfront.net/installer/ddr_installer_2022012601.msi) | 32,956,416 | `c938081a9eab7d0a35b14a36d392a8a56a10f379dd3f8f62d2b4a906deef729e` | Mon, 31 Jan 2022 01:38:14 GMT |
| GITADORA | [GITADORA_installer.msi](https://d1omkh45tn6edw.cloudfront.net/inst/GITADORA_installer.msi) | 31,993,856 | `08481302fcb53f88df459b81928ae6158d3f6c18a5b7dbb057ac76af3a11bc2c` | Tue, 15 Dec 2020 06:17:54 GMT |
| NOSTALGIA | [NOSTALGIA_installer.msi](https://d26kzmeiv4899f.cloudfront.net/installer/NOSTALGIA_installer.msi) | 33,095,680 | `f4678d82ed35d502634568892a9171b4e46854b67c13ac901998b3c676adead0` | Mon, 01 Feb 2021 00:16:50 GMT |
| pop'n | [popn_installer_2021021801.msi](https://d1twxdn5j8sbm3.cloudfront.net/installer/popn_installer_2021021801.msi) | 54,095,872 | `e12b940d3a03344bca39e4e2ae8934cdcd4a0e8c9967d90a7cfa3cd09b341cac` | Wed, 03 Mar 2021 00:42:20 GMT |

MSI summary revision is **500** for every package (Unicode). Summary author is KONAMI. Package code is not a product code and is not a runtime pin:

| Package | Summary subject | Package code |
|---|---|---|
| INFINITAS | beatmania IIDX INFINITAS インストーラ | `{0DC977F8-D420-4A0F-9DDF-714ED8C264BA}` |
| DDR | DanceDanceRevolution インストーラ | `{42B292B0-53D4-421F-8FE2-F29B304D7506}` |
| GITADORA | GITADORA インストーラ | `{037E4BDA-BD68-417A-B784-E0C25D91C9C0}` |
| NOSTALGIA | NOSTALGIA インストーラ | `{7F4E58B2-07A5-47F8-A82A-A39CBE7D72DE}` |
| pop'n | pop'n music Lively インストーラ | `{9325F339-E364-4528-B80A-49C87E330623}` |

The WiX `Property` table could not be read through this host's PowerShell COM binding (`StringData` returned empty and `FormatRecord` was not exposed). ProductVersion inside the MSI database is therefore **unknown**. The PE ProductVersion values below were read from extracted executables, not from the MSI Property table.

## 3. INFINITAS first: extracted layout

7-Zip names inside `infinitas_installer_2022060800.msi`. No `libeacnet.dll` member exists. The string `libeacnet` is absent from the MSI bytes. Launcher and updater PE import tables list `avs2-core.dll`, `afp-core.dll`, `afp-utils.dll`, and `winhttp.dll`. They do **not** import `libeacnet.dll`. `bm2dx_er.exe` imports `avs2-core.dll` and `winhttp.dll`.

| Cabinet member | Bytes | SHA-256 | PE ProductVersion | FileVersion | Description |
|---|---:|---|---|---|---|
| `launcher_modules_bm2dx_launcher.exe` | 8,910,728 | `51977e18612c8e957fabec6cce8dfbdb03d4f9c706e8208939eaa4aefac8d6ee` | `P2D:J:A:A:2020122300` | `2.0.1.120` | beatmania IIDX INFINITAS Launcher |
| `launcher_modules_bm2dx_updater.exe` | 8,782,728 | `21deff9644a998520e26b1e603fd4b287670cefcad3fe626a09b0c2898c2a59c` | `P2D:J:C:A:2020122300` | `2.0.1.110` | beatmania IIDX INFINITAS Updater |
| `launcher_modules_bm2dx_settings.exe` | 1,963,472 | `a277052cf5d0aa16f822e1fa3e227fd2fce6056c40d677149f32ff96b7fa6425` | `P2D:J:K:A:2021111700` | `2.0.1.110` | beatmania IIDX INFINITAS Settings |
| `launcher_modules_bm2dx_er.exe` | 8,039,304 | `0e758a4b6aacfafe575b249d5a33f97cf0a99327dee7a0a004fa8ef8290d9b4d` | `P2D:J:D:A:2020122300` | `2.0.1.110` | beatmania IIDX INFINITAS Error Reporter |
| `launcher_modules_avs2_core.dll` | 1,634,184 | `368308269aad94332509071126ee427c0b4ef18ef3b51d8ffbb5712fa067f32c` | `2.17.3 release (r8311)` | same | AVS2 core library |
| `launcher_modules_afp_core.dll` | 1,000,328 | `fc2d5c53e66b85f1a1b0a4e88724cbd948830541209f213443471ca81e9c54bd` | `2.14.20 RC2 release (r3595)` | same | AFP core library |

All of these are PE32+ x64 (`magic 0x20b`). `WinVerifyTrust` returned 0 for the launcher, updater, and `avs2-core.dll`. The launcher and updater export **zero** functions.

ASCII strings in the launcher/updater, not a proven install path:

| String | Where observed |
|---|---|
| `SOFTWARE\KONAMI\beatmania IIDX INFINITAS` | launcher, updater, error reporter |
| `game\app\bm2dx.exe` and `/id/game/app/bm2dx.exe` | launcher |
| `bm2dxinf://login?tk=` | launcher and updater |
| `https://p.eagate.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices` | launcher, updater, error reporter |
| `https://p.eagate.dev.573.jp/game/eac2dx/infinitas/APIEX/servletex2/preProcess/GetServices` | same three, development host |
| `ri.bin`, `rid.bin`, `resource_info/ri_ver` | launcher |
| `/InstallDir/ri.bin`, `/InstallDir/rid.bin` | updater |
| `launcher\modules\bm2dx_updater.exe` | launcher |
| `launcher\modules\bm2dx_launcher.exe` | updater |

The updater also contains the literal query pieces `tk`, `dn`, `trial`, and `bm2dxinf://login?tk=`. It contains `getResourceInfo` and `EacnetRequestResourceInfo`. It does **not** contain `OA043711`, `game_id`, or the launcher ProductVersion as an ASCII boot literal. The ProductVersion exists as UTF-16 in the version resource.

Anonymous GET of the third-party script's INFINITAS login URL, `https://p.eagate.573.jp/game/infinitas/2/api/login/login.html`, returned **302** to the e-amusement login page. Not followed. No `game_id` query is present in the launcher strings or that URL.

### Why INFINITAS is not a `Title` yet

`Title` requires a pinned `libeacnet`/`avs2-core` pair and a verified caller of the `OA0437110000xx` exports. This package has the AVS DLL and an embedded eacnet implementation inside the launcher/updater. It does not have the separate DLL the current worker loads. The boot record, mode value, and export ordinals were not recovered by static string search. Do not map `P2D:J:C:A:2020122300` onto the Bombergirl mode-2 record.

The `.text` section of this `avs2-core.dll` is 709,120 bytes and SHA-256 `5206ffda34c8a860e526152cd7031fb77fd120efeaa2395a83f476742eb81192`. The same `.text` digest appears in the other four packages below. Whole-file hashes still differ. Matching code is not a shared boot ABI.

## 4. DDR, GITADORA, NOSTALGIA, pop'n: shared launcher shape

These four packages do contain `launcher\modules\libeacnet.dll` and `launcher\modules\avs2-core.dll`, plus `launcher.exe`, `updater.exe`, `settings.exe`, `errorreporter.exe`, AFP, and `libsettings.dll`. Game executables are not in the packages. Every inspected module is PE32+ x64. `WinVerifyTrust` returned 0 for each title's `launcher.exe` and `libeacnet.dll`, and for DDR `avs2-core.dll`.

`avs2-core.dll` FileVersion is `2.17.3 release (r8311)` in all four, and the `.text` digest matches INFINITAS. Whole-file sizes and hashes do not match.

| Title | `libeacnet.dll` bytes / SHA-256 | FileVersion | Export count |
|---|---|---|---|
| DDR | 8,742,400 / `aa91b712f2c2ffc63eda73efc53544199299670640ca6b49463c693790f494e0` | `1.3.3.104` | 50, `OA043711000000`–`000049` |
| GITADORA | 8,724,824 / `f016c1f61928124c1f658ef9b3ddba105fccc6cfef489ba5269a6705155812de` | `1.1.0.84` | 47, through `000046` |
| NOSTALGIA | 8,720,744 / `0ceebfbbdfcf862ad877e1eb8741aea7980bd5a5a00c32f2addbe1c2e6cb6e00` | `1.0.1.10` | 46, through `000045` |
| pop'n | 8,724,912 / `59f5ab7f6151b111c9686e5d52a1fc9b737398cee1d85037598773da8d2c8f6c` | `1.1.0.84` | 47, through `000046` |

DDR's export names include every name the current Bombergirl worker calls (`000000`, `000001`, `000002`, `000003`, `000009`, `000026`–`000028`, `000030`–`000032`, `000040`, `000041`). The other three include those same names through `000045`. DDR's `000000` RVA is `0x1A4E0`, which matches the RVA recorded for the Bombergirl pin in [updater-mode bootstrap](updater-mode-bootstrap.md). DDR's `000002` RVA is `0x1A8A0`, not the Bombergirl note's `0x1A820`. GITADORA, NOSTALGIA, and pop'n RVAs differ again. Identical export names are not an ABI proof.

pop'n and GITADORA `libeacnet` section contents hash equal for all 13 sections, while the files differ by 88 bytes and 7,370 byte positions. The later [older-title note](older-title-contracts.md) reduces that difference to the PE checksum, security-directory size, and certificate blob.

Launcher ProductVersion is the version resource of `launcher.exe`. Updater ProductVersion is the version resource of `updater.exe`. These are not interchangeable.

| Title | Launcher ProductVersion | Updater ProductVersion | FileVersion of both | Strings found in both |
|---|---|---|---|---|
| DDR | `VGP:J:B:A:2022012600` | `VGP:J:C:A:2022012600` | `1.0.2.0` | `game_id=ddr`, `konaste.ddr://login?tk=`, `SOFTWARE\KONAMI\DanceDanceRevolution`, `game\modules\ddr-konaste.exe` |
| GITADORA | `U32:J:C:A:2020121600` | `U32:J:D:A:2020121600` | `1.0.0.0` | `game_id=gitadora`, `konaste.gitadora://login?tk=`, `SOFTWARE\KONAMI\GITADORA`, `game\modules\gitadora.exe` |
| NOSTALGIA | `UAN:J:A:A:2021020100` | `UAN:J:C:A:2021020100` | `1.0.0.0` | `konaste.nostalgia://login?tk=`, `SOFTWARE\KONAMI\NOSTALGIA`, `game\modules\nostalgia.exe`. No `game_id=` string in the launcher |
| pop'n | `U39:J:B:A:2021020900` | `U39:J:A:A:2020110400` | launcher `0.1.0.1`, updater `2.0.1.100` | `game_id=popn-music`, `konaste.popn-music://login?tk=`, `SOFTWARE\KONAMI\pop'n music Lively`, `game\module\popnLively.exe` |

Login URLs embedded in the launchers:

- DDR: `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=ddr`
- GITADORA: `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=gitadora`
- pop'n: `https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=popn-music`
- NOSTALGIA launcher: no `login.html` string. The third-party script uses `game_id=NOSTALGIA`.

Anonymous GETs on 2026-10-16, not followed when redirected:

| Request | Result |
|---|---|
| `game_id=ddr` | **302** to `/gate/p/login.html?path=/game/konasteapp/API/login/login.html%3fgame_id%3dddr` |
| `game_id=gitadora` | **302** to the same login path for `gitadora` |
| `game_id=popn-music` | **302** to the same login path for `popn-music` |
| `game_id=nostalgia` | **302** to the same login path for `nostalgia` |
| `game_id=NOSTALGIA` | **200**, 11,854 bytes, HTML body text `通信エラー ERROR-03` and an instruction to close the page and restart from the shortcut |

Case differs for NOSTALGIA. Neither result is a token. The uppercase URL is not a working anonymous login.

All four launchers contain `ri.bin`, `rid.bin`, and `resource_info/ri_ver`. All four updaters contain `/InstallDir/ri.bin`. No CloudFront content host or `ResourceInfo/` URL was present in these launcher/updater strings. `libeacnet` in each package contains `https://p.eagate.573.jp/game/konasteapp/APIEX/servletex/pre_process/GetServices` and the `p.eagate.dev.573.jp` counterpart. That is the service-list URL, not a resource-index URL.

pop'n's installed game path uses `game\module\`, singular. The other three use `game\modules\`. INFINITAS uses `game\app\`. These are string facts, not verified install roots.

## 5. What remains unsupported

No title here has a verified caller for the 0xA0 boot record, AVS property tree, or `OA0437110000xx` sequence. The following stay unknown and must not be filled from Bombergirl:

- Which ProductVersion the updater writes at boot offset `+0x45` for these binaries.
- Whether mode 2 is accepted, and whether resource exports `000026`–`000028` have the same completion values.
- INFINITAS's embedded eacnet calling convention. It is not the loaded `libeacnet.dll` contract.
- Registry value names. The packages were not installed. A third-party registry file uses `InstallDir` under the key names above; that file is not an official source.
- Current content hosts, resource-index URLs, and module/index paths after a real updater run. `ri.bin` is a filename string, not proof that the index path is `ri.bin` at the install root.
- Whether these 2020–2022 bootstrap packages can update to the current public game. Official pages still gate the current installer behind login.

`konaste-runtime::title` should continue to return `Unsupported` for all five until one title has a pinned DLL pair and a statically matched caller, then a bounded live metadata check. INFINITAS is the wrong first port of the current DLL worker: its package has no `libeacnet.dll`. DDR is the closest DLL-shaped candidate, and it is still not admitted.

# GITADORA, pop'n Lively, and NOSTALGIA caller contracts

Research date: **2026-10-16**. Static evidence only. No official launcher, updater, installer, game, or extracted PE was executed. No account, browser credential, network request, or installed-tree write was used. Shared IDA was not opened.

The current runtime in `crates/konaste-runtime/src/title.rs` now admits all three discovery profiles and installation descriptors. `launch` is `None` for each, so installation does not authorize creation. The later module identities are in [multi-title integration](multi-title-integration.md), not in these 2020–2022 caller binaries.

Primary files are the extracted cabinet members retained at `%TEMP%\konaste-remain-vBJRq9\x\{gitadora,popn,nostalgia}`. Package identity is the 2026-10-16 survey in [remaining title contracts](remaining-title-contracts.md). Image base is `0x140000000` for the executables and `0x180000000` for the DLLs. All inspected modules are PE32+ x64.

## 1. Pinned bootstrap pair

7-Zip extracted each MSI through its embedded `Sample.cab` and flattened the cabinet names. The original names below are the long names recorded by the package survey, not names created by extraction.

| Title | File | Cabinet long name | Bytes | SHA-256 |
|---|---|---|---:|---|
| GITADORA | `launcher_modules_libeacnet.dll` | `launcher\modules\libeacnet.dll` | 8,724,824 | `f016c1f61928124c1f658ef9b3ddba105fccc6cfef489ba5269a6705155812de` |
| GITADORA | `launcher_modules_avs2_core.dll` | `launcher\modules\avs2-core.dll` | 1,634,136 | `c380e64b89bc2785cb452e7ef97ce77b67e606ce5d28859fe95d6319d7109778` |
| pop'n | `launcher_modules_libeacnet.dll` | `launcher\modules\libeacnet.dll` | 8,724,912 | `59f5ab7f6151b111c9686e5d52a1fc9b737398cee1d85037598773da8d2c8f6c` |
| pop'n | `launcher_modules_avs2_core.dll` | `launcher\modules\avs2-core.dll` | 1,634,224 | `01f18b206d7cc1b7af40187daf5ea766ceeed026adfafe22431ce3912e9f803a` |
| NOSTALGIA | `launcher_modules_libeacnet.dll` | `launcher\modules\libeacnet.dll` | 8,720,744 | `0ceebfbbdfcf862ad877e1eb8741aea7980bd5a5a00c32f2addbe1c2e6cb6e00` |
| NOSTALGIA | `launcher_modules_avs2_core.dll` | `launcher\modules\avs2-core.dll` | 1,634,664 | `6f4f9f5882a4f9f7326d58047114e71e9c92331a2c74dfe7be672e5c19ec9efc` |

`avs2-core.dll` FileVersion and ProductVersion are `2.17.3 release (r8311)` in all three. All seven sections have identical RVA, raw size, and SHA-256 across the three files. The whole files differ. Export base is 1. The ordinals used by the updater have the same RVAs in every copy:

| Ordinal | Name | RVA |
|---:|---|---:|
| 145 | `XCgsqzn0000090` | `0x1D180` |
| 146 | `XCgsqzn0000091` | `0x1D480` |
| 159 | `XCgsqzn000009E` | `0x193C0` |
| 162 | `XCgsqzn00000A1` | `0x1B600` |
| 163 | `XCgsqzn00000A2` | `0x19C00` |
| 171 | `XCgsqzn00000AA` | `0x1C930` |
| 298 | `XCgsqzn0000129` | `0x2F300` |

GITADORA and pop'n `libeacnet.dll` have identical contents in all 13 sections. After zeroing the PE checksum and the security-directory size, the bytes before the certificate are identical. The only other differences are the certificate blob and its recorded size: GITADORA certificate is 7,512 bytes, SHA-256 `a047d0b70dd8a1d1876b63433c50872b822eb0f01c3148900e68a9e9198ebeb2`; pop'n certificate is 7,600 bytes, SHA-256 `d4b776fd1a59dce52cc58a31c829d28503544b644a46aec8710d43b118930256`. This corrects the package survey, which left that difference unreduced. Their export RVAs are therefore the same:

| Export | RVA |
|---|---:|
| `OA043711000000` | `0x1A230` |
| `OA043711000001` | `0x1A4F0` |
| `OA043711000002` | `0x1A590` |
| `OA043711000003` | `0x1D3D0` |
| `OA043711000009` | `0x1D3F0` |
| `OA043711000026` | `0x1C400` |
| `OA043711000027` | `0x1C4D0` |
| `OA043711000028` | `0x1C720` |
| `OA043711000030` | `0x1B8D0` |
| `OA043711000031` | `0x1B9C0` |
| `OA043711000032` | `0x1BB80` |
| `OA043711000040` | `0x1BDE0` |
| `OA043711000041` | `0x1BF10` |

NOSTALGIA `libeacnet.dll` is not that image. Its code section is `.ecode`, 487,424 raw bytes, not `.link`. The same exports are 496 bytes lower (`0x1A040`, `0x1A300`, `0x1A3A0`, `0x1D1E0`, `0x1D200`, `0x1C210`, `0x1C2E0`, `0x1C530`). The `.pdata` bodies of boot, send, poll, and copy have the same lengths as GITADORA (698, 201, 115, and 140 bytes) but are not byte-identical. Zeroing DIR64 relocation slots does not make them equal. The first 24 bytes of `OA043711000000` match; 113 later bytes differ in 57 clusters. NOSTALGIA's normal-terminal and resource completion behavior is therefore not proven by GITADORA/pop'n equality.

## 2. Caller executables

These are the callers, not game PEs. No game executable exists in any of the three packages.

| Title | Role | Bytes | SHA-256 | ProductVersion |
|---|---|---:|---|---|
| GITADORA | `launcher.exe` | 1,798,488 | `560d741888186f3491bd7c4e1d9dc537976b6e34b2356fa5c5585a19618e8501` | `U32:J:C:A:2020121600` |
| GITADORA | `updater.exe` | 1,800,024 | `57b0d0c606afb66bcd7092ef91cf9460e87e5ed24adebfe0b9a6dcdc86cf4fa3` | `U32:J:D:A:2020121600` |
| GITADORA | `errorreporter.exe` | 1,129,304 | `4cc864d532e86b85cc5f5d5cdc147214f60059a28a8568c14688797294b1946e` | `U32:J:E:A:2020121600` |
| pop'n | `launcher.exe` | 1,875,808 | `f95495878a347690c465bcd9894c11c0a8aeb6089729dd0680d5ed0a5d6f1d36` | `U39:J:B:A:2021020900` |
| pop'n | `updater.exe` | 1,877,936 | `743dbe2f6e1b1d4dd02403b598c1104c65b12e47ddaa1993d6403cfad192533a` | `U39:J:A:A:2020110400` |
| pop'n | `errorreporter.exe` | 1,225,648 | `0cab221150fcf50a2f3ba92635f340973efeb9514a8241ea66761f332425d2e7` | `U39:J:A:A:2020110400` |
| NOSTALGIA | `launcher.exe` | 1,786,728 | `152500ecd958620a081879e3115d1e79f758ee35ede36ad32d9f4282379467b1` | `UAN:J:A:A:2021020100` |
| NOSTALGIA | `updater.exe` | 1,791,336 | `dc8bd8204a342e0a833ca93ff6149511541619d0b99ce375de6f52abefcfb8db` | `UAN:J:C:A:2021020100` |
| NOSTALGIA | `errorreporter.exe` | 1,104,744 | `ba694aa4d8f588951caa67ef8913aa03be0e345f16269f09d08cfc773e4e07a6` | `UAN:J:D:A:2021020100` |

Launcher, updater, and reporter ProductVersion values are distinct fields. Launcher and updater `.text` sections are different for every pair of titles. No launcher function was proven byte-identical across titles.

## 3. Mode-2 discovery record

All three updaters build and pass the record in one straight-line sequence, `0x1400131A0` through `0x1400133F0`. `.pdata` splits that sequence into fragments beginning at `0x132E0` and `0x133CE`; neither fragment has a separate entry call. `0x1400133F0` is `call [IAT]`, and that IAT slot is `libeacnet.dll!OA043711000000`.

The record is one zeroed `0x98`-byte tail at `[rsp+0x24]`, so the mode dword at `[rsp+0x20]` makes the pointer passed to boot a `0xA0`-byte record:

| Offset | Write | Value |
|---|---|---|
| `+0x00` | `mov dword ptr [rsp+0x20], 2` at `0x14001331C` | mode **2** |
| `+0x04` | `lea rcx, [rsp+0x24]`; `mov edx, 0x41` | game id, capacity `0x41` |
| `+0x45` | `lea rcx, [rsp+0x65]`; `mov edx, 0x15` | client version, capacity `0x15` |
| `+0x5A` | `lea rcx, [rsp+0x7A]`; `mov edx, 0x41` | token, capacity `0x41` |
| `+0x9B` | `mov byte ptr [rbp-0x45], al` at `0x1400133E9` | `dn` |

The id literal differs and is the only intended difference in this record:

| Title | Address of the id LEA | Literal |
|---|---|---|
| GITADORA | `0x140013315` | `gitadora` |
| pop'n | `0x140013315` | `popn-music` |
| NOSTALGIA | `0x140013315` | `nostalgia` |

The version source is `[rbx+0xD0]` at `0x14001333B`. That field is written at `0x14001298A` from the buffer filled by `0x14001293B`. The reader copies `0x15` bytes from `VerQueryValue` of `\StringFileInfo\041104b0\ProductVersion`. Its path is the 260-byte result of `GetModuleFileNameA(NULL)`. This is the updater executable's own ProductVersion, not a game PE version:

| Title | Version copied to `+0x45` |
|---|---|
| GITADORA | `U32:J:D:A:2020121600` |
| pop'n | `U39:J:A:A:2020110400` |
| NOSTALGIA | `UAN:J:C:A:2021020100` |

The token is not a constant. `0x14001335C` calls `0x140042510` (pop'n `0x1400424E0`, NOSTALGIA `0x140042490`), returns the command object, and slot `+0x10` supplies the string copied to `+0x5A`. `dn` is a second call to that getter, using slot `+0x18`, stored as one byte. No literal `public-metadata` is present. Empty-token behavior was not established.

The same three updaters then call the resource exports by IAT at the same instruction addresses and with the same compares. pop'n's AVS boot call is `0x30` earlier but uses the same immediates. None of the three updaters imports `OA043711000009`, so this caller does not test boot result 1.

| Call | Address in all three | Caller branch |
|---|---|---|
| `OA043711000000` | `0x1400133F0` | `test eax, eax`; zero continues, nonzero is the error path at `0x140013426` |
| `OA043711000001` | `0x140013567` | `cmp eax, 2`; equal continues, any other value leaves at `0x14001372E` |
| `OA043711000003` | `0x140013581` after worker state 2 | `test eax, eax`; zero is the success path at `0x1400136D2`. Nonzero, including `0x3F8`, is an error path |
| `OA043711000026` | `0x1400137C3` | no arguments. `test eax, eax`; zero continues, nonzero goes to the error getter at `0x1400137DB` |
| `OA043711000027` | `0x140013B0A` | `cmp eax, 2`; equal continues, any other value leaves at `0x140013EC0` without copying |
| `OA043711000003` | `0x140013B19` after poll 2 | `test ebx, ebx` uses that error value; zero reaches copy, nonzero goes to `0x140013C2E` |
| `OA043711000028` | `0x140013B7A` | URL buffer `0x800` at `[rbp-0x20]`, hash buffer `0x41` at `[rbp-0x70]`. The return is not compared |

Accepted values at these branches are boot 0, worker state 2, error 0, send 0, and poll 2. The copy call has no success test. The copy buffers have those capacities, but the caller does not pass the capacities as arguments. `000009 == 1` is not a condition in this updater. A later download call, `OA043711000033` at `0x140013BE1`, is inside the same function but is outside metadata discovery.

AVS setup is `sub_140017D70` in every updater. It allocates `0x08000000` bytes at `0x140017DB9`, zeroes a `0xD18`-byte descriptor, and calls ordinal 145 as `(0x17, descriptor, 0xD18)`. Ordinal 298 is called as `(config, allocation, 0x08000000, 0, 0, 0)`: GITADORA and NOSTALGIA at `0x140018324`, pop'n at `0x1400182F4`.

The three updater callers therefore admit the same metadata operation: boot 0, worker state 2 with error 0, resource send 0, one poll whose accepted completion is 2 followed by error 0, then one copy into an `0x800` URL buffer and an `0x41` hash buffer. They do not admit a `000009 == 1` test. No CloudFront host or `ResourceInfo/` URL is in these updater strings, so a profile can carry the pinned pair, game id, and updater version only. The resource host and prefix remain absent.

## 4. Mode-1 launcher record

Each launcher has one call to `OA043711000000`: GITADORA `0x140015146`, pop'n `0x140014F4A`, NOSTALGIA `0x140014F6A`. The record fields are the same shape as mode 2:

| Title | Clear | Mode | Id | Version capacity | Token capacity | `dn` |
|---|---|---|---|---|---|---|
| GITADORA | `0x14001506A`, `r8d=0x98` | `0x140015079`, dword 1 | `gitadora` | `0x1400150AE`, `edx=0x15` | `0x1400150E5`, `edx=0x41` | `0x14001513F` |
| pop'n | `0x140014E6E` | `0x140014E7D`, dword 1 | `popn-music` | `0x140014EB2` | `0x140014EE9` | `0x140014F43` |
| NOSTALGIA | `0x140014E8E` | `0x140014E9D`, dword 1 | `nostalgia` | `0x140014ED2` | `0x140014F09` | `0x140014F63` |

The version is `[rdi+0xD0]` or `[r8+0xD0]`. Launcher initialization writes that field from the same self-version getter: GITADORA `0x1400144D4` through `0x1400144F4`, pop'n `0x140014404` through `0x140014424`, NOSTALGIA `0x140014424` through `0x140014444`. The source is `GetModuleFileNameA(NULL)` followed by the `0x15`-byte ProductVersion copy. The launcher versions are therefore:

| Title | Launcher client version |
|---|---|
| GITADORA | `U32:J:C:A:2020121600` |
| pop'n | `U39:J:B:A:2021020900` |
| NOSTALGIA | `UAN:J:A:A:2021020100` |

The token and `dn` come from the command object. Token slot `+0x10` is copied to `+0x5A`. Slot `+0x18` returns the byte stored at record `+0x9B`. The command parser rebuilds ` -t `, optional ` --dev`, and optional ` --trial` from `tk`, `dn`, and `trial`. That reconstruction is visible at GITADORA `0x140041BE4`, with the same literals in the other two launchers. The exact command-object field numbers used by the vtable calls were not recovered.

## 5. Local paths and process arguments

`InstallDir` and `ResourceDir` are registry value names in all three. GITADORA reads them through `0x140045410`, which calls `0x140046830` for each name. The install-key selection is `0x140045470`: argument 0 returns `SOFTWARE\KONAMI\GITADORA`, argument 1 returns another key pointer. pop'n and NOSTALGIA have the same function shape and their own key strings. HKCU-before-HKLM order and the 260-byte cap were not re-derived instruction by instruction for these three binaries.

Local index selection is the same in all three launchers. GITADORA `0x1400161A0` calls command slot `+0x18`. Nonzero selects `rid.bin`; zero selects `ri.bin`. It formats `%s%s` with the install directory and that tail into a 260-byte buffer, then calls `CreateFileA` with desired access `0x80000000`: GITADORA at `0x140016212`, pop'n at `0x140015F82`, and NOSTALGIA at `0x140015FA2`. The tail selection itself is pop'n `0x140015F33` and NOSTALGIA `0x140015F53`. The proven local path is therefore `<InstallDir> + "ri.bin"` on the normal zero `dn` branch, or `rid.bin` when that command byte is nonzero. It is not a virtual `/InstallDir/ri.bin` path.

The updater's `/InstallDir/ri.bin` string is a mount source, not that local file path. GITADORA `0x14001CC57` searches an AVS node, chooses `ri.bin` or `rid.bin`, and passes the resulting 260-byte path to AVS file calls. Its filesystem location is not proven.

Launcher creation of the reporter is GITADORA `0x14001BD50`, called from `0x14001883C`. The caller first writes byte 1 at `0x140018823`. The callee appends the 34-byte tail `launcher\modules\errorreporter.exe` to the install directory and calls `0x140045D50`. That wrapper sets `cbSize=0x70`, verb `open`, show 1, file from `rdx`, parameters from `r8`, and directory from `r9`. Mask starts at `0x40` and becomes `0x100` when the passed byte is nonzero, so this caller requests `0x100`. pop'n and NOSTALGIA have the same flag byte: `0x1400183ED` and `0x14001840D`. Their ShellExecute wrapper addresses were not separately disassembled.

The reporter then creates the game image. All three use `cbSize=0x70`, mask `0x40`, verb `open`, and show 1:

| Title | Shell call | File tail and copy length | Directory and parameters |
|---|---|---|---|
| GITADORA | `0x14000D1D8` | `modules\gitadora.exe`, 20 bytes (`0x14`) at `0x140001FA5` | `r9` at `0x14000204E` is the buffer started from `InstallDir`; `r8` is the rebuilt command |
| pop'n | `0x14000CAE8` | `game\module\popnLively.exe"`, 27 bytes (`0x1B`) at `0x140001709` | `r9` at `0x1400021FA` is `[rbp+0x138]`, the same buffer; `r8` command, `rdx` file |
| NOSTALGIA | `0x14000A068` | `modules\nostalgia.exe`, 21 bytes (`0x15`) at `0x1400016D9` | `r9` at `0x140001747` is `[rbp+0x0F]`, the same buffer; `r8` command, `rdx` file |

`lpDirectory` is one mutable buffer, not a separate game directory. Argument 0 of GITADORA `0x14000D080`, pop'n `0x14000C990`, and NOSTALGIA `0x140009F10` returns the `InstallDir` registry buffer. The create function appends five bytes `game\` at GITADORA `0x140001EE6` and NOSTALGIA `0x14000161C`, or four bytes `game` at pop'n `0x140001C0B`. The resulting buffer is the `r9` directory argument. The game-file tail is copied to a different buffer. `SetCurrentDirectoryA` is a separate startup call and is not `lpDirectory`. Whether the registry value already ends in a separator is not tested here, so the joined bytes are `<InstallDir bytes> + game\` or, for pop'n, `<InstallDir bytes> + game`.

The pop'n tail includes the trailing quote byte. That is the literal C string and the length passed to the copy; it is not a transcription artifact. The command is rebuilt from the token as ` -t ` plus the optional suffixes before ShellExecute. No reporter imports `CreateProcess`.

Launcher strings for the game image, distinct from the reporter tails, are:

| Title | Local tail | Virtual tail |
|---|---|---|
| GITADORA | `game\modules\gitadora.exe` | `/id/game/modules/gitadora.exe` |
| pop'n | `game\module\popnLively.exe` | `/id/game/module/popnLively.exe` |
| NOSTALGIA | `game\modules\nostalgia.exe` | `/id/game/modules/nostalgia.exe` |

pop'n is singular `module` in both the launcher string and the reporter string. GITADORA and NOSTALGIA are plural `modules`. The virtual string is not the `CreateFile` or ShellExecute path.

## 6. Current admission boundary

The caller ABI above is the historical bootstrap contract. It is not the current game identity. Fresh mode-2 discovery, the production decoder, and the installation descriptors are now admitted. `title()` returns all three. `launch()` remains unsupported because each descriptor sets `launch: None`.

The authoritative hosts and prefixes are `title.rs`, with the checked receipts in [multi-title integration](multi-title-integration.md):

| Title | Host | Resource prefix | Content prefix | Index path |
|---|---|---|---|---|
| pop'n | `d1twxdn5j8sbm3.cloudfront.net` | `/ResourceInfo/` | `/Updater/` | `ri.bin` |
| GITADORA | `d1omkh45tn6edw.cloudfront.net` | `/data/list/` | `/data/` | `ri.bin` |
| NOSTALGIA | `d26kzmeiv4899f.cloudfront.net` | `/data/list/` | `/data/` | `ri.bin` |

The fresh indexes and their main modules, read but not executed:

| Title | Index revision | Files | Declared stored bytes | Index SHA-256 | Module bytes | Module SHA-256 | Module ProductVersion |
|---|---:|---:|---:|---|---:|---|---|
| pop'n | 2026080500 | 4,031 | 5,568,649,986 | `9a94d58a8d3d23708ad4110978a2f4925d7d815230bd5c15ae4ed8e94ee42a0c` | 8,528,512 | `dd3983dc1ce2de03d86207287f2a97cc0bf2cef93d1a3e55ddcaf59c2c939502` | `U39:J:A:A:2026080500` |
| GITADORA | 2026062300 | 1,597 | 4,504,906,876 | `95c09deae96b790f69001fee6ee54b7abf84b318d49df13cef0ac162f7192ce7` | 4,245,096 | `1c3d888e1a2fff5637cfd28e18b82188fddc36aa30a8caad4bc24e66e11edd09` | `U32:J:A:A:2026062300` |
| NOSTALGIA | 2023112800 | 3,663 | 1,009,711,542 | `5256582a366d2028da7488f1c1fde8ab04f50e727eb82af1631a6403faccf1c0` | 12,399,208 | `328d0edacc7229ddf0132d447e84d39e72656bb4d740a48954de327c39453657` | `UAN:J:B:A:2023112800` |

The module paths are `game/module/popnLively.exe`, `game/modules/gitadora.exe`, and `game/modules/nostalgia.exe`. These ProductVersion values belong to those checked files. They are not the launcher or updater versions in the 2020–2022 packages.

Two production updater samples passed per title, with verified stored bytes 8,528,517 for pop'n, 4,245,101 for GITADORA, and 12,399,213 for NOSTALGIA. That is not a full installation, authentication, or game launch. Registry open order, command-field numbers, and NOSTALGIA's internal eacnet completion values remain unresolved. Nothing here authorizes a patch, bypass, or substitution of one title's pair or version for another.

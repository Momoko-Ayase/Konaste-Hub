# Bombergirl game-process creation contract

Date: **2026-10-16**. Static evidence from the pinned official launcher and error reporter. A temporary offline extraction helper was built with two jobs; no official executable, DLL, game, or authenticated call was run for this research. The requester subsequently reported successful actual Bombergirl launch through the implemented handoff; that interactive acceptance is recorded below, separately from the prior read-only Verify result and the static evidence.

## Primary input

| Field | Value |
|---|---|
| Binary | Pinned signed `launcher.exe`, 1,873,384 bytes |
| SHA-256 | `2e5ccec5c14797c8cda662eb4eebe89086ad761d1a8ff34a53713a65322f66bc` |
| MD5 | `12d2a212c2168b1f9c3b70d2b03b10e1` |
| Image base used below | `0x140000000` |
| PDB path string | `D:\Project\BomberGirl\konasta\Launcher\build\launcher_x64Release\out\launcher.pdb` at `0x140139834` |

`libeacnet.dll` was not opened. Heartbeat lifetime below is only what this launcher calls. The paired library pin remains `a8cdc52b3eec6002011b2742f4cea84c01770e3d4ee7bb12ea7cacb3f21e2203` from [updater-mode evidence](updater-mode-bootstrap.md).

## Creation call

The launcher imports `ShellExecuteA` and `ShellExecuteExA`. It does not import `CreateProcessA/W`, `ShellExecuteW`, `ShellExecuteExW`, job-object APIs, or `OpenProcess`.

Normal game creation is `GameAppCaller::call` at `sub_14001B720`. Its only caller is `sub_140018280`. That state is scheduled solely by `sub_1400179B0` when `OA043711000041` has returned 2 and `OA043711000003` is 0. A nonzero `checkGameStart` result never reaches this caller.

`sub_14001B720` builds two strings and calls generic `AppCaller` `sub_140045620(handleSlot, file, parameters, directory, flag)`:

| `ShellExecuteExA` field | Value |
|---|---|
| `cbSize` | 112 (`0x70`) |
| `fMask` | `SEE_MASK_NOCLOSEPROCESS` (`0x40`) when `flag == 0`; `SEE_MASK_NOASYNC` (`0x100`) when `flag != 0` |
| `hwnd` | null |
| `lpVerb` | `"open"` (`0x14012F260` region; literal at the call) |
| `lpFile` | `<InstallDir> + "launcher\\modules\\errorreporter.exe"` (34 bytes at `0x14012BA28`) |
| `lpParameters` | fresh command string from `CommonCommandLinesImpl` vtable slot 7 |
| `lpDirectory` | `InstallDir` exactly, including a missing trailing separator |
| `nShow` | `SW_SHOWNORMAL` (1) |
| `hProcess` | stored into the scene slot only when `flag == 0` and the call succeeds |

The game path always passes `flag = 1` (`sub_140018280` writes a stack byte 1). Therefore the launcher call uses mask `0x100`, not `0x40`, and discards `hProcess`. In the installed `windows-sys 0.61.2` constants, `0x100` is `SEE_MASK_NOASYNC` (256), not `SEE_MASK_FLAG_NO_UI` (`0x400`, 1024). The call waits until the reporter process has been created; it does not suppress Shell error UI, and it does not retain the reporter handle. `GetLastError` is logged as `failed to ShellExecuteEx:%d` at `0x14012C700`, under the copied tag `UpdateAppCaller`, and the function returns 0. Success returns 1. There is no second launcher creation call, no elevation (`"runas"` is updater-only), and no stdout/stderr redirection.

`errorreporter.exe` is the file ShellExecute opens. The game executable is not `lpFile`. The reporter, not the launcher, creates `modules\bomber.exe`, as traced below.

## Executable path

`InstallDir` is a `REG_SZ`, at most 260 bytes including its NUL, read by `sub_140044CE0` / `sub_140046100`:

1. `HKCU\SOFTWARE\KONAMI\BOMBERGIRL`
2. Only if that open fails, `HKLM\SOFTWARE\KONAMI\BOMBERGIRL`

Access is `KEY_READ` (`0x20019`). The value is copied unnormalized into a 260-byte buffer at `unk_1407B7700` (`sub_140044D40(0)`). No trailing separator is added. `ResourceDir` (`unk_1407B7804`) is not used for creation.

Both the module check (`sub_140016930`) and game creation (`sub_14001B720`) concatenate that raw value with a relative tail:

| Use | Tail | Result |
|---|---|---|
| Module hash / version | `modules\bomber.exe` (18 bytes, `0x140127BD8`) | file whose ProductVersion and SHA-256 were already checked |
| Game creation file argument | `launcher\modules\errorreporter.exe` | the process ShellExecute opens |
| Game creation parameter | same `InstallDir` plus the command string below | `lpDirectory` is the raw install value |

If `InstallDir` has no trailing `\` or `/`, the joined path is wrong. The official code does not test or repair that. The virtual `/id/modules/bomber.exe` (`0x140127C08`) is only the hash-compare mount, not a creation path.

## Arguments and the token

`WinMain` (`0x14001C270`) passes `lpCmdLine` to `CommonCommandLinesImpl::parse` (`sub_1400410F0`, vtable `0x14012EF10` slot 1) with mode 1. That mode splits on `?` and `&`, then on `=`.

On a well-formed callback it stores:

| Object field | Source | Game argv contribution |
|---|---|---|
| `+8` | full callback text | not passed to the game |
| `+72` | value of query key `tk` | the token |
| `+104` | key `dn` present | if nonzero, append ` --dev` |
| `+105` | key equal to `trial` | if nonzero, append ` --trial` |
| `+40` | rebuilt only for game/updater parameters | ` -t ` + token [+ ` --dev`] [+ ` --trial`] |

The rebuild is at `0x1400414A4`–`0x140041588`: literal ` -t ` is 4 bytes (`0x14012EC90`), ` --dev` is 6 (`0x14012EC98`), ` --trial` is 8 (`0x14012ECA0`). There is no quoting, escaping, or extra separator. The token is the raw `tk` value. A token containing spaces or quotes is not rejected here.

Malformed callbacks return 3 (no `&` pairs), 4 (bad pair), or 5 (`tk` without a value). `WinMain` then shows an error and does not start the UI. An absent `tk` key does not set those codes; the token string stays empty and ` -t ` is still built. No maximum token length is enforced in this parser. The separate eacnet boot copy uses a 65-byte destination, but that limit is not applied again at game creation.

`lpParameters` is this `+40` string, not the callback URI. The token therefore **does** reach the child command line. It is not placed in an environment variable by launcher code.

Mode 0 of the same parser is the opposite direction: it tokenizes an existing argv (`-t`, `--dev`, `--trial`, `-c`) and rebuilds `konaste.bomber-girl://login?tk=` plus optional `&dn=` and `&trial=`. Game creation does not call mode 0. `-c` sets `+106` and is consumed by the updater caller (`sub_14001D760`), not by the game caller.

## Directory, environment, flags

Before the UI exists, `sub_1400230A0` reads the current directory (260-byte cap) and compares it with `InstallDir`. On mismatch it calls `SetCurrentDirectoryA(InstallDir)`. Failure codes are launcher startup errors 4–6, not game-creation errors. The later game `lpDirectory` is still the raw registry string, not a fresh `GetCurrentDirectory` result.

`SetEnvironmentVariableW` and `GetEnvironmentStringsW` occur only in CRT code (`0x1400ED000`–`0x1400F9600`). Launcher game creation does not build or replace an environment block. ShellExecute therefore inherits the launcher environment. Nothing in this binary adds the token, install path, or eacnet handle to that environment.

| Behavior | Game call | Not used for the game |
|---|---|---|
| Verb | `open` | updater may use `runas` |
| Show | `SW_SHOWNORMAL` | launcher window itself is `WS_POPUP \| WS_VISIBLE` (`0x90000000`) at `sub_140022BA0` |
| Mask | `SEE_MASK_NOASYNC` (`0x100`) | `SEE_MASK_NOCLOSEPROCESS` (`0x40`) only when the caller asks to keep the handle; `SEE_MASK_FLAG_NO_UI` (`0x400`) is not used |
| Elevation | none | no manifest behavior was inferred |

`nShowCmd` from `WinMain` is ignored. `sub_140022630` forces windowed launcher mode (`byte_14015AA14 = 0`) before UI startup.

## Handles and IPC

Because the game flag is 1, `hProcess` is not saved. The scene still has a game-handle slot at object `+0x158` (344). Its destructor `sub_140013A60` calls `sub_1400455F0`, which `CloseHandle`s a non-null value. On the normal success path that slot remains null, so there is nothing to close and no process to signal.

No pipe, file mapping, job object, inherited handle list, or game-specific IPC setup exists at this call. `ShellExecuteExA` does not receive `SEE_MASK_NOASYNC`, a class, or an ID list. Whether `errorreporter.exe` then creates `bomber.exe`, and whether those two processes inherit handles from each other, is outside this binary.

The other handle slot, `+0x168` (360), belongs to `UpdateAppCaller` (`sub_14001D760`). That path can request `SEE_MASK_NOCLOSEPROCESS` and is waited on by `sub_14001DA30`. It is not the game path. `settings.exe` (`sub_14001C590`) uses the same `AppCaller` with an empty parameter string, flag 1, and handle slot `+0x178`.

## Wait, exit, and launcher lifetime

`sub_140018280` on success:

1. Sets status `ゲーム開始` (`qword_14015A8C0`).
2. Calls `GameAppCaller`.
3. On success, sets status `ゲーム開始成功` (`qword_14015A8E0`) and schedules `sub_140018490`.
4. On failure, sets status `ゲームの起動に失敗しました` (`qword_14015A900`) and schedules the terminal state `sub_140018BD0`.

`sub_140018490` sends `WM_CLOSE` (`0x10`) to the launcher window through `sub_140019110`, then schedules `sub_140016700`. The window procedure `sub_140022AF0` sets `byte_1407B755C` on `WM_CLOSE` and on `WM_DESTROY`. It does not call `DestroyWindow` itself for `WM_CLOSE`. The frame loop `sub_140022810` exits once that flag is set. Teardown `sub_1400227E0` calls `OA043711000002` exactly once, via `sub_1400235B0`, then releases UI/D3D objects.

There is no `WaitForSingleObject`, `MsgWait`, `GetExitCodeProcess`, or `TerminateProcess` on the game slot. `MsgWaitForMultipleObjectsEx(1, handles, INFINITE, QS_ALLINPUT|QS_ALLPOSTMESSAGE, 0)` exists only in updater wait `sub_14001DA30`. The launcher therefore does not wait for `bomber.exe` or `errorreporter.exe`. A successful ShellExecute starts launcher shutdown on the next state tick (`sub_140019330` from per-frame `sub_1400148A0`).

Closing the launcher does not explicitly terminate a game process. If ShellExecute fails, the launcher stays open in `sub_140018BD0`: both buttons disabled, no next state.

Game-process exit is not observed. Status `ゲームが終了しました` (`qword_14015A940`) is initialized at `0x140001614` but has no code reference. No launcher path sets it.

## Heartbeat

`OA043711000001` is polled only by boot-completion state `sub_140015280`. Value 2 there means the boot worker finished; it is not a game-process heartbeat. This launcher never calls a heartbeat export in the game-creation or game-exit path.

`OA043711000002` is the shutdown call above. It is reached when the launcher window loop ends, including the success path's `WM_CLOSE`, the exit button (`sub_140019100`), and the close-box callback. It is not tied to `bomber.exe` exiting. The library's internal heartbeat thread and the effect of `000002` on an already-running game are not visible in this executable.

## Errors that do not create a process

`sub_1400179B0`, after poll result 2:

| `OA043711000003` | Next state | Process |
|---|---|---|
| 0 | `sub_140018280` | game creation |
| 1 or 2005, and `++*(scene+392) <= 5` | `sub_140018D70` | no; re-enters `sub_1400174A0` after `180 * attempt` frame counter ticks at `sub_140018F00` |
| 1 or 2005 after more than 5 attempts | `sub_140018BD0` | no |
| 2004 | `sub_140016700` | no; status is the maintenance string plus `(2004-<000004>)` |
| any other nonzero | `sub_140016700` | no; status is the generic error string plus `(<000003>-<000004>)` |

`sub_1400174A0` treats a zero return from `OA043711000040` as send failure, logs `failed to eacnet_lch_check_game_start`, and goes terminal. It does not create a process. The poll itself is not success; only the later error getter is.

Displayed suffixes use decimal formatting from `sub_140019FA0`. The fixed Japanese prefixes are CP932 literals initialized by `sub_1400012BA`–`sub_14000170A`. Relevant ones: `起動確認中`, `ゲーム開始`, `ゲーム開始成功`, `ゲームの起動に失敗しました`, `ゲームバージョンの確認に失敗しました`, and `エラーが発生しました。ランチャーを終了し、再度ログインしてください。`

Module-check failures (`(002-GFV)`, `(001-GSF)`, `(000-STO)`) happen in `sub_140016930` / `sub_140016E60` before `sub_1400174A0` and also do not create a process.

## Contradictions to older BG notes

- The game child is not created by `CreateProcess`, and `bomber.exe` is not the ShellExecute file. The file is `launcher\modules\errorreporter.exe`.
- A `-t <token>` tail is real, but it is a parameter of `errorreporter.exe`, with working directory `InstallDir`. It is not evidence that this launcher spawns `bomber.exe` directly.
- `checkGameStart` is called (`sub_1400174A0` / `sub_1400179B0`). Claims that the launcher skips it are false for this pin.
- The launcher does not wait for or terminate the game. A successful call closes the launcher UI and then calls `OA043711000002`.
- `ゲームが終了しました` is unused data, not an observed exit handler.

## Implementation contract

1. Create a process only after the same gate as `sub_1400179B0`: fresh check poll consumed with value 2 and error getter 0. Do not reuse a Verify token or treat a Verify receipt as this gate.
2. Use the already-pinned imported install root. Do not read or concatenate the official registry value, and do not reproduce a missing-separator join. The game image is `<imported root>\modules\bomber.exe`.
3. Create that image directly. Do not create `errorreporter.exe`. Parameters are exactly ` -t ` plus the fresh callback token: no `--dev`, no `--trial`, and no other arguments. The token belongs on this game argv and nowhere else.
4. Working directory is the imported root. Inherit the parent environment unchanged. Do not put the token in an environment variable, log, upload, or IPC reply.
5. Verb `open`, show normal, no forced `runas` or elevation bypass; honor the executable's normal Windows manifest behavior. Retain the process handle and wait for it, which is the reporter's observed role. No stdin/stdout pipe, job, hook, or patch.
6. The launcher-side eacnet worker stays alive through this creation, matching the official ordering, and shuts down when the launcher session ends. Do not perform the reporter's post-exit GameLog boot or upload; that upload includes the token and is not a launch prerequisite.
7. On create failure, leave the hub UI open, surface a controlled failure, clean up the SDK attempt, and do not reuse its token. A successful game creation must not be reported as a failed launch merely because subsequent SDK shutdown failed.
8. The official launcher permits at most five retries for error getter 1 or 2005. The hub deliberately fails closed on every nonzero getter, as in its accepted verifier, rather than inventing a retry policy; none creates a process.

## errorreporter.exe handoff

The launcher does not create `bomber.exe`. The pinned MSI's one embedded cabinet, `#Sample.cab` on Media disk 1, has `LastSequence` 88. `File` key `launcher_modules_errorreporter.exe` is sequence 64, long name `errorreporter.exe`, `FileSize` 1,190,888. The decoded member matches that size.

| Field | Value |
|---|---|
| SHA-256 | `b19b7df036cff3a8c6d0a34a9c46b7a72c32cedcf5e83752716f0276be506570` |
| Authenticode | `WinVerifyTrust` 0, UI none, revocation and network lookup disabled |
| FileVersion | `0.1.0.1` |
| ProductVersion | `QBG:J:B:A:2021122100` |
| FileDescription | `BOMBERGIRL Error Reporter` |
| OriginalFilename | `error_reporter.exe` |
| PDB | `D:\Project\BomberGirl\konasta\Launcher\build\errorreporter_x64Release\out\errorreporter.pdb` at `0x1400AE684` |
| Image base | `0x140000000` |

Package hash and signature were checked before decoding. No MSI action or extracted PE was executed. The reporter imports `ShellExecuteExA` once and does not import `CreateProcess`. Its only creation call is `sub_140009FE0`, called only from `CApplication::MainLoop` at `0x14000175F`.

`WinMain` (`0x140003790`) takes the mutex `BOMBERGIRL Error Reporter`. It parses its command line with the same `CommonCommandLinesImpl` mode-1 parser (`sub_1400056E0`, vtable `0x1400AA5B0`). The rebuilt field at object `+0x28` is again ` -t ` + token, then optional ` --dev` and ` --trial`. An empty parse result exits before the game. It does not call eacnet first.

Before `sub_140009FE0`, the reporter does only local setup. `sub_140001410` registers and creates its window, then calls `sub_1400019E0`. That function reads the reporter ProductVersion and FileVersion, reads `HKCU\SOFTWARE\KONAMI\BOMBERGIRL\InstallDir`, else HKLM, into a 260-byte buffer, and sets the current directory to that value when they differ. `ResourceDir` is loaded but is not the game directory. `sub_1400029B0` is AVS/log initialization, not an entitlement or protection call. No eacnet import runs before the game-creation call. A failure in this setup returns without creating `bomber.exe`.

`sub_1400015B0` then calls `ShellExecuteExA`:

| Field | Value |
|---|---|
| `cbSize` | 112 |
| `fMask` | `SEE_MASK_NOCLOSEPROCESS` (`0x40`) always |
| `hwnd` | null |
| `lpVerb` | `"open"` |
| `lpFile` | `<InstallDir> + "modules\\bomber.exe"` (18 bytes, `0x1400A9DC8`) |
| `lpParameters` | the `+0x28` command string, so `-t <token>` reaches `bomber.exe` argv |
| `lpDirectory` | the same raw `InstallDir` buffer |
| `nShow` | `SW_SHOWNORMAL` (1) |
| `hProcess` | stored in the caller slot |

There is no environment block, pipe, job, or inherited-handle setup. `SetEnvironmentVariableW` appears only in CRT functions. The token is not copied to an environment variable. The official file string is a raw concatenation with `InstallDir`; a missing separator would make that path wrong. Reproducing that concatenation is not a required property when the install root is already a trusted, pinned path.

`sub_14000A140` waits with `MsgWaitForMultipleObjectsEx(1, &hProcess, INFINITE, 0x1DFF, 0)`, pumping `QS_ALLINPUT` messages. Exit code `259` (`STILL_ACTIVE`) returns to a 100 ms `avs2_core_8` sleep and waits again. Any other code closes the handle and stores the code at `dword_1400C214C`. The reporter does not terminate the game. `TerminateProcess` at `0x1400704F4` is CRT failure handling, not this path.

A zero exit continues without a dialog. A nonzero exit shows `BOMBERGIRL\n` in a yes/no box (`0x50014`: `MB_YESNO | MB_ICONEXCLAMATION | MB_SYSTEMMODAL | MB_SETFOREGROUND | MB_TOPMOST`). Yes creates dialog 103 and runs `sub_140001120`. No, or a zero exit, skips it. Either way the reporter then returns 0 and `WinMain` releases its mutex.

`sub_140001120` is the only eacnet user. It runs after `bomber.exe` has exited, and only when the user accepts the crash dialog. It is not a pre-launch entitlement gate:

1. Boot record mode is **2**, not the launcher's mode 1. Game id is `bomber-girl`. The 21-byte version is this reporter's own `041104b0\ProductVersion` (`QBG:J:B:A:2021122100`), previously copied to `unk_1400C2150`. The token is the command token. The `dn` byte is the parsed flag.
2. `OA043711000000` must return 0. It polls `OA043711000001` until 2, then requires `OA043711000003 == 0`.
3. `OA043711000036("GameLog")` must return 0. It polls `OA043711000037` until 2 and again requires error 0.
4. It formats `<ResourceDir>work\game\dev\raw\log\default.log` and `exit code : %d, cmd line : %s`. The command line included here is the full `-t <token>` string. `OA043711000038` sends that path and text; `OA043711000039` is polled until 2.
5. `OA043711000002` shuts the library down. The dialog-done flag is set on every exit path, including boot failure.

No `checkGameStart`, module-hash, or game-version export is imported. The reporter does not repeat the launcher's authorization, and nothing before its creation call is an entitlement, protection, or IPC step.

The reporter is the official process that creates `bomber.exe`, retains its handle, and waits. That does not prove the game requires this parent. No pre-launch check inside `bomber.exe` was available to inspect, so a parent requirement is unknown rather than established. Replacing the observed creation directly is consistent with the static evidence: create `modules\bomber.exe` with normal `-t <token>` argv, the install root as the working directory, and a retained wait. Do not launch the reporter and do not perform its optional post-exit GameLog upload. That upload includes the token and is absent unless the game exits nonzero and the user accepts the dialog. No hook or patch is implied. Whether `bomber.exe` itself rejects a missing reporter process remains an unknown game-internal check, not a prerequisite found in these two binaries.

## Implemented handoff and validation

The runtime now prepares a single-use verified session in its own restricted SDK worker, retains that session through game creation, and confirms fresh live SDK health with a fixed CHECK/ALIVE exchange immediately before the parent creates the game. A living process alone is not accepted: native shutdown can remain alive after SDK failure. The parent rechecks installation, fresh token identity, maintenance, an already-running process, and a 20-second admission lifetime. SDK cleanup is bounded to five seconds; it never kills the separately owned game or converts a successful creation into a failure.

The actual process module uses `ShellExecuteExW` with the pinned imported root's `modules/bomber.exe`, the exact normal ` -t <token>` parameter, that root as working directory, `open`, `SW_SHOWNORMAL`, and `NOCLOSEPROCESS | NOASYNC | FLAG_NO_UI`. This is not an official reporter launch, report upload, forced elevation, hook, patch, or parent spoof. The token's presence on normal game argv is unavoidable; it stays out of worker argv, environment, hub logs/settings, and UI state. The retained process handle and installed-file leases last until the owned lifetime ends. Handle cleanup never terminates the game.

Validation passed the Rust tests, including the same private ShellExecuteExW/COM/wait implementation with only a synthetic `cmd.exe` fixture. Those source tests alone are not live game acceptance. The final anonymous protocol-3 discovery retry returned worker failure while the exact login endpoint returned HTTP 503 during daily maintenance at 2026-10-16 05:17 JST; it was not treated as success or bypassed.

### Interactive launch acceptance

On **2026-10-16**, after testing the actual Play implementation, the requester reported: “Game launch verified successfully.” This accepts actual Bombergirl launch in their development app, separately from the earlier no-game Verify action. No credentials, live token, raw SDK trace, or private game log was supplied or captured. No maintainer-run authenticated session or game launch was performed. The report does not separately confirm gameplay, game-exit recovery, or close-to-minimize behavior, and does not certify other titles or account entitlements. Every later Play still requires fresh checks and a new token.

## Remaining gates

- Gameplay, game-exit recovery, and close-to-minimize behavior still need interactive acceptance. The successful launch report is not an inspection of game-internal parent or SDK-shutdown checks; the two researched binaries do not establish such a requirement.
- `bomber.exe` was not opened. Its argv parsing and any internal entitlement or protection expectation are unknown. Do not infer a bypass or a hook.
- The conditional GameLog upload uses exports `000036`–`000039`, which were not decompiled from `libeacnet.dll`. It is not part of the creation contract and must not receive the token.
- Token length is unconstrained under both parsers. The 65-byte boot-token limit comes from the launcher's eacnet record, not from these creation calls.
- The verified game handoff puts the token on `bomber.exe` argv. No environment or IPC alternative appears in either binary.

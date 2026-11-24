# Release

How to build, package, and publish Konaste Hub, and what must be true before a public release.

## Prerequisites

- Windows 10/11 x64.
- Rust with the `x86_64-pc-windows-msvc` target. `rust-toolchain.toml` pins the version; `rustup` installs it on first build. `Cargo.lock` is committed; release builds use `--locked`.
- Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK.
- Inno Setup 6.3 or newer (7 works), only to compile the installer.

`.cargo/config.toml` limits builds to two parallel jobs to keep memory use down.

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --locked -p konaste-hub
```

```powershell
# Build, then stage the executable, LICENSE, notices, and SHA-256 lists in dist/.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1

# Same, then compile the installer. ISCC is found in the standard Inno Setup
# folders or passed with -Iscc. The script never downloads tools.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Installer

# Package an existing target/release/konaste-hub.exe.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -SkipBuild -Installer
```

The version comes from `[workspace.package] version` in `Cargo.toml` and must be `MAJOR.MINOR.PATCH`. Override with `-Version`.

`dist/` then holds `konaste-hub.exe`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `KonasteHub-<version>-setup.exe` (with `-Installer`), and `SHA256SUMS.txt` / `SHA256SUMS.json` covering all of them.

## CI

- `.github/workflows/ci.yml` runs on pushes and pull requests: format check, Clippy with warnings as errors, tests, and a release build on `windows-latest`.
- `.github/workflows/release.yml` runs when a GitHub release is published: checks its tag matches `Cargo.toml`, tests, compiles the installer, and attaches the installer, then `SHA256SUMS.txt`, to that release.

To release: bump `version` in `Cargo.toml` and push it to `main`, then publish a GitHub release tagged `v<version>` (the tag must match the workspace version). Publishing is what ships the update: once the workflow has attached both files, every running Hub offers it within six hours. If the workflow fails, the release stays without assets and is never offered; fix it and re-run the job. Keep the asset names the workflow produces (`KonasteHub-<version>-setup.exe`, `SHA256SUMS.txt`); the Hub ignores a release without both. Mark a test build as a prerelease to keep it from being offered.

No signing credential is used yet. Users will see an unknown-publisher warning; the published SHA-256 lists let them verify downloads.

## Installer

`installer/konaste-hub.iss`:

- Per-user install to `%LOCALAPPDATA%\Programs\Konaste Hub`. No administrator prompt for the Hub itself.
- Refuses to run while any Hub process is running, including one hidden in the notification area and headless runs. Every Hub process holds the named mutex `KonasteHub`; `[Code]` checks it at start and again right before replacing files.
- `/RESTARTHUB` marks a self-update: Setup waits up to 30 s for the Hub that started it to exit, and starts the Hub again when done.
- Prerequisite prompts default to No in silent installs, so `/SILENT` never blocks.
- Registers no URI schemes.
- Uninstall refuses to run while any Hub process is running, removes the program folder, then asks (default No; skipped when silent) whether to delete the data folder. Game installations always stay.
- Checks prerequisites on the Ready page; declining does not abort setup.

### Prerequisites the installer checks

| Prerequisite | Why | Detection |
|---|---|---|
| Visual C++ 2015-2022 Redistributable (x64) | The executable imports `VCRUNTIME140.dll` and `VCRUNTIME140_1.dll`. | Both DLLs in native `System32` and `HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\RUNTIME\X64`. |
| WebView2 Evergreen Runtime | Sign-in runs the official page in a hidden WebView2. The rest of the Hub works without it. | EdgeUpdate client key `{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` under HKCU or HKLM, or an `EdgeWebView\Application` folder. |

If `installer/redist/vc_redist.x64.exe` or `installer/redist/MicrosoftEdgeWebView2RuntimeInstallerX64.exe` exists at compile time, it is embedded and run only after the user agrees. Otherwise the installer opens the Microsoft download page. Check Microsoft's terms before embedding either file. The WebView2 loader is linked statically, so no `WebView2Loader.dll` ships.

## Data

The Hub keeps its state in `%LOCALAPPDATA%\konaste-hub` (see [architecture](architecture.md#data-folder)). The default folder for new games is `<system drive>\Games`. Nothing deletes game installations; only the uninstaller, when the user says Yes, deletes Hub state.

## Clean-machine checklist

Not yet run.

1. Build with `scripts/build-installer.ps1 -Installer` and verify `SHA256SUMS.txt` against `dist/`.
2. On clean Windows 10 and 11 x64: test missing prerequisites with both answers, then install, upgrade, and uninstall. Confirm uninstall with No keeps the data folder, with Yes deletes it, and keeps game folders either way.
3. Sign in with a real KONAMI ID. Test Stay signed in and Sign out across a restart, plus code and passkey sign-in.
4. From an empty data folder, install a title. Interrupt and recover a real update of an older version. Exercise the queue, pause and retry, and Install elsewhere.
5. Play each supported title for the first time without opening game settings first. Then open game settings and check close-to-tray. Cover pop'n without existing registry values, clean SDVX first run, and both GITADORA instruments. INFINITAS must stay unsupported.
6. Confirm stage backgrounds download and the fallback art appears offline.
7. Self-update: with an older version installed and a newer release published, the title bar offers Update. With a game running it is refused; otherwise the Hub closes, Setup replaces it, and the new version starts.

## Distribution gates

- `THIRD_PARTY_NOTICES.md` lists direct dependencies only. A public binary needs notices for the full resolved dependency graph, including the gpui stack, `wry`/`webview2-com`, and any Microsoft runtime redistributed.
- No KONAMI DLLs, installers, game files, or game artwork are bundled. The PASELI and e-amusement marks are embedded; stage backgrounds are served from this repository. Rights for both are unresolved.
- Whether extracting official runtime DLLs on the user's machine is permitted is unresolved.
- No code-signing identity exists yet.

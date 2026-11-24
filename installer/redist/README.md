# Optional installer prerequisites

`scripts/build-installer.ps1` and the release workflow never download files into
this folder. Place these Microsoft installers here only if you have chosen to
redistribute them and have checked the applicable Microsoft terms.

- `vc_redist.x64.exe`: Microsoft Visual C++ 2015-2022 Redistributable (x64).
  Required by `konaste-hub.exe`, which imports `VCRUNTIME140.dll` and
  `VCRUNTIME140_1.dll`.
- `MicrosoftEdgeWebView2RuntimeInstallerX64.exe`: Microsoft Edge WebView2
  Evergreen Standalone Installer (x64). Required for the hidden WebView2 used
  during official sign-in.

When a file is present at compile time, Inno Setup embeds it and asks the user
before running it. Without the file, the installer detects the missing
prerequisite and offers the official Microsoft page instead. Detection is
documented in `docs/release.md`.

Neither file is part of the MIT-licensed source tree. Do not commit them.

# Third-party notices

## Konaste Hub code

Original project code and documentation are licensed under the [MIT License](LICENSE), copyright © 2026 Konaste Hub contributors. That license does not relicense dependencies, KONAMI components, game content, artwork, or trademarks.

## KONAMI material

- The PASELI and e-amusement marks in `crates/konaste-hub/assets/services/` are embedded in the executable to identify those services.
- Stage backgrounds in `assets/backgrounds/` are served from this repository and downloaded by the Hub at runtime.
- Game icons are read, without executing anything, from the official executables of the user's own installation.
- Games and the official launcher components are downloaded from KONAMI's servers on the user's device. None are stored in this repository or the installer.

This material remains the property of its owners and is not covered by the MIT license. Its use has not been cleared with KONAMI; see [release engineering](docs/release.md).

The fallback stage art in `crates/konaste-hub/assets/art/`, the pass card face (`crates/konaste-hub/assets/pass-card.svg`), and the Konaste Hub icon are original project artwork under the project license.

## Direct Rust dependencies

Versions are recorded in `Cargo.lock`. The list covers runtime, build, Windows-only, and test dependencies.

| Component | License | Upstream |
|---|---|---|
| base64 | MIT OR Apache-2.0 | <https://github.com/marshallpierce/rust-base64> |
| cab | MIT | <https://github.com/mdsteele/rust-cab> |
| cookie | MIT OR Apache-2.0 | <https://github.com/SergioBenitez/cookie-rs> |
| embed-resource | MIT | <https://github.com/nabijaczleweli/rust-embed-resource> |
| fs2 | MIT OR Apache-2.0 | <https://github.com/danburkert/fs2-rs> |
| futures-util | MIT OR Apache-2.0 | <https://github.com/rust-lang/futures-rs> |
| gpui-kit (gpui, gpui-component) | Apache-2.0 | <https://github.com/longbridge/gpui-kit> |
| hex | MIT OR Apache-2.0 | <https://github.com/KokaKiwi/rust-hex> |
| image | MIT OR Apache-2.0 | <https://github.com/image-rs/image> |
| msi | MIT | <https://github.com/mdsteele/rust-msi> |
| quick-xml | MIT | <https://github.com/tafia/quick-xml> |
| raw-window-handle | MIT OR Apache-2.0 OR Zlib | <https://github.com/rust-windowing/raw-window-handle> |
| reqwest | MIT OR Apache-2.0 | <https://github.com/seanmonstar/reqwest> |
| roxmltree | MIT OR Apache-2.0 | <https://github.com/RazrFalcon/roxmltree> |
| scraper | ISC | <https://github.com/rust-scraper/scraper> |
| serde, serde_json | MIT OR Apache-2.0 | <https://github.com/serde-rs> |
| sha2 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/hashes> |
| tempfile | MIT OR Apache-2.0 | <https://github.com/Stebalien/tempfile> |
| thiserror | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |
| tokio | MIT | <https://github.com/tokio-rs/tokio> |
| tray-icon | MIT OR Apache-2.0 | <https://github.com/tauri-apps/tray-icon> |
| url | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| windows-sys | MIT OR Apache-2.0 | <https://github.com/microsoft/windows-rs> |
| winreg | MIT | <https://github.com/gentoo90/winreg-rs> |
| wry (webview2-com) | Apache-2.0 OR MIT | <https://github.com/tauri-apps/wry> |
| zeroize | Apache-2.0 OR MIT | <https://github.com/RustCrypto/utils> |

The WebView2 loader is linked statically; no `WebView2Loader.dll` is distributed. Windows SDK, Visual C++ runtime, and WebView2 terms also apply.

This is a direct-dependency inventory. A published binary must also carry the notices its transitive dependency graph requires.

## Trademarks

KONAMI, コナステ / KONASTE, e-amusement, PASELI, KONAMI ID, and the names and marks of the referenced games belong to their respective owners. They are used to identify the games and services this unofficial project works with. Konaste Hub is not affiliated with, endorsed by, or an official product of KONAMI.

No proprietary DLLs, MSIs, game files, reverse-engineering databases, credentials, or session captures are distributed in this repository.

# Konaste Hub

Unofficial Windows launcher for KONAMI's downloadable コナステ games. One gpui executable (`crates/konaste-hub`) on library crates (the other `crates/konaste-*`). Windows 10/11 x64 only.

## Read first

- `docs/architecture.md`: before touching any crate or Hub module; the crate map, data folder, and main flows.
- `docs/spec.md`: before changing checks, Play, sign-in, updater, or which titles are supported.
- `docs/design.md`: before any UI change.
- `docs/release.md`: before touching the installer, `scripts/`, or `.github/workflows/`.
- `docs/research/README.md`: before reverse-engineering or changing how the Hub talks to KONAMI's launcher, DLLs, or servers. Check for an existing note first.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p konaste-hub
```

CI runs all of these with `--locked`; keep them green. `rustfmt.toml` sets a 140-column style. Ignored tests need local official files named in their `#[ignore]` reason.

## Hard rules

- **Games stay with the user.** Never launch a game, start Play, or use a KONAMI account without the maintainer asking. Launch acceptance is done by the maintainer.
- **Registry writes are approved one by one.** The approved set is in `crates/konaste-hub/src/official_setup.rs`; ask before adding another.
- **KONAMI binaries never enter the repo.** Games, MSIs, and official DLLs are downloaded from KONAMI at runtime. `.gitignore` blocks `*.dll`, `*.exe`, `*.msi`, `*.cab`, `*.i64`.
- **Assets have fixed homes.** Service marks (PASELI, e-amusement) are embedded from `crates/konaste-hub/assets/services/`. Stage backgrounds live in `assets/backgrounds/<game>.jpg` and the Hub downloads them from this repo's `main` branch, so a new or changed background ships by pushing to `main`, not by a release.
- **INFINITAS is unsupported.** Keep it listed as unsupported; its reference modules in `konaste-bootstrap` and `konaste-runtime` are unused by the Hub.
- **No web window.** WebView2 runs hidden, for sign-in only. All UI is native.
- **Only the current format exists.** Nothing is released yet: when a file format, folder, or setting changes, change it outright. Readers accept the current version only, with no migration from earlier development builds.
- **Paths stay relative.** No absolute or machine-specific paths in code, tests, or docs; use `D:\Konaste\...`-style fixtures or `%TEMP%`.

## UI

Brand red is `#ad091b` (`theme::ACCENT`); every red element derives from it. Copy is minimal: each label and message has a function, with no slogans, filler subtitles, or decorative punctuation. Dropdowns for settings with more than two options. Every overlay and state change animates (`ui/motion.rs`). Details in `docs/design.md`.

## Docs

Update the matching doc in the same change as the behaviour. Research notes record evidence as found; add a new note rather than rewriting an old one's findings, and list it in `docs/research/README.md`.

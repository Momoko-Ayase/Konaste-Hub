# Konaste Hub

An unofficial launcher for KONAMI's downloadable コナステ PC games. Sign in with your KONAMI ID, then install, update, and play from one window.

| Game | Supported |
|---|---|
| Bombergirl | Yes |
| SOUND VOLTEX EXCEED GEAR | Yes |
| DanceDanceRevolution GRAND PRIX | Yes |
| pop'n music Lively | Yes |
| GITADORA | Yes |
| NOSTALGIA | Yes |
| beatmania IIDX INFINITAS | No |

## Install

Requires Windows 10 or 11 (64-bit).

1. Download `KonasteHub-<version>-setup.exe` from [Releases](https://github.com/Momoko-Ayase/Konaste-Hub/releases).
2. Run it. No administrator rights are needed.

Setup asks for the Visual C++ Redistributable and the WebView2 Runtime if they are missing.

## Use

- **Add game** finds games you installed with the official launcher, or installs a new one into an empty folder.
- Games and official components are downloaded from KONAMI. Downloads can be paused and resumed.
- Your sign-in is saved only when **Stay signed in** is on.
- Removing a game from the Hub does not delete its files.

Settings and logs are kept in `%LOCALAPPDATA%\konaste-hub`. The uninstaller asks whether to delete them. Your games are never deleted.

## Build

See [docs/release.md](docs/release.md).

## License

[MIT](LICENSE). See [third-party notices](THIRD_PARTY_NOTICES.md).

Konaste Hub is not affiliated with or endorsed by KONAMI. KONAMI, コナステ, and the game names are trademarks of their respective owners.

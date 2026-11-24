# Research notes

Primary evidence behind the Hub's behavior. Each note records its date and what was and was not done. Notes are snapshots: check the code before relying on a detail, and add a new dated section rather than rewriting history.

Binaries, installers, and captured bodies referenced here were kept in temporary folders on the researcher's machine and are not in this repository.

## Platform and accounts

- [konaste-platform.md](konaste-platform.md): overview of the Konaste platform, launcher handoff, and early title comparison.
- [native-signin.md](native-signin.md): KONAMI ID sign-in API, bot check, and why requests run inside a hidden page.
- [eamusement-profile.md](eamusement-profile.md): e-amusement page markup for profile, pass card, and PASELI.
- [playdata.md](playdata.md): per-title play data pages and selectors.
- [maintenance.md](maintenance.md): scheduled maintenance windows and 503 detection.

## Discovery, index, and updater

- [updater-mode-bootstrap.md](updater-mode-bootstrap.md): anonymous mode-2 discovery through the pinned runtime.
- [manifest-contract.md](manifest-contract.md): the binary resource index (`ri.bin`) format.
- [updater-integration.md](updater-integration.md): wiring fresh discovery to the recoverable updater.
- [updater-install-performance.md](updater-install-performance.md): journal write amplification during installation.
- [download-reliability.md](download-reliability.md): retries, resume, outage handling, and progress reporting.

## Launch

- [launch-verification.md](launch-verification.md): mode-1 verification and `checkGameStart` (Bombergirl).
- [game-creation.md](game-creation.md): how the official reporter creates the game process.
- [multi-title-integration.md](multi-title-integration.md): extending discovery, install, and launch to all six titles.

## Per title

- [sdvx-contract.md](sdvx-contract.md): SOUND VOLTEX package, runtime, and launch contract.
- [sdvx-first-run.md](sdvx-first-run.md): SDVX first-run failure report and game settings target.
- [remaining-title-contracts.md](remaining-title-contracts.md): package survey for DDR, pop'n, GITADORA, NOSTALGIA, and INFINITAS.
- [older-title-contracts.md](older-title-contracts.md): caller contracts for GITADORA, pop'n, and NOSTALGIA.
- [ddr-contract.md](ddr-contract.md): DDR package and graphics arguments.
- [ddr-current-launch.md](ddr-current-launch.md): DDR current launcher modules.
- [popn-current-launch.md](popn-current-launch.md): pop'n current launch contract and settings DLL.
- [gitadora-nostalgia-current-launch.md](gitadora-nostalgia-current-launch.md): GITADORA and NOSTALGIA current launch.
- [infinitas-adapter.md](infinitas-adapter.md): first INFINITAS read; superseded by the next note.
- [infinitas-current.md](infinitas-current.md): current INFINITAS contract and why it stays unsupported.

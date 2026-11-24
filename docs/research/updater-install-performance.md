# Installation journal write amplification

Date: **2026-10-16**. The requester reported slow installation after downloads finished. Scope: the shared updater's installation phase, not CDN speed, game launch, or installer packaging. No existing game installation was modified to diagnose or optimize it.

## Reproducer and measurements

```sh
cargo run \
  -p konaste-updater --offline -j 2 --example install-bench -- 160
```

The fixture uses the public `Updater` interface, an owned temporary destination, already-staged synthetic 64-byte files, and a synthetic official index. No content request is made. Measurement starts after `prepare` and surrounds `run`, so bootstrap, planning, and network time are excluded. Windows `GetProcessIoCounters` reports native process write calls/bytes; elapsed time also includes validation, hashing, flushes, and publication. These are development-profile measurements on the recorded Windows host, not a promised real-game or release-build speedup.

| Files | Before: install time | After: install time | Before: bytes written | After: bytes written |
|---:|---:|---:|---:|---:|
| 160 | 1,208 ms | 851 ms | 15,898,661 | 381,741 |
| 320 | 4,424 ms | 1,603 ms | 62,668,581 | 761,101 |

Doubling files increased writes **3.942× before**, versus **1.994× after**. The 320-file run was about **2.8× faster** and wrote about **82× fewer bytes**. Native write calls remained approximately two per file (328/648 before, 329/649 after): the improvement does not come from skipping critical durability flushes.

The original command exited 1 with the explicit quadratic-scaling assertion. The same command passed after the change. An initial post-fix run measured 831/1,560 ms; the table records the final version-2-checkpoint rerun at 851/1,603 ms, with identical byte/call counts. [`installation-scaling.rs`](../../crates/konaste-updater/tests/installation-scaling.rs) exercises the same shared fixture in its own test process and asserts byte amplification, not a flaky wall-clock deadline.

## Confirmed cause

Payload placement already uses a same-volume handle-relative rename. Fresh installs do not copy old destination files to backup because there are none. The dominant tiny-file regression was `journal::store` serializing, writing, flushing, and replacing the **whole file list twice for every installed file**. Each document grows with file count, producing quadratic metadata writes.

Three hypotheses were ranked before examining the implementation: full-plan persistence, repeated hashing, and backup copies. The minimized fresh-install fixture excludes downloads and backups and keeps payloads tiny. The measured write ratio and the per-file full-checkpoint calls establish the persistence cause. Hashing has not been removed or parallelized in this change.

## Implementation and invariants

The journal module keeps the same caller-facing updater interface. Its implementation now combines occasional full checkpoints with small installation-only progress records:

- Checkpoints use JSON version **2**. Version-1 transactions remain readable and migrate at the next successful checkpoint. An older version-1-only executable refuses version 2 rather than silently ignoring new progress records.
- `install.log` starts with `KHUPWAL1` and the SHA-256 of the **exact checkpoint bytes**. Each 56-byte record contains sequence, existing file index, phase, received count, prior-file flag, reserved bytes, and a chained SHA-256. It contains no path, URL, credential, or token.
- Before replacement: copy any previous file through protected handles and **flush the backup**, then append and **flush** the small `Replacing` record. Hash the protected rename handle, run the final process guard, rename, verify, then append and **flush** `Installed`.
- The log writer closes before Publishing/Committed full checkpoints. A complete checkpoint is flushed and atomically replaced **before** the preceding log is deleted. If interruption leaves that old log, its checkpoint digest no longer matches and it cannot rewind the newer checkpoint.
- Loading validates the bounded JSON, replays only matching, contiguous, checksum-valid records, and validates the result. Only an incomplete final record is ignored. Complete corrupt records, invalid indexes/phases/sizes, and oversized logs fail closed.
- Recovery folds completed records into a checkpoint and starts a new empty log. It does not append old generations or repeat verified installed-file records. Repeated interrupted recovery is tested through the public updater interface.
- Existing metadata freshness, source binding, no-follow file/directory leases, hardlink refusal, game-running guards, complete staged-delta verification, actual file hashes, backups, and official-index-last publication are retained. Download-stage persistence is unchanged by this targeted optimization.

## Validation scope

Ten journal tests cover legacy reads, new checkpoints, retained intents, torn tails, complete corruption, bounds, stale-generation retirement, cleanup, and hardlink refusal. A separate public-interface test interrupts installation and recovery repeatedly, reopens each transaction, checks that log generations remain bounded, and completes content plus official-index publication.

The earlier isolated full Bombergirl check also completed: 8,736 files verified, actual module ProductVersion `QBG:J:A:A:2026091500`, exact `ri.bin` published last, in a fresh `konaste-install-check-*` folder. **That check used the pre-optimization binary**, so it is content-install acceptance, not a real-game timing measurement of this change. No game was started, no existing installation was modified, and no account/registry settings were changed by that check.

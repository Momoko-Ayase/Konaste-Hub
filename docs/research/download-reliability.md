# Download reliability and progress

Date: **2026-10-16**. Follow-up to the requester's SDVX download failures, slow transfers, library overflow, and missing download details. Validation uses synthetic content on loopback. No existing installation, active download, account session, or official executable is a test target.

## Download bookkeeping

The download phase rewrote the entire JSON journal when learning each file's response ETag and again when finishing the file. This was separate from the earlier installation-only optimization. More parallel connections cannot remove that shared persistence bottleneck.

The initial 48/96-file loopback experiment measured 1,430,715 / 5,662,971 native process write bytes: doubling the file count caused **3.96x** the writes, while the synthetic payload only doubled. These Windows process I/O counters include the loopback fixture's traffic, not just physical disk writes. The full-plan stores account for the superlinear growth.

Small download records now retain the response validator and file state against the exact checkpoint hash. A strong ETag is flushed before writing the first original-stage byte. Complete checkpoints fold those records into the plan before retiring the log. Truncated final records, checksum errors, invalid indices, oversized validators, obsolete logs, and protected-file constraints have separate tests. `install.log` keeps its existing installation/recovery role. New checkpoints use journal version 3; versions 1 and 2 remain readable, while old executables refuse version 3 rather than silently ignoring the added download records. The writable log handle is owned by one download call and closed before checkpoint retirement, including on pause, error, or cancellation. Neither a checkpoint nor a log authorizes a transaction without a fresh source bind and the existing file verification.

The original timing fixture paused by observing staged files from another thread. The retained regression instead refuses the first destination mutation through the production guarded-run interface, making the stop deterministic. Old pause-fixture timings are not claimed as an identical before/after speed measurement for that revised fixture.

## Interrupted and slow transfers

A production-updater loopback regression reproduced the fixed lifetime retry limit: after five 512-byte cuts, the old downloader stopped at **2,560 of 6,144 bytes**, despite advancing on every request. The minimized regression failed in 0.85 seconds. The retry budget now counts consecutive failures without new original-stage bytes, with a separate absolute limit. Useful forward progress resets that budget and the backoff; unsuccessful range-ignored replacements do not.

The old 60-second whole-request deadline could interrupt a large object even while bytes kept arriving. There is no whole-file deadline now: connection/header waiting and each silent read remain bounded separately. Moving bodies may take longer than that idle interval in total. Pause must interrupt both header waiting and body stalls. Tests use the same production transport with private timing scaled down, not a copied downloader.

Error responses are not content: their bodies and ETags cannot become staged bytes or resume validators. Transient HTTP 408, 429, 500, 502, 503, and 504 responses use bounded, pause-responsive backoff. Numeric `Retry-After` delays up to 30 seconds are waited as requested. Longer delays and nonnumeric values, including HTTP-date values not parsed by this implementation, leave a resumable failure after one request instead of silently shortening the server's requested wait. Other error statuses stop without writing their bodies or learning their ETags. Host, HTTPS, identity encoding, range, validator, size, and final SHA-256 admission remain unchanged.

## Transfer information

The native watcher samples at 250 ms intervals. Throughput uses a monotonic count of accepted network writes over a short rolling window, not changes in percentage or retained bytes from an earlier session. No ETA is invented. Stalls decay to zero; paused, failed, checking, verifying, installing, and recovering states have no live rate.

A separate absolute offset for each active file prevents a retry or range-ignored replacement from double-counting useful content. Discarding an incomplete replacement immediately restores the retained-prefix count, including during backoff; a production-updater regression went red at eight reported bytes versus four retained bytes before this correction. At most the configured parallel count, bounded at eight, is exposed. File labels are validated index-relative paths, never installation roots, source URLs, or account values.

Downloads shows percentage, ready bytes / total bytes, ready files / total files, live speed, and active-file bytes with retry state. Ready totals include reusable content already on disk; they are not presented as bytes transferred over the network in this run. Failed pending downloads remain visible with their error and Retry action. Pause, Resume, and Retry use the existing native commands. Ordinary startup or launch checks do not appear as downloads.

Transfer samples are not persisted in settings and cannot grant launch permission. A new attempt clears the previous sample until the fresh plan is known. Native generation/destination checks reject samples from a retired attempt.

## SQLite

The requester asked whether SQLite would solve the write amplification. Row-level transactional updates would, whereas repeatedly replacing one large JSON database value would not. SQLite is a reasonable future backend for the bookkeeping and would replace custom record/replay code, but it would still need a compatible migration for retained transactions, protected filesystem integration, and explicit recovery for game-file replacements outside the database. It would not fix network timeouts or retry policy. No SQLite migration was requested or performed in this increment.

SQLite's [WAL documentation](https://www.sqlite.org/wal.html) and [synchronous pragma](https://www.sqlite.org/pragma.html#pragma_synchronous) distinguish durable `FULL` commits from `NORMAL` commits that may be lost after power failure. No SQLite performance measurement is claimed, and weakening durability is not an acceptable way to claim an updater speed improvement.

## Library layout validation

The stage once overflowed short windows because a fixed minimum height grew past the viewport. The shell now gives the stage the remaining client height and scrolls long content internally. This was checked in the retired web frontend; the native Hub follows the same rule.

## Validation, 2026-10-16

The workspace tests passed, including the updater library and integration checks. The deterministic scaling fixture measured **38,201 / 76,121 native process write bytes** for 48 / 96 files: **1.993x** growth for twice the file count. Payload bytes were 2,688 / 5,376. Log retirement is exercised on the guarded path; no writable log handle survives into checkpoint deletion. This is not a claim about real CDN speed.

## Repeatable checks

```sh
cargo test --offline -p konaste-updater --test download-scaling -- --nocapture
cargo test --offline -p konaste-updater --lib reliability
```

Pause an active download before rebuilding the Hub, then resume the same pending destination without deleting its journal or staging files.

## Local-file work follow-up, 2026-10-17

The requester next reported a long Downloading 100% gap before installation, a shorter gap after installation, and slow Resume. The production trace found that `transfer_progress()` locks the journal, while preparation, full-delta verification, recovery, and installation hold that same mutex across file hashing. Moving the watcher thread alone cannot fix this: it waits for the operation it is meant to observe. Downloaded bytes and Staged-file counts also remain at their totals during verification and replacement, so those quantities cannot describe local work.

A separate read-only work snapshot carries the active phase, phase-owned byte/file counts, and one validated relative current file. The Hub begins observation before preparation, never waits on the journal or lease for that snapshot, and only attempts a nonblocking transfer sample while downloading/paused. The transfer read must also use try-locks: checking a phase and then taking the old blocking transfer read would retain a transition race. Work updates never perform I/O while holding their telemetry lock.

Downloads distinguishes Checking local files, Verifying, Installing, and Checking installation. Local checks/verification use their own byte progress, installation uses completed destination files, and unknown totals remain indeterminate. The final post-run metadata discovery and module/index inspection explicitly show Checking installation before publishing Ready. Paused/failed/old-generation samples cannot grant launch permission. There is no fake network speed for local work.

A regression first failed because a 100% download was shown as 100% local checking. Progress now uses phase-owned counts or an indeterminate bar.

### Resume read measurement

An owned two-file fixture stages **2,097,152 bytes**, drops the session, reopens it, binds a fresh synthetic source, and calls the production `prepare` and `recover` interfaces. A private test-only hash-read hook counts the bytes read by real pinned file hashing, not a copied verification algorithm or an estimated percentage.

The regression went red with **2,097,152 bytes in prepare plus another 2,097,152 in immediate recovery**. After the fix, prepare still reads **2,097,152**, and immediate recovery reads **0**: total pre-run payload reads fall from 4 MiB to 2 MiB for this fixture. The observed single-run elapsed values were approximately 58 ms before and 31 ms after, but these are small warm-cache synthetic runs, not a stable disk/network throughput benchmark or a claimed whole-SDVX speedup.

The shortcut is a one-use in-memory marker created only by a normal successful prepare, consumed by immediate recovery, and cleared on entry to run or a new prepare. It is false on reopen and on the interrupted Installing/Publishing prepare path. Recovery first checks that the bound journal still matches disk and only takes the shortcut for pre-install transaction phases, returning `needs_run: true`. Run still hashes the entire changed delta before any destination mutation. A regression changes a same-sized stage after prepare, takes the recovery shortcut, then verifies that run refuses it before its first mutation guard. Interrupted replacement recovery and final protected rename checks are not skipped.

The observer regression now uses a private thread-local hash seam and owned OS threads. It blocks inside the actual pinned-file hash during Preparing, Verifying, Installing, and Recovering; the reader must return within 300 ms with current phase/file/counts, and busy transfer samples must return `None`. Restoring the old journal dependency makes that test fail with `work read blocked behind file hashing`; the gate is released and both threads are joined before asserting. Earlier global-gate fixtures hung without a verdict and were removed, not counted as passing evidence. Phase changes reset stale work, and each busy journal/phase/transfer lock has an all-or-nothing sampling regression.

Installation progress logging also keeps one run-local log handle for its file batch instead of reopening/replaying the full checkpoint and prior records for every file intent. The handle closes before publication/checkpoint retirement, including error unwinding. Per-file flushes and bound record checks remain. Completed-file counters advance incrementally rather than rescanning each earlier journal prefix. The existing installation-scaling and repeated-recovery checks remain the regression surface; no new physical-disk speed claim is made.

Follow-up validation passed the workspace tests and strict Clippy for the shared crates. Retained-file tests are ignored by default and were not rerun.

## Session-level outage survival, 2026-11-20

A CDN speed limit is server-side; more connections cannot raise it. This change is about finishing a long install across Wi-Fi loss, suspend/resume, DNS blips, 5xx/403 pages, resets, and slow-drip sockets without a Resume press.

**Failure classes.** Transport failures before a status line (connect, DNS, TLS, reset before headers, header timeout) are the outage class. That is the only class that sets the session-level `net_failed` marker, because a dropped body or a 5xx page means a server did answer. A 403/404, a second complete body that fails its SHA-256, a policy refusal, a disk-full refusal, and a redirect still stop the run; they remain `Download` errors and so are resumable.

**Requeue.** A file that spends its whole per-file budget now returns a transient class and is pushed to the back of the pending queue once. A second such budget for the same file stops the session with the original error. The per-file ceilings are unchanged (`STALL_ATTEMPTS = 5`, `ABSOLUTE_ATTEMPTS = 64`); requeueing changes who retries, not how hard one visit tries.

**Outage wait.** When a file reaches no server at all, the supervisor starts no new files. It sleeps one capped sequence, 1, 2, 4, 8, 16, then 30 s seven times: **211 s**, a bit over three minutes, below the several-minute bound in the brief. Each step ends in one probe file. A probe that also reaches nothing re-arms the marker and the next step applies; any completed attempt resets the sequence. Files woken by the marker return `Offline` without spending a budget. The wait is pausable and the latched pause wins. Exhausting the sequence gives a resumable `Download` error, so a genuinely offline machine surfaces instead of hanging.

**Slow drip.** Each body now has a `RateWatch`: under 4 KiB/s over a 10 s window, the connection is aborted and reconnected from the staged byte. One connection to the SDVX CDN measured about 4.6 MB/s, so the floor sits more than a thousand times below a real connection and far below any server speed cap. A moving body above the floor is never cut; the existing 15 s read-idle deadline still catches silence. The watchdog is a reconnect trigger, not a throughput goal.

**Client.** The reqwest client now sets `tcp_keepalive` 20 s with 4 retries, `pool_idle_timeout` 300 s, and HTTP/2 keep-alive at 20 s / 10 s timeout while streams are active. An idle pooled socket wedged by suspend is retired before reuse; a live body is still bounded only per read.

**Public surface.** `TransferSnapshot` gained an additive `waiting_for_network: bool`, true while the session is inside the outage wait. Existing `FileProgress.retrying` covers per-file backoff.

**Measured, loopback.** `cargo test -p konaste-updater`: 74 lib, 4 integration, 0 doc. The 17 transport regressions include: an exhausted five-503 budget requeued and finished in 8.6 s; two budgets stop as a resumable error in 17.1 s; a 404 still stops after one request; a refused port consumed the scaled 211 s outage sequence, stayed resumable, and left no destination file; a 64 B/60 ms drip (~1 KiB/s) was cut and resumed from a later offset; a 2 KiB/40 ms body (~50 KiB/s) finished on one connection; a silent origin shorter than the session budget finished in one run in 9.7 s (scaled). The loopback fixture now answers each connection on its own thread, because a serial accept loop made a stalled answer hide the retry it was meant to test.

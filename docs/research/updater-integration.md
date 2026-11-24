# Bombergirl native updater integration

Date: **2026-10-16**. Scope: normal content installation/update, not game launch or launcher self-update. The requester asked to integrate the updater and offered fresh-install testing because their existing Bombergirl installation is current. Tests must leave that installation untouched. No account session, launch token, installer action, registry change, game execution, asset decryption, or protection bypass is needed for content transfer.

## Metadata to plan

The source is a new `konaste_discovery::check` result, not a retained research manifest or the last successful library check. Discovery authenticates the independently supplied resource digest and retains the exact body accepted by the decoder. `konaste-install::UpdateSource` re-decodes that body instead of trusting the mutable decoded projection.

| Official field | Updater field |
|---|---|
| `ri_ver` | decimal `Manifest.version`; never a PE ProductVersion |
| `path` | `ManifestFile.relative_path`, in original order |
| `size` | final stored byte count |
| `sum` | final stored SHA-256 |
| `url` | exact discovered HTTPS URL; no reconstructed build stamp |
| No ETag field | `etag: None`; transfer validators belong to the downloader |
| Complete original body | `OfficialManifest { path: "ri.bin", bytes }` |

Only the verified Bombergirl CDN `d1q1iflay6y5jj.cloudfront.net` is allowed. The observed `/Updater/` prefix and exact relative-path suffix must agree. The source requires the nonempty `modules/bomber.exe` entry and rejects a content entry colliding with `ri.bin`. SDVX bootstrap compatibility is not an update contract.

The source does not decrypt, unpack, patch, rename, or infer logical names for `x/` content. The opaque bytes must match both the declared stored length and SHA-256. Unknown installed files are not a deletion list. See [the manifest contract](manifest-contract.md) and [`konaste-install`](../../crates/konaste-install/src/lib.rs).

## Write admission and recovery

The updater must bind each opened/reopened session to fresh authenticated metadata before any run or recovery can apply a loaded journal. An editable journal alone is not authorization. Matching transactions retain validated staged/partial bytes. A different source must not silently replace an interrupted installation/publication transaction or destroy its recovery evidence.

Normal updates use the selected installation root. Fresh installs use a separately chosen empty destination, not an existing game's root or its parent/child. The pending operation kind and destination live alongside imports in the app's settings, not in the game tree and not as a launch root. A complete, verified installation atomically replaces the selected import and removes its pending record. Explicitly importing another installation also retires pending tracking without deleting staged files. Updates never start a game automatically.

Known safe-first limitation: if official metadata changes while an older transaction is interrupted in `Installing` or `Publishing`, automatic source migration is not implemented. The engine returns `RecoveryRequired` and preserves that transaction rather than applying an unauthenticated old journal or discarding recovery evidence. The UI directs the user to Install elsewhere. Ordinary same-source interruption/restart and pre-replacement re-planning remain separate supported paths; this limitation must not be described as full changed-release recovery.

The host must exclude its own launch attempt and check the exact game's running process immediately before destination replacement, recovery mutation, and official-index publication. Maintenance blocks Play, not install/update/resume. Progress and errors must not publish stale installation identities or account/URL secrets.

Filesystem write admission requires top-down no-follow directory pins, checked file handles, hardlink rejection, and handle-safe replacement. Checking `symlink_metadata` and then reopening by path is insufficient. Pinning only the final root is insufficient when an ancestor can be replaced. The same rules apply to staging, backups, journal, official-stage, and session-lock files, not only installed content.

Primary Win32 references:

- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew): omitting `FILE_SHARE_DELETE` prevents other delete/rename opens; omitting `FILE_SHARE_WRITE` denies other writers; `FILE_FLAG_OPEN_REPARSE_POINT` applies to the opened leaf; directory handles use `FILE_FLAG_BACKUP_SEMANTICS`.
- [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info): a relative `FileName` can be resolved against an opened `RootDirectory`; length is bytes; replacement behavior is explicit.

Both reference pages were fetched through the configured source-search CLI. One separate `SetFileInformationByHandle` page fetch failed without a diagnostic body; it is not counted as fetched evidence.

### Direct Windows handle-contract checks

Disposable-directory experiments on this Windows 11 host established the details needed by the implementation; no official file, existing installation, registry, or account was involved:

- A read-only directory handle with `BACKUP_SEMANTICS | OPEN_REPARSE_POINT` can be the `RootDirectory` of `NtCreateFile` when creating ordinary file/directory children. Both calls returned status 0. Removing no-follow is not a necessary workaround.
- A newly created source handle needs `DELETE` access before it can be renamed. Missing access caused the initial production `prepare` regression to fail with access denied.
- The Win32 `SetFileInformationByHandle(FileRenameInfo)` wrapper rejected a non-null relative `RootDirectory` with error 87 on this host. `NtSetInformationFile(FileRenameInformation)` supports the actual handle-relative operation.
- Native relative rename returned `STATUS_SHARING_VIOLATION` with a read-only-sharing parent pin; it succeeded when the parent allowed read/write sharing but still denied delete sharing. This held both with and without `FILE_ADD_FILE` desired access on the parent. Child creation did not require a writable parent handle.
- Read/write sharing permits another handle to set a junction on an empty pinned directory. After an actual `FSCTL_SET_REPARSE_POINT`, both no-follow `NtCreateFile` below the held parent and native relative rename into it returned `STATUS_REPARSE_POINT_NOT_RESOLVED` (`0xC0000280`). Neither created the target outside the directory; the independent sentinel remained unchanged. Thus the implementation must preserve no-follow handle-relative operations and fail closed on reparse status, not claim that all directory writes are denied by sharing.

These checks narrow the contract; they are not a blanket proof against every filesystem race. The session must retain the approved root/ancestor pins across emptiness checks, work-directory creation, and later writes.

## Transport and isolated live checks

The production downloader must request `Accept-Encoding: identity`, disable automatic decoding, refuse other content encodings and redirects, validate ranges/validators, and verify the complete stored size/hash before installation. A mismatch never authorizes a transformed or partially verified payload.

The explicit example dispatches the fixed SDK worker before initializing any application runtime:

```sh
cargo run --offline -j 2 -p konaste-install --example install -- \
  "C:/path/BomberGirl_Installer.msi" --probe

cargo run --offline -j 2 -p konaste-install --example install -- \
  "C:/path/BomberGirl_Installer.msi" --fresh-in "D:/parent-for-new-test-directory"

# Only if an isolated check above was interrupted:
cargo run --offline -j 2 -p konaste-install --example install -- \
  "C:/path/BomberGirl_Installer.msi" --resume-test "D:/parent/konaste-install-check-example"
```

`--offline` applies to Cargo dependency resolution, **not** these live checks. The retained MSI is hash/signature-checked and statically extracted into a temporary bootstrap cache. It is never executed.

- `--probe` selects the smallest nonzero module and opaque-store entries, each capped at 16 MiB, and downloads them through the production updater into a temporary tree. It does not publish a full official index for the sample-only tree.
- `--fresh-in` creates a unique `konaste-install-check-*` directory under the supplied parent. It does not accept an existing game destination. It tests a partial-download pause, releases the entire session, discovers fresh metadata, reopens/binds the durable transaction, completes the install, verifies all content again, and checks the actual module PE ProductVersion. The new test directory is retained; no existing installation or app settings are changed.

`--resume-test` is only for a retained directory created by `--fresh-in`: it requires both the generated directory-name prefix and the exact isolated-test marker. It obtains fresh metadata and rebinds the existing transaction; it is not a general existing-install updater command.

These are native module integration checks, not GUI acceptance, installer acceptance, or permission to launch a newly installed game.

## Evidence status

- Fresh anonymous discovery outside maintenance passed: revision `2026091500`, 8,736 files, 11,722,749,506 declared stored bytes, index SHA-256 `205bb4a45e1b38ad421ea8b0ab5c41214b99c7cc5714bbdecf8d15dd3e881e9a`.
- Source translation tests passed before filesystem integration.
- Identity content sample check passed through the production updater: two nonzero module/store entries, 47,135 bytes total, exact fresh-index size and SHA-256. No official index was published for the sample-only tree.
- 214 native tests passed after the installation-only performance fix, with strict updater/install-source/runtime/discovery all-target lint. Tests include an actual successful FSCTL junction mutation followed by failed handle-relative create/rename, retained real response ETags across a new session, changed-source prefix refusal, stage mutation at the final guard, and atomic pending/import publication.
- Full fresh-install check completed in a fresh isolated folder: 8,736 files / 11,722,749,506 stored bytes, a pause at 4,297,445 received bytes without early `ri.bin`, complete session release, fresh discovery, reopen/resume, all-file verification, and exact `ri.bin`-last publication. The actual module ProductVersion is `QBG:J:A:A:2026091500`. This test used the pre-optimization binary; it is not a real-game timing measurement of the [installation journal performance change](updater-install-performance.md).
- No existing game installation has been used as a write-test target.

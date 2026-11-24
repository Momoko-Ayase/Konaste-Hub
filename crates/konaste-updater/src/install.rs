//! Replace installed files from an already-verified stage.
//!
//! Each file is its own small transaction: back up through pinned handles,
//! record `Replacing`, rename the staged handle into the pinned destination
//! directory, verify through the new handle, record `Installed`. The official
//! manifest path is not touched until every content file is `Installed`.
//! `guard` runs immediately before each of those mutations and before a
//! recovery rollback. A rejection leaves the journal and stage unchanged.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use crate::error::UpdateError;
use crate::journal::{self, FilePhase, Journal, JournalPhase, WorkPaths};
use crate::manifest::Sha256;
use crate::paths::SafeRelative;

fn relative_parts(relative: &str) -> Result<SafeRelative, UpdateError> {
    crate::paths::parse_relative(relative)
}

fn parent_parts(relative: &SafeRelative) -> Result<(Vec<String>, String), UpdateError> {
    let display = relative.display();
    let mut parts: Vec<String> = display.split('/').map(str::to_string).collect();
    let name = parts.pop().ok_or_else(|| UpdateError::path(display, "path does not name a file"))?;
    Ok((parts, name))
}

pub(crate) fn install_path(paths: &WorkPaths, relative: &str) -> Result<PathBuf, UpdateError> {
    let safe = relative_parts(relative)?;
    // Pinning the root rejects a junction on any existing ancestor. The joined
    // path is lexical and stays under the root because `safe` has no `..`.
    let joined = safe.join_under(&paths.root);
    if !joined.starts_with(&paths.root) {
        return Err(UpdateError::path(relative, "resolved path escapes the installation root"));
    }
    Ok(joined)
}

pub(crate) fn verify_staged_delta<'a>(
    _paths: &WorkPaths,
    journal: &Journal,
    lease_of: &mut dyn FnMut() -> Result<std::sync::MutexGuard<'a, crate::pin::Lease>, UpdateError>,
    mut on_file: impl FnMut(&str, u64, u64, u64, u64),
) -> Result<(), UpdateError> {
    let pending: Vec<_> = journal.files.iter().filter(|file| file.phase != FilePhase::Installed).collect();
    let official = journal.official.as_ref().filter(|file| file.phase != FilePhase::Installed);
    let files_total = pending.len() as u64 + u64::from(official.is_some());
    let bytes_total: u64 = pending.iter().map(|file| file.size).sum::<u64>() + official.map_or(0, |file| file.size);
    let mut files_completed = 0_u64;
    let mut bytes_completed = 0_u64;
    for file in &journal.files {
        if file.phase == FilePhase::Installed {
            continue;
        }
        on_file(&file.relative, files_completed, files_total, bytes_completed, bytes_total);
        let mut opened = {
            let mut lease = lease_of()?;
            open_staged(&mut lease, &file.relative)?
        };
        on_file(&file.relative, files_completed, files_total, bytes_completed, bytes_total);
        check_hash(&mut opened, file.size, &file.sha256, &file.relative, &mut |read| {
            on_file(&file.relative, files_completed, files_total, bytes_completed + read, bytes_total);
        })
        .map_err(|err| UpdateError::Staging { relative: file.relative.clone(), message: err.to_string() })?;
        files_completed += 1;
        bytes_completed += file.size;
        on_file(&file.relative, files_completed, files_total, bytes_completed, bytes_total);
    }
    if let Some(official) = official {
        on_file(&official.relative, files_completed, files_total, bytes_completed, bytes_total);
        let parent = lease_of()?.work()?;
        let mut opened = crate::pin::open_file_read(&parent, std::ffi::OsStr::new("official.stage"))?
            .ok_or_else(|| UpdateError::journal("official stage is missing"))?;
        check_hash(&mut opened, official.size, &official.sha256, &official.relative, &mut |read| {
            on_file(&official.relative, files_completed, files_total, bytes_completed + read, bytes_total);
        })?;
        on_file(&official.relative, files_completed + 1, files_total, bytes_completed + official.size, bytes_total);
    }
    Ok(())
}

pub(crate) fn verify_file(path: &Path, size: u64, sha: &str, lease: &mut crate::pin::Lease) -> Result<(), UpdateError> {
    let mut file = lease
        .open_path(path)?
        .ok_or_else(|| UpdateError::Staging { relative: path.display().to_string(), message: "file is missing".into() })?;
    check_hash_quiet(&mut file, size, sha, &path.display().to_string())
}

fn verify_named(base: &Path, relative: &str, size: u64, sha: &str, lease: &mut crate::pin::Lease) -> Result<(), UpdateError> {
    let safe = relative_parts(relative)?;
    verify_file(&safe.join_under(base), size, sha, lease)
}

fn verify_work_file(lease: &mut crate::pin::Lease, name: &str, size: u64, sha: &str) -> Result<(), UpdateError> {
    let parent = lease.work()?;
    let mut file = crate::pin::open_file_read(&parent, std::ffi::OsStr::new(name))?
        .ok_or_else(|| UpdateError::Staging { relative: name.into(), message: "staged file is missing".into() })?;
    check_hash_quiet(&mut file, size, sha, name)
}

fn open_staged(lease: &mut crate::pin::Lease, relative: &str) -> Result<crate::pin::OpenedFile, UpdateError> {
    let safe = relative_parts(relative)?;
    let (parts, name) = parent_parts(&safe)?;
    let parent = descend_under(lease, "staging", &parts)?;
    crate::pin::open_file_read(&parent, std::ffi::OsStr::new(&name))?
        .ok_or_else(|| UpdateError::Staging { relative: name, message: "staged file is missing".into() })
}

fn check_hash(
    file: &mut crate::pin::OpenedFile,
    size: u64,
    sha: &str,
    label: &str,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), UpdateError> {
    let expected = Sha256::parse(sha)?;
    if file.size() != size {
        return Err(UpdateError::Staging { relative: label.into(), message: format!("size {} != expected {size}", file.size()) });
    }
    let digest = file.hash_with(on_bytes)?;
    if !expected.matches(&digest) {
        return Err(UpdateError::Staging { relative: label.into(), message: "sha256 mismatch".into() });
    }
    Ok(())
}

fn check_hash_quiet(file: &mut crate::pin::OpenedFile, size: u64, sha: &str, label: &str) -> Result<(), UpdateError> {
    check_hash(file, size, sha, label, &mut |_| {})
}

pub(crate) fn apply<'a>(
    paths: &WorkPaths,
    journal: &mut Journal,
    lease_of: &mut dyn FnMut() -> Result<std::sync::MutexGuard<'a, crate::pin::Lease>, UpdateError>,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
    mut on_file: impl FnMut(&str, u64, u64, u64, u64),
) -> Result<(), UpdateError> {
    journal.phase = JournalPhase::Installing;
    {
        let mut lease = lease_of()?;
        journal::store(paths, journal, &mut lease)?;
    }

    {
        let mut log = {
            let mut lease = lease_of()?;
            journal::ProgressLog::open(&mut lease)?
        };
        let files_total = journal.files.len() as u64;
        let bytes_total: u64 = journal.files.iter().map(|file| file.size).sum();
        let mut done_bytes = 0_u64;
        for index in 0..journal.files.len() {
            on_file(&journal.files[index].relative, index as u64, files_total, done_bytes, bytes_total);
            {
                let mut lease = lease_of()?;
                replace_one(paths, journal, &mut lease, &mut log, index, guard)?;
            }
            done_bytes += journal.files[index].size;
            on_file(&journal.files[index].relative, index as u64 + 1, files_total, done_bytes, bytes_total);
        }
    }

    if journal.official.is_some() {
        journal.phase = JournalPhase::Publishing;
        {
            let mut lease = lease_of()?;
            journal::store(paths, journal, &mut lease)?;
        }
        {
            let mut lease = lease_of()?;
            publish_official(paths, journal, &mut lease, guard)?;
        }
    }

    journal.phase = JournalPhase::Committed;
    {
        let mut lease = lease_of()?;
        journal::store(paths, journal, &mut lease)?;
    }
    {
        let mut lease = lease_of()?;
        write_committed_record(paths, journal, &mut lease)?;
    }
    Ok(())
}

fn replace_one(
    paths: &WorkPaths,
    journal: &mut Journal,
    lease: &mut crate::pin::Lease,
    log: &mut journal::ProgressLog,
    index: usize,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    if journal.files[index].phase == FilePhase::Installed {
        let dest_ok =
            verify_named(&paths.root, &journal.files[index].relative, journal.files[index].size, &journal.files[index].sha256, lease)
                .is_ok();
        if dest_ok {
            return Ok(());
        }
        if verify_named(&paths.staging, &journal.files[index].relative, journal.files[index].size, &journal.files[index].sha256, lease)
            .is_err()
        {
            return Err(UpdateError::Install {
                path: PathBuf::from(&journal.files[index].relative),
                message: "file marked installed does not match and no verified stage remains; official manifest was not published".into(),
            });
        }
        journal.files[index].phase = FilePhase::Staged;
    }
    let relative = journal.files[index].relative.clone();
    let size = journal.files[index].size;
    let sha = journal.files[index].sha256.clone();
    let safe = relative_parts(&relative)?;
    let (parts, name) = parent_parts(&safe)?;

    // A rejected guard must leave the journal, the stage, the destination, and
    // the backup tree untouched. Backup and the Replacing record are mutations.
    guard().map_err(|err| match err {
        UpdateError::OperationRejected(_) => err,
        other => UpdateError::operation_rejected(other.to_string()),
    })?;
    journal.files[index].had_previous |= backup_previous(lease, &parts, &name)?;
    journal.files[index].phase = FilePhase::Replacing;
    log.record_file(index, &journal.files[index])?;

    let staged_ok = verify_named(&paths.staging, &relative, size, &sha, lease).is_ok();
    let dest_ok = verify_named(&paths.root, &relative, size, &sha, lease).is_ok();
    if !staged_ok && dest_ok {
        // A previous attempt renamed the stage into place and crashed before
        // recording Installed.
    } else if !staged_ok {
        return Err(UpdateError::Install {
            path: PathBuf::from(&relative),
            message: "staged file is missing; installed file was not modified".into(),
        });
    } else {
        guard().map_err(|err| match err {
            UpdateError::OperationRejected(_) => err,
            other => UpdateError::operation_rejected(other.to_string()),
        })?;
        // Re-check the stage after the guard. The pin is taken again so a
        // junction planted while the guard ran is refused.
        move_staged(lease, &parts, &name, size, &sha, guard)?;
    }
    verify_named(&paths.root, &relative, size, &sha, lease)
        .map_err(|err| UpdateError::Install { path: PathBuf::from(&relative), message: err.to_string() })?;
    journal.files[index].phase = FilePhase::Installed;
    log.record_file(index, &journal.files[index])?;
    Ok(())
}

fn descend_lease(lease: &mut crate::pin::Lease, parts: &[String]) -> Result<std::os::windows::io::OwnedHandle, UpdateError> {
    lease.descend(parts)
}

fn descend_under(
    lease: &mut crate::pin::Lease,
    directory: &str,
    parts: &[String],
) -> Result<std::os::windows::io::OwnedHandle, UpdateError> {
    let mut all = Vec::with_capacity(parts.len() + 1);
    all.push(directory.to_string());
    all.extend(parts.iter().cloned());
    lease.descend_from_work(&all)
}

fn move_staged(
    lease: &mut crate::pin::Lease,
    parts: &[String],
    name: &str,
    size: u64,
    sha: &str,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let stage_parent = descend_under(lease, "staging", parts)?;
    let dest_parent = descend_lease(lease, parts)?;
    let mut file = crate::pin::open_file_rename(&stage_parent, std::ffi::OsStr::new(name))?
        .ok_or_else(|| UpdateError::Install { path: PathBuf::from(name), message: "staged file disappeared before rename".into() })?;
    if file.serial() != lease.serial() {
        return Err(UpdateError::Root("staged file is not on the installation volume".into()));
    }
    check_hash_quiet(&mut file, size, sha, name)?;
    guard()?;
    crate::pin::rename_into(file.handle(), &dest_parent, std::ffi::OsStr::new(name), true)?;
    Ok(())
}

fn publish_official(
    paths: &WorkPaths,
    journal: &mut Journal,
    lease: &mut crate::pin::Lease,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let official = journal.official.clone().expect("checked by caller");
    if official.phase == FilePhase::Installed {
        verify_named(&paths.root, &official.relative, official.size, &official.sha256, lease)?;
        return Ok(());
    }
    let safe = relative_parts(&official.relative)?;
    let (parts, name) = parent_parts(&safe)?;
    guard().map_err(|err| match err {
        UpdateError::OperationRejected(_) => err,
        other => UpdateError::operation_rejected(other.to_string()),
    })?;
    let had_previous = backup_previous(lease, &parts, &name)?;
    if let Some(slot) = journal.official.as_mut() {
        slot.had_previous |= had_previous;
    }
    if let Some(slot) = journal.official.as_mut() {
        slot.phase = FilePhase::Replacing;
    }
    journal::store(paths, journal, lease)?;
    if verify_work_file(lease, "official.stage", official.size, &official.sha256).is_ok() {
        guard().map_err(|err| match err {
            UpdateError::OperationRejected(_) => err,
            other => UpdateError::operation_rejected(other.to_string()),
        })?;
        let work = lease.work()?;
        let dest_parent = descend_lease(lease, &parts)?;
        let mut staged = crate::pin::open_file_rename(&work, std::ffi::OsStr::new("official.stage"))?.ok_or_else(|| {
            UpdateError::Install { path: PathBuf::from(&official.relative), message: "official stage disappeared before publish".into() }
        })?;
        check_hash_quiet(&mut staged, official.size, &official.sha256, "official.stage")?;
        guard()?;
        crate::pin::rename_into(staged.handle(), &dest_parent, std::ffi::OsStr::new(&name), true)?;
    }
    verify_named(&paths.root, &official.relative, official.size, &official.sha256, lease)?;
    if let Some(slot) = journal.official.as_mut() {
        slot.phase = FilePhase::Installed;
    }
    journal::store(paths, journal, lease)?;
    Ok(())
}

fn backup_previous(lease: &mut crate::pin::Lease, parts: &[String], name: &str) -> Result<bool, UpdateError> {
    let destination = descend_lease(lease, parts)?;
    let Some(mut previous) = crate::pin::open_file_read(&destination, std::ffi::OsStr::new(name))? else {
        return Ok(false);
    };
    let parent = descend_under(lease, "backup", parts)?;
    crate::pin::delete_file(&parent, std::ffi::OsStr::new(name))?;
    let backup = crate::pin::create_file(&parent, std::ffi::OsStr::new(name))?;
    previous.seek(std::io::SeekFrom::Start(0))?;
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = previous.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        crate::pin::write_all(&backup, &buffer[..count])?;
        total += count as u64;
    }
    if total != previous.size() {
        return Err(UpdateError::Install { path: name.into(), message: "backup copy length changed".into() });
    }
    crate::pin::flush(&backup)?;
    Ok(true)
}

fn write_committed_record(paths: &WorkPaths, journal: &Journal, lease: &mut crate::pin::Lease) -> Result<(), UpdateError> {
    let record = serde_json::json!({
        "version": journal.manifest_version,
        "files": journal.files.iter().map(|file| serde_json::json!({
            "path": file.relative,
            "sha256": file.sha256,
            "size": file.size,
        })).collect::<Vec<_>>(),
        "official": journal.official.as_ref().map(|official| serde_json::json!({
            "path": official.relative,
            "sha256": official.sha256,
            "size": official.size,
        })),
    });
    let bytes = serde_json::to_vec_pretty(&record).map_err(|err| UpdateError::journal(err.to_string()))?;
    journal::publish_bytes(lease, "committed.manifest.json.tmp", crate::journal::COMMITTED_MANIFEST_FILE, &bytes)
        .map_err(|err| UpdateError::Install { path: paths.committed.clone(), message: err.to_string() })
}

pub(crate) fn recover_install(
    paths: &WorkPaths,
    journal: &mut Journal,
    lease: &mut crate::pin::Lease,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    match journal.phase {
        JournalPhase::Installing | JournalPhase::Publishing | JournalPhase::Committed => {}
        _ => return Ok(()),
    }
    {
        let mut log = journal::ProgressLog::open(lease)?;
        for index in 0..journal.files.len() {
            match journal.files[index].phase {
                FilePhase::Installed => replace_one(paths, journal, lease, &mut log, index, guard)?,
                FilePhase::Replacing => finish_replacing(paths, journal, lease, &mut log, index, guard)?,
                FilePhase::Staged | FilePhase::Pending | FilePhase::Partial => {
                    if journal.phase == JournalPhase::Installing {
                        replace_one(paths, journal, lease, &mut log, index, guard)?;
                    }
                }
            }
        }
    }
    let content_done = journal.files.iter().all(|file| file.phase == FilePhase::Installed);
    if content_done
        && let Some(official) = journal.official.clone()
        && official.phase != FilePhase::Installed
    {
        journal.phase = JournalPhase::Publishing;
        journal::store(paths, journal, lease)?;
        publish_official(paths, journal, lease, guard)?;
    }
    if journal.files.iter().all(|file| file.phase == FilePhase::Installed)
        && journal.official.as_ref().map(|official| official.phase == FilePhase::Installed).unwrap_or(true)
    {
        journal.phase = JournalPhase::Committed;
        journal::store(paths, journal, lease)?;
        write_committed_record(paths, journal, lease)?;
    }
    Ok(())
}

fn finish_replacing(
    paths: &WorkPaths,
    journal: &mut Journal,
    lease: &mut crate::pin::Lease,
    log: &mut journal::ProgressLog,
    index: usize,
    guard: &mut dyn FnMut() -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let relative = journal.files[index].relative.clone();
    if verify_named(&paths.root, &relative, journal.files[index].size, &journal.files[index].sha256, lease).is_ok() {
        journal.files[index].phase = FilePhase::Installed;
        log.record_file(index, &journal.files[index])?;
        return Ok(());
    }
    if verify_named(&paths.staging, &relative, journal.files[index].size, &journal.files[index].sha256, lease).is_ok() {
        return replace_one(paths, journal, lease, log, index, guard);
    }
    if journal.files[index].had_previous {
        let safe = relative_parts(&relative)?;
        let (parts, name) = parent_parts(&safe)?;
        // Restoring the backup is a destination mutation.
        guard().map_err(|err| match err {
            UpdateError::OperationRejected(_) => err,
            other => UpdateError::operation_rejected(other.to_string()),
        })?;
        move_backup(lease, &parts, &name)?;
        journal.files[index].phase = FilePhase::Pending;
        journal.files[index].had_previous = false;
        log.record_file(index, &journal.files[index])?;
        return Err(UpdateError::Install {
            path: PathBuf::from(&relative),
            message: "replacement was incomplete; previous file restored from backup".into(),
        });
    }
    Err(UpdateError::Install {
        path: PathBuf::from(&relative),
        message: "replacement was interrupted and neither the stage nor a backup is available".into(),
    })
}

fn move_backup(lease: &mut crate::pin::Lease, parts: &[String], name: &str) -> Result<(), UpdateError> {
    let backup_parent = descend_under(lease, "backup", parts)?;
    let dest_parent = descend_lease(lease, parts)?;
    let file = crate::pin::open_file_rename(&backup_parent, std::ffi::OsStr::new(name))?
        .ok_or_else(|| UpdateError::Install { path: PathBuf::from(name), message: "backup disappeared before restore".into() })?;
    crate::pin::rename_into(file.handle(), &dest_parent, std::ffi::OsStr::new(name), true)?;
    Ok(())
}

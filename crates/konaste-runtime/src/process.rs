use std::{
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use konaste_bootstrap::GameFamily;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        },
        Threading::{CREATE_NO_WINDOW, GetCurrentProcess},
    },
};

use crate::{
    FINISH, MAX_REQUEST_BYTES, Operation, PROTOCOL, Request, RuntimeError, VerificationInput, WORKER_ARGUMENT, WorkerReceipt, read_frame,
};
use zeroize::Zeroizing;

const LIFETIME: Duration = Duration::from_secs(45);
const PREPARE_READY: Duration = Duration::from_secs(60);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
const FLAGS: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    | JOB_OBJECT_LIMIT_PROCESS_MEMORY;

struct Job(HANDLE);
// SAFETY: an exclusively owned, non-inheritable kernel job handle is not bound
// to the creating thread. No COM/window pointer or borrowed native state moves.
unsafe impl Send for Job {}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: this module owns the non-inheritable job handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Job {
    fn new() -> Result<Self, RuntimeError> {
        // SAFETY: null security/name creates a private non-inheritable job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(RuntimeError::Worker);
        }
        let job = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = FLAGS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = MEMORY_LIMIT;
        // SAFETY: pointer and size match the extended-limit structure.
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(RuntimeError::Worker);
        }
        Ok(job)
    }
}

pub(crate) fn require_job() -> Result<(), RuntimeError> {
    let mut member = 0;
    // SAFETY: output BOOL is valid; pseudo process handle needs no close.
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) } == 0 || member == 0 {
        return Err(RuntimeError::Worker);
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: null job queries the current process's job, with the matching buffer.
    if unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
            std::ptr::null_mut(),
        )
    } == 0
        || limits.BasicLimitInformation.LimitFlags & FLAGS != FLAGS
        || limits.BasicLimitInformation.ActiveProcessLimit != 1
        || limits.ProcessMemoryLimit == 0
        || limits.ProcessMemoryLimit > MEMORY_LIMIT
    {
        return Err(RuntimeError::Worker);
    }
    Ok(())
}

pub(crate) fn request(
    family: GameFamily,
    cache_root: &Path,
    executable: &Path,
    operation: Operation,
) -> Result<WorkerReceipt, RuntimeError> {
    // Detect missing/stale pairs before creating a child. The child checks again;
    // editable cache metadata or a serialized RuntimePair is never authority.
    let _ = crate::inspected_pair(family, cache_root)?;
    let cache_root = std::fs::canonicalize(cache_root).map_err(|_| RuntimeError::Runtime)?;
    let executable = std::fs::canonicalize(executable).map_err(|_| RuntimeError::Worker)?;
    let scratch = tempfile::Builder::new().prefix("konaste-runtime-").tempdir().map_err(|_| RuntimeError::Worker)?;
    let lifetime = if matches!(&operation, Operation::Verify(_)) { Duration::from_secs(60) } else { LIFETIME };
    let request = Request { protocol: PROTOCOL, family, cache_root, operation };
    let bytes = Zeroizing::new(serde_json::to_vec(&request).map_err(|_| RuntimeError::Protocol)?);
    if bytes.len() as u64 > crate::MAX_REQUEST_BYTES {
        return Err(RuntimeError::Protocol);
    }
    let output = run(&executable, scratch.path(), &bytes, lifetime)?;
    serde_json::from_slice(&output).map_err(|_| RuntimeError::Protocol)
}

/// Same child as [`request`], but it stays alive after its one Prepared receipt.
/// Drop writes the finish byte, waits briefly, then kills and reaps. Scratch and
/// the job close only after the child is gone.
pub(crate) fn prepare(
    family: GameFamily,
    cache_root: &Path,
    executable: &Path,
    input: &VerificationInput,
) -> Result<HeldSession, RuntimeError> {
    let _ = crate::inspected_pair(family, cache_root)?;
    let cache_root = std::fs::canonicalize(cache_root).map_err(|_| RuntimeError::Runtime)?;
    let executable = std::fs::canonicalize(executable).map_err(|_| RuntimeError::Worker)?;
    let scratch = tempfile::Builder::new().prefix("konaste-runtime-").tempdir().map_err(|_| RuntimeError::Worker)?;
    let request = Request {
        protocol: PROTOCOL,
        family,
        cache_root,
        operation: Operation::Prepare(VerificationInput {
            product_version: input.product_version.clone(),
            module_sha256: input.module_sha256,
            token: crate::LaunchToken::new(Zeroizing::new(input.token.expose().to_string()))?,
        }),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&request).map_err(|_| RuntimeError::Protocol)?);
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err(RuntimeError::Protocol);
    }
    let job = Job::new()?;
    let mut child = spawn(&executable, scratch.path())?;
    if assign(&job, &child).is_err() {
        reap(&mut child, &job);
        return Err(RuntimeError::Worker);
    }
    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            reap(&mut child, &job);
            return Err(RuntimeError::Worker);
        }
    };
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            reap(&mut child, &job);
            return Err(RuntimeError::Worker);
        }
    };
    let started = Instant::now();
    let (written, writer) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut stdin = stdin;
        let result = write_frame(&mut stdin, &bytes).map(|()| stdin);
        let _ = written.send(result);
    });
    let stdin = match receive_before(&writer, &mut child, started, PREPARE_READY) {
        Ok(stdin) => stdin,
        Err(error) => {
            reap(&mut child, &job);
            return Err(error);
        }
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    // The reader owns the pipe. A timeout or an early child exit does not join
    // it: the pipe read unblocks when the child is reaped and this function
    // returns. Process teardown joins nothing that is still blocked.
    thread::spawn(move || {
        let result = read_frame(&mut stdout, crate::MAX_RECEIPT_BYTES).map(|body| (body, stdout));
        let _ = sender.send(result);
    });
    let (receipt, stdout) = match receive_before(&receiver, &mut child, started, PREPARE_READY) {
        Ok(receipt) => receipt,
        Err(error) => {
            reap(&mut child, &job);
            return Err(error);
        }
    };
    let receipt: WorkerReceipt = match serde_json::from_slice(&receipt) {
        Ok(receipt) => receipt,
        Err(_) => {
            reap(&mut child, &job);
            return Err(RuntimeError::Protocol);
        }
    };
    if let Err(error) = crate::checked_prepared(&receipt, family) {
        reap(&mut child, &job);
        return Err(error);
    }
    Ok(HeldSession { child, job, scratch, stdin: Some(stdin), stdout: Some(stdout) })
}

/// SDK child retained through verification. Not a game handle and not `Clone`.
/// `Child`, the job handle, and the scratch directory are all `Send`.
pub(crate) struct HeldSession {
    child: Child,
    job: Job,
    scratch: tempfile::TempDir,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
}

impl HeldSession {
    /// Process liveness alone is not SDK permission; `confirm` checks the latter.
    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub(crate) fn confirm(&mut self) -> Result<(), RuntimeError> {
        if !self.alive() {
            return Err(RuntimeError::Worker);
        }
        self.stdin.as_mut().ok_or(RuntimeError::Protocol)?.write_all(&[crate::CHECK]).map_err(|_| RuntimeError::Protocol)?;
        let mut stdout = self.stdout.take().ok_or(RuntimeError::Protocol)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut byte = [0_u8; 1];
            let result = stdout.read_exact(&mut byte).map(|()| (byte[0], stdout)).map_err(|_| RuntimeError::Protocol);
            let _ = sender.send(result);
        });
        let (value, stdout) = receive_before(&receiver, &mut self.child, Instant::now(), Duration::from_secs(2))?;
        self.stdout = Some(stdout);
        if value != crate::ALIVE {
            return Err(RuntimeError::Protocol);
        }
        Ok(())
    }
}

impl Drop for HeldSession {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(&[FINISH]);
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        loop {
            if self.child.try_wait().ok().flatten().is_some() || Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        reap(&mut self.child, &self.job);
        // `scratch` drops after the child, so pinned files are not removed first.
        let _ = &self.scratch;
    }
}

fn reap(child: &mut Child, _job: &Job) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn(executable: &Path, scratch: &Path) -> Result<Child, RuntimeError> {
    spawn_with(executable, &[WORKER_ARGUMENT], scratch)
}

fn spawn_with(executable: &Path, arguments: &[&str], scratch: &Path) -> Result<Child, RuntimeError> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(scratch)
        .env_clear()
        .env("TEMP", scratch)
        .env("TMP", scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", &root).env("WINDIR", &root).env("PATH", Path::new(&root).join("System32"));
    }
    let mut child = command.spawn().map_err(|_| RuntimeError::Worker)?;
    // One short secret-free failure line, kept for the host's log.
    if let Some(mut stderr) = child.stderr.take() {
        let id = crate::diag::spawned();
        thread::spawn(move || {
            // Keep only the tail; the failure line is the worker's last write.
            let mut tail = Vec::new();
            let mut chunk = [0_u8; 1024];
            while let Ok(read) = stderr.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                tail.extend_from_slice(&chunk[..read]);
                if tail.len() > 4096 {
                    tail.drain(..tail.len() - 2048);
                }
            }
            crate::diag::finished(id, &tail);
        });
    }
    Ok(child)
}

fn assign(job: &Job, child: &Child) -> Result<(), RuntimeError> {
    // SAFETY: Child owns a live process handle; the job remains owned by `job`.
    if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle().cast()) } == 0 { Err(RuntimeError::Worker) } else { Ok(()) }
}

fn receive_before<T>(
    receiver: &std::sync::mpsc::Receiver<Result<T, RuntimeError>>,
    child: &mut Child,
    started: Instant,
    timeout: Duration,
) -> Result<T, RuntimeError> {
    loop {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(RuntimeError::Worker);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if child.try_wait().map_err(|_| RuntimeError::Worker)?.is_some() {
                    return Err(RuntimeError::Worker);
                }
                if started.elapsed() >= timeout {
                    return Err(RuntimeError::Timeout);
                }
            }
        }
    }
}

fn write_frame(stdin: &mut impl Write, body: &[u8]) -> Result<(), RuntimeError> {
    let mut frame = Zeroizing::new(Vec::with_capacity(4 + body.len()));
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    stdin.write_all(&frame).map_err(|_| RuntimeError::Protocol)?;
    stdin.flush().map_err(|_| RuntimeError::Protocol)
}

fn run(executable: &Path, scratch: &Path, request: &[u8], deadline: Duration) -> Result<Zeroizing<Vec<u8>>, RuntimeError> {
    run_child(executable, &[WORKER_ARGUMENT], scratch, request, deadline)
}

fn run_child(
    executable: &Path,
    arguments: &[&str],
    scratch: &Path,
    request: &[u8],
    deadline: Duration,
) -> Result<Zeroizing<Vec<u8>>, RuntimeError> {
    let job = Job::new()?;
    let mut child = spawn_with(executable, arguments, scratch)?;
    // The worker reads one framed request before loading a library. Assignment
    // failure sends nothing and reaps the child.
    if assign(&job, &child).is_err() {
        reap(&mut child, &job);
        return Err(RuntimeError::Worker);
    }
    let (Some(mut stdout), Some(mut stdin)) = (child.stdout.take(), child.stdin.take()) else {
        reap(&mut child, &job);
        return Err(RuntimeError::Worker);
    };
    // Both pipes must make progress while the deadline is monitored. A full
    // input/output pipe must not suspend the parent outside its timeout loop.
    let request = Zeroizing::new(request.to_vec());
    let started = Instant::now();
    let writer = thread::spawn(move || {
        let written = write_frame(&mut stdin, &request);
        drop(stdin);
        written
    });
    let reader = thread::spawn(move || {
        let body = read_frame(&mut stdout, crate::MAX_RECEIPT_BYTES)?;
        let mut extra = [0_u8; 1];
        if stdout.read(&mut extra).map_err(|_| RuntimeError::Protocol)? != 0 {
            return Err(RuntimeError::Protocol);
        }
        Ok(body)
    });
    let status = (|| {
        loop {
            if let Some(status) = child.try_wait().map_err(|_| RuntimeError::Worker)? {
                return if status.success() { Ok(()) } else { Err(RuntimeError::Worker) };
            }
            if started.elapsed() >= deadline {
                return Err(RuntimeError::Timeout);
            }
            thread::sleep(Duration::from_millis(20));
        }
    })();
    // Reap before scratch or pins disappear. A timed-out child cannot be left.
    reap(&mut child, &job);
    drop(job);
    let written = writer.join();
    let output = reader.join();
    status?;
    written.map_err(|_| RuntimeError::Protocol)??;
    output.map_err(|_| RuntimeError::Protocol)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_has_process_memory_and_child_creation_limits() {
        let job = Job::new().unwrap();
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: test queries an owned live job into the matching structure.
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 1);
        assert_eq!(limits.BasicLimitInformation.LimitFlags & FLAGS, FLAGS);
        assert_eq!(limits.ProcessMemoryLimit, MEMORY_LIMIT);
    }

    #[test]
    fn job_denies_descendants_and_oversized_output_cannot_become_a_receipt() {
        let system = std::env::var_os("SystemRoot").unwrap();
        let command = Path::new(&system).join("System32").join("cmd.exe");
        let scratch = tempfile::tempdir().unwrap();
        // `run` spawns the executable directly. A second cmd is the descendant
        // the one-process job must reject.
        std::fs::write(scratch.path().join("spawn.cmd"), b"@echo off\r\n\"%SystemRoot%\\System32\\cmd.exe\" /D /C exit 0\r\n").unwrap();
        assert_eq!(run_child(&command, &["/D", "/C", "spawn.cmd"], scratch.path(), b"", Duration::from_secs(3)), Err(RuntimeError::Worker));
        std::fs::write(scratch.path().join("noisy.cmd"), b"@echo off\r\nfor /L %%i in (1,1,1000) do echo 01234567890123456789\r\n")
            .unwrap();
        assert!(run_child(&command, &["/D", "/C", "noisy.cmd"], scratch.path(), b"", Duration::from_secs(3)).is_err());
    }

    #[test]
    fn crashed_and_hung_workers_are_reaped_without_receipts() {
        let system = std::env::var_os("SystemRoot").unwrap();
        let command = Path::new(&system).join("System32").join("cmd.exe");
        let scratch = tempfile::tempdir().unwrap();
        assert_eq!(run_child(&command, &["/D", "/C", "exit 23"], scratch.path(), b"", Duration::from_secs(3)), Err(RuntimeError::Worker));
        // A scratch-only batch fixture starts no descendants, reads no profiles,
        // and performs no writes or network work. Only this child is terminated.
        std::fs::write(scratch.path().join("spin.cmd"), b"@echo off\r\n:again\r\ngoto again\r\n").unwrap();
        assert_eq!(
            run_child(&command, &["/D", "/C", "spin.cmd"], scratch.path(), b"", Duration::from_millis(150)),
            Err(RuntimeError::Timeout)
        );
    }

    #[test]
    fn full_input_and_output_pipes_do_not_escape_the_deadline() {
        let system = std::env::var_os("SystemRoot").unwrap();
        let command = Path::new(&system).join("System32").join("cmd.exe");
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(scratch.path().join("spin.cmd"), b"@echo off\r\n:again\r\ngoto again\r\n").unwrap();
        let started = Instant::now();
        assert_eq!(
            run_child(&command, &["/D", "/C", "spin.cmd"], scratch.path(), &[b'x'; MAX_REQUEST_BYTES as usize], Duration::from_millis(150)),
            Err(RuntimeError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        let body = vec![b'x'; crate::MAX_RECEIPT_BYTES];
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&body);
        std::fs::write(scratch.path().join("reply.bin"), &frame).unwrap();
        assert_eq!(
            run_child(&command, &["/D", "/C", "type reply.bin"], scratch.path(), b"", Duration::from_secs(3)).unwrap().as_slice(),
            body
        );
        frame.push(b'x');
        std::fs::write(scratch.path().join("reply.bin"), frame).unwrap();
        assert_eq!(
            run_child(&command, &["/D", "/C", "type reply.bin"], scratch.path(), b"", Duration::from_secs(3)),
            Err(RuntimeError::Protocol)
        );
    }

    #[test]
    fn dropping_a_held_session_reaps_the_child() {
        let system = std::env::var_os("SystemRoot").unwrap();
        let command = Path::new(&system).join("System32").join("cmd.exe");
        let scratch = tempfile::tempdir().unwrap();
        let job = Job::new().unwrap();
        std::fs::write(scratch.path().join("spin.cmd"), b"@echo off\r\n:again\r\ngoto again\r\n").unwrap();
        let child = spawn_with(&command, &["/D", "/C", "spin.cmd"], scratch.path()).unwrap();
        assign(&job, &child).unwrap();
        let mut duplicate = std::ptr::null_mut();
        // SAFETY: duplicate a live process handle only to observe its termination.
        assert_ne!(
            unsafe {
                windows_sys::Win32::Foundation::DuplicateHandle(
                    GetCurrentProcess(),
                    child.as_raw_handle().cast(),
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    0,
                    windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS,
                )
            },
            0
        );
        use std::os::windows::io::FromRawHandle;
        // SAFETY: this new non-inheritable duplicate is owned by the test.
        let probe = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(duplicate) };
        let held = HeldSession { child, job, scratch, stdin: None, stdout: None };
        let started = Instant::now();
        drop(held);
        // SAFETY: observe the same process, not a potentially reused numeric PID.
        assert_eq!(
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(probe.as_raw_handle().cast(), 1000,) },
            windows_sys::Win32::Foundation::WAIT_OBJECT_0
        );
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn a_living_worker_needs_the_exact_final_health_acknowledgment() {
        for reply in [vec![crate::ALIVE], vec![99], vec![]] {
            let system = std::env::var_os("SystemRoot").unwrap();
            let scratch = tempfile::tempdir().unwrap();
            std::fs::write(scratch.path().join("reply.bin"), &reply).unwrap();
            std::fs::write(scratch.path().join("worker.cmd"), b"@echo off\r\ntype reply.bin\r\n:again\r\ngoto again\r\n").unwrap();
            let job = Job::new().unwrap();
            let mut child =
                spawn_with(&Path::new(&system).join("System32").join("cmd.exe"), &["/D", "/C", r".\worker.cmd"], scratch.path()).unwrap();
            assign(&job, &child).unwrap();
            let stdin = child.stdin.take();
            let stdout = child.stdout.take();
            let mut held = HeldSession { child, job, scratch, stdin, stdout };
            let started = Instant::now();
            let result = held.confirm();
            let was_alive = held.alive();
            reap(&mut held.child, &held.job);
            drop(held);
            assert!(was_alive);
            if reply == [crate::ALIVE] {
                assert_eq!(result, Ok(()));
            } else if reply.is_empty() {
                assert_eq!(result, Err(RuntimeError::Timeout));
            } else {
                assert_eq!(result, Err(RuntimeError::Protocol));
            }
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }

    #[test]
    fn a_verification_receipt_is_not_a_prepared_hold() {
        let receipt = WorkerReceipt::Verification(crate::VerificationReceipt {
            protocol: PROTOCOL,
            family: konaste_bootstrap::GameFamily::Bombergirl,
            status: crate::VerificationStatus::Allowed,
        });
        assert_eq!(crate::checked_prepared(&receipt, GameFamily::Bombergirl), Err(RuntimeError::Protocol));
        let denied = WorkerReceipt::Prepared(crate::VerificationReceipt {
            protocol: PROTOCOL,
            family: konaste_bootstrap::GameFamily::Bombergirl,
            status: crate::VerificationStatus::Denied,
        });
        assert_eq!(crate::checked_prepared(&denied, GameFamily::Bombergirl), Err(RuntimeError::VerificationDenied));
    }
}

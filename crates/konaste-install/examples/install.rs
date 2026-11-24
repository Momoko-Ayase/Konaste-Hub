//! Explicit anonymous content checks. Never targets a detected/imported install,
//! starts an official executable, requests an account token, or writes registry.
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

use konaste_bootstrap::GameFamily;
use konaste_install::UpdateSource;
use konaste_manifest::ResourceFile;
use konaste_runtime::{Title, WORKER_ARGUMENT};
use konaste_updater::{Manifest, ManifestFile, SessionPhase, Sha256, Updater, UpdaterOptions};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const DIRECTORY_PREFIX: &str = "konaste-install-check-";
const SAMPLE_LIMIT: u64 = 16 * 1024 * 1024;

fn main() {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|value| value == WORKER_ARGUMENT) {
        let success = arguments.len() == 1 && konaste_runtime::worker_main().is_ok();
        std::process::exit(if success { 0 } else { 1 });
    }
    let family = match take_game(&mut arguments) {
        Ok(family) => family,
        Err(message) => {
            eprintln!("{message}");
            usage();
            std::process::exit(2);
        }
    };
    let runtime_library = if arguments.first().is_some_and(|value| value == "--runtime-library") {
        if arguments.len() < 2 {
            usage();
            std::process::exit(2);
        }
        let path = std::path::PathBuf::from(arguments.remove(1));
        arguments.remove(0);
        Some(path)
    } else {
        None
    };
    if !(arguments.len() == 2 && arguments[1] == "--probe"
        || arguments.len() == 3 && (arguments[1] == "--fresh-in" || arguments[1] == "--resume-test"))
    {
        usage();
        std::process::exit(2);
    }
    if arguments.len() == 3 && arguments[1] == "--resume-test" {
        require_test_root(family, Path::new(&arguments[2])).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
    }
    let result = (|| -> Result<()> {
        let title = konaste_runtime::title(family)?;
        let cache = tempfile::tempdir()?;
        konaste_bootstrap::test_support::acquire_from_verified_files(
            family,
            cache.path(),
            Path::new(&arguments[0]),
            runtime_library.as_deref(),
        )?;
        let metadata = discover(family, cache.path())?;
        println!(
            "family={} revision={} files={} index_sha256={}",
            metadata.family().as_str(),
            metadata.index.revision,
            metadata.index.files.len(),
            hex(&metadata.sha256)
        );
        println!(
            "module={} index={} content_root={}",
            title.module_path,
            title.index_path,
            if title.content_root.is_empty() { "/" } else { title.content_root }
        );
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        if arguments[1] == "--probe" {
            runtime.block_on(probe(family, metadata))
        } else if arguments[1] == "--resume-test" {
            let root = Path::new(&arguments[2]);
            require_test_root(family, root)?;
            runtime.block_on(complete(family, metadata, root))
        } else {
            let destination = tempfile::Builder::new().prefix(DIRECTORY_PREFIX).tempdir_in(Path::new(&arguments[2]))?.keep();
            println!("new_test_directory={}", destination.display());
            runtime.block_on(fresh(family, metadata, &destination, cache.path()))
        }
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!(
        "Usage: cargo run --offline -j 2 -p konaste-install --example install -- [--game <family>] [--runtime-library <pinned-DLL>] <retained-pinned-official-MSI> --probe"
    );
    eprintln!(
        "   or: cargo run --offline -j 2 -p konaste-install --example install -- [--game <family>] [--runtime-library <pinned-DLL>] <retained-pinned-official-MSI> --fresh-in <parent-of-NEW-test-directory>"
    );
    eprintln!("   or: ... --resume-test <previous-konaste-install-check-directory>");
    eprintln!("       --game defaults to bombergirl");
}

/// Optional leading `--game <family>`; Bombergirl when absent.
fn take_game(arguments: &mut Vec<OsString>) -> std::result::Result<GameFamily, &'static str> {
    if arguments.first().is_none_or(|value| value != "--game") {
        return Ok(GameFamily::Bombergirl);
    }
    if arguments.len() < 2 {
        return Err("--game requires a pinned family name");
    }
    let name = arguments.remove(1);
    arguments.remove(0);
    name.to_str().and_then(GameFamily::parse).ok_or("unknown --game family")
}

fn marker_name(family: GameFamily) -> String {
    format!(".konaste-install-check-{}", family.as_str())
}

fn marker_bytes(family: GameFamily) -> Vec<u8> {
    format!("Konaste Hub isolated {} install check\n", family.as_str()).into_bytes()
}

fn discover(family: GameFamily, cache: &Path) -> Result<konaste_discovery::CurrentMetadata> {
    let metadata = konaste_discovery::check(family, cache, &std::env::current_exe()?)?;
    if metadata.family() != family {
        return Err("discovered metadata is not the selected title".into());
    }
    Ok(metadata)
}

fn require_test_root(family: GameFamily, root: &Path) -> Result<()> {
    if !root.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(DIRECTORY_PREFIX)) {
        return Err("resume accepts only a directory created by --fresh-in".into());
    }
    let expected = marker_bytes(family);
    let mut bytes = Vec::new();
    std::fs::File::open(root.join(marker_name(family)))?.take(expected.len() as u64 + 1).read_to_end(&mut bytes)?;
    if bytes != expected {
        return Err("missing or invalid isolated-test marker".into());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn session_options(source: &UpdateSource) -> UpdaterOptions {
    UpdaterOptions::new(source.download_policy())
}

fn content_destination(title: &Title, path: &str) -> Result<String> {
    if path.is_empty() || path.contains('\\') || path.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Err("sample path is not a content-relative path".into());
    }
    let relative = if title.content_root.is_empty() { path.to_string() } else { format!("{}/{}", title.content_root, path) };
    if relative.eq_ignore_ascii_case(title.index_path) {
        return Err("sample collides with the official index".into());
    }
    Ok(relative)
}

fn smallest(files: &[ResourceFile], pred: impl Fn(&ResourceFile) -> bool) -> Option<&ResourceFile> {
    files.iter().filter(|file| pred(file) && file.stored_size > 0 && file.stored_size <= SAMPLE_LIMIT).min_by_key(|file| file.stored_size)
}

fn title_confirmed(title: &Title, file: &ResourceFile) -> bool {
    let path = file.url.path();
    path.starts_with(title.content_prefix)
        && path.ends_with(&format!("/{}", file.path))
        && !file.path.eq_ignore_ascii_case(title.index_path)
}

async fn probe(family: GameFamily, metadata: konaste_discovery::CurrentMetadata) -> Result<()> {
    let title = konaste_runtime::title(family)?;
    let body = metadata.bytes().to_vec();
    let digest = metadata.sha256;
    let source = UpdateSource::from_metadata(metadata)?;
    if source.family() != family {
        return Err("update source is not the selected title".into());
    }
    let index = konaste_manifest::decode(&body, digest, &[title.host]).map_err(|_| "authenticated index rejected")?;
    let module_directory =
        title.manifest_module.rsplit_once('/').map(|(parent, _)| format!("{parent}/")).ok_or("title has no module directory")?;
    let module = smallest(&index.files, |file| file.path == title.manifest_module && title_confirmed(title, file))
        .or_else(|| smallest(&index.files, |file| file.path.starts_with(&module_directory) && title_confirmed(title, file)));
    let store = smallest(&index.files, |file| file.path.starts_with("x/") && title_confirmed(title, file)).or_else(|| {
        smallest(&index.files, |file| {
            !file.path.starts_with(&module_directory) && !file.path.starts_with("launcher/") && title_confirmed(title, file)
        })
    });
    // Prefer the checked main module when it fits, plus a small content object.
    // Unknown storage layouts are not transformed or reclassified as archives.
    let mut chosen: Vec<&ResourceFile> = [module, store].into_iter().flatten().collect();
    if chosen.is_empty() {
        chosen.extend(smallest(&index.files, |file| title_confirmed(title, file)));
    }
    if chosen.is_empty() {
        return Err("no module, opaque store, or title-confirmed sample within 16 MiB".into());
    }
    let mut files = Vec::new();
    for entry in &chosen {
        println!("sample={} stored_bytes={}", entry.path, entry.stored_size);
        files.push(ManifestFile {
            relative_path: content_destination(title, &entry.path)?,
            size: entry.stored_size,
            sha256: Sha256::parse(&hex(&entry.stored_sha256))?,
            url: Some(entry.url.to_string()),
            etag: None,
        });
    }
    let bytes: u64 = files.iter().map(|file| file.size).sum();
    let count = files.len();
    let directory = tempfile::tempdir()?;
    let updater = Updater::open_fresh_with(directory.path(), session_options(&source))?;
    updater.prepare(Manifest { version: source.revision().to_string(), files }, None)?;
    run(&updater).await?;
    if directory.path().join(title.index_path).exists() {
        return Err("official index was published for a sample-only tree".into());
    }
    println!("identity_transport_verified={count} samples stored_bytes={bytes}");
    println!(
        "Sample bodies matched the freshly discovered stored size and SHA-256 through the production updater. No official index was published for this sample-only tree."
    );
    Ok(())
}

async fn fresh(family: GameFamily, metadata: konaste_discovery::CurrentMetadata, root: &Path, cache: &Path) -> Result<()> {
    if std::fs::read_dir(root)?.next().is_some() {
        return Err("fresh test destination is not empty".into());
    }
    {
        let source = UpdateSource::from_metadata(metadata)?;
        if source.family() != family {
            return Err("update source is not the selected title".into());
        }
        let updater = Updater::open_fresh_with(root, session_options(&source))?;
        let plan = source.prepare(&updater)?;
        let mut marker = std::fs::OpenOptions::new().write(true).create_new(true).open(root.join(marker_name(family)))?;
        marker.write_all(&marker_bytes(family))?;
        marker.sync_all()?;
        drop(marker);
        println!("fresh_plan_files={} download_bytes={} unchanged={}", plan.download_files, plan.download_bytes, plan.unchanged_files);
        pause_after_partial(&updater).await?;
        let index = konaste_runtime::title(family)?.index_path;
        if root.join(index).exists() {
            return Err("official index appeared before the changed delta completed".into());
        }
    }
    // Release every session/lock, then obtain fresh official metadata before
    // reopening the durable transaction. Never recover from an unbound journal.
    let cache = cache.to_path_buf();
    let metadata = tokio::task::spawn_blocking(move || discover(family, &cache)).await??;
    complete(family, metadata, root).await
}

async fn complete(family: GameFamily, metadata: konaste_discovery::CurrentMetadata, root: &Path) -> Result<()> {
    require_test_root(family, root)?;
    let title = konaste_runtime::title(family)?;
    let pin = metadata.installation_pin()?;
    let source = UpdateSource::from_metadata(metadata)?;
    if source.family() != family {
        return Err("update source is not the selected title".into());
    }
    let updater = Updater::open_with(root, session_options(&source))?;
    source.prepare(&updater)?;
    updater.resume();
    println!("reopened_with_fresh_metadata=true");
    run(&updater).await?;
    let verify = source.prepare(&updater)?;
    if verify.download_files != 0 {
        return Err("installed content no longer matches the authenticated index".into());
    }
    run(&updater).await?;
    let installation = konaste_runtime::inspect_installation(family, root, &pin)?;
    if !root.join(title.index_path).is_file() || !root.join(title.module_path).is_file() {
        return Err("official index or module is missing".into());
    }
    println!("fresh_install_verified_files={} product_version={}", verify.unchanged_files, installation.product_version());
    println!(
        "Exact {} published last. No existing installation, registry, account session, or game process was touched.",
        title.index_path
    );
    Ok(())
}

async fn pause_after_partial(updater: &Updater) -> Result<()> {
    let work = updater.run_guarded(|| Ok(()));
    tokio::pin!(work);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut requested = false;
    loop {
        tokio::select! {
            result = &mut work => {
                if result? != SessionPhase::Paused || !requested {
                    return Err("fresh test did not pause during a partial transfer".into());
                }
                let progress = updater.progress();
                println!("paused_partial_bytes={} total={}", progress.bytes_received, progress.bytes_total);
                return Ok(());
            }
            _ = interval.tick() => {
                if !requested && updater.progress().bytes_received >= 4 * 1024 * 1024 {
                    updater.pause();
                    requested = true;
                }
            }
        }
    }
}

async fn run(updater: &Updater) -> Result<()> {
    let work = updater.run_guarded(|| Ok(()));
    tokio::pin!(work);
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            result = &mut work => {
                if result? != SessionPhase::Idle {
                    return Err("content check unexpectedly paused".into());
                }
                return Ok(());
            }
            _ = interval.tick() => {
                let progress = updater.progress();
                println!("phase={:?} received={}/{} files={}/{}", progress.phase, progress.bytes_received, progress.bytes_total, progress.files_completed, progress.files_total);
            }
        }
    }
}

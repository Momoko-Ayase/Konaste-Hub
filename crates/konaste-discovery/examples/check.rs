//! Explicit read-only live check with an already-retained pinned official MSI.
//! No download consent is implied by this example: package acquisition is offline.
use konaste_bootstrap::GameFamily;

fn main() {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|value| value == konaste_runtime::WORKER_ARGUMENT) {
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
    if !(arguments.len() == 1 || arguments.len() == 2 && arguments[1] == "--locate") {
        usage();
        std::process::exit(2);
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        konaste_runtime::discovery_profile(family)?;
        let cache = tempfile::tempdir()?;
        konaste_bootstrap::test_support::acquire_from_verified_files(
            family,
            cache.path(),
            std::path::Path::new(&arguments[0]),
            runtime_library.as_deref(),
        )?;
        if arguments.len() == 2 {
            let location = konaste_runtime::discover(family, cache.path(), &std::env::current_exe()?)?;
            println!("family={} index_url={} index_sha256={}", family.as_str(), location.url, location.sha256);
            println!("Location only. No index or content request, installation write, account token, or game launch.");
            return Ok(());
        }
        let metadata = konaste_discovery::check(family, cache.path(), &std::env::current_exe()?)?;
        if metadata.family() != family {
            return Err("discovered metadata is not the selected title".into());
        }
        let total: u64 = metadata.index.files.iter().map(|file| file.stored_size).sum();
        println!(
            "family={} revision={} files={} stored_bytes={total}",
            family.as_str(),
            metadata.index.revision,
            metadata.index.files.len()
        );
        println!("index_sha256={}", metadata.sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        println!("index_url={}", metadata.index_url());
        for file in metadata
            .index
            .files
            .iter()
            .filter(|file| {
                file.path.ends_with(".exe")
                    || file.path.ends_with("/libeacnet.dll")
                    || file.path.ends_with("/avs2-core.dll")
                    || file.path.ends_with("/libsettings.dll")
                    || file.path.ends_with(".xml")
            })
            .take(24)
        {
            println!(
                "module={} stored_bytes={} url={} sha256={}",
                file.path,
                file.stored_size,
                file.url,
                file.stored_sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
            );
        }
        println!("No game payload, installation write, account token, or game launch.");
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!(
        "Usage: cargo run --offline -j 2 -p konaste-discovery --example check -- [--game <family>] [--runtime-library <pinned-DLL>] <retained-pinned-official-MSI> [--locate]"
    );
    eprintln!("       --game defaults to bombergirl");
}

/// Optional leading `--game <family>`; Bombergirl when absent.
fn take_game(arguments: &mut Vec<std::ffi::OsString>) -> Result<GameFamily, &'static str> {
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

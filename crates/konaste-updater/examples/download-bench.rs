//! Download-phase scaling check. Bodies are synthetic and served on loopback.
//! The run stops immediately before the first installation, so destination
//! copies are not measured. No game directory, official content, account, or
//! external network is used.
#[path = "../tests/support/download_bench.rs"]
mod fixture;

#[tokio::main(flavor = "current_thread")]
async fn main() -> fixture::Result<()> {
    let count = std::env::args().nth(1).map(|value| value.parse()).transpose()?.unwrap_or(48_usize);
    if !(8..=2048).contains(&count) {
        return Err("file count must be 8..=2048".into());
    }
    let small = fixture::measure(count).await?;
    let large = fixture::measure(count * 2).await?;
    for value in [&small, &large] {
        println!(
            "files={} download_ms={} native_write_bytes={} native_write_calls={} journal_bytes={} staged_bytes={}",
            value.files,
            value.elapsed.as_millis(),
            value.written,
            value.writes,
            value.journal_bytes,
            value.staged_bytes
        );
    }
    let ratio = large.written as f64 / small.written.max(1) as f64;
    println!("doubling_files_write_ratio={ratio:.3}");
    if large.written > small.written.saturating_mul(3) {
        return Err("download metadata writes scale quadratically rather than approximately linearly".into());
    }
    Ok(())
}

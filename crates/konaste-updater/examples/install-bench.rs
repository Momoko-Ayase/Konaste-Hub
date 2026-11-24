//! Offline installation-only scaling check. All files are synthetic and already
//! staged. No game directory, official content, account, or network is used.
#[path = "../tests/support/install_bench.rs"]
mod fixture;

#[tokio::main(flavor = "current_thread")]
async fn main() -> fixture::Result<()> {
    let count = std::env::args().nth(1).map(|value| value.parse()).transpose()?.unwrap_or(160_usize);
    if !(16..=2000).contains(&count) {
        return Err("file count must be 16..=2000".into());
    }
    let small = fixture::measure(count).await?;
    let large = fixture::measure(count * 2).await?;
    for value in [&small, &large] {
        println!(
            "files={} install_ms={} native_write_bytes={} native_write_calls={}",
            value.files,
            value.elapsed.as_millis(),
            value.written,
            value.writes
        );
    }
    let ratio = large.written as f64 / small.written.max(1) as f64;
    println!("doubling_files_write_ratio={ratio:.3}");
    if large.written > small.written.saturating_mul(3) {
        return Err("installation metadata writes scale quadratically rather than approximately linearly".into());
    }
    Ok(())
}

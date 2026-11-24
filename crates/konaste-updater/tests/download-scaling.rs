#[path = "support/download_bench.rs"]
mod fixture;

// A separate integration-test process keeps Windows process I/O counters free
// from other unit tests. Assert write amplification of the download phase only.
#[tokio::test]
async fn download_journal_writes_scale_with_file_count() {
    let small = fixture::measure(48).await.unwrap();
    let large = fixture::measure(96).await.unwrap();
    for value in [&small, &large] {
        eprintln!(
            "files={} download_ms={} native_write_bytes={} native_write_calls={} journal_bytes={} staged_bytes={}",
            value.files,
            value.elapsed.as_millis(),
            value.written,
            value.writes,
            value.journal_bytes,
            value.staged_bytes
        );
    }
    assert!(
        large.written <= small.written.saturating_mul(3),
        "doubling downloaded files must not quadruple download-phase writes: {small:?}, {large:?}"
    );
}

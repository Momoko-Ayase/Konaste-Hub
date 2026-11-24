#[path = "support/install_bench.rs"]
mod fixture;

// A separate integration-test process keeps Windows process I/O counters free
// from other unit tests. Assert write amplification, not a flaky wall-clock cap.
#[tokio::test]
async fn installation_metadata_writes_scale_with_file_count() {
    let small = fixture::measure(160).await.unwrap();
    let large = fixture::measure(320).await.unwrap();
    for value in [&small, &large] {
        eprintln!(
            "files={} install_ms={} native_write_bytes={} native_write_calls={}",
            value.files,
            value.elapsed.as_millis(),
            value.written,
            value.writes
        );
    }
    assert!(
        large.written <= small.written.saturating_mul(3),
        "doubling staged files must not quadruple installation metadata writes: {small:?}, {large:?}"
    );
}

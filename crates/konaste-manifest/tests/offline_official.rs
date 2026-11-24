//! Read explicitly supplied retained indexes without fetching or copying them.
//! These pins validate research inputs, not freshness for a production check.

use std::{fs::File, io::Read};

use konaste_manifest::{MAX_INDEX_BYTES, ResourceIndex, decode};

fn retained_index(variable: &str, pin: &str, host: &str) -> ResourceIndex {
    let path = std::env::var_os(variable).unwrap_or_else(|| panic!("set {variable} to the retained resource-index path"));
    let file = File::open(path).expect("open retained index read-only");
    let metadata = file.metadata().expect("read retained index metadata");
    assert!(metadata.is_file());
    assert!(metadata.len() <= MAX_INDEX_BYTES as u64);
    let mut bytes = Vec::new();
    file.take(MAX_INDEX_BYTES as u64 + 1).read_to_end(&mut bytes).expect("read retained index without executing any official code");
    let mut digest = [0; 32];
    assert_eq!(pin.len(), 64);
    for (position, value) in digest.iter_mut().enumerate() {
        *value = u8::from_str_radix(&pin[position * 2..position * 2 + 2], 16).unwrap();
    }
    decode(&bytes, digest, &[host]).expect("pinned retained official index must decode completely")
}

#[test]
#[ignore = "needs KONASTE_OFFLINE_BG_MANIFEST; read-only, no network or DLL execution"]
fn retained_bombergirl_2026091500_matches_the_verified_structural_count() {
    let index = retained_index(
        "KONASTE_OFFLINE_BG_MANIFEST",
        "205bb4a45e1b38ad421ea8b0ab5c41214b99c7cc5714bbdecf8d15dd3e881e9a",
        "d1q1iflay6y5jj.cloudfront.net",
    );
    assert_eq!(index.revision, 2026091500);
    assert_eq!(index.files.len(), 8736);
    assert_eq!(index.files.iter().filter(|file| file.path.starts_with("x/")).count(), 8720);
    assert_eq!(index.files.iter().filter(|file| file.stored_size == 0).count(), 2820);
    assert!(index.files.iter().all(|file| file.revision == index.revision));
}

#[test]
#[ignore = "needs KONASTE_OFFLINE_BG_OLD_MANIFEST; read-only, no network or DLL execution"]
fn retained_bombergirl_2026041400_matches_the_verified_structural_count() {
    let index = retained_index(
        "KONASTE_OFFLINE_BG_OLD_MANIFEST",
        "e09db7544263e3155c902425edcb9a5ee322ea8436b01aa7b61a61a00962fe3c",
        "d1q1iflay6y5jj.cloudfront.net",
    );
    assert_eq!(index.revision, 2026041400);
    assert_eq!(index.files.len(), 8321);
    assert_eq!(index.files.iter().filter(|file| file.path.starts_with("x/")).count(), 8305);
    assert_eq!(index.files.iter().filter(|file| file.stored_size == 0).count(), 2820);
    assert!(index.files.iter().all(|file| file.revision == index.revision));
}

#[test]
#[ignore = "needs KONASTE_OFFLINE_SDVX_MANIFEST; read-only, no network or DLL execution"]
fn retained_sdvx_2026070300_decodes_the_verified_main_module() {
    let index = retained_index(
        "KONASTE_OFFLINE_SDVX_MANIFEST",
        "97697900e0ea87faf6caaf8d55b024f5204b867a613857c55db7d00540beb3b5",
        "dks1q2aivwkd6.cloudfront.net",
    );
    assert_eq!(index.revision, 2026070300);
    assert_eq!(index.files.len(), 19311);
    let main = index.files.iter().find(|file| file.path == "game/modules/sv6c.exe").expect("main module");
    assert_eq!(main.revision, 2026070300);
    assert_eq!(main.stored_size, 16_338_056);
    assert_eq!(
        main.stored_sha256,
        [
            0x20, 0xcc, 0xfd, 0x6c, 0x76, 0x3f, 0x67, 0xd7, 0x1d, 0x79, 0x46, 0x36, 0xde, 0x82, 0x93, 0xdd, 0xff, 0xdb, 0x49, 0xdc, 0x08,
            0x36, 0x43, 0x62, 0x4d, 0xc1, 0xac, 0x95, 0xa6, 0xad, 0xb1, 0x1d,
        ]
    );
    assert_eq!(main.url.as_str(), "https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource/2026070300/game/modules/sv6c.exe");
}

#[test]
#[ignore = "needs KONASTE_OFFLINE_INFINITAS_MANIFEST; read-only, no network or DLL execution"]
fn retained_infinitas_2026111100_uses_the_existing_packed_index_decoder() {
    let index = retained_index(
        "KONASTE_OFFLINE_INFINITAS_MANIFEST",
        "314dfcab394868ff6655be43908a35f3c2e3179138e4778a8c027a2ae6598835",
        "d1rc4pwxnc0pe0.cloudfront.net",
    );
    assert_eq!(index.revision, 2026111100);
    assert_eq!(index.files.len(), 3302);
    assert_eq!(index.files.iter().map(|file| file.stored_size).sum::<u64>(), 3_021_076_215);
    let mut game_files = 0;
    let mut launcher_files = 0;
    for file in &index.files {
        assert_eq!(file.url.path(), format!("/v3/resource/distribution/jp/{}/{}", file.revision, file.path));
        if file.path.starts_with("game/") {
            game_files += 1;
        } else {
            assert!(file.path.starts_with("launcher/"));
            launcher_files += 1;
        }
    }
    assert_eq!((game_files, launcher_files), (3288, 14));
    assert!(index.files.iter().any(|file| file.revision != index.revision));
    let main = index.files.iter().find(|file| file.path == "game/app/bm2dx.exe").expect("main module");
    assert_eq!(main.revision, index.revision);
    assert_eq!(main.stored_size, 16_087_176);
}

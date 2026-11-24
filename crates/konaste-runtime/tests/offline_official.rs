//! Explicit retained-file read check. No network, game execution, or installed
//! tree mutation. Both inputs are copied into a new temporary fixture.
#![cfg(all(windows, target_arch = "x86_64"))]

use konaste_bootstrap::GameFamily;
use konaste_runtime::{InstallationPin, inspect_installation, title};
use sha2::{Digest, Sha256};

fn digest(value: &str) -> [u8; 32] {
    std::array::from_fn(|index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap())
}

#[test]
#[ignore = "needs KONASTE_OFFLINE_SDVX_MANIFEST and KONASTE_OFFLINE_SDVX_MODULE; no network or execution"]
fn current_sdvx_index_and_module_pass_the_production_read_check() {
    let index = std::env::var_os("KONASTE_OFFLINE_SDVX_MANIFEST").expect("retained SDVX index");
    let module = std::env::var_os("KONASTE_OFFLINE_SDVX_MODULE").expect("retained SDVX module");
    let index = std::fs::read(index).unwrap();
    let module = std::fs::read(module).unwrap();
    let pin = InstallationPin {
        index_sha256: digest("97697900e0ea87faf6caaf8d55b024f5204b867a613857c55db7d00540beb3b5"),
        module_sha256: digest("20ccfd6c763f67d71d794636de8293ddffdb49dc083643624dc1ac95a6adb11d"),
        module_bytes: 16_338_056,
    };
    assert_eq!(<[u8; 32]>::from(Sha256::digest(&index)), pin.index_sha256);
    assert_eq!(<[u8; 32]>::from(Sha256::digest(&module)), pin.module_sha256);
    assert_eq!(module.len() as u64, pin.module_bytes);
    let root = tempfile::tempdir().unwrap();
    let title = title(GameFamily::Sdvx).unwrap();
    let image = root.path().join(title.module_path);
    std::fs::create_dir_all(image.parent().unwrap()).unwrap();
    std::fs::write(&image, module).unwrap();
    std::fs::write(root.path().join(title.index_path), index).unwrap();
    let game = inspect_installation(GameFamily::Sdvx, root.path(), &pin).unwrap();
    assert_eq!(game.family(), GameFamily::Sdvx);
    assert_eq!(game.product_version(), "QCV:J:B:A:2026070300");
    assert!(std::fs::write(&image, b"must stay pinned").is_err());
}

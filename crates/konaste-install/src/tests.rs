use super::*;
use sha2::{Digest, Sha256 as Hasher};

const HOST: &str = "d1q1iflay6y5jj.cloudfront.net";
const MODULE: &str = "modules/bomber.exe";

const EMPTY: &str = "x/a/b/c/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MODULE_BYTES: &[u8] = b"synthetic module";

struct Entry {
    path: String,
    bytes: Vec<u8>,
    url: String,
}

impl Entry {
    fn new(path: &str, bytes: &[u8]) -> Self {
        Self { path: path.into(), bytes: bytes.to_vec(), url: format!("https://{HOST}/Updater/synthetic/build/{path}") }
    }
}

struct Encoder {
    nodes: Vec<u8>,
    data: Vec<u8>,
}

impl Encoder {
    fn open(&mut self, kind: u8, name: &str) {
        const ALPHABET: &[u8] = b"0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
        self.nodes.push(kind);
        self.nodes.push(name.len() as u8);
        let start = self.nodes.len();
        self.nodes.resize(start + (name.len() * 6).div_ceil(8), 0);
        for (position, value) in name.bytes().enumerate() {
            let encoded = ALPHABET.iter().position(|character| *character == value).unwrap();
            for bit in 0..6 {
                if encoded & (1 << (5 - bit)) != 0 {
                    let output = position * 6 + bit;
                    self.nodes[start + output / 8] |= 1 << (7 - output % 8);
                }
            }
        }
    }

    fn close(&mut self) {
        self.nodes.push(0xfe);
    }

    fn text(&mut self, name: &str, value: &str) {
        self.open(0x0b, name);
        self.data.resize(self.data.len().next_multiple_of(4), 0);
        self.data.extend(((value.len() + 1) as u32).to_be_bytes());
        self.data.extend(value.as_bytes());
        self.data.push(0);
        self.data.resize(self.data.len().next_multiple_of(4), 0);
        self.close();
    }

    fn number(&mut self, name: &str, value: u32) {
        self.open(0x06, name);
        self.data.resize(self.data.len().next_multiple_of(4), 0);
        self.data.extend(value.to_be_bytes());
        self.close();
    }
}

fn body(entries: &[Entry]) -> Vec<u8> {
    let mut wire = Encoder { nodes: Vec::new(), data: Vec::new() };
    wire.open(0x01, "resource_info");
    for entry in entries {
        wire.open(0x01, "file");
        wire.text("path", &entry.path);
        wire.number("version", 2026010100);
        wire.number("size", entry.bytes.len() as u32);
        wire.text("sum", &hex::encode(Hasher::digest(&entry.bytes)));
        wire.text("url", &entry.url);
        wire.close();
    }
    wire.number("ri_ver", 2026020200);
    wire.close();
    wire.nodes.push(0xff);
    wire.nodes.resize(wire.nodes.len().next_multiple_of(4), 0);
    let mut bytes = vec![0xa0, 0x42, 0x00, 0xff];
    bytes.extend((wire.nodes.len() as u32).to_be_bytes());
    bytes.extend(wire.nodes);
    bytes.extend((wire.data.len() as u32).to_be_bytes());
    bytes.extend(wire.data);
    bytes
}

fn entries() -> Vec<Entry> {
    vec![Entry::new(MODULE, MODULE_BYTES), Entry::new(EMPTY, b"")]
}

fn source(bytes: &[u8]) -> Result<UpdateSource, SourceError> {
    UpdateSource::decode(GameFamily::Bombergirl, bytes, Hasher::digest(bytes).into())
}

#[test]
fn translation_preserves_order_stored_hashes_revision_and_original_index() {
    let bytes = body(&entries());
    let source = source(&bytes).unwrap();
    assert_eq!(source.revision(), 2026020200);
    assert_eq!(source.manifest.version, "2026020200");
    assert_eq!(source.manifest.files.len(), 2);
    assert_eq!(source.manifest.files[0].relative_path, MODULE);
    assert_eq!(source.manifest.files[0].size, MODULE_BYTES.len() as u64);
    assert_eq!(source.manifest.files[0].sha256.as_str(), hex::encode(Hasher::digest(MODULE_BYTES)));
    assert_eq!(source.manifest.files[1].relative_path, EMPTY);
    assert_eq!(source.manifest.files[1].size, 0);
    assert!(source.manifest.files.iter().all(|file| file.etag.is_none()));
    assert_eq!(source.official.path, "ri.bin");
    assert_eq!(source.official.bytes, bytes);
}

#[tokio::test]
async fn admitted_titles_map_content_and_publish_their_own_index() {
    for family in GameFamily::ALL {
        let Ok(title) = konaste_runtime::title(family) else {
            continue;
        };
        let files = [title.manifest_module, "data/synthetic-empty.bin"].map(|path| Entry {
            path: path.into(),
            bytes: if path == title.manifest_module { MODULE_BYTES.to_vec() } else { Vec::new() },
            url: format!("https://{}{}synthetic/{path}", title.host, title.content_prefix),
        });
        let bytes = body(&files);
        let source = UpdateSource::decode(family, &bytes, Hasher::digest(&bytes).into()).unwrap();
        assert_eq!(source.family(), family);
        assert_eq!(source.official.path, title.index_path);
        let root = tempfile::tempdir().unwrap();
        for (entry, planned) in files.iter().zip(&source.manifest.files) {
            let expected_path =
                if title.content_root.is_empty() { entry.path.clone() } else { format!("{}/{}", title.content_root, entry.path) };
            assert_eq!(planned.relative_path, expected_path);
            let path = root.path().join(&planned.relative_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &entry.bytes).unwrap();
        }
        let updater = Updater::open(root.path(), source.download_policy()).unwrap();
        assert_eq!(source.prepare(&updater).unwrap().download_files, 0);
        updater.run().await.unwrap();
        assert_eq!(std::fs::read(root.path().join(title.index_path)).unwrap(), bytes);
        assert_eq!(std::fs::read(root.path().join(title.module_path)).unwrap(), MODULE_BYTES);
    }
}

#[test]
fn one_titles_index_cannot_supply_another_titles_source() {
    let bytes = body(&entries());
    for family in GameFamily::ALL {
        if family != GameFamily::Bombergirl {
            assert!(UpdateSource::decode(family, &bytes, Hasher::digest(&bytes).into()).is_err());
        }
    }
}

#[test]
fn metadata_requires_the_independently_supplied_digest() {
    let bytes = body(&entries());
    assert!(matches!(UpdateSource::decode(GameFamily::Bombergirl, &bytes, [0; 32]), Err(SourceError::InvalidMetadata)));
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(UpdateSource::decode(GameFamily::Bombergirl, &changed, Hasher::digest(&bytes).into()).is_err());
}

#[test]
fn title_source_requires_module_and_rejects_index_collision_or_unbound_urls() {
    assert!(source(&body(&[Entry::new(EMPTY, b"")])).is_err());
    assert!(source(&body(&[Entry::new(MODULE, b"")])).is_err());
    let mut collision = entries();
    collision.push(Entry::new("ri.bin", b"not a second index"));
    assert!(source(&body(&collision)).is_err());
    for url in [
        format!("https://{HOST}/ResourceInfo/synthetic/{MODULE}"),
        format!("https://{HOST}/Updater/synthetic/modules/different.exe"),
        format!("https://{HOST}.example.test/Updater/synthetic/{MODULE}"),
        format!("http://{HOST}/Updater/synthetic/{MODULE}"),
    ] {
        let mut files = entries();
        files[0].url = url;
        assert!(source(&body(&files)).is_err());
    }
}

#[test]
fn prepare_uses_local_byte_evidence_not_file_revisions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("modules")).unwrap();
    std::fs::write(root.path().join(MODULE), MODULE_BYTES).unwrap();
    let bytes = body(&entries());
    let source = source(&bytes).unwrap();
    let updater = Updater::open(root.path(), source.download_policy()).unwrap();
    let plan = source.prepare(&updater).unwrap();
    assert_eq!(plan.manifest_version, "2026020200");
    assert_eq!(plan.unchanged_files, 1);
    assert_eq!(plan.download_files, 1);
    assert_eq!(plan.download_bytes, 0);
    assert!(!root.path().join("ri.bin").exists());
    assert_eq!(std::fs::read(root.path().join(MODULE)).unwrap(), MODULE_BYTES);
}

#[tokio::test]
async fn reopened_transaction_requires_a_fresh_source_bind_before_publication() {
    let root = tempfile::tempdir().unwrap();
    for entry in entries() {
        let path = root.path().join(&entry.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, entry.bytes).unwrap();
    }
    let bytes = body(&entries());
    let source = source(&bytes).unwrap();
    {
        let updater = Updater::open(root.path(), source.download_policy()).unwrap();
        source.prepare(&updater).unwrap();
        assert!(updater.snapshot().resumable);
    }
    let updater = Updater::open(root.path(), source.download_policy()).unwrap();
    assert!(matches!(updater.run().await, Err(UpdateError::State(_))));
    assert!(!root.path().join("ri.bin").exists());
    assert_eq!(source.prepare(&updater).unwrap().download_files, 0);
    assert_eq!(updater.run().await.unwrap(), konaste_updater::SessionPhase::Idle);
    assert_eq!(std::fs::read(root.path().join("ri.bin")).unwrap(), bytes);
}

#[tokio::test]
async fn unchanged_content_still_publishes_the_exact_index_without_a_transfer() {
    let root = tempfile::tempdir().unwrap();
    for entry in entries() {
        let path = root.path().join(&entry.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, entry.bytes).unwrap();
    }
    std::fs::write(root.path().join("unknown.txt"), b"keep").unwrap();
    let bytes = body(&entries());
    let source = source(&bytes).unwrap();
    let updater = Updater::open(root.path(), source.download_policy()).unwrap();
    let plan = source.prepare(&updater).unwrap();
    assert_eq!(plan.download_files, 0);
    assert_eq!(updater.run().await.unwrap(), konaste_updater::SessionPhase::Idle);
    assert_eq!(std::fs::read(root.path().join("ri.bin")).unwrap(), bytes);
    assert_eq!(std::fs::read(root.path().join("unknown.txt")).unwrap(), b"keep");
}

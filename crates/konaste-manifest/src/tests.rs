use std::collections::BTreeMap;

use super::*;

const HOST: &str = "cdn.example.test";
const REVISION: u32 = 2026010100;
const ALPHABET: &[u8] = b"0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

#[derive(Clone)]
struct File {
    path: String,
    revision: u32,
    size: u32,
    sum: String,
    url: String,
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file(path: &str) -> File {
    File { path: path.to_owned(), revision: REVISION, size: 0, sum: hex(&digest(b"")), url: format!("https://{HOST}/Updater/build/{path}") }
}

fn name(text: &str) -> Vec<u8> {
    let mut packed = vec![0; (text.len() * 6).div_ceil(8)];
    for (position, character) in text.bytes().enumerate() {
        let index = ALPHABET.iter().position(|value| *value == character).unwrap();
        for bit in 0..6 {
            if index & (1 << (5 - bit)) != 0 {
                let output = position * 6 + bit;
                packed[output / 8] |= 1 << (7 - output % 8);
            }
        }
    }
    let mut encoded = vec![text.len() as u8];
    encoded.extend(packed);
    encoded
}

/// `A0 45` stores the name as raw ASCII. The length byte is `0x40 | (count - 1)`.
fn plain_name(text: &str) -> Vec<u8> {
    assert!((1..=64).contains(&text.len()));
    let mut encoded = vec![0x40 | (text.len() as u8 - 1)];
    encoded.extend(text.as_bytes());
    encoded
}

fn align(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}

#[derive(Clone, Copy)]
enum Names {
    Packed,
    Plain,
}

#[derive(Clone)]
struct Encoding {
    nodes: Vec<u8>,
    data: Vec<u8>,
    node_fields: BTreeMap<String, usize>,
    data_fields: BTreeMap<String, usize>,
    closes: Vec<usize>,
    end: usize,
    names: Names,
}

impl Default for Encoding {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            data: Vec::new(),
            node_fields: BTreeMap::new(),
            data_fields: BTreeMap::new(),
            closes: Vec::new(),
            end: 0,
            names: Names::Packed,
        }
    }
}

impl Encoding {
    fn open(&mut self, kind: u8, text: &str, key: &str) {
        self.node_fields.insert(key.to_owned(), self.nodes.len());
        self.nodes.push(kind);
        self.nodes.extend(match self.names {
            Names::Packed => name(text),
            Names::Plain => plain_name(text),
        });
    }

    fn close(&mut self) {
        self.closes.push(self.nodes.len());
        self.nodes.push(0xfe);
    }

    fn text(&mut self, text: &str, value: &str, key: &str) {
        self.open(0x0b, text, key);
        align(&mut self.data);
        self.data_fields.insert(key.to_owned(), self.data.len());
        self.data.extend(((value.len() + 1) as u32).to_be_bytes());
        self.data.extend(value.as_bytes());
        self.data.push(0);
        align(&mut self.data);
        self.close();
    }

    fn number(&mut self, text: &str, value: u32, key: &str) {
        self.open(0x06, text, key);
        align(&mut self.data);
        self.data_fields.insert(key.to_owned(), self.data.len());
        self.data.extend(value.to_be_bytes());
        self.close();
    }

    fn body(&self) -> Vec<u8> {
        let mut nodes = self.nodes.clone();
        align(&mut nodes);
        let mut body = match self.names {
            Names::Packed => vec![0xa0, 0x42, 0x00, 0xff],
            Names::Plain => vec![0xa0, 0x45, 0x00, 0xff],
        };
        body.extend((nodes.len() as u32).to_be_bytes());
        body.extend(nodes);
        body.extend((self.data.len() as u32).to_be_bytes());
        body.extend(&self.data);
        body
    }
}

fn encode_with(files: &[File], revision: u32, names: Names) -> Encoding {
    let mut encoded = Encoding { names, ..Encoding::default() };
    encoded.open(0x01, "resource_info", "root");
    for (position, file) in files.iter().enumerate() {
        encoded.open(0x01, "file", &format!("{position}.file"));
        encoded.text("path", &file.path, &format!("{position}.path"));
        encoded.number("version", file.revision, &format!("{position}.version"));
        encoded.number("size", file.size, &format!("{position}.size"));
        encoded.text("sum", &file.sum, &format!("{position}.sum"));
        encoded.text("url", &file.url, &format!("{position}.url"));
        encoded.close();
    }
    encoded.number("ri_ver", revision, "ri_ver");
    encoded.close();
    encoded.end = encoded.nodes.len();
    encoded.nodes.push(0xff);
    encoded
}

fn encode(files: &[File], revision: u32) -> Encoding {
    encode_with(files, revision, Names::Packed)
}

fn example() -> Encoding {
    encode(&[file("data/empty.bin")], REVISION)
}

fn parse(body: &[u8]) -> Result<ResourceIndex, IndexError> {
    // Synthetic mutation tests intentionally bless their generated body so the
    // parser runs. Production callers must obtain the digest independently.
    decode(body, digest(body), &[HOST])
}

fn rejects(body: &[u8]) {
    assert!(parse(body).is_err(), "accepted malformed synthetic body");
}

#[test]
fn synthetic_index_preserves_order_revisions_and_legitimate_empty_files() {
    let mut first = file("data/z.bin");
    first.revision = 2015010100;
    let body = encode(&[first, file("data/a.bin")], REVISION).body();
    let index = parse(&body).unwrap();
    assert_eq!(index.revision, REVISION);
    assert_eq!(index.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["data/z.bin", "data/a.bin"]);
    assert_eq!(index.files[0].revision, 2015010100);
    assert_eq!(index.files[0].stored_size, 0);
    assert_eq!(index.files[0].stored_sha256, digest(b""));
    assert_eq!(index.files[0].url.host_str(), Some(HOST));
}

#[test]
fn integers_are_unsigned_big_endian_and_sizes_widen_to_u64() {
    let mut entry = file("modules/example.bin");
    entry.revision = 0xf0102030;
    entry.size = u32::MAX;
    entry.sum = hex(&digest(b"synthetic nonempty payload"));
    let body = encode(&[entry], 0x80402010).body();
    let index = parse(&body).unwrap();
    assert_eq!(index.revision, 0x80402010);
    assert_eq!(index.files[0].revision, 0xf0102030);
    assert_eq!(index.files[0].stored_size, u64::from(u32::MAX));
}

#[test]
fn digest_mismatch_is_rejected_before_binary_parsing_or_host_validation() {
    let body = example().body();
    assert_eq!(decode(&body, [0; 32], &[HOST]), Err(IndexError::HashMismatch));
    assert_eq!(decode(b"not an index", [0; 32], &["*"]), Err(IndexError::HashMismatch));
}

#[test]
fn byte_limit_is_enforced_before_hashing_or_allocating_sections() {
    assert_eq!(decode(&vec![0; MAX_INDEX_BYTES + 1], [0; 32], &[HOST]), Err(IndexError::TooLarge));
}

#[test]
fn ri_ver_may_occur_among_root_files_exactly_once() {
    for names in [Names::Packed, Names::Plain] {
        let mut encoded = encode_with(&[file("data/a.bin"), file("data/b.bin")], REVISION, names);
        let start = encoded.node_fields["ri_ver"];
        let end = encoded.closes[encoded.closes.len() - 2] + 1;
        let revision = encoded.nodes[start..end].to_vec();
        let value_at = encoded.data_fields["ri_ver"];
        let value = encoded.data[value_at..value_at + 4].to_vec();
        encoded.nodes.drain(start..end);
        encoded.data.drain(value_at..value_at + 4);
        let middle = encoded.node_fields["1.file"];
        encoded.nodes.splice(middle..middle, revision);
        let first_value = encoded.data_fields["1.path"];
        encoded.data.splice(first_value..first_value, value);
        let index = parse(&encoded.body()).unwrap();
        assert_eq!(index.revision, REVISION);
        assert_eq!(index.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["data/a.bin", "data/b.bin"]);

        let mut duplicate = encoded.clone();
        let again = duplicate.nodes[middle..middle + (end - start)].to_vec();
        duplicate.nodes.splice(middle..middle, again);
        duplicate.data.extend(REVISION.to_be_bytes());
        rejects(&duplicate.body());
    }
}

#[test]
fn uncompressed_node_names_round_trip_without_changing_packed_names() {
    let mut first = file("game/modules/example.exe");
    first.revision = 2015010100;
    first.size = 16338056;
    first.sum = hex(&digest(b"synthetic nonempty payload"));
    let body = encode_with(&[first, file("data/a.bin")], REVISION, Names::Plain).body();
    assert_eq!(&body[..4], &[0xa0, 0x45, 0x00, 0xff]);
    assert!(body.windows(14).any(|window| window == b"\x4cresource_info"));
    let index = parse(&body).unwrap();
    assert_eq!(index.revision, REVISION);
    assert_eq!(index.files[0].path, "game/modules/example.exe");
    assert_eq!(index.files[0].revision, 2015010100);
    assert_eq!(index.files[0].stored_size, 16338056);
    assert_eq!(index.files[1].path, "data/a.bin");
    assert_eq!(index.files[1].stored_size, 0);
    assert_eq!(index.files[1].stored_sha256, digest(b""));
}

#[test]
fn plain_name_length_prefix_must_be_count_minus_one_in_the_low_six_bits() {
    for length in [0u8, 1, 0x3f, 0x41, 0x80, 0xc0, 0xff] {
        let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
        encoded.nodes[1] = length;
        rejects(&encoded.body());
    }
    let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
    encoded.nodes[2] = b'R';
    rejects(&encoded.body());
    let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
    let start = encoded.node_fields["root"];
    encoded.nodes.splice(start + 1..start + 1 + plain_name("resource_info").len(), plain_name("resource_info_"));
    rejects(&encoded.body());
}

#[test]
fn plain_names_reject_truncation_and_control_bytes() {
    let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
    encoded.nodes.truncate(4);
    rejects(&encoded.body());
    let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
    encoded.nodes[2] = 0x1f;
    rejects(&encoded.body());
    let mut encoded = encode_with(&[file("data/empty.bin")], REVISION, Names::Plain);
    encoded.nodes[2] = 0x7f;
    rejects(&encoded.body());
}

#[test]
fn plain_header_over_four_mebibytes_is_accepted_when_the_tree_is_valid() {
    let paths = (0..20_000).map(|index| format!("data/file-{index:05}.bin"));
    let files: Vec<_> = paths.map(|path| file(&path)).collect();
    let body = encode_with(&files, REVISION, Names::Plain).body();
    assert!(body.len() > 4 * 1024 * 1024);
    assert!(body.len() <= MAX_INDEX_BYTES);
    let index = parse(&body).unwrap();
    assert_eq!(index.files.len(), 20_000);
    assert_eq!(index.files[0].path, "data/file-00000.bin");
    assert_eq!(index.files[19_999].path, "data/file-19999.bin");
    assert_eq!(index.revision, REVISION);
}

#[test]
fn plain_header_still_rejects_a_body_above_the_index_cap() {
    let paths = (0..50_000).map(|index| format!("data/file-{index:05}.bin"));
    let files: Vec<_> = paths.map(|path| file(&path)).collect();
    let body = encode_with(&files, REVISION, Names::Plain).body();
    assert!(body.len() > MAX_INDEX_BYTES);
    assert_eq!(decode(&body, digest(&body), &[HOST]), Err(IndexError::TooLarge));
}

#[test]
fn unsupported_headers_and_encodings_are_rejected() {
    for position in 0..4 {
        let mut body = example().body();
        body[position] ^= 1;
        rejects(&body);
    }
}

#[test]
fn every_truncated_prefix_and_outer_trailing_bytes_are_rejected() {
    let body = example().body();
    for length in 0..body.len() {
        rejects(&body[..length]);
    }
    for suffix in [&[0][..], &[0, 0, 0, 0], &[0xff]] {
        let mut extra = body.clone();
        extra.extend(suffix);
        rejects(&extra);
    }
}

#[test]
fn impossible_unaligned_and_inconsistent_section_lengths_are_rejected() {
    let body = example().body();
    let node_length = u32::from_be_bytes(body[4..8].try_into().unwrap());
    for value in [0, 1, u32::MAX, node_length - 4, node_length + 4] {
        let mut mutated = body.clone();
        mutated[4..8].copy_from_slice(&value.to_be_bytes());
        rejects(&mutated);
    }
    let data_offset = 8 + node_length as usize;
    let data_length = u32::from_be_bytes(body[data_offset..data_offset + 4].try_into().unwrap());
    for value in [0, 1, u32::MAX, data_length - 4, data_length + 4] {
        let mut mutated = body.clone();
        mutated[data_offset..data_offset + 4].copy_from_slice(&value.to_be_bytes());
        rejects(&mutated);
    }
}

#[test]
fn unsupported_kinds_arrays_and_attributes_are_rejected() {
    for kind in [0x00, 0x01, 0x06, 0x2e, 0x46, 0x4b, 0xfe, 0xff] {
        let mut encoded = example();
        let position = encoded.node_fields["0.path"];
        encoded.nodes[position] = kind;
        rejects(&encoded.body());
    }
    let mut encoded = example();
    encoded.nodes[0] = 0x0b;
    rejects(&encoded.body());
}

#[test]
fn zero_oversized_and_truncated_node_names_are_rejected() {
    for length in [0, 65, 255] {
        let mut encoded = example();
        encoded.nodes[1] = length;
        rejects(&encoded.body());
    }
    let mut encoded = example();
    encoded.nodes = vec![0x01, 64, 0, 0];
    rejects(&encoded.body());
}

#[test]
fn name_bits_must_match_the_expected_schema_and_have_zero_padding() {
    let mut encoded = example();
    encoded.nodes[2..12].copy_from_slice(&name("Resource_info")[1..]);
    rejects(&encoded.body());
    let mut encoded = example();
    encoded.nodes[11] |= 1;
    rejects(&encoded.body());
}

#[test]
fn file_fields_must_have_the_exact_order_and_multiplicity() {
    let mut encoded = example();
    let start = encoded.node_fields["0.version"];
    let middle = encoded.node_fields["0.size"];
    let end = encoded.closes[2] + 1;
    let version = encoded.nodes[start..middle].to_vec();
    let size = encoded.nodes[middle..end].to_vec();
    encoded.nodes.splice(start..end, size.into_iter().chain(version));
    rejects(&encoded.body());

    let mut encoded = example();
    let start = encoded.node_fields["0.path"];
    let end = encoded.closes[0] + 1;
    let duplicate = encoded.nodes[start..end].to_vec();
    encoded.nodes.splice(end..end, duplicate);
    rejects(&encoded.body());
}

#[test]
fn root_requires_files_then_one_final_ri_ver() {
    rejects(&encode(&[], REVISION).body());
    let mut encoded = example();
    let start = encoded.node_fields["ri_ver"];
    let end = encoded.closes[6] + 1;
    let duplicate = encoded.nodes[start..end].to_vec();
    encoded.nodes.splice(end..end, duplicate);
    encoded.data.extend(REVISION.to_be_bytes());
    rejects(&encoded.body());

    let mut encoded = example();
    let start = encoded.node_fields["ri_ver"];
    encoded.nodes.splice(start..start + 1 + name("ri_ver").len(), [0x06].into_iter().chain(name("badrev")));
    rejects(&encoded.body());
}

#[test]
fn unbalanced_closes_and_missing_or_premature_end_markers_are_rejected() {
    for closing in 0..8 {
        let mut encoded = example();
        encoded.nodes[encoded.closes[closing]] = 0xff;
        rejects(&encoded.body());
    }
    for replacement in [0, 0x01, 0xfe] {
        let mut encoded = example();
        encoded.nodes[encoded.end] = replacement;
        rejects(&encoded.body());
    }
}

#[test]
fn string_lengths_are_bounded_u32_values_not_single_byte_scans() {
    for length in [0u32, 2049, u32::MAX] {
        let mut encoded = example();
        let offset = encoded.data_fields["0.path"];
        encoded.data[offset..offset + 4].copy_from_slice(&length.to_be_bytes());
        rejects(&encoded.body());
    }
    let mut entry = file("data/empty.bin");
    entry.url = format!("https://{HOST}/{}", "a".repeat(2048));
    rejects(&encode(&[entry], REVISION).body());

    // The old scanner interpreted this size's bytes as a printable string.
    let mut entry = file("data/nonempty.bin");
    entry.size = 0x03603400;
    entry.sum = hex(&digest(b"synthetic nonempty payload"));
    assert_eq!(parse(&encode(&[entry], REVISION).body()).unwrap().files[0].stored_size, 0x03603400);
}

#[test]
fn strings_require_one_final_nul_and_printable_ascii() {
    for value in [0, 0x1f, 0x7f, 0x80] {
        let mut encoded = example();
        let offset = encoded.data_fields["0.path"] + 4;
        encoded.data[offset] = value;
        rejects(&encoded.body());
    }
    let mut encoded = example();
    let offset = encoded.data_fields["0.path"];
    let length = u32::from_be_bytes(encoded.data[offset..offset + 4].try_into().unwrap()) as usize;
    encoded.data[offset + 4 + length - 1] = b'x';
    rejects(&encoded.body());
}

#[test]
fn node_and_data_padding_must_be_zero_and_sections_fully_consumed() {
    let mut encoded = encode(&[file("z")], REVISION);
    let offset = encoded.data_fields["0.path"];
    encoded.data[offset + 6] = 1;
    rejects(&encoded.body());

    let encoded = example();
    let mut body = encoded.body();
    let node_length = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
    assert!(encoded.nodes.len() < node_length);
    body[8 + encoded.nodes.len()] = 1;
    rejects(&body);

    let mut encoded = example();
    encoded.nodes.extend([0; 4]);
    rejects(&encoded.body());
    let mut encoded = example();
    encoded.data.extend([0; 4]);
    rejects(&encoded.body());
}

#[test]
fn malformed_sha256_and_impossible_empty_file_hashes_are_rejected() {
    for sum in ["0".repeat(63), "g".repeat(64), hex(&digest(b"")).to_ascii_uppercase(), "0".repeat(64)] {
        let mut entry = file("data/empty.bin");
        entry.sum = sum;
        assert!(matches!(parse(&encode(&[entry], REVISION).body()), Err(IndexError::Schema(_))));
    }
}

#[test]
fn duplicate_case_insensitive_paths_and_file_directory_conflicts_are_rejected() {
    for (first, second) in [
        ("data/file.bin", "data/file.bin"),
        ("Data/File.BIN", "data/file.bin"),
        ("data", "data/file.bin"),
        ("Data", "data/file.bin"),
        ("data/file.bin", "DATA"),
    ] {
        assert_eq!(parse(&encode(&[file(first), file(second)], REVISION).body()), Err(IndexError::PathAlias));
    }
}

#[test]
fn windows_roots_traversal_streams_devices_short_names_and_aliases_are_rejected() {
    for path in [
        "",
        "/file.bin",
        "C:/file.bin",
        "C:file.bin",
        "//server/share/file.bin",
        "data\\file.bin",
        "../file.bin",
        "data/../file.bin",
        "data/./file.bin",
        "data//file.bin",
        "data/file.bin/",
        "data/file.bin:stream",
        "data/file.",
        "data/file ",
        "data/ file.bin",
        "CON",
        "con.txt",
        "NUL.txt",
        "aux.log",
        "data/COM1.ext",
        "data/LpT9.bin",
        "data/CONIN$",
        "data/CONOUT$.txt",
        "data/NUL .txt",
        "data/PRN  .log",
        ".konaste-hub/journal.json",
        ".KONASTE-HUB/session.lock",
        "DATA~1/file.bin",
        "data/file<.bin",
        "data/file>.bin",
        "data/file?.bin",
        "data/file*.bin",
        "data/file|.bin",
        "data/file\".bin",
        "data/é.bin",
        "data/file\n.bin",
    ] {
        assert!(parse(&encode(&[file(path)], REVISION).body()).is_err(), "accepted {path:?}");
    }
}

#[test]
fn path_and_segment_lengths_are_bounded_but_valid_nested_names_survive() {
    let long_segment = "a".repeat(256);
    assert_eq!(parse(&encode(&[file(&long_segment)], REVISION).body()), Err(IndexError::UnsafePath));
    let long_path = format!("{}/{}/{}/{}/z", "a".repeat(255), "b".repeat(255), "c".repeat(255), "d".repeat(255));
    assert_eq!(parse(&encode(&[file(&long_path)], REVISION).body()), Err(IndexError::UnsafePath));
    let valid = format!("{}/.hidden-file.bin", "a".repeat(255));
    assert_eq!(parse(&encode(&[file(&valid)], REVISION).body()).unwrap().files[0].path, valid);
}

#[test]
fn allowed_hosts_are_explicit_dns_names_not_wildcards_ips_or_urls() {
    let body = example().body();
    assert_eq!(decode(&body, digest(&body), &[]), Err(IndexError::InvalidAllowedHosts));
    for host in [
        "",
        "*",
        "*.example.test",
        "https://cdn.example.test",
        "cdn.example.test/",
        "cdn.example.test:443",
        "cdn.example.test.",
        " cdn.example.test",
        "cdn..example.test",
        "-cdn.example.test",
        "cdn-.example.test",
        "cdn_example.test",
        "127.0.0.1",
        "127.1",
        "0x7f000001",
        "[::1]",
        "é.example.test",
    ] {
        assert_eq!(decode(&body, digest(&body), &[host]), Err(IndexError::InvalidAllowedHosts), "accepted {host}");
    }
    assert_eq!(decode(&body, digest(&body), &vec![HOST; 65]), Err(IndexError::InvalidAllowedHosts));
    assert!(decode(&body, digest(&body), &["CDN.EXAMPLE.TEST", HOST]).is_ok());
}

#[test]
fn urls_are_https_443_exact_host_without_userinfo_query_or_fragment() {
    for url in [
        "http://cdn.example.test/file.bin",
        "ftp://cdn.example.test/file.bin",
        "file:///C:/file.bin",
        "https://127.0.0.1/file.bin",
        "https://1.2.3.0xzz/file.bin",
        "https://0x7f.1/file.bin",
        "https://0177.0.0.1/file.bin",
        "https://2130706433/file.bin",
        "https://cdn.example.test.attacker.test/file.bin",
        "https://other.example.test/file.bin",
        "https://cdn.example.test:8080/file.bin",
        "https://cdn.example.test./file.bin",
        "https://user@cdn.example.test/file.bin",
        "https://@cdn.example.test/file.bin",
        "https://user:pass@cdn.example.test/file.bin",
        "https://cdn.example.test/file.bin?x=1",
        "https://cdn.example.test/file.bin?",
        "https://cdn.example.test/file.bin#x",
        "https://cdn.example.test/file.bin#",
        " https://cdn.example.test/file.bin",
        "https://cdn.example.test/file.bin ",
        "https://cdn.example.test\\file.bin",
        "https:cdn.example.test/file.bin",
    ] {
        let mut entry = file("data/empty.bin");
        entry.url = url.to_owned();
        assert_eq!(parse(&encode(&[entry], REVISION).body()), Err(IndexError::UnsafeUrl), "accepted {url}");
    }
    let mut entry = file("data/empty.bin");
    entry.url = "https://CDN.EXAMPLE.TEST:443/file.bin".to_owned();
    assert!(parse(&encode(&[entry], REVISION).body()).is_ok());
    let body = example().body();
    assert_eq!(decode(&body, digest(&body), &["other.example.test"]), Err(IndexError::UnsafeUrl));
}

#[test]
fn deterministic_single_byte_mutations_never_panic() {
    let body = example().body();
    for position in 0..body.len() {
        for value in [0, 1, 0x7f, 0xfe, 0xff] {
            let mut mutated = body.clone();
            mutated[position] = value;
            let _ = parse(&mutated);
        }
    }
}

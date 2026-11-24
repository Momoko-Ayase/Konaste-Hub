# Bombergirl binary resource-index contract

Research date: **2026-10-14**. Offline-only decoding of two retained resource indexes. No network request, official DLL execution, launcher/updater/game execution, credential access, registry write, or installed-game write was performed for this note. No new IDA session was opened.

## Current decoder extension (2026-10-16)

The original sections below describe the retained Bombergirl packed-name subset. The implementation now also accepts the independently digest-checked SDVX `A0 45 00 FF` raw-ASCII name form and a bounded **8 MiB** body (the verified SDVX body is 6,158,980 bytes). Its single root `ri_ver` may occur between file records; duplicate or missing revisions remain errors. The original packed `A0 42` format, exact schema, bounded names/strings/tree, padding/EOF checks, caller-supplied digest, and Windows path/HTTPS policy remain enforced. Current anonymous discovery and identity-content samples have since passed for six titles; that is later evidence, not a claim about this 2026-10-14 offline experiment. See [multi-title integration](multi-title-integration.md) and [SDVX contract](sdvx-contract.md).

## Result

A pure Rust decoder is demonstrably possible for the **observed ASCII binary-property subset**. A small standalone Python decoder consumed both complete bodies, recovered the full tree and all values, checked the exact schema, and rejected malformed synthetic inputs. Neither body requires LZ77, XML text parsing, AVS, or game-content decryption.

The output should initially be a `ResourceIndex`, not an automatically executable updater plan. The indexes establish **final stored file sizes and SHA-256 values**. This offline work does not independently establish that an ordinary identity-encoded HTTP payload from every listed URL is exactly those final bytes. The recorded official install path renames the downloader's temporary file without a caller-side transform, which supports that interpretation, but the downloader is inside a DLL and no retained response body was verified here. Keep transport verification separate.

Production current-version discovery is also separate and remains blocked pending the normal mode-2 worker contract described in [updater-mode-bootstrap.md](updater-mode-bootstrap.md). A retained index must not substitute for a fresh official discovery result.

## Inputs and reproducibility

| Retained input | Bytes | SHA-256 | `ri_ver` |
|---|---:|---|---:|
| `<temporary research directory>/manifest.bin` | 2,681,096 | `205bb4a45e1b38ad421ea8b0ab5c41214b99c7cc5714bbdecf8d15dd3e881e9a` | 2026091500 |
| Older retained Bombergirl `ri.bin` (BG notes, not in this repository) | 2,553,692 | `e09db7544263e3155c902425edcb9a5ee322ea8436b01aa7b61a61a00962fe3c` | 2026041400 |

The first body was fetched and hash-checked in the separate updater-mode research. Its timestamped URL is an observation, not a stable discovery endpoint.

The temporary script is named `decode_manifest_offline.py` next to the first body. It uses only Python's standard library, emits aggregate validation results, and keeps all real entries in memory. It does not import or execute official code. The script and bodies are not included in the source tree. The binary contract and synthetic encoder below are sufficient to reproduce a source-only decoder.

The synthetic test passed one generated round trip and rejected five malformed cases: truncated body, trailing byte, wrong header, impossible node-section length, and unsupported node kind. These are research checks, not a comprehensive Rust test suite.

## Complete observed binary layout

All offsets below are relative to the body or their own section as specified. All observed numeric values and section/string lengths are **unsigned big-endian 32-bit integers**.

```text
body:
    A0 42 00 FF                observed four-byte header
    be32 node_section_length
    node_section[node_section_length]
    be32 data_section_length
    data_section[data_section_length]
    EOF                       no other bytes accepted
```

Both section lengths include their own padding. `0x42` accompanies packed node names in these bodies. `0x00` accompanies ASCII field values. Other header/encoding variants were not tested and must be rejected by an initial strict decoder; do not claim general binary-property-format coverage.

| Build | Node-section bytes | Data-section bytes | Data-section start |
|---|---:|---:|---:|
| 2026091500 | 340,728 | 2,340,356 | `0x53304` |
| 2026041400 | 324,544 | 2,229,136 | `0x4F3CC` |

### Node stream

An opening node consists of a one-byte kind followed by its packed name. Its value, if any, is read separately from the data stream in opening-node traversal order. Closing markers do not consume a name or value.

| Kind/marker | Meaning in these bodies |
|---|---|
| `0x01` | Container, no data value |
| `0x06` | Scalar `u32`, read one `be32` from the data stream |
| `0x0B` | Scalar string, read `be32 length` and exactly that many bytes from the data stream |
| `0xFE` | Close current node |
| `0xFF` | End tree; stack must already be empty |

After `0xFF`, only zero bytes needed to align the node section to a multiple of four occur. There are no unconsumed nodes, extra roots, attributes, array kinds, mixed-value containers, or unknown kinds in either body. In particular, `path`, `version`, `size`, `sum`, and `url` are **child nodes**, not serialized attributes, despite the older notes using “attrs” informally.

Every opening node is closed by exactly one `0xFE`. A parser must check balanced nesting and require exactly one root and a final `0xFF`; searching for recognizable strings is not a tree parser.

### Packed names

```text
name:
    u8 character_count
    ceil(character_count * 6 / 8) packed bytes
```

Each character is a six-bit index, consumed most-significant-bit first, into this 64-character alphabet:

```text
0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz
```

Unused low bits in the last byte are zero. No four-byte alignment is applied between node names or node markers. All names used in the observed schema are shorter than 128 characters; other name-length modes were not exercised.

### Data stream

For every value-bearing node, align the **data-stream cursor** to four bytes using zero padding before reading its value:

- `0x06`: one `be32`. No sign extension, variable-length integer, string conversion, or eight-byte field.
- `0x0B`: one `be32 length`, then exactly `length` bytes, then zero padding to a multiple of four. `length` includes the one final NUL. No embedded NUL is present. Every preceding byte in both indexes is printable ASCII.

A container consumes no data. After the final value, the cursor must equal the declared data-section length. Both bodies satisfy this, with every field decoded; there is no opaque trailing payload.

**String lengths are not `u8`.** Older probes happened to read the low byte of a four-byte length because observed strings were short. That scanner also mistakes some numeric bytes for strings and is unsuitable for production.

## Complete observed schema and multiplicities

```text
resource_info [kind 0x01, exactly one root]
    file [kind 0x01, repeated, original order preserved]
        path    [kind 0x0B, exactly one]
        version [kind 0x06, exactly one]
        size    [kind 0x06, exactly one]
        sum     [kind 0x0B, exactly one]
        url     [kind 0x0B, exactly one]
    ri_ver [kind 0x06, exactly one, last root child]
```

There are no other root/file/field children or attributes. Every file has the same five children in the order above. Its node-stream encoding occupies exactly 39 bytes, including its opening and all closing markers. The current body contains 8,736 repetitions of that same 39-byte sequence; the old body contains 8,321. This independently cross-checks the generic stack decoder's counts against the node section's literal repeated structure.

| Measurement | 2026091500 | 2026041400 |
|---|---:|---:|
| All `file` nodes | 8,736 | 8,321 |
| `launcher/` paths | 2 | 2 |
| `modules/` paths | 14 | 14 |
| `x/` paths | 8,720 | 8,305 |
| Zero-size files | 2,820 | 2,820 |
| Total declared file bytes | 11,722,749,506 | 10,943,129,099 |
| Largest declared file | 342,874,734 | 322,621,978 |
| Maximum path length, excluding NUL | 48 | 48 |
| Maximum URL length, excluding NUL | 125 | 125 |
| Case-insensitive duplicate paths | 0 | 0 |

Every file's `version` equals that body's `ri_ver`. This is an observation across these two builds, not proof that future indexes cannot contain mixed file revisions.

### Field semantics and validation

- `path`: installation-root-relative final path, preserving case. All observed paths use `/`, have no empty, `.` or `..` segment, and no backslash, colon, absolute prefix, or embedded NUL. All `x/` paths have five segments with a 40-character leaf. The leaf is not a content SHA-256; do not substitute it for `sum`.
- `version`: numeric file revision. It is not a path, product-version string, HTTP validator, or hash.
- `size`: declared final stored file byte count. Widen the decoded `u32` to `u64` for file operations and checked totals. A `u32` wire field does not mean the total installation fits in `u32`.
- `sum`: exactly 64 lowercase hexadecimal characters in every entry, interpreted as SHA-256 of the final stored bytes. This matches the digest operation and installed-file samples recorded in the earlier research. No MD5 or SHA-1 conversion is needed.
- `url`: absolute HTTPS URL. Every URL here uses exactly `d1q1iflay6y5jj.cloudfront.net`, has no userinfo, query, or fragment, and its path ends in `/` plus the exact `path` value. All current URLs share one updater stamp/build prefix; all old URLs share a different one. URLs belong to entries and must not be reconstructed from a hardcoded current stamp.
- `ri_ver`: numeric resource-index revision. It is `2026091500`, not the full game PE ProductVersion such as `QBG:J:A:A:2026091500`. Keep build revision and product version distinct; do not compare their raw strings as though they were interchangeable.

All 2,820 zero-size entries in each body use `SHA256(empty)`. There are no nonzero-size entries using that empty digest. Empty files are legitimate intended contents, not corrupt transfers or missing assets.

The metadata contains **no ETag, Last-Modified, compressed-size field, unpacked-size field, compression method, cipher key, transport transform flag, archive member list, patch source hash, or deletion directive**. HTTP resume validators must be obtained and checked from actual responses. Removed/unknown installation files must not be deleted based on absence alone.

## Download and replacement semantics: what is established

| Claim | Evidence and status |
|---|---|
| The manifest itself is not compressed | **Verified here:** full structural decode, exact declared section lengths, all nodes/data consumed. Earlier research also found the CDN body byte-identical to committed `ri.bin`. |
| `size`/`sum` describe final stored files | **Established by recorded original updater behavior and installed-file sampling:** updater reads these fields and verifies SHA-256 at the final path. The new offline decode confirms their types and values, but did not reread installed payloads. |
| No caller-side archive extraction or cryptographic transform after download | **Established by the recorded install call chain:** downloader temp path becomes `tmp`; existing destination moves aside; temp file is renamed to the destination; then final-file SHA-256 is checked. No transform step is recorded between temp and final. |
| Plain identity-encoded HTTP body is exactly the final stored bytes for every URL | **Not independently verified here:** no content GET was made and no retained payload body was read. A transform inside the official downloader or HTTP content encoding has not been excluded by this offline analysis. |
| Game-store payloads need decrypting in the hub | **Not supported and not required:** content remains opaque; the game owns its runtime filesystem/DRM. This work did not inspect keys, decrypt assets, or bypass entitlements. |
| Manifest has chunk compression or binary-delta instructions | **False for these two complete decoded schemas:** none of those fields/nodes exists. Downloading an entry is not a binary patch operation described by this index. |

The earlier private BG notes on the original updater path and measurements, with original function addresses, support preserving the whole official index verbatim and publishing it as `ri.bin` **last**, after the complete changed-file set is installed. This research does not authorize writes to the original installation.

## Correction to the earlier string-scanner counts

The prior probe scans arbitrary bytes for printable `<u8 length><string+NUL>` fragments and then treats consecutive path/hash/URL strings as an entry. This is not the serialized format. A `size` value can look like another printable length-prefixed string, inserting an unrelated fragment between a real path and its hash. Conversely, unrelated strings can accidentally make a false triple.

Re-running that scanner in memory on the current retained body produced 8,681 total triples and 8,663 `x/` triples. It omitted **57 real `x/` records**. The structural tree contains **8,720** `x/` records, and its fixed 39-byte file-node repetition proves the full count without heuristics. The old body's structural count is **8,305**, not 8,251. Therefore the earlier claims of 57 current / 54 old “orphan” store files are not established by those scanner results. This note corrects that conclusion without rewriting the historical BG log.

The full structural cross-build diff is:

| Comparison | All files | `x/` only |
|---|---:|---:|
| Same path and SHA-256 | 7,920 | 7,908 |
| Same path, changed SHA-256 | 401 | 397 |
| New path | 415 | 415 |
| Removed path | 0 | 0 |

Four changed entries are under `modules/`. Declared bytes for changed entries are 2,084,835,964; new entries add 543,772,717. Combined nominal full-file delta: **2,628,608,681 bytes**. Actual transfer selection still depends on verifying local content and may differ if a previous interrupted run already installed valid files. These are index comparisons, not a fresh update run.

## Synthetic fixture/encoder recipe

Use only generated bytes and fictional URLs in repository fixtures. This is a sufficient minimal synthetic tree:

```text
resource_info
    file
        path = "modules/example.bin"
        version = 2026010100
        size = 0
        sum = SHA256(empty), as lowercase hex
        url = "https://cdn.example.test/Updater/build/modules/example.bin"
    ri_ver = 2026010100
```

Encoder pseudocode:

```text
encode_name(text):
    append u8 len(text)
    bits = concatenation of alphabet_index(char) as six bits, MSB first
    append bits, padded with zero low bits to an integral byte count

open_node(kind, name, value):
    node_stream += u8(kind) + encode_name(name)
    if kind == U32:
        zero_align(data_stream, 4)
        data_stream += be32(value)
    if kind == STRING:
        zero_align(data_stream, 4)
        raw = ASCII(value) + NUL
        data_stream += be32(len(raw)) + raw
        zero_align(data_stream, 4)

close_node(): node_stream += FE

emit root, file, five children, and their balanced closes
emit ri_ver and close it, close root, append FF
zero_align(node_stream, 4)
body = A0 42 00 FF + be32(len(node_stream)) + node_stream
     + be32(len(data_stream)) + data_stream
```

An initial decoder should reject unsupported header/kind/array/attribute/name modes rather than silently skip them. Bound body size, node count, depth, name length, string length, and checked arithmetic before allocating; check zero padding, terminal NUL, ASCII, balanced markers, exact EOF, root/schema/field multiplicity, safe paths, duplicate path aliases, SHA-256 format, URL policy, and aggregate byte overflow.

## Safe module seam

A source-only decoder can expose a small interface such as:

```rust
pub fn decode(bytes: &[u8]) -> Result<ResourceIndex, IndexError>;

pub struct ResourceIndex {
    pub revision: u32,
    pub files: Vec<ResourceFile>,
}

pub struct ResourceFile {
    pub path: String,
    pub revision: u32,
    pub stored_size: u64,
    pub stored_sha256: [u8; 32],
    pub url: String,
}
```

The decoder parses and validates the observed wire subset. It does **not** authenticate metadata, fetch URLs, establish entitlement, compare PE versions, manipulate installations, or claim general support for other titles/encodings.

Once identity-encoded content transport is verified against both stored size and SHA-256, a provider can translate in original order to the existing updater `ManifestFile { relative_path, size, sha256, url: Some(...), etag: None }`. The updater obtains/respects actual response validators itself; the index supplies none. `Manifest.version` can carry the canonical numeric index revision string, but the native launch/version model must not equate that string with a full PE ProductVersion. Preserve the original authenticated index bytes separately as `OfficialManifest { path: "ri.bin", bytes }`.

Until that transport check and fresh discovery are established, returning `ResourceIndex` is safe; scheduling updates from retained fixtures is not. The offline evidence removes the **manifest decoding** blocker, not the current-metadata or end-to-end-download verification blockers.

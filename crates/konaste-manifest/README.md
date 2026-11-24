# konaste-manifest

A read-only decoder for the observed ASCII Konaste binary resource-index subset.

```rust,ignore
let index = konaste_manifest::decode(
    &body,
    expected_sha256_from_validated_discovery,
    &["d1q1iflay6y5jj.cloudfront.net"],
)?;
```

The expected digest is a `[u8; 32]`. It must come from independently validated official discovery, not from hashing the supplied body. The decoder verifies that digest before parsing, then validates the complete bounded tree, exact observed schema, padding, EOF, file hashes, safe Windows relative paths and aliases, and HTTPS URLs on the caller's exact DNS host list. `ri_ver` is one root-level `u32` and may sit between `file` records; file order is the order of those records.

It returns an ordered `ResourceIndex`, **not an updater plan**. Numeric index/file revisions are separate from the game's PE ProductVersion. Stored sizes and hashes describe final files; identity-encoded content transfer semantics need separate verification before update scheduling. Metadata does not declare deletions or HTTP resume validators.

Two headers are accepted. `A0 42 00 FF` stores node names as six-bit alphabet indexes with a raw character count and zero low-bit padding. `A0 45 00 FF` stores node names as raw printable ASCII; its length byte is `0x40 | (count - 1)` and there is no extra name padding. Both share the same section layout, kinds, schema, and data stream. Other encodings, node kinds, arrays, attributes, and schema variants are rejected.

The interface caps input at 8 MiB. That bound covers the verified 6,158,980-byte SDVX index and is not a general size. File count stays at 100,000, depth at three, node names at 64 characters, serialized strings at 2,048 bytes including NUL, paths at 1,024 bytes, and path segments at 255 bytes. Only printable ASCII field values are accepted. Path validation is lexical and does not solve filesystem reparse races.

The implementation performs no filesystem or network I/O and executes no official code. Unit fixtures are generated synthetic data. `tests/offline_official.rs` contains ignored, read-only tests for explicitly supplied retained research indexes:

- `KONASTE_OFFLINE_BG_MANIFEST`: pinned 2026091500 index.
- `KONASTE_OFFLINE_BG_OLD_MANIFEST`: pinned 2026041400 backup.
- `KONASTE_OFFLINE_SDVX_MANIFEST`: pinned 2026070300 SDVX index, when that path is supplied separately.

No official body is included. These research pins establish input provenance, not current-version freshness. The full decoding evidence is in `docs/research/manifest-contract.md` at the repository root.

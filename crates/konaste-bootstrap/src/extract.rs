//! Static extraction of the two launcher DLLs.
//!
//! The MSI is opened read-only through the pure-Rust `msi` crate (compound
//! document plus the installer database). Cabinet bytes come from an embedded
//! `_Streams` entry named by the Media table, and are decoded with `cab`.
//! No Windows Installer engine, `msiexec`, or custom action runs.
//!
//! Only a `File` row whose long `FileName` is exactly `libeacnet.dll` or
//! `avs2-core.dll` is extracted. Its `File` key identifies the cabinet member,
//! while `Sequence` selects its Media row. Unrelated members are not opened;
//! paths, traversal, and ambiguous member names reject the package.

use std::fs::File;
use std::path::Path;

use msi::{Package, Select, Value};

use crate::MAX_PACKAGE_BYTES;
use crate::error::BootstrapError;
use crate::hashutil::sha256_reader;
use crate::names::{WANTED_FILES, is_exact_runtime_name};

/// Both DLLs, still in the stage directory, with the digests just computed.
#[derive(Debug)]
pub(crate) struct ExtractedPair {
    pub libeacnet: std::path::PathBuf,
    pub avs2_core: std::path::PathBuf,
    pub libeacnet_bytes: u64,
    pub avs2_core_bytes: u64,
    pub libeacnet_sha256: String,
    pub avs2_core_sha256: String,
}

pub(crate) fn extract_pair(msi_path: &Path, stage: &Path) -> Result<ExtractedPair, BootstrapError> {
    let file = File::open(msi_path)?;
    let mut package = Package::open(file).map_err(|err| BootstrapError::layout(format!("msi database could not be read: {err}")))?;

    let cabinet = embedded_cabinet_name(&mut package)?;
    let targets = wanted_file_rows(&mut package, cabinet.last_sequence)?;
    let members = read_members(&mut package, &cabinet.name)?;
    for target in &targets {
        let member = members
            .iter()
            .find(|member| member.name == target.member_name)
            .ok_or_else(|| BootstrapError::layout(format!("embedded cabinet has no member for File key `{}`", target.member_name)))?;
        if member.bytes != target.bytes {
            return Err(BootstrapError::layout(format!("cabinet size for `{}` does not match File.FileSize", target.member_name)));
        }
    }

    let decoded = decode_members(&mut package, &cabinet.name, &targets)?;
    let mut libeacnet = None;
    let mut avs2 = None;
    for (name, bytes, sha256) in decoded {
        let dest = stage.join(&name);
        crate::cache::reject_reparse_path(&dest)?;
        std::fs::write(&dest, &bytes)?;
        let record = Written { path: dest, bytes: bytes.len() as u64, sha256 };
        if name == "libeacnet.dll" {
            libeacnet = Some(record);
        } else if name == "avs2-core.dll" {
            avs2 = Some(record);
        }
    }

    let libeacnet = libeacnet.ok_or_else(|| BootstrapError::layout("libeacnet.dll missing"))?;
    let avs2 = avs2.ok_or_else(|| BootstrapError::layout("avs2-core.dll missing"))?;
    Ok(ExtractedPair {
        libeacnet: libeacnet.path,
        avs2_core: avs2.path,
        libeacnet_bytes: libeacnet.bytes,
        avs2_core_bytes: avs2.bytes,
        libeacnet_sha256: libeacnet.sha256,
        avs2_core_sha256: avs2.sha256,
    })
}

struct Written {
    path: std::path::PathBuf,
    bytes: u64,
    sha256: String,
}

struct FileTarget {
    file_name: String,
    member_name: String,
    sequence: i32,
    bytes: u64,
}

struct EmbeddedCabinet {
    name: String,
    last_sequence: i32,
}

/// Both pinned packages have exactly one Media row. Refuse external cabinets
/// or a changed layout instead of guessing where a target's Sequence belongs.
fn embedded_cabinet_name(package: &mut Package<File>) -> Result<EmbeddedCabinet, BootstrapError> {
    if !package.has_table("Media") {
        return Err(BootstrapError::layout("MSI has no Media table"));
    }
    let rows = package.select_rows(Select::table("Media")).map_err(|err| BootstrapError::layout(format!("Media query failed: {err}")))?;
    let mut embedded = Vec::new();
    for row in rows {
        let cabinet = row["Cabinet"].as_str().ok_or_else(|| BootstrapError::layout("Media row has no cabinet"))?;
        let Some(name) = cabinet.strip_prefix('#') else {
            return Err(BootstrapError::layout(format!("refusing external cabinet `{cabinet}`")));
        };
        if is_hostile(name) {
            return Err(BootstrapError::name(name, "embedded cabinet name is not a single token"));
        }
        let last_sequence = row["LastSequence"]
            .as_int()
            .filter(|value| *value > 0)
            .ok_or_else(|| BootstrapError::layout("Media.LastSequence must be positive"))?;
        if row["DiskId"].as_int() != Some(1) {
            return Err(BootstrapError::layout("recorded cabinet must be Media disk 1"));
        }
        embedded.push(EmbeddedCabinet { name: name.to_string(), last_sequence });
    }
    match embedded.len() {
        1 => Ok(embedded.remove(0)),
        0 => Err(BootstrapError::layout("MSI has no embedded cabinet")),
        _ => Err(BootstrapError::layout("MSI has more than one Media row; refusing to guess")),
    }
}

fn wanted_file_rows(package: &mut Package<File>, last_sequence: i32) -> Result<Vec<FileTarget>, BootstrapError> {
    if !package.has_table("File") {
        return Err(BootstrapError::layout("MSI has no File table"));
    }
    let rows = package.select_rows(Select::table("File")).map_err(|err| BootstrapError::layout(format!("File query failed: {err}")))?;
    let mut found = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for row in rows {
        let member_name = row["File"].as_str().ok_or_else(|| BootstrapError::layout("File.File is not a string"))?;
        if is_hostile(member_name)
            || member_name.len() > 255
            || !member_name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.".contains(&byte))
            || !member_name.bytes().next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            return Err(BootstrapError::name(member_name, "File key is not an MSI identifier"));
        }
        if !keys.insert(member_name.to_ascii_lowercase()) {
            return Err(BootstrapError::layout("File table has duplicate or case-aliased keys"));
        }
        let file_name = long_file_name(&row["FileName"])?;
        if WANTED_FILES.iter().any(|wanted| file_name.eq_ignore_ascii_case(wanted)) {
            if !is_exact_runtime_name(&file_name) {
                return Err(BootstrapError::name(file_name, "matching file is not the exact lowercase runtime name"));
            }
            if found.iter().any(|existing: &FileTarget| existing.file_name == file_name) {
                return Err(BootstrapError::layout(format!("File table lists {file_name} more than once")));
            }
            let sequence = row["Sequence"]
                .as_int()
                .filter(|value| *value > 0 && *value <= last_sequence)
                .ok_or_else(|| BootstrapError::layout("target Sequence is outside its Media row"))?;
            if found.iter().any(|existing| existing.sequence == sequence) {
                return Err(BootstrapError::layout("runtime targets share a Sequence"));
            }
            let bytes = row["FileSize"]
                .as_int()
                .filter(|value| *value > 0 && *value as u64 <= MAX_PACKAGE_BYTES)
                .ok_or_else(|| BootstrapError::layout("target FileSize is invalid"))? as u64;
            found.push(FileTarget { file_name, member_name: member_name.to_string(), sequence, bytes });
        }
    }
    if found.len() != WANTED_FILES.len() {
        return Err(BootstrapError::layout("File table does not contain both libeacnet.dll and avs2-core.dll"));
    }
    Ok(found)
}

/// File.FileName is `short|long` or just `long`. Only the long name is trusted,
/// and only when it is a single path segment.
fn long_file_name(value: &Value) -> Result<String, BootstrapError> {
    let raw = value.as_str().ok_or_else(|| BootstrapError::layout("File.FileName is not a string"))?;
    let mut parts = raw.split('|');
    let short = parts.next().unwrap_or(raw);
    let long = parts.next().unwrap_or(short);
    if parts.next().is_some() || is_hostile(short) || is_hostile(long) {
        return Err(BootstrapError::name(raw, "FileName is not one safe short|long filename"));
    }
    Ok(long.to_string())
}

struct CabinetMember {
    name: String,
    bytes: u64,
}

fn read_members(package: &mut Package<File>, cabinet_name: &str) -> Result<Vec<CabinetMember>, BootstrapError> {
    let reader = open_cabinet(package, cabinet_name)?;
    let cabinet = cab::Cabinet::new(reader).map_err(|err| BootstrapError::layout(format!("embedded cabinet could not be read: {err}")))?;
    let mut members = Vec::new();
    let mut names = std::collections::HashSet::new();
    for folder in cabinet.folder_entries() {
        for file in folder.file_entries() {
            let name = file.name();
            if is_hostile(name) || !name.is_ascii() {
                return Err(BootstrapError::name(name, "cabinet member is not a single ASCII token"));
            }
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(BootstrapError::name(name, "duplicate or case-aliased cabinet member"));
            }
            members.push(CabinetMember { name: name.to_string(), bytes: file.uncompressed_size() as u64 });
        }
    }
    Ok(members)
}

/// Decode each target from one open of the cabinet stream.
///
/// The cabinet reader borrows the stream, so every member is copied out
/// before the stream (and then the package) is released.
fn decode_members(
    package: &mut Package<File>,
    cabinet_name: &str,
    targets: &[FileTarget],
) -> Result<Vec<(String, Vec<u8>, String)>, BootstrapError> {
    for target in targets {
        if !is_exact_runtime_name(&target.file_name) {
            return Err(BootstrapError::name(&target.file_name, "refusing to read this cabinet member"));
        }
    }
    let reader = open_cabinet(package, cabinet_name)?;
    let mut cabinet =
        cab::Cabinet::new(reader).map_err(|err| BootstrapError::layout(format!("embedded cabinet could not be read: {err}")))?;
    let mut decoded = Vec::with_capacity(targets.len());
    for target in targets {
        let file = cabinet
            .read_file(&target.member_name)
            .map_err(|err| BootstrapError::layout(format!("cabinet member `{}` could not be decoded: {err}", target.member_name)))?;
        let (bytes, sha256) = sha256_reader(file, target.bytes)?;
        if bytes.len() as u64 != target.bytes {
            return Err(BootstrapError::layout("decoded member does not match FileSize"));
        }
        decoded.push((target.file_name.clone(), bytes, sha256));
    }
    Ok(decoded)
}

fn open_cabinet<'a>(package: &'a mut Package<File>, cabinet: &str) -> Result<impl std::io::Read + std::io::Seek + 'a, BootstrapError> {
    // Embedded cabinets are stored as an MSI stream of the same name.
    if !package.has_stream(cabinet) {
        return Err(BootstrapError::layout(format!("Media names embedded cabinet `{cabinet}` but that stream is absent")));
    }
    package.read_stream(cabinet).map_err(|err| BootstrapError::layout(format!("reading cabinet stream `{cabinet}` failed: {err}")))
}

fn is_hostile(name: &str) -> bool {
    name.is_empty()
        || name.contains(['/', '\\', ':', '\0'])
        || name.contains("..")
        || name.trim() != name
        || name.ends_with('.')
        || name.chars().any(char::is_control)
}

/// Build a tiny in-memory MSI with one embedded cabinet. Test-only.
#[cfg(test)]
pub(crate) mod synth {
    use std::io::Cursor;

    use cab::{CabinetBuilder, CompressionType};
    use msi::{Column, Insert, Package, PackageType, Value};

    pub struct Synthetic {
        pub bytes: Vec<u8>,
    }

    pub fn package(libeacnet: &[u8], avs2: &[u8], extra_member: Option<(&str, &[u8])>) -> Synthetic {
        let mut builder = CabinetBuilder::new();
        {
            let folder = builder.add_folder(CompressionType::MsZip);
            folder.add_file("launcher_modules_libeacnet.dll");
            folder.add_file("launcher_modules_avs2_core.dll");
            if let Some((name, _)) = extra_member {
                folder.add_file(name);
            }
        }
        let mut writer = builder.build(Cursor::new(Vec::new())).unwrap();
        let payloads: Vec<&[u8]> = if let Some((_, bytes)) = extra_member { vec![libeacnet, avs2, bytes] } else { vec![libeacnet, avs2] };
        for payload in payloads {
            let mut file = writer.next_file().unwrap().expect("file declared above");
            std::io::copy(&mut Cursor::new(payload), &mut file).unwrap();
        }
        let cab = writer.finish().unwrap().into_inner();

        let cursor = Cursor::new(Vec::new());
        let mut package = Package::create(PackageType::Installer, cursor).unwrap();
        package
            .create_table(
                "Media",
                vec![
                    Column::build("DiskId").primary_key().int16(),
                    Column::build("LastSequence").int16(),
                    Column::build("DiskPrompt").nullable().string(64),
                    Column::build("Cabinet").nullable().string(255),
                    Column::build("VolumeLabel").nullable().string(32),
                ],
            )
            .unwrap();
        package
            .create_table(
                "File",
                vec![
                    Column::build("File").primary_key().string(72),
                    Column::build("Component_").string(72),
                    Column::build("FileName").string(255),
                    Column::build("FileSize").int32(),
                    Column::build("Sequence").int16(),
                ],
            )
            .unwrap();
        package
            .insert_rows(Insert::into("Media").row(vec![
                Value::from(1_i16),
                Value::from(2_i16),
                Value::Null,
                Value::from("#runtime.cab"),
                Value::Null,
            ]))
            .unwrap();
        package
            .insert_rows(Insert::into("File").row(vec![
                Value::from("launcher_modules_libeacnet.dll"),
                Value::from("Runtime"),
                Value::from("LIBEAC~1.DLL|libeacnet.dll"),
                Value::from(libeacnet.len() as i32),
                Value::from(1_i16),
            ]))
            .unwrap();
        package
            .insert_rows(Insert::into("File").row(vec![
                Value::from("launcher_modules_avs2_core.dll"),
                Value::from("Runtime"),
                Value::from("AVS2CO~1.DLL|avs2-core.dll"),
                Value::from(avs2.len() as i32),
                Value::from(2_i16),
            ]))
            .unwrap();
        {
            let mut stream = package.write_stream("runtime.cab").unwrap();
            std::io::copy(&mut Cursor::new(cab), &mut stream).unwrap();
        }
        Synthetic { bytes: package.into_inner().unwrap().into_inner() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_names_are_rejected() {
        for name in ["../libeacnet.dll", "a/b", "a\\b", "x:y", " ", ".."] {
            assert!(is_hostile(name), "{name}");
        }
        assert!(!is_hostile("launcher.exe"));
        assert!(!is_hostile("libeacnet.dll"));
    }

    #[test]
    fn synthetic_msi_yields_the_pair_and_nothing_else() {
        let made = synth::package(b"eacnet-bytes", b"avs-bytes", Some(("launcher.exe", b"nope")));
        let dir = tempfile::tempdir().unwrap();
        let msi_path = dir.path().join("package.msi");
        std::fs::write(&msi_path, &made.bytes).unwrap();
        let extracted = extract_pair(&msi_path, dir.path()).unwrap();
        assert_eq!(std::fs::read(&extracted.libeacnet).unwrap(), b"eacnet-bytes");
        assert_eq!(std::fs::read(&extracted.avs2_core).unwrap(), b"avs-bytes");
        assert!(!dir.path().join("launcher.exe").exists());
    }

    #[test]
    fn traversal_member_fails_the_package() {
        let made = synth::package(b"eacnet", b"avs", Some(("../libeacnet.dll", b"bad")));
        let dir = tempfile::tempdir().unwrap();
        let msi_path = dir.path().join("package.msi");
        std::fs::write(&msi_path, &made.bytes).unwrap();
        let err = extract_pair(&msi_path, dir.path()).unwrap_err();
        assert!(matches!(err, BootstrapError::NameRejected { .. }), "{err}");
    }

    #[test]
    fn cabinet_case_alias_is_refused() {
        let made = synth::package(b"eacnet", b"avs", Some(("LAUNCHER_MODULES_LIBEACNET.DLL", b"bad")));
        let dir = tempfile::tempdir().unwrap();
        let msi_path = dir.path().join("package.msi");
        std::fs::write(&msi_path, made.bytes).unwrap();
        let err = extract_pair(&msi_path, dir.path()).unwrap_err();
        assert!(matches!(err, BootstrapError::NameRejected { .. }), "{err}");
        assert!(!dir.path().join("libeacnet.dll").exists());
    }

    #[test]
    fn malformed_file_mapping_is_refused_before_writing() {
        use msi::{Expr, Update};
        let cases = [
            ("launcher_modules_libeacnet.dll", "File", Value::from("../libeacnet.dll")),
            ("launcher_modules_libeacnet.dll", "File", Value::from("LAUNCHER_MODULES_AVS2_CORE.DLL")),
            ("launcher_modules_libeacnet.dll", "FileName", Value::from("Libeacnet.dll")),
            ("launcher_modules_libeacnet.dll", "FileName", Value::from("dir/libeacnet.dll")),
            ("launcher_modules_libeacnet.dll", "FileName", Value::from("x|libeacnet.dll|other.dll")),
            ("launcher_modules_avs2_core.dll", "FileName", Value::from("libeacnet.dll")),
            ("launcher_modules_libeacnet.dll", "Sequence", Value::from(9_i16)),
            ("launcher_modules_avs2_core.dll", "Sequence", Value::from(1_i16)),
            ("launcher_modules_libeacnet.dll", "FileSize", Value::from(123_i32)),
        ];
        for (key, column, value) in cases {
            let made = synth::package(b"eacnet", b"avs", None);
            let mut package = Package::open(std::io::Cursor::new(made.bytes)).unwrap();
            package.update_rows(Update::table("File").set(column, value.clone()).with(Expr::col("File").eq(Expr::string(key)))).unwrap();
            let bytes = package.into_inner().unwrap().into_inner();
            let dir = tempfile::tempdir().unwrap();
            let msi_path = dir.path().join("package.msi");
            std::fs::write(&msi_path, bytes).unwrap();
            let err = extract_pair(&msi_path, dir.path()).unwrap_err();
            assert!(matches!(err, BootstrapError::NameRejected { .. } | BootstrapError::PackageLayout(_)), "{column}={value:?}: {err}");
            assert!(!dir.path().join("libeacnet.dll").exists());
            assert!(!dir.path().join("avs2-core.dll").exists());
        }
    }

    #[test]
    fn external_cabinet_is_refused() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut package = msi::Package::create(msi::PackageType::Installer, cursor).unwrap();
        package
            .create_table(
                "Media",
                vec![
                    msi::Column::build("DiskId").primary_key().int16(),
                    msi::Column::build("LastSequence").int16(),
                    msi::Column::build("Cabinet").nullable().string(255),
                ],
            )
            .unwrap();
        package
            .insert_rows(msi::Insert::into("Media").row(vec![
                msi::Value::from(1_i16),
                msi::Value::from(1_i16),
                msi::Value::from("payload.cab"),
            ]))
            .unwrap();
        let bytes = package.into_inner().unwrap().into_inner();
        let dir = tempfile::tempdir().unwrap();
        let msi_path = dir.path().join("package.msi");
        std::fs::write(&msi_path, bytes).unwrap();
        let err = extract_pair(&msi_path, dir.path()).unwrap_err();
        assert!(matches!(err, BootstrapError::PackageLayout(_)), "{err}");
    }
}

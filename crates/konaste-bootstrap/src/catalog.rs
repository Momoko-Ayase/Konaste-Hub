//! Literal official package pins.
//!
//! Bombergirl and SDVX values are copied from `docs/research/konaste-platform.md`
//! §3 (research date 2026-10-14). DDR, pop'n, GITADORA, and NOSTALGIA values are
//! copied from `docs/research/remaining-title-contracts.md` and
//! `docs/research/older-title-contracts.md` (research date 2026-10-16). The
//! independent DLL pins are whole-file sizes and SHA-256 of the extracted
//! cabinet members, not certificate-blob sizes. They are not discovered at
//! runtime. A digest mismatch is a hard failure, not a prompt to accept the
//! new bytes. INFINITAS is not pinned: that package has no `libeacnet.dll`.

use crate::names::GameFamily;

/// A pinned replacement for the `libeacnet.dll` extracted from the MSI.
///
/// The MSI pin stays the bootstrap package. This file is the published runtime
/// DLL for a family whose current launcher is newer than that package. It is
/// downloaded only after the extracted cabinet member matches
/// [`OfficialPackage::bootstrap_libeacnet_sha256`].
#[derive(Debug, Clone, Copy)]
pub struct RuntimeUpdate {
    /// Full literal HTTPS URL on the package host.
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the extracted MSI `libeacnet.dll`.
    pub bootstrap_sha256: &'static str,
    /// Exact byte length of that extracted cabinet member.
    pub bootstrap_bytes: u64,
}

/// One recorded official bootstrap package.
#[derive(Debug, Clone, Copy)]
pub struct OfficialPackage {
    pub family: GameFamily,
    /// Stable id used in cache paths. Not an account or product code.
    pub id: &'static str,
    /// Full literal URL. Scheme, host, and path are all pinned.
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the MSI bytes.
    pub sha256: &'static str,
    /// Host that every redirect must stay on. No other host is followed.
    pub host: &'static str,
    /// Independent pins of the pair that is published. For SDVX this is the
    /// current `libeacnet.dll`, not the older cabinet member.
    pub libeacnet_sha256: &'static str,
    pub avs2_core_sha256: &'static str,
    pub libeacnet_bytes: u64,
    pub avs2_core_bytes: u64,
    /// Present only when the published `libeacnet.dll` is not the MSI member.
    pub runtime_update: Option<RuntimeUpdate>,
}

impl OfficialPackage {
    /// SHA-256 the cabinet member must have before any replacement is applied.
    pub const fn bootstrap_libeacnet_sha256(self) -> &'static str {
        match self.runtime_update {
            Some(update) => update.bootstrap_sha256,
            None => self.libeacnet_sha256,
        }
    }

    /// Byte length the cabinet member must have before any replacement.
    pub const fn bootstrap_libeacnet_bytes(self) -> u64 {
        match self.runtime_update {
            Some(update) => update.bootstrap_bytes,
            None => self.libeacnet_bytes,
        }
    }
}

/// Identity of a pin, separate from the family so tests can name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageId {
    Bombergirl2021122200,
    Sdvx2022011800,
    Ddr2022012601,
    Popn2021021801,
    Gitadora,
    Nostalgia,
}

pub fn package_for(family: GameFamily) -> &'static OfficialPackage {
    match family {
        GameFamily::Bombergirl => &BOMBERGIRL,
        GameFamily::Sdvx => &SDVX,
        GameFamily::Ddr => &DDR,
        GameFamily::Popn => &POPN,
        GameFamily::Gitadora => &GITADORA,
        GameFamily::Nostalgia => &NOSTALGIA,
    }
}

impl PackageId {
    pub const fn package(self) -> &'static OfficialPackage {
        match self {
            Self::Bombergirl2021122200 => &BOMBERGIRL,
            Self::Sdvx2022011800 => &SDVX,
            Self::Ddr2022012601 => &DDR,
            Self::Popn2021021801 => &POPN,
            Self::Gitadora => &GITADORA,
            Self::Nostalgia => &NOSTALGIA,
        }
    }
}

pub const BOMBERGIRL: OfficialPackage = OfficialPackage {
    family: GameFamily::Bombergirl,
    id: "bombergirl-2021122200",
    url: "https://d1q1iflay6y5jj.cloudfront.net/Installer/2021122200_b37bc853/BomberGirl_Installer.msi",
    sha256: "977f60c5d05828ca5263b7367cd0e153ed330fc134a5a141bb0ca32f98355aae",
    host: "d1q1iflay6y5jj.cloudfront.net",
    libeacnet_sha256: "a8cdc52b3eec6002011b2742f4cea84c01770e3d4ee7bb12ea7cacb3f21e2203",
    avs2_core_sha256: "2ac717d929f368d199a3fc4cb85f8066e79377db3675e66f8050bbf61277887e",
    libeacnet_bytes: 8_768_488,
    avs2_core_bytes: 1_635_400,
    runtime_update: None,
};

/// SHA-256 of `libeacnet.dll` inside the 2022-01-18 SDVX MSI. The published
/// pair uses [`SDVX::libeacnet_sha256`], which is the later pinned file.
pub const SDVX_BOOTSTRAP_LIBEACNET_SHA256: &str = "8f12689e3693065c837a13315fa066467bbc6b24461a1b1e4b299edcf227e438";

pub const SDVX_BOOTSTRAP_LIBEACNET_BYTES: u64 = 8_742_352;

pub const SDVX: OfficialPackage = OfficialPackage {
    family: GameFamily::Sdvx,
    id: "sdvx-2022011800",
    url: "https://dks1q2aivwkd6.cloudfront.net/vi/installer/sdvx_installer_2022011800.msi",
    sha256: "af3f96f91832a81882a44c9d5232163aaf0aa379df221b68541ee27e12e33894",
    host: "dks1q2aivwkd6.cloudfront.net",
    libeacnet_sha256: "84564b3f804dd66e6d7008895502b7d4caab17bdb19317590c0450cf2447df2c",
    avs2_core_sha256: "92132343f29b28dec9f9e513c34230435ee14658c988031f42498dcf699d6a85",
    libeacnet_bytes: 8_742_720,
    avs2_core_bytes: 1_633_232,
    runtime_update: Some(RuntimeUpdate {
        url: "https://dks1q2aivwkd6.cloudfront.net/vi/downloads/resource/2023011200/launcher/modules/libeacnet.dll",
        bootstrap_sha256: SDVX_BOOTSTRAP_LIBEACNET_SHA256,
        bootstrap_bytes: SDVX_BOOTSTRAP_LIBEACNET_BYTES,
    }),
};

pub const DDR: OfficialPackage = OfficialPackage {
    family: GameFamily::Ddr,
    id: "ddr-2022012601",
    url: "https://d2el0dli9l0x2p.cloudfront.net/installer/ddr_installer_2022012601.msi",
    sha256: "c938081a9eab7d0a35b14a36d392a8a56a10f379dd3f8f62d2b4a906deef729e",
    host: "d2el0dli9l0x2p.cloudfront.net",
    libeacnet_sha256: "aa91b712f2c2ffc63eda73efc53544199299670640ca6b49463c693790f494e0",
    avs2_core_sha256: "8a1473ae12064edf92228ed9c306346c1dc1e02031581e099a4a46d955b496c0",
    libeacnet_bytes: 8_742_400,
    avs2_core_bytes: 1_633_304,
    runtime_update: None,
};

/// SHA-256 of `libeacnet.dll` inside the pop'n MSI. The published pair uses
/// [`POPN::libeacnet_sha256`], which is the later pinned file.
pub const POPN_BOOTSTRAP_LIBEACNET_SHA256: &str = "59f5ab7f6151b111c9686e5d52a1fc9b737398cee1d85037598773da8d2c8f6c";

pub const POPN_BOOTSTRAP_LIBEACNET_BYTES: u64 = 8_724_912;

pub const POPN: OfficialPackage = OfficialPackage {
    family: GameFamily::Popn,
    id: "popn-2021021801",
    url: "https://d1twxdn5j8sbm3.cloudfront.net/installer/popn_installer_2021021801.msi",
    sha256: "e12b940d3a03344bca39e4e2ae8934cdcd4a0e8c9967d90a7cfa3cd09b341cac",
    host: "d1twxdn5j8sbm3.cloudfront.net",
    libeacnet_sha256: "119bfb79688e8ea323c183c319e0ec665fc0c27cc3587d5edc32d4ce0b6b60ae",
    avs2_core_sha256: "01f18b206d7cc1b7af40187daf5ea766ceeed026adfafe22431ce3912e9f803a",
    libeacnet_bytes: 8_753_008,
    avs2_core_bytes: 1_634_224,
    runtime_update: Some(RuntimeUpdate {
        url: "https://d1twxdn5j8sbm3.cloudfront.net/Updater/2026080500/launcher/modules/libeacnet.dll",
        bootstrap_sha256: POPN_BOOTSTRAP_LIBEACNET_SHA256,
        bootstrap_bytes: POPN_BOOTSTRAP_LIBEACNET_BYTES,
    }),
};

pub const GITADORA: OfficialPackage = OfficialPackage {
    family: GameFamily::Gitadora,
    id: "gitadora",
    url: "https://d1omkh45tn6edw.cloudfront.net/inst/GITADORA_installer.msi",
    sha256: "08481302fcb53f88df459b81928ae6158d3f6c18a5b7dbb057ac76af3a11bc2c",
    host: "d1omkh45tn6edw.cloudfront.net",
    libeacnet_sha256: "f016c1f61928124c1f658ef9b3ddba105fccc6cfef489ba5269a6705155812de",
    avs2_core_sha256: "c380e64b89bc2785cb452e7ef97ce77b67e606ce5d28859fe95d6319d7109778",
    libeacnet_bytes: 8_724_824,
    avs2_core_bytes: 1_634_136,
    runtime_update: None,
};

pub const NOSTALGIA: OfficialPackage = OfficialPackage {
    family: GameFamily::Nostalgia,
    id: "nostalgia",
    url: "https://d26kzmeiv4899f.cloudfront.net/installer/NOSTALGIA_installer.msi",
    sha256: "f4678d82ed35d502634568892a9171b4e46854b67c13ac901998b3c676adead0",
    host: "d26kzmeiv4899f.cloudfront.net",
    libeacnet_sha256: "0ceebfbbdfcf862ad877e1eb8741aea7980bd5a5a00c32f2addbe1c2e6cb6e00",
    avs2_core_sha256: "6f4f9f5882a4f9f7326d58047114e71e9c92331a2c74dfe7be672e5c19ec9efc",
    libeacnet_bytes: 8_720_744,
    avs2_core_bytes: 1_634_664,
    runtime_update: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_https_on_their_own_host() {
        for package in GameFamily::ALL.map(package_for) {
            let url = url::Url::parse(package.url).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some(package.host));
            for digest in [package.sha256, package.libeacnet_sha256, package.avs2_core_sha256] {
                assert_eq!(digest.len(), 64);
                assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
                assert_eq!(digest, digest.to_ascii_lowercase());
            }
            assert!(package.libeacnet_bytes > 0);
            assert!(package.avs2_core_bytes > 0);
            assert!(package.libeacnet_bytes <= crate::MAX_PACKAGE_BYTES);
            assert!(package.avs2_core_bytes <= crate::MAX_PACKAGE_BYTES);
            assert_eq!(package.family.as_str(), package.family.as_str().to_ascii_lowercase());
            assert_eq!(package_for(package.family).id, package.id);
            match package.runtime_update {
                None => {
                    assert_eq!(package.bootstrap_libeacnet_sha256(), package.libeacnet_sha256);
                    assert_eq!(package.bootstrap_libeacnet_bytes(), package.libeacnet_bytes);
                }
                Some(update) => {
                    let update_url = url::Url::parse(update.url).unwrap();
                    assert_eq!(update_url.scheme(), "https");
                    assert_eq!(update_url.host_str(), Some(package.host));
                    assert_ne!(update.bootstrap_sha256, package.libeacnet_sha256);
                    assert_ne!(update.bootstrap_bytes, package.libeacnet_bytes);
                    assert_eq!(update.bootstrap_sha256.len(), 64);
                    assert!(matches!(package.family, GameFamily::Sdvx | GameFamily::Popn));
                }
            }
        }
        assert_eq!(SDVX.sha256, "af3f96f91832a81882a44c9d5232163aaf0aa379df221b68541ee27e12e33894");
        assert_eq!(SDVX.avs2_core_sha256, "92132343f29b28dec9f9e513c34230435ee14658c988031f42498dcf699d6a85");
        assert_eq!(SDVX.bootstrap_libeacnet_bytes(), 8_742_352);
        assert_eq!(SDVX.libeacnet_bytes, 8_742_720);
        assert_eq!(POPN.sha256, "e12b940d3a03344bca39e4e2ae8934cdcd4a0e8c9967d90a7cfa3cd09b341cac");
        assert_eq!(POPN.avs2_core_sha256, "01f18b206d7cc1b7af40187daf5ea766ceeed026adfafe22431ce3912e9f803a");
        assert_eq!(POPN.avs2_core_bytes, 1_634_224);
        assert_eq!(POPN.bootstrap_libeacnet_bytes(), 8_724_912);
        assert_eq!(POPN.libeacnet_bytes, 8_753_008);
        assert_eq!(GameFamily::ALL.len(), 6);
        assert!(GameFamily::parse("infinitas").is_none());
        let ids = [
            PackageId::Bombergirl2021122200,
            PackageId::Sdvx2022011800,
            PackageId::Ddr2022012601,
            PackageId::Popn2021021801,
            PackageId::Gitadora,
            PackageId::Nostalgia,
        ];
        assert_eq!(ids.len(), GameFamily::ALL.len());
        for (id, family) in ids.into_iter().zip(GameFamily::ALL) {
            assert_eq!(id.package().family, family);
            assert_eq!(id.package().sha256, package_for(family).sha256);
        }
    }
}

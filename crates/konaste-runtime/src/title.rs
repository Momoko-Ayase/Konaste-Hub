//! Verified title data for the shared native runtime, discovery, and installer.
//!
//! A checked native discovery caller does not imply a checked game installation
//! or creation contract. Each is admitted separately from primary evidence.

use konaste_bootstrap::GameFamily;
use url::Url;

use crate::RuntimeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootLayout {
    Legacy,
    Services,
}

#[derive(Clone, Copy)]
pub struct DiscoveryProfile {
    pub host: &'static str,
    pub(crate) resource_prefix: &'static str,
    pub(crate) game_id: &'static str,
    pub(crate) updater_version: &'static str,
    pub(crate) launcher_version: &'static str,
    pub(crate) layout: BootLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitadoraMode {
    GuitarFreaks,
    DrumMania,
}

impl GitadoraMode {
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::GuitarFreaks => "-gf",
            Self::DrumMania => "-dm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArgumentProfile {
    Token,
    DdrSettings,
    GitadoraSettings,
    /// pop'n: the reporter's fixed bootstrap prefix, see `arguments::popn_prefix`.
    PopnBootstrap,
}

pub(crate) struct Launch {
    pub working_directory: &'static str,
    pub profile: &'static DiscoveryProfile,
    pub arguments: ArgumentProfile,
}

pub struct Title {
    pub module_path: &'static str,
    pub index_path: &'static str,
    pub manifest_module: &'static str,
    pub content_root: &'static str,
    pub host: &'static str,
    pub content_prefix: &'static str,
    pub(crate) product_code: &'static str,
    launch: Option<Launch>,
}

/// A checked mode-2 caller and fixed HTTPS host, not installation permission.
pub fn discovery_profile(family: GameFamily) -> Result<&'static DiscoveryProfile, RuntimeError> {
    match family {
        GameFamily::Bombergirl => Ok(&BG_PROFILE),
        GameFamily::Sdvx => Ok(&SDVX_PROFILE),
        GameFamily::Ddr => Ok(&DDR_PROFILE),
        GameFamily::Popn => Ok(&POPN_PROFILE),
        GameFamily::Gitadora => Ok(&GITADORA_PROFILE),
        GameFamily::Nostalgia => Ok(&NOSTALGIA_PROFILE),
    }
}

/// Verified installation data. Launch is a separate, explicit capability.
/// Neither contract comes from frontend IPC or an editable transaction.
pub fn title(family: GameFamily) -> Result<&'static Title, RuntimeError> {
    match family {
        GameFamily::Bombergirl => Ok(&BOMBERGIRL),
        GameFamily::Sdvx => Ok(&SDVX),
        GameFamily::Ddr => Ok(&DDR),
        GameFamily::Popn => Ok(&POPN),
        GameFamily::Gitadora => Ok(&GITADORA),
        GameFamily::Nostalgia => Ok(&NOSTALGIA),
    }
}

impl DiscoveryProfile {
    pub(crate) fn record_bytes(&self) -> usize {
        match self.layout {
            BootLayout::Legacy => 0xA0,
            BootLayout::Services => 0x89C,
        }
    }

    pub fn resource_url(&self, value: &str) -> Option<Url> {
        if value.len() >= 0x800 || !value.is_ascii() || value.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace()) {
            return None;
        }
        let url = Url::parse(value).ok()?;
        (url.scheme() == "https"
            && url.host_str() == Some(self.host)
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path().starts_with(self.resource_prefix)
            && url.path().ends_with(".xml"))
        .then_some(url)
    }
}

impl Title {
    pub fn launch_supported(&self) -> bool {
        self.launch.is_some()
    }

    pub(crate) fn launch(&self) -> Result<&Launch, RuntimeError> {
        self.launch.as_ref().ok_or(RuntimeError::Unsupported)
    }

    pub(crate) fn valid_game_version(&self, value: &str) -> bool {
        let parts: Vec<_> = value.split(':').collect();
        parts.len() == 5
            && parts[0] == self.product_code
            && parts[1..4].iter().all(|part| part.len() == 1 && part.as_bytes()[0].is_ascii_uppercase())
            && parts[4].len() == 10
            && parts[4].bytes().all(|byte| byte.is_ascii_digit())
    }
}

const BG_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "d1q1iflay6y5jj.cloudfront.net",
    resource_prefix: "/ResourceInfo/",
    game_id: "bomber-girl",
    updater_version: "QBG:J:C:A:2021122100",
    launcher_version: "QBG:J:B:A:2021122100",
    layout: BootLayout::Legacy,
};

// Caller ABI/client versions are statically verified; the resource namespace
// was confirmed by a fresh SDK receipt on 2026-10-16. Discovery alone does not
// enable installation or launch.
const SDVX_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "dks1q2aivwkd6.cloudfront.net",
    resource_prefix: "/vi/downloads/resource_info/",
    game_id: "sdvx",
    updater_version: "QCV:J:C:A:2023021500",
    launcher_version: "QCV:J:A:A:2023021500",
    layout: BootLayout::Services,
};

const DDR_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "d2el0dli9l0x2p.cloudfront.net",
    resource_prefix: "/list/",
    game_id: "ddr",
    updater_version: "VGP:J:C:A:2026012800",
    launcher_version: "VGP:J:B:A:2026012800",
    layout: BootLayout::Legacy,
};

const POPN_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "d1twxdn5j8sbm3.cloudfront.net",
    resource_prefix: "/ResourceInfo/",
    game_id: "popn-music",
    updater_version: "U39:J:D:A:2026031100",
    launcher_version: "U39:J:B:A:2026031100",
    layout: BootLayout::Services,
};

const GITADORA_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "d1omkh45tn6edw.cloudfront.net",
    resource_prefix: "/data/list/",
    game_id: "gitadora",
    updater_version: "U32:J:D:A:2026062300",
    launcher_version: "U32:J:C:A:2026062300",
    layout: BootLayout::Legacy,
};

const NOSTALGIA_PROFILE: DiscoveryProfile = DiscoveryProfile {
    host: "d26kzmeiv4899f.cloudfront.net",
    resource_prefix: "/data/list/",
    game_id: "nostalgia",
    updater_version: "UAN:J:C:A:2023042500",
    launcher_version: "UAN:J:A:A:2023042500",
    layout: BootLayout::Legacy,
};

const BOMBERGIRL: Title = Title {
    module_path: "modules/bomber.exe",
    index_path: "ri.bin",
    manifest_module: "modules/bomber.exe",
    content_root: "",
    host: BG_PROFILE.host,
    content_prefix: "/Updater/",
    product_code: "QBG",
    launch: Some(Launch { working_directory: "", profile: &BG_PROFILE, arguments: ArgumentProfile::Token }),
};

const SDVX: Title = Title {
    module_path: "game/modules/sv6c.exe",
    index_path: "ri.bin",
    manifest_module: "game/modules/sv6c.exe",
    content_root: "",
    host: SDVX_PROFILE.host,
    content_prefix: "/vi/downloads/resource/",
    product_code: "QCV",
    launch: Some(Launch { working_directory: "", profile: &SDVX_PROFILE, arguments: ArgumentProfile::Token }),
};

const DDR: Title = Title {
    module_path: "game/modules/ddr-konaste.exe",
    index_path: "ri.bin",
    manifest_module: "game/modules/ddr-konaste.exe",
    content_root: "",
    host: DDR_PROFILE.host,
    content_prefix: "/update/",
    product_code: "VGP",
    launch: Some(Launch { working_directory: "game", profile: &DDR_PROFILE, arguments: ArgumentProfile::DdrSettings }),
};

const POPN: Title = Title {
    module_path: "game/module/popnLively.exe",
    index_path: "ri.bin",
    manifest_module: "game/module/popnLively.exe",
    content_root: "",
    host: POPN_PROFILE.host,
    content_prefix: "/Updater/",
    product_code: "U39",
    launch: Some(Launch { working_directory: "game", profile: &POPN_PROFILE, arguments: ArgumentProfile::PopnBootstrap }),
};

const GITADORA: Title = Title {
    module_path: "game/modules/gitadora.exe",
    index_path: "ri.bin",
    manifest_module: "game/modules/gitadora.exe",
    content_root: "",
    host: GITADORA_PROFILE.host,
    content_prefix: "/data/",
    product_code: "U32",
    launch: Some(Launch { working_directory: "game", profile: &GITADORA_PROFILE, arguments: ArgumentProfile::GitadoraSettings }),
};

const NOSTALGIA: Title = Title {
    module_path: "game/modules/nostalgia.exe",
    index_path: "ri.bin",
    manifest_module: "game/modules/nostalgia.exe",
    content_root: "",
    host: NOSTALGIA_PROFILE.host,
    content_prefix: "/data/",
    product_code: "UAN",
    launch: Some(Launch { working_directory: "game", profile: &NOSTALGIA_PROFILE, arguments: ArgumentProfile::Token }),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_profile_data_fits_the_native_record() {
        for family in GameFamily::ALL {
            let Ok(profile) = discovery_profile(family) else {
                continue;
            };
            assert!(!profile.game_id.is_empty() && profile.game_id.len() < 0x41);
            assert!(profile.game_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
            assert!(profile.updater_version.len() < 0x15);
            assert!(profile.launcher_version.len() < 0x15);
            assert_ne!(profile.updater_version, profile.launcher_version);
        }
    }

    #[test]
    fn full_title_paths_and_native_profiles_agree() {
        for family in GameFamily::ALL {
            let Ok(title) = title(family) else { continue };
            assert_eq!(title.host, discovery_profile(family).unwrap().host);
            let module = if title.content_root.is_empty() {
                title.manifest_module.to_string()
            } else {
                format!("{}/{}", title.content_root, title.manifest_module)
            };
            assert_eq!(title.module_path, module);
            assert_eq!(title.index_path, "ri.bin");
            assert!(!title.valid_game_version("ZZZ:J:A:A:2026010100"));
            for path in [title.module_path, title.index_path] {
                assert!(!path.starts_with('/') && !path.contains(['\\', ':']));
                assert!(path.split('/').all(|part| !part.is_empty() && part != "." && part != ".."));
            }
        }
    }

    #[test]
    fn resource_urls_never_accept_another_title_host_or_installer_path() {
        let profile = discovery_profile(GameFamily::Bombergirl).unwrap();
        assert!(profile.resource_url("https://d1q1iflay6y5jj.cloudfront.net/ResourceInfo/synthetic/index.xml").is_some());
        for value in [
            "https://dks1q2aivwkd6.cloudfront.net/ResourceInfo/synthetic/index.xml",
            "https://d1q1iflay6y5jj.cloudfront.net/Installer/index.xml",
            "https://d1q1iflay6y5jj.cloudfront.net/ResourceInfo/index.xml?token=synthetic",
            "https://d1q1iflay6y5jj.cloudfront.net/ResourceInfo/index.xml#fragment",
            "https://user@d1q1iflay6y5jj.cloudfront.net/ResourceInfo/index.xml",
            "http://d1q1iflay6y5jj.cloudfront.net/ResourceInfo/index.xml",
            "https://d1q1iflay6y5jj.cloudfront.net:444/ResourceInfo/index.xml",
        ] {
            assert!(profile.resource_url(value).is_none());
        }
    }
}

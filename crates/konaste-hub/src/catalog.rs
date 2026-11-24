use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameId {
    BomberGirl,
    Sdvx,
    Infinitas,
    Ddr,
    Popn,
    Gitadora,
    Nostalgia,
}

impl GameId {
    /// Bootstrap family only. Presence here is not an install or launch contract.
    pub fn runtime_family(self) -> Option<konaste_bootstrap::GameFamily> {
        match self {
            Self::BomberGirl => Some(konaste_bootstrap::GameFamily::Bombergirl),
            Self::Sdvx => Some(konaste_bootstrap::GameFamily::Sdvx),
            Self::Ddr => Some(konaste_bootstrap::GameFamily::Ddr),
            Self::Popn => Some(konaste_bootstrap::GameFamily::Popn),
            Self::Gitadora => Some(konaste_bootstrap::GameFamily::Gitadora),
            Self::Nostalgia => Some(konaste_bootstrap::GameFamily::Nostalgia),
            Self::Infinitas => None,
        }
    }

    /// Verified title contract. `None` means discovery, install, and launch stay closed.
    pub fn title(self) -> Option<&'static konaste_runtime::Title> {
        self.runtime_family().and_then(|family| konaste_runtime::title(family).ok())
    }

    /// Selected id accepts this family only when that family's runtime contract exists.
    pub fn admits(self, family: konaste_bootstrap::GameFamily) -> bool {
        self.runtime_family() == Some(family) && self.title().is_some()
    }

    /// A local presence marker is not update or launch evidence.
    pub fn installation_marker(self) -> Option<&'static str> {
        self.title().map(|title| title.module_path).or(descriptor(self).executable)
    }

    /// Running-image path from the verified contract, never a catalog guess.
    pub fn module_image(self, root: &std::path::Path) -> Option<std::path::PathBuf> {
        self.title().map(|title| root.join(title.module_path))
    }

    /// Install evidence is not permission to create the game.
    pub fn launch_supported(self) -> bool {
        self.title().is_some_and(|title| title.launch_supported())
    }

    /// Settings are available without implying a shared runtime contract.
    pub fn has_settings(self) -> bool {
        self.runtime_family().is_some_and(|family| konaste_runtime::settings_path(family).is_ok())
    }
}

pub struct GameDescriptor {
    pub id: GameId,
    pub title: &'static str,
    pub registry: Option<&'static str>,
    pub executable: Option<&'static str>,
    pub scheme: Option<&'static str>,
    pub login_id: Option<&'static str>,
}

pub const CATALOG: [GameDescriptor; 7] = [
    GameDescriptor {
        id: GameId::BomberGirl,
        title: "Bombergirl",
        registry: Some(r"SOFTWARE\KONAMI\BOMBERGIRL"),
        executable: Some("modules/bomber.exe"),
        scheme: Some("konaste.bomber-girl"),
        login_id: Some("bomber-girl"),
    },
    GameDescriptor {
        id: GameId::Sdvx,
        title: "SOUND VOLTEX EXCEED GEAR",
        registry: Some(r"SOFTWARE\KONAMI\SOUND VOLTEX EXCEED GEAR"),
        executable: Some("game/modules/sv6c.exe"),
        scheme: Some("konaste.sdvx"),
        login_id: Some("sdvx"),
    },
    // Unsupported: the game embeds its own client, so there is no libeacnet
    // worker to reuse. No registry or marker means no detection, import, or settings.
    GameDescriptor {
        id: GameId::Infinitas,
        title: "beatmania IIDX INFINITAS",
        registry: None,
        executable: None,
        scheme: None,
        login_id: None,
    },
    GameDescriptor {
        id: GameId::Ddr,
        title: "DanceDanceRevolution GRAND PRIX",
        registry: Some(r"SOFTWARE\KONAMI\DanceDanceRevolution"),
        executable: Some("game/modules/ddr-konaste.exe"),
        scheme: Some("konaste.ddr"),
        login_id: Some("ddr"),
    },
    GameDescriptor {
        id: GameId::Popn,
        title: "pop'n music Lively",
        registry: Some(r"SOFTWARE\KONAMI\pop'n music Lively"),
        executable: Some("game/module/popnLively.exe"),
        scheme: Some("konaste.popn-music"),
        login_id: Some("popn-music"),
    },
    GameDescriptor {
        id: GameId::Gitadora,
        title: "GITADORA",
        registry: Some(r"SOFTWARE\KONAMI\GITADORA"),
        executable: Some("game/modules/gitadora.exe"),
        scheme: Some("konaste.gitadora"),
        login_id: Some("gitadora"),
    },
    GameDescriptor {
        id: GameId::Nostalgia,
        title: "NOSTALGIA",
        registry: Some(r"SOFTWARE\KONAMI\NOSTALGIA"),
        executable: Some("game/modules/nostalgia.exe"),
        scheme: Some("konaste.nostalgia"),
        login_id: Some("nostalgia"),
    },
];

pub fn descriptor(id: GameId) -> &'static GameDescriptor {
    CATALOG.iter().find(|game| game.id == id).expect("catalog contains every GameId")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_match_the_frontend_without_guessed_contracts() {
        assert_eq!(serde_json::to_string(&GameId::BomberGirl).unwrap(), "\"bomber-girl\"");
        assert_eq!(serde_json::to_string(&GameId::Sdvx).unwrap(), "\"sdvx\"");
        assert!(descriptor(GameId::Infinitas).login_id.is_none());
        assert_eq!(CATALOG.len(), 7);
        assert!(GameId::BomberGirl.admits(konaste_bootstrap::GameFamily::Bombergirl));
        assert!(!GameId::BomberGirl.admits(konaste_bootstrap::GameFamily::Sdvx));
        assert!(GameId::Sdvx.admits(konaste_bootstrap::GameFamily::Sdvx));
        assert!(!GameId::Sdvx.admits(konaste_bootstrap::GameFamily::Bombergirl));
        let sdvx = GameId::Sdvx.title().unwrap();
        assert_eq!(sdvx.module_path, "game/modules/sv6c.exe");
        assert_eq!(sdvx.manifest_module, "game/modules/sv6c.exe");
        assert_eq!(sdvx.index_path, "ri.bin");
        assert!(sdvx.content_root.is_empty());
        assert_eq!(sdvx.content_prefix, "/vi/downloads/resource/");
        assert_eq!(sdvx.host, "dks1q2aivwkd6.cloudfront.net");
        assert_eq!(
            GameId::Sdvx.module_image(std::path::Path::new(r"C:\synthetic")).unwrap(),
            std::path::Path::new(r"C:\synthetic\game\modules\sv6c.exe")
        );
        assert!(GameId::Infinitas.runtime_family().is_none());
        assert!(GameId::Nostalgia.admits(konaste_bootstrap::GameFamily::Nostalgia));
        assert!(!GameId::Nostalgia.admits(konaste_bootstrap::GameFamily::Bombergirl));
        assert!(GameId::Ddr.launch_supported());
        assert!(GameId::Popn.launch_supported());
        assert!(GameId::Gitadora.launch_supported());
        assert!(GameId::Nostalgia.launch_supported());
        assert!(!GameId::Infinitas.launch_supported());
        assert_eq!(descriptor(GameId::Ddr).scheme, Some("konaste.ddr"));
        assert_eq!(descriptor(GameId::Ddr).login_id, Some("ddr"));
        assert_eq!(descriptor(GameId::Popn).scheme, Some("konaste.popn-music"));
        assert_eq!(descriptor(GameId::Popn).login_id, Some("popn-music"));
        assert_eq!(descriptor(GameId::Nostalgia).scheme, Some("konaste.nostalgia"));
        assert_eq!(descriptor(GameId::Nostalgia).login_id, Some("nostalgia"));
        assert_eq!(descriptor(GameId::Gitadora).login_id, Some("gitadora"));
        assert_eq!(descriptor(GameId::Gitadora).scheme, Some("konaste.gitadora"));
        // INFINITAS has no shared libeacnet worker and a different login path.
        assert!(descriptor(GameId::Infinitas).login_id.is_none());
        assert!(GameId::Infinitas.runtime_family().is_none());
        assert!(GameId::Infinitas.module_image(std::path::Path::new("root")).is_none());
        assert!(GameId::Ddr.runtime_family().is_some());
        assert!(GameId::Popn.runtime_family().is_some());
        assert!(GameId::Gitadora.runtime_family().is_some());
        assert!(GameId::Nostalgia.runtime_family().is_some());
        assert_eq!(descriptor(GameId::Nostalgia).login_id, Some("nostalgia"));
        assert_eq!(descriptor(GameId::Ddr).executable, Some("game/modules/ddr-konaste.exe"));
        assert_eq!(descriptor(GameId::Popn).executable, Some("game/module/popnLively.exe"));
        assert_eq!(descriptor(GameId::Gitadora).executable, Some("game/modules/gitadora.exe"));
        assert_eq!(descriptor(GameId::Nostalgia).executable, Some("game/modules/nostalgia.exe"));
        assert_eq!(descriptor(GameId::Ddr).registry, Some(r"SOFTWARE\KONAMI\DanceDanceRevolution"));
        assert!(GameId::BomberGirl.launch_supported());
        assert!(GameId::Sdvx.launch_supported());
        // Settings programs exist for five titles; Bombergirl ships none.
        assert!(!GameId::BomberGirl.has_settings());
        for id in [GameId::Sdvx, GameId::Ddr, GameId::Popn, GameId::Gitadora, GameId::Nostalgia] {
            assert!(id.has_settings());
        }
        assert_eq!(descriptor(GameId::Sdvx).executable, Some("game/modules/sv6c.exe"));
        // INFINITAS is listed as unsupported: nothing to detect, import, or configure.
        assert!(!GameId::Infinitas.has_settings());
        assert!(GameId::Infinitas.installation_marker().is_none());
        assert!(descriptor(GameId::Infinitas).registry.is_none());
    }
}

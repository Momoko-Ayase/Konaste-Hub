//! Embedded UI icons and the app icon. Game artwork lives in `art.rs`.
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(pub Icons, [
    Download, Settings, Plus, Minus, Square, X, Play, Pause, RotateCw, LoaderCircle, FolderOpen,
    ShieldCheck, SlidersHorizontal, Trash, ExternalLink, Eye, EyeOff, UserRound, ChevronLeft,
    ChevronRight, ChevronDown, ChevronUp, ArrowUpToLine, ArrowUpNarrowWide, Clock, LogOut, Wallet, TriangleAlert, Ellipsis, Check, KeyRound, ArrowLeft, RefreshCw,
]);

const ART: &[(&str, &[u8])] = &[("icon.svg", include_bytes!("../../assets/icon.svg"))];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ART.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(found) = Icons.load(path)? {
            return Ok(Some(found));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all: Vec<SharedString> = ART.iter().map(|(n, _)| SharedString::from(*n)).filter(|n| n.starts_with(path)).collect();
        all.extend(Icons.list(path)?);
        all.extend(gpui_kit::assets::Assets.list(path)?);
        all.sort();
        all.dedup();
        Ok(all)
    }
}

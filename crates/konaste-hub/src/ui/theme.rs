//! Colors and shared element builders. The brand red is #ad091b.
use gpui_kit::*;

pub const ACCENT: u32 = 0xad091b;

pub fn bg() -> Hsla {
    rgb(0x0e0f12).into()
}
pub fn rail() -> Hsla {
    rgb(0x121317).into()
}
pub fn panel() -> Hsla {
    rgba(0x0e0f13d1).into()
}
pub fn panel_solid() -> Hsla {
    rgb(0x17181d).into()
}
pub fn line() -> Hsla {
    rgba(0xffffff14).into()
}
pub fn text() -> Hsla {
    rgb(0xf2f3f5).into()
}
pub fn muted() -> Hsla {
    rgb(0x9a9ca5).into()
}
pub fn accent() -> Hsla {
    rgb(ACCENT).into()
}
pub fn accent_hover() -> Hsla {
    rgb(0xc20d22).into()
}
pub fn accent_active() -> Hsla {
    rgb(0x930716).into()
}
/// The brand red lifted for text on dark surfaces, where #ad091b is too dim to read.
pub fn accent_text() -> Hsla {
    rgb(0xf0707e).into()
}
pub fn soft() -> Hsla {
    rgba(0x1e1f24eb).into()
}
pub fn ghost() -> Hsla {
    rgba(0xffffff0f).into()
}
pub fn ghost_hover() -> Hsla {
    rgba(0xffffff1f).into()
}
pub fn ok() -> Hsla {
    rgb(0x3ddc97).into()
}
pub fn warn() -> Hsla {
    rgb(0xffb020).into()
}
pub fn danger_text() -> Hsla {
    rgb(0xff8a80).into()
}
pub fn gold() -> Hsla {
    rgb(0xffd36b).into()
}
pub fn blue() -> Hsla {
    rgb(0x4d9bff).into()
}

pub const FONT: &str = "Segoe UI";

pub fn shadow(y: f32, blur: f32, alpha: f32) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(y), hsla(0., 0., 0., alpha)).blur_radius(px(blur))]
}

pub fn icon(path: &'static str, size: f32, color: Hsla) -> Svg {
    svg().path(path).size(px(size)).flex_none().text_color(color)
}

pub mod icons {
    pub const DOWNLOAD: &str = "icons/download.svg";
    pub const SETTINGS: &str = "icons/settings.svg";
    pub const PLUS: &str = "icons/plus.svg";
    pub const MINUS: &str = "icons/minus.svg";
    pub const SQUARE: &str = "icons/square.svg";
    pub const X: &str = "icons/x.svg";
    pub const PLAY: &str = "icons/play.svg";
    pub const PAUSE: &str = "icons/pause.svg";
    pub const ROTATE: &str = "icons/rotate-cw.svg";
    pub const LOADER: &str = "icons/loader-circle.svg";
    pub const FOLDER: &str = "icons/folder-open.svg";
    pub const SHIELD: &str = "icons/shield-check.svg";
    pub const SLIDERS: &str = "icons/sliders-horizontal.svg";
    pub const TRASH: &str = "icons/trash.svg";
    pub const EXTERNAL: &str = "icons/external-link.svg";
    pub const EYE: &str = "icons/eye.svg";
    pub const EYE_OFF: &str = "icons/eye-off.svg";
    pub const USER: &str = "icons/user-round.svg";
    pub const CHEVRON_DOWN: &str = "icons/chevron-down.svg";
    pub const ARROW_UP_TO_LINE: &str = "icons/arrow-up-to-line.svg";
    pub const CLOCK: &str = "icons/clock.svg";
    pub const ARROW_UP_NARROW_WIDE: &str = "icons/arrow-up-narrow-wide.svg";
    pub const LOG_OUT: &str = "icons/log-out.svg";
    pub const WALLET: &str = "icons/wallet.svg";
    pub const ALERT: &str = "icons/triangle-alert.svg";
    pub const MORE: &str = "icons/ellipsis.svg";
    pub const CHECK: &str = "icons/check.svg";
    pub const KEY: &str = "icons/key-round.svg";
    pub const BACK: &str = "icons/arrow-left.svg";
    pub const REFRESH: &str = "icons/refresh-cw.svg";
}

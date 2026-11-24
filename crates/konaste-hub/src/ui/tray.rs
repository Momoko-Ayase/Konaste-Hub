//! Notification-area icon for "Minimize to tray".
//!
//! Close hides the main window (no taskbar button) and shows this icon. A left
//! click, or Show in its menu, brings the window back. Quit exits even while
//! hidden. The icon exists only while the window is hidden.
#![cfg(windows)]

use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow},
};

/// What the user asked for from the tray. Read on the UI loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Show,
    Quit,
}

pub struct Tray {
    icon: Option<TrayIcon>,
    show_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
    show: MenuItem,
    quit: MenuItem,
}

impl Tray {
    pub fn new() -> Self {
        let show = MenuItem::new("Show", true, None);
        let quit = MenuItem::new("Quit", true, None);
        Self { icon: None, show_id: show.id().clone(), quit_id: quit.id().clone(), show, quit }
    }

    pub fn visible(&self) -> bool {
        self.icon.is_some()
    }

    /// Hide `hwnd` from the screen and taskbar, then show the icon.
    pub fn hide_to_tray(&mut self, hwnd: HWND, show_label: &str, quit_label: &str) {
        self.show.set_text(show_label);
        self.quit.set_text(quit_label);
        if self.icon.is_none() {
            let menu = Menu::new();
            let _ = menu.append(&self.show);
            let _ = menu.append(&self.quit);
            self.icon = TrayIconBuilder::new()
                .with_tooltip("Konaste Hub")
                .with_icon(icon())
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .ok();
        }
        // Without an icon there is no way back, so keep the window instead.
        if self.icon.is_some() {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    /// Bring the window back and remove the icon.
    pub fn restore(&mut self, hwnd: HWND) {
        unsafe {
            ShowWindow(hwnd, if IsIconic(hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
            SetForegroundWindow(hwnd);
        }
        self.icon = None;
    }

    /// Drain pending tray and menu events.
    pub fn poll(&self) -> Option<Request> {
        let mut request = None;
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event
            {
                request.get_or_insert(Request::Show);
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit_id {
                request = Some(Request::Quit);
            } else if event.id == self.show_id {
                request.get_or_insert(Request::Show);
            }
        }
        request
    }
}

/// The app icon from the bundled ICO, scaled to the tray size.
fn icon() -> Icon {
    const ICO: &[u8] = include_bytes!("../../assets/icon.ico");
    decode_ico(ICO)
        .and_then(|(rgba, size)| Icon::from_rgba(rgba, size, size).ok())
        .unwrap_or_else(|| Icon::from_rgba([0xad, 0x09, 0x1b, 0xff].repeat(16 * 16), 16, 16).expect("solid icon"))
}

const TRAY_PX: u32 = 32;

fn decode_ico(bytes: &[u8]) -> Option<(Vec<u8>, u32)> {
    let icon = image::load_from_memory_with_format(bytes, image::ImageFormat::Ico).ok()?.into_rgba8();
    let icon = image::imageops::resize(&icon, TRAY_PX, TRAY_PX, image::imageops::FilterType::Lanczos3);
    Some((icon.into_raw(), TRAY_PX))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bundled_icon_decodes() {
        let (rgba, size) = super::decode_ico(include_bytes!("../../assets/icon.ico")).unwrap();
        assert_eq!(size, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0));
    }
}

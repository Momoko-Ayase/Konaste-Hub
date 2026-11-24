# Interface design

Rules for the native Hub UI. Code: `crates/konaste-hub/src/ui/`.

## Layout

- Frameless window with a custom title bar. Dark theme only.
- Left rail: icons of installed games, then **Add game**. Downloads and Settings sit at the bottom of the rail.
- The selected game fills the window with its stage background. The primary action (Play, Update, Install, Retry) sits at the lower right.
- A stage panel holds news, details, and e-amusement play data for the selected title.
- Add game lists titles not yet in the library, with Install and Locate installed. INFINITAS appears as unsupported.
- Account: an avatar chip with the PASELI balance in the title bar opens a right-side drawer with profile, e-amusement pass card, PASELI balance and points, links (My Page, My KONAMI, e-amusement pass menu, 購買部, コース加入), and Sign out.
- Settings: General (language, close button, default install folder, parallel downloads), Account (Stay signed in), About (license, unofficial notice, data folder).

## Copy

- Show only what is needed to pick, install, update, launch, or configure a game, understand its state, or change a setting.
- No slogans, filler subtitles, flavor text, decorative labels, or decorative dots. Don't overuse dashes.
- Don't repeat a title, status, or action without a reason.
- Errors say what went wrong and what to do next, briefly.
- Use plain action names: Play, Update, Retry, Install, Install elsewhere, Cancel install.
- Keep official KONAMI text (terms, notices) in the language KONAMI supplies.
- Every string goes through `i18n.rs` in all five locales.

## Color

- Brand red: **`#ad091b`**. Used for primary buttons, active indicators, selection outlines, progress bars, toggles, and the app icon. Hover and pressed states are lighter and darker steps of the same red (`theme::accent_hover`, `accent_active`); `accent_text` lifts it for text on dark surfaces. Destructive actions and the close-window hover use the same red.
- PASELI green and e-amusement card colors are only used on those service elements.
- Text, panels, and lines come from `ui/theme.rs`. Add new colors there rather than inline.

## Controls

- A setting with more than two options uses a dropdown. Segmented toggles are for two options only.
- Switches for on/off settings.

## Motion

- Overlays, drawers, dialogs, and toasts animate in and out. State changes crossfade or spring; nothing swaps instantly.
- Use the helpers in `ui/motion.rs` (`MOVE`, `SETTLE`, `slide`, `fade`, `leave`, `pulse`). Exits last `LEAVE_MS`.
- gpui renders animations in their end state when Windows asks for reduced motion.

## Artwork

- No official logo, key art, or screenshot ships inside the binary except the PASELI and e-amusement service marks.
- Stage backgrounds are downloaded from this repository (see [architecture](architecture.md#asset-sources)). Until one is available, an original SVG fallback is shown.
- Game icons are read from the user's own installation.

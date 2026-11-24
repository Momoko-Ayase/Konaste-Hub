//! Shared motion. gpui renders every animation in its end state when the
//! system asks for reduced motion.
use gpui_kit::*;
use std::time::Duration;

/// Slightly underdamped: for things that move (indicators, knobs, bars).
pub const MOVE: SpringConfig = SpringConfig::new(320., 30., 1.);
/// Critically damped: settles without overshoot (opacity, colors).
pub const SETTLE: SpringConfig = SpringConfig::new(400., 40., 1.);

/// Exit length for overlays and toasts; the closing layer is dropped after this.
pub const LEAVE_MS: u64 = 140;

pub fn ease(ms: u64) -> Animation {
    Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint())
}

/// Fade in while moving `dx` pixels into place. Does not affect layout.
pub fn slide<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>, ms: u64, dx: f32) -> AnimationElement<E> {
    el.with_animation(id, ease(ms), move |el, t| el.opacity(t).left(px(dx * (1. - t))))
}

pub fn fade<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>, ms: u64) -> AnimationElement<E> {
    el.with_animation(id, ease(ms), |el, t| el.opacity(t))
}

/// Fade out while drifting `dy` pixels.
pub fn leave<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>, dy: f32) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(Duration::from_millis(LEAVE_MS)).with_easing(quadratic), move |el, t| {
        el.opacity(1. - t).top(px(dy * t))
    })
}

/// Endless soft pulse for placeholders.
pub fn pulse<E: Styled + IntoElement + 'static>(el: E, id: impl Into<ElementId>, low: f32) -> AnimationElement<E> {
    el.with_animation(id, Animation::new(Duration::from_millis(1400)).repeat().with_easing(pulsating_between(low, 1.)), |el, t| {
        el.opacity(t)
    })
}

/// Spring a value toward `target`, starting from `from` the first time it renders.
pub fn spring<E: IntoElement + 'static>(
    el: E,
    id: impl Into<ElementId>,
    config: SpringConfig,
    from: f32,
    target: f32,
    animator: impl FnOnce(E, f32) -> E + 'static,
) -> SpringAnimationElement<E> {
    el.with_spring(id, SpringAnimation::new(config).to(target).from(from).with_epsilon(0.002), animator)
}

/// Linear blend between two colors.
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0., 1.);
    let a = a.to_rgb();
    let b = b.to_rgb();
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }.into()
}

/// `rise` after a delay, for staggering a group.
pub fn rise_after<E: Styled + IntoElement + 'static>(
    el: E,
    id: impl Into<ElementId>,
    delay_ms: u64,
    ms: u64,
    dy: f32,
) -> AnimationElement<E> {
    el.with_animations(id, vec![Animation::new(Duration::from_millis(delay_ms.max(1))), ease(ms)], move |el, ix, t| {
        if ix == 0 { el.opacity(0.).top(px(dy)) } else { el.opacity(t).top(px(dy * (1. - t))) }
    })
}

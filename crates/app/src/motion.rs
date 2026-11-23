use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub use gpui_ce_motion::Easing;
use gpui_ce_motion::{MotionValue, Spring, Transition};

use crate::native;

const UNIT: Duration = Duration::from_secs(1);

/// A clock for gpui-motion values, counted from the first call.
pub fn now() -> Duration {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

/// Whether Windows' "Animation effects" setting is off.
pub fn reduced() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(native::animations_disabled)
}

/// A gpui-motion easing as a curve over 0..1, for gpui's animation elements.
/// With reduced motion every curve jumps straight to its end.
pub fn curve(easing: Easing) -> impl Fn(f32) -> f32 + Clone + 'static {
    let reduced = reduced();
    move |progress| {
        let mut value = MotionValue::new(0.0);
        value.animate(1.0, Transition::Tween(UNIT, easing), Duration::ZERO);
        value
            .sample(UNIT.mul_f32(progress.clamp(0.0, 1.0)), reduced)
            .value() as f32
    }
}

/// A spring without bounce that settles in about `duration`.
pub fn spring(duration: Duration) -> Transition {
    Spring::visual(duration, 0.0).map_or_else(|_| Transition::default(), Transition::Spring)
}

/// A value that springs between 0 (hidden) and 1 (shown) and keeps its
/// velocity when it is sent back mid-way.
#[derive(Debug, Clone)]
pub struct Toggle {
    shown: bool,
    value: MotionValue,
    duration: Duration,
}

impl Toggle {
    pub fn new(duration: Duration) -> Self {
        Self {
            shown: false,
            value: MotionValue::new(0.0),
            duration,
        }
    }

    pub fn set(&mut self, shown: bool) {
        self.shown = shown;
        let target = if shown { 1.0 } else { 0.0 };
        self.value.animate(target, spring(self.duration), now());
    }

    pub fn jump(&mut self, shown: bool) {
        self.shown = shown;
        self.value.jump(if shown { 1.0 } else { 0.0 }, now());
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The last sampled position, 0 to 1.
    pub fn fraction(&self) -> f32 {
        (self.value.get() as f32).clamp(0.0, 1.0)
    }

    /// Moves the value to the current time; true while it is still moving.
    pub fn advance(&mut self) -> bool {
        self.value.sample(now(), reduced()).is_animating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_and_end_at_one() {
        // With Windows animations off (as on CI runners) every motion jumps to its end.
        if reduced() {
            return;
        }
        let ease = curve(Easing::EaseOut);
        assert!(ease(0.0).abs() < 1e-4);
        assert!((ease(1.0) - 1.0).abs() < 1e-4);
        assert!(ease(0.5) > 0.5);
        assert!((curve(Easing::Linear)(0.25) - 0.25).abs() < 1e-3);
    }

    #[test]
    fn toggle_settles_on_its_target() {
        let mut toggle = Toggle::new(Duration::from_millis(1));
        toggle.set(true);
        std::thread::sleep(Duration::from_millis(50));
        while toggle.advance() {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(toggle.shown());
        assert!((toggle.fraction() - 1.0).abs() < 1e-3);
    }
}

//! Timing and easing for overlay animations.

use crate::template::Timing;

/// Easing function type.
#[derive(Debug, Clone, Copy, Default)]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// Compute the interpolation factor for a given time within a fade.
pub fn ease(t: f64, easing: Easing) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => t,
        Easing::EaseIn => t * t,
        Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        Easing::EaseInOut => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
    }
}

/// Compute the opacity (0.0–1.0) at a given `time` based on `timing`.
///
/// The timeline is:
/// - `[0, fade_in)` — fading in (0.0 → 1.0)
/// - `[fade_in, fade_in + hold)` — fully visible (1.0)
/// - `[fade_in + hold, fade_in + hold + fade_out)` — fading out (1.0 → 0.0)
/// - After total duration — 0.0
/// - Before 0.0 — 0.0
pub fn compute_opacity(time: f64, timing: &Timing) -> f64 {
    if time < 0.0 {
        return 0.0;
    }

    let fade_in_end = timing.fade_in;
    let hold_end = fade_in_end + timing.hold;
    let total = hold_end + timing.fade_out;

    if time >= total {
        return 0.0;
    }

    if time < fade_in_end {
        if timing.fade_in == 0.0 {
            return 1.0;
        }
        return time / timing.fade_in;
    }

    if time < hold_end {
        return 1.0;
    }

    // fade_out phase
    if timing.fade_out == 0.0 {
        return 0.0;
    }
    let fade_progress = (time - hold_end) / timing.fade_out;
    1.0 - fade_progress
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ease tests ---

    #[test]
    fn test_ease_linear_boundaries() {
        assert!((ease(0.0, Easing::Linear) - 0.0).abs() < f64::EPSILON);
        assert!((ease(1.0, Easing::Linear) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_linear_midpoint() {
        assert!((ease(0.5, Easing::Linear) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_boundaries() {
        assert!((ease(0.0, Easing::EaseIn) - 0.0).abs() < f64::EPSILON);
        assert!((ease(1.0, Easing::EaseIn) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_midpoint() {
        assert!((ease(0.5, Easing::EaseIn) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_out_boundaries() {
        assert!((ease(0.0, Easing::EaseOut) - 0.0).abs() < f64::EPSILON);
        assert!((ease(1.0, Easing::EaseOut) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_out_midpoint() {
        assert!((ease(0.5, Easing::EaseOut) - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_out_boundaries() {
        assert!((ease(0.0, Easing::EaseInOut) - 0.0).abs() < f64::EPSILON);
        assert!((ease(1.0, Easing::EaseInOut) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_out_midpoint() {
        assert!((ease(0.5, Easing::EaseInOut) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_clamps_below_zero() {
        assert!((ease(-0.5, Easing::Linear) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_clamps_above_one() {
        assert!((ease(1.5, Easing::Linear) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_out_first_half() {
        // t=0.25: 2 * 0.25^2 = 0.125
        assert!((ease(0.25, Easing::EaseInOut) - 0.125).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_in_out_second_half() {
        // t=0.75: 1 - (-2*0.75 + 2)^2 / 2 = 1 - (0.5)^2 / 2 = 1 - 0.125 = 0.875
        assert!((ease(0.75, Easing::EaseInOut) - 0.875).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ease_default() {
        let e = Easing::default();
        assert!(matches!(e, Easing::Linear));
    }

    // --- compute_opacity tests ---

    #[test]
    fn test_opacity_before_start() {
        let timing = Timing {
            fade_in: 0.3,
            hold: 10.0,
            fade_out: 0.5,
        };
        assert!((compute_opacity(-1.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_at_start() {
        let timing = Timing {
            fade_in: 0.3,
            hold: 10.0,
            fade_out: 0.5,
        };
        assert!((compute_opacity(0.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_during_fade_in() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 10.0,
            fade_out: 1.0,
        };
        assert!((compute_opacity(0.5, &timing) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_at_fade_in_end() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 10.0,
            fade_out: 1.0,
        };
        assert!((compute_opacity(1.0, &timing) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_during_hold() {
        let timing = Timing {
            fade_in: 0.3,
            hold: 10.0,
            fade_out: 0.5,
        };
        assert!((compute_opacity(5.0, &timing) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_during_fade_out() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 1.0,
            fade_out: 1.0,
        };
        // time=2.5 => in fade_out phase. progress = (2.5 - 2.0) / 1.0 = 0.5
        assert!((compute_opacity(2.5, &timing) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_at_end() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 1.0,
            fade_out: 1.0,
        };
        assert!((compute_opacity(3.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_after_end() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 1.0,
            fade_out: 1.0,
        };
        assert!((compute_opacity(100.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_zero_fade_in() {
        let timing = Timing {
            fade_in: 0.0,
            hold: 1.0,
            fade_out: 1.0,
        };
        assert!((compute_opacity(0.0, &timing) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_zero_fade_out() {
        let timing = Timing {
            fade_in: 1.0,
            hold: 1.0,
            fade_out: 0.0,
        };
        // At time=2.0 (end of hold), fade_out=0 => total=2.0, so time >= total => 0.0
        assert!((compute_opacity(2.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_all_zero() {
        let timing = Timing {
            fade_in: 0.0,
            hold: 0.0,
            fade_out: 0.0,
        };
        assert!((compute_opacity(0.0, &timing) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_opacity_quarter_fade_in() {
        let timing = Timing {
            fade_in: 2.0,
            hold: 5.0,
            fade_out: 2.0,
        };
        assert!((compute_opacity(0.5, &timing) - 0.25).abs() < f64::EPSILON);
    }
}

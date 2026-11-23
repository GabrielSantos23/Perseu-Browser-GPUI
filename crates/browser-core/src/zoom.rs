//! Page zoom steps, matching Chromium's menu.

/// Zoom factors offered by zoom in and out, smallest first.
pub const LEVELS: [f64; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

const SAME: f64 = 0.001;

/// The next level above `factor`, or the largest level.
pub fn zoom_in(factor: f64) -> f64 {
    LEVELS
        .into_iter()
        .find(|level| *level > factor + SAME)
        .unwrap_or(LEVELS[LEVELS.len() - 1])
}

/// The next level below `factor`, or the smallest level.
pub fn zoom_out(factor: f64) -> f64 {
    LEVELS
        .into_iter()
        .rev()
        .find(|level| *level < factor - SAME)
        .unwrap_or(LEVELS[0])
}

/// The factor as a whole percentage, such as `90` for `0.9`.
pub fn percent(factor: f64) -> u32 {
    (factor * 100.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_levels_and_stop_at_the_ends() {
        assert_eq!(zoom_in(1.0), 1.1);
        assert_eq!(zoom_out(1.0), 0.9);
        assert_eq!(zoom_in(5.0), 5.0);
        assert_eq!(zoom_out(0.25), 0.25);
    }

    #[test]
    fn off_level_factors_snap_to_the_next_level() {
        assert_eq!(zoom_in(1.05), 1.1);
        assert_eq!(zoom_out(1.05), 1.0);
        assert_eq!(percent(0.67), 67);
        assert_eq!(percent(1.1), 110);
    }
}

//! What a screen card reads from the meters.

use super::*;

#[test]
fn silence_is_level_zero_and_full_scale_is_one() {
    assert_eq!(level_of([f32::NEG_INFINITY, f32::NEG_INFINITY]), 0.0);
    assert_eq!(level_of([-120.0, -90.0]), 0.0);
    assert!((level_of([0.0, -20.0]) - 1.0).abs() < 1e-6);
    assert!((level_of([6.0, 6.0]) - 1.0).abs() < 1e-6);
}

#[test]
fn the_louder_side_sets_the_level_over_the_floor() {
    let half = level_of([-30.0, -60.0]);
    assert!((half - 0.5).abs() < 1e-6, "{half}");
    assert_eq!(level_of([-60.0, -30.0]), half);
}

#[test]
fn a_still_screen_shows_no_level() {
    assert_eq!(seen(0.8, true), 0.0);
    assert_eq!(seen(0.8, false), 0.8);
}

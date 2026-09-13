use super::*;

#[test]
fn core_colors_match_the_wcode_brand_palette() {
    assert_eq!(BACKGROUND, Color::Rgb(11, 8, 18));
    assert_eq!(SURFACE, Color::Rgb(21, 16, 32));
    assert_eq!(SURFACE_RAISED, Color::Rgb(18, 13, 28));
    assert_eq!(ACCENT, Color::Rgb(139, 124, 255));
    assert_eq!(LINK, Color::Rgb(102, 92, 255));
    assert_eq!(SECONDARY, Color::Rgb(240, 90, 166));
    assert_eq!(SUCCESS, Color::Rgb(120, 174, 148));
    assert_eq!(WARNING, Color::Rgb(194, 154, 98));
    assert_eq!(DANGER, Color::Rgb(194, 119, 136));
}

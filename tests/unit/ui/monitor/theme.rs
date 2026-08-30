use super::*;

#[test]
fn core_colors_match_the_documentation_theme() {
    assert_eq!(BACKGROUND, Color::Rgb(11, 8, 18));
    assert_eq!(ACCENT, Color::Rgb(139, 124, 255));
    assert_eq!(SECONDARY, Color::Rgb(240, 90, 166));
    assert_eq!(SUCCESS, Color::Rgb(102, 227, 173));
}

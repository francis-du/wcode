use ratatui::style::Color;

// Mirrors docs/assets/site.css so the terminal and website read as one product.
pub(super) const BACKGROUND: Color = Color::Rgb(11, 8, 18); // #0b0812
pub(super) const SURFACE: Color = Color::Rgb(21, 16, 32); // #151020
pub(super) const SURFACE_RAISED: Color = Color::Rgb(18, 13, 28); // #120d1c
pub(super) const SURFACE_SELECTED: Color = Color::Rgb(28, 21, 41); // #1c1529
pub(super) const TEXT: Color = Color::Rgb(250, 247, 255); // #faf7ff
pub(super) const TEXT_MUTED: Color = Color::Rgb(168, 157, 183); // #a89db7
pub(super) const TEXT_DIM: Color = Color::Rgb(116, 105, 130); // #746982
pub(super) const OUTLINE: Color = Color::Rgb(61, 52, 73);
pub(super) const ACCENT: Color = Color::Rgb(139, 124, 255); // #8b7cff
pub(super) const LINK: Color = Color::Rgb(102, 92, 255); // #665cff
pub(super) const SECONDARY: Color = Color::Rgb(240, 90, 166); // #f05aa6
pub(super) const SUCCESS: Color = Color::Rgb(102, 227, 173); // #66e3ad
pub(super) const WARNING: Color = Color::Rgb(255, 189, 92); // #ffbd5c
pub(super) const DANGER: Color = Color::Rgb(255, 116, 138); // #ff748a

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/theme.rs"]
mod tests;

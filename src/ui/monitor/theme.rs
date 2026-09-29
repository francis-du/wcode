use ratatui::style::Color;

// Mirrors the canonical wcode Logo/homepage palette. Semantic state colors stay
// subdued so #8b7cff / #665cff / #f05aa6 remain the primary visual hierarchy.
pub(super) const BACKGROUND: Color = Color::Rgb(11, 8, 18); // #0b0812
pub(super) const SURFACE: Color = Color::Rgb(21, 16, 32); // #151020
pub(super) const SURFACE_RAISED: Color = Color::Rgb(18, 13, 28); // #120d1c
pub(super) const SURFACE_SELECTED: Color = Color::Rgb(38, 27, 59); // #261b3b
pub(super) const TEXT: Color = Color::Rgb(250, 247, 255); // #faf7ff
pub(super) const TEXT_MUTED: Color = Color::Rgb(184, 172, 199); // #b8acc7
pub(super) const TEXT_DIM: Color = Color::Rgb(145, 133, 166); // #9185a6
pub(super) const OUTLINE: Color = Color::Rgb(75, 59, 98); // #4b3b62
pub(super) const ACCENT: Color = Color::Rgb(139, 124, 255); // #8b7cff
pub(super) const LINK: Color = Color::Rgb(102, 92, 255); // #665cff
pub(super) const SECONDARY: Color = Color::Rgb(240, 90, 166); // #f05aa6
pub(super) const SUCCESS: Color = Color::Rgb(120, 174, 148); // #78ae94
pub(super) const WARNING: Color = Color::Rgb(194, 154, 98); // #c29a62
pub(super) const DANGER: Color = Color::Rgb(194, 119, 136); // #c27788

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/theme.rs"]
mod tests;

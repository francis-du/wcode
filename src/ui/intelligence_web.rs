pub(crate) const INTELLIGENCE_APP_PAGE: &str = include_str!("intelligence_web/page.html");
pub(crate) const INTELLIGENCE_LOGO_SVG: &str = include_str!("../../docs/assets/wcode-logo.svg");
pub(crate) const INTELLIGENCE_CSS: &str = concat!(
    include_str!("intelligence_web/styles/theme.css"),
    include_str!("intelligence_web/styles/shell.css"),
    include_str!("intelligence_web/styles/features.css"),
    include_str!("intelligence_web/styles/data.css"),
    include_str!("intelligence_web/styles/architecture.css"),
    include_str!("intelligence_web/styles/code_graph.css"),
    include_str!("intelligence_web/styles/engineering.css"),
    include_str!("intelligence_web/styles/structure.css"),
    include_str!("intelligence_web/styles/observability.css"),
    // Keep responsive overrides last so component-local desktop rules cannot
    // accidentally win on narrow touch screens.
    include_str!("intelligence_web/styles/responsive.css"),
);
pub(crate) const INTELLIGENCE_JS: &str = concat!(
    include_str!("intelligence_web/app/i18n.js"),
    include_str!("intelligence_web/app/core.js"),
    include_str!("intelligence_web/app/access.js"),
    include_str!("intelligence_web/app/overview.js"),
    include_str!("intelligence_web/app/architecture.js"),
    include_str!("intelligence_web/app/code_graph_explorer.js"),
    include_str!("intelligence_web/app/code_graph.js"),
    include_str!("intelligence_web/app/engineering.js"),
    include_str!("intelligence_web/app/features.js"),
    include_str!("intelligence_web/app/quality.js"),
    include_str!("intelligence_web/app/structure.js"),
    include_str!("intelligence_web/app/runtime.js"),
);

#[cfg(test)]
#[path = "../../tests/unit/ui/web.rs"]
mod tests;

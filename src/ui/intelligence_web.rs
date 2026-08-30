pub(crate) const INTELLIGENCE_APP_PAGE: &str = include_str!("intelligence_web/page.html");
pub(crate) const INTELLIGENCE_CSS: &str = concat!(
    include_str!("intelligence_web/styles/theme.css"),
    include_str!("intelligence_web/styles/shell.css"),
    include_str!("intelligence_web/styles/features.css"),
    include_str!("intelligence_web/styles/data.css"),
    include_str!("intelligence_web/styles/responsive.css"),
    include_str!("intelligence_web/styles/architecture.css"),
    include_str!("intelligence_web/styles/structure.css"),
);
pub(crate) const INTELLIGENCE_JS: &str = concat!(
    include_str!("intelligence_web/app/core.js"),
    include_str!("intelligence_web/app/access.js"),
    include_str!("intelligence_web/app/overview.js"),
    include_str!("intelligence_web/app/architecture.js"),
    include_str!("intelligence_web/app/features.js"),
    include_str!("intelligence_web/app/quality.js"),
    include_str!("intelligence_web/app/structure.js"),
    include_str!("intelligence_web/app/runtime.js"),
);

#[cfg(test)]
#[path = "../../tests/unit/ui/web.rs"]
mod tests;

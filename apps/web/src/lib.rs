//! Browser-compatible ARUS Dioxus web components, 3D Studio shell, and bilingual SSR renderer.
//!
//! Excludes native persistence, GIS, workers, and numerical engines.

pub mod studio;
pub mod styles;
pub mod view;

pub use studio::{
    FONT_INTER_400, FONT_INTER_500, FONT_INTER_600, FONT_JBMONO_400, FONT_JBMONO_500, MAPLIBRE_CSS,
    MAPLIBRE_JS, STUDIO_CSS, STUDIO_JS, render_studio_page_html,
};
pub use view::{
    Locale, OutlookCard, RunIndexPage, RunPage, SourcesPage, render_index_page_html,
    render_outlook_html, render_run_page_html, render_sources_page_html,
};

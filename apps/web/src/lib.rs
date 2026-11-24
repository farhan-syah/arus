//! Browser-compatible ARUS Dioxus web components and bilingual (BM / EN) SSR renderer.
//!
//! Excludes native persistence, GIS, workers, and numerical engines.

pub mod view;

pub use view::{
    Locale, OutlookCard, RunIndexPage, RunPage, render_index_page_html, render_outlook_html,
    render_run_page_html,
};

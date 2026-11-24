//! Browser-compatible ARUS web view-model and bilingual (BM / EN) outlook renderer.
//!
//! Excludes native persistence, GIS, workers, and numerical engines.

pub mod view;

pub use view::{Locale, render_outlook_html};

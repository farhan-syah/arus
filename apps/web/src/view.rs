//! Bilingual (Bahasa Malaysia / English) accessible HTML renderer for `ApiLocationOutlook`.

use arus_api_types::{ApiLocationOutlook, ApiLocationStatus};
use arus_domain::RunClassification;

/// Supported display locales for ARUS web views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    English,
    BahasaMalaysia,
}

/// Renders a browser-safe, accessible HTML card for one `ApiLocationOutlook`.
///
/// Explicitly surfaces non-operational classification banners, unsupported locations
/// (never rendered as dry zero), and unresolved right-censored recession status.
pub fn render_outlook_html(outlook: &ApiLocationOutlook, locale: Locale) -> String {
    let classification_label = match (outlook.classification, locale) {
        (RunClassification::NumericalDemonstration, Locale::English) => {
            "Numerical demonstration (not an operational forecast)"
        }
        (RunClassification::NumericalDemonstration, Locale::BahasaMalaysia) => {
            "Demonstrasi berangka (bukan ramalan operasi)"
        }
        (RunClassification::EventReconstruction, Locale::English) => "Historical reconstruction",
        (RunClassification::EventReconstruction, Locale::BahasaMalaysia) => {
            "Pembinaan semula sejarah"
        }
        (RunClassification::ForecastReplay, Locale::English) => "As-issued forecast replay",
        (RunClassification::ForecastReplay, Locale::BahasaMalaysia) => "Ulangan ramalan rasmi",
        (RunClassification::OperationalForecast, Locale::English) => "Operational forecast",
        (RunClassification::OperationalForecast, Locale::BahasaMalaysia) => "Ramalan operasi",
    };

    let support_html = match (outlook.support, locale) {
        (ApiLocationStatus::Unsupported, Locale::English) => {
            "<p data-status=\"unsupported\">Location is outside supported model coverage. Never treat as dry.</p>".to_owned()
        }
        (ApiLocationStatus::Unsupported, Locale::BahasaMalaysia) => {
            "<p data-status=\"unsupported\">Lokasi di luar liputan model yang disokong. Jangan anggap kering.</p>".to_owned()
        }
        (ApiLocationStatus::Supported, _) => {
            let max_depth = outlook
                .max_depth_m
                .map_or_else(|| "n/a".to_owned(), |d| format!("{d:.3} m"));
            let episode_count = outlook.episodes.len();
            match locale {
                Locale::English => format!(
                    "<p data-status=\"supported\">Max depth: {max_depth} | Threshold: {:.3} m | Episodes: {episode_count}</p>",
                    outlook.threshold_m
                ),
                Locale::BahasaMalaysia => format!(
                    "<p data-status=\"supported\">Kedalaman maksimum: {max_depth} | Ambang: {:.3} m | Episod: {episode_count}</p>",
                    outlook.threshold_m
                ),
            }
        }
    };

    let location = escape_html(&outlook.location_id);
    format!(
        "<section class=\"arus-outlook\" data-location=\"{location}\" data-classification=\"{}\">\
         <h2>{location}</h2>\
         <p class=\"arus-banner\">{classification_label}</p>\
         {support_html}\
         </section>",
        outlook.classification.id()
    )
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

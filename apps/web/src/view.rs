//! Bilingual (Bahasa Malaysia / English) Dioxus components and SSR helpers for ARUS web views.

use arus_api_types::{ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus};
use arus_domain::RunClassification;
use dioxus::prelude::*;

/// Supported display locales for ARUS web views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    English,
    BahasaMalaysia,
}

impl Locale {
    /// Returns the URL query code (`en` or `bm`) for this locale.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::BahasaMalaysia => "bm",
        }
    }
}

fn classification_label(classification: RunClassification, locale: Locale) -> &'static str {
    match (classification, locale) {
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
    }
}

fn episode_line(episode: &ApiEpisodeSummary, locale: Locale) -> String {
    match locale {
        Locale::English => format!(
            "Onset: {} ({:.0} s) | Peak: {:.3} m at {:.0} s | Recession: {}",
            episode.onset_status,
            episode.onset_time_s,
            episode.peak_depth_m,
            episode.peak_time_s,
            episode.recession_status,
        ),
        Locale::BahasaMalaysia => format!(
            "Mula: {} ({:.0} s) | Puncak: {:.3} m pada {:.0} s | Surut: {}",
            episode.onset_status,
            episode.onset_time_s,
            episode.peak_depth_m,
            episode.peak_time_s,
            episode.recession_status,
        ),
    }
}

/// Dioxus component rendering one `ApiLocationOutlook` with explicit support and classification banners.
#[component]
pub fn OutlookCard(outlook: ApiLocationOutlook, locale: Locale) -> Element {
    let banner = classification_label(outlook.classification, locale);
    let class_id = outlook.classification.id();

    rsx! {
        section {
            class: "arus-outlook",
            "data-location": "{outlook.location_id}",
            "data-classification": "{class_id}",
            h2 { "{outlook.location_id}" }
            p { class: "arus-banner", "{banner}" }
            match outlook.support {
                ApiLocationStatus::Unsupported => {
                    let msg = match locale {
                        Locale::English => "Location is outside supported model coverage. Never treat as dry.",
                        Locale::BahasaMalaysia => "Lokasi di luar liputan model yang disokong. Jangan anggap kering.",
                    };
                    rsx! {
                        p { "data-status": "unsupported", "{msg}" }
                    }
                }
                ApiLocationStatus::Supported => {
                    let max_depth = outlook
                        .max_depth_m
                        .map_or_else(|| "n/a".to_owned(), |d| format!("{d:.3} m"));
                    let episode_count = outlook.episodes.len();
                    let summary = match locale {
                        Locale::English => format!(
                            "Max depth: {max_depth} | Threshold: {:.3} m | Episodes: {episode_count}",
                            outlook.threshold_m
                        ),
                        Locale::BahasaMalaysia => format!(
                            "Kedalaman maksimum: {max_depth} | Ambang: {:.3} m | Episod: {episode_count}",
                            outlook.threshold_m
                        ),
                    };
                    rsx! {
                        p { "data-status": "supported", "{summary}" }
                        if !outlook.episodes.is_empty() {
                            ul { class: "arus-episodes",
                                for ep in &outlook.episodes {
                                    li { "{episode_line(ep, locale)}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Dioxus page component rendering all location outlooks for a single archived run.
#[component]
pub fn RunPage(run_id: String, outlooks: Vec<ApiLocationOutlook>, locale: Locale) -> Element {
    let back_label = match locale {
        Locale::English => "All runs",
        Locale::BahasaMalaysia => "Semua larian",
    };
    let lang = locale.code();

    rsx! {
        main { class: "arus-run-page",
            header { class: "arus-header",
                h1 { "ARUS — {run_id}" }
                nav {
                    a { href: "/?lang={lang}", "{back_label}" }
                    " | "
                    a { href: "/runs/{run_id}?lang=en", "English" }
                    " | "
                    a { href: "/runs/{run_id}?lang=bm", "Bahasa Malaysia" }
                }
            }
            for outlook in outlooks {
                OutlookCard { outlook, locale }
            }
        }
    }
}

/// Dioxus home page component listing archived runs and the currently published forecast pointer.
#[component]
pub fn RunIndexPage(
    run_ids: Vec<String>,
    latest_run_id: Option<String>,
    published_run_id: Option<String>,
    locale: Locale,
) -> Element {
    let heading = match locale {
        Locale::English => "ARUS Flood Forecasting & Scenario Archive",
        Locale::BahasaMalaysia => "Arkib Ramalan & Senario Banjir ARUS",
    };
    let pub_text = match (published_run_id.as_deref(), locale) {
        (Some(id), Locale::English) => format!("Published operational forecast: {id}"),
        (Some(id), Locale::BahasaMalaysia) => format!("Ramalan operasi diterbitkan: {id}"),
        (None, Locale::English) => "No operational forecast is currently published.".to_owned(),
        (None, Locale::BahasaMalaysia) => {
            "Tiada ramalan operasi diterbitkan buat masa ini.".to_owned()
        }
    };
    let latest_text = match (latest_run_id.as_deref(), locale) {
        (Some(id), Locale::English) => format!("Latest archived run: {id}"),
        (Some(id), Locale::BahasaMalaysia) => format!("Larian arkib terkini: {id}"),
        (None, Locale::English) => "No archived runs yet.".to_owned(),
        (None, Locale::BahasaMalaysia) => "Tiada larian diarkibkan lagi.".to_owned(),
    };
    let lang = locale.code();

    rsx! {
        main { class: "arus-index-page",
            header { class: "arus-header",
                h1 { "{heading}" }
                nav {
                    a { href: "/?lang=en", "English" }
                    " | "
                    a { href: "/?lang=bm", "Bahasa Malaysia" }
                }
            }
            p { class: "arus-published-status", "{pub_text}" }
            p { class: "arus-latest-status", "{latest_text}" }
            if !run_ids.is_empty() {
                ul { class: "arus-run-list",
                    for id in run_ids {
                        li {
                            a { href: "/runs/{id}?lang={lang}", "{id}" }
                        }
                    }
                }
            }
        }
    }
}

/// Renders a single `ApiLocationOutlook` card to HTML via Dioxus SSR.
///
/// Explicitly surfaces non-operational classification banners, unsupported locations
/// (never rendered as dry zero), and unresolved right-censored recession status.
#[must_use]
pub fn render_outlook_html(outlook: &ApiLocationOutlook, locale: Locale) -> String {
    let owned = outlook.clone();
    dioxus_ssr::render_element(rsx! {
        OutlookCard { outlook: owned, locale }
    })
}

/// Renders a full run detail HTML document via Dioxus SSR.
#[must_use]
pub fn render_run_page_html(
    run_id: &str,
    outlooks: &[ApiLocationOutlook],
    locale: Locale,
) -> String {
    let run_id_owned = run_id.to_owned();
    let outlooks_owned = outlooks.to_vec();
    let body = dioxus_ssr::render_element(rsx! {
        RunPage {
            run_id: run_id_owned,
            outlooks: outlooks_owned,
            locale,
        }
    });
    let safe_title = run_id
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>ARUS {safe_title}</title></head><body>{body}</body></html>"
    )
}

/// Renders the ARUS portal index HTML document via Dioxus SSR.
#[must_use]
pub fn render_index_page_html(
    run_ids: &[String],
    latest_run_id: Option<&str>,
    published_run_id: Option<&str>,
    locale: Locale,
) -> String {
    let run_ids_owned = run_ids.to_vec();
    let latest_owned = latest_run_id.map(ToOwned::to_owned);
    let published_owned = published_run_id.map(ToOwned::to_owned);
    let body = dioxus_ssr::render_element(rsx! {
        RunIndexPage {
            run_ids: run_ids_owned,
            latest_run_id: latest_owned,
            published_run_id: published_owned,
            locale,
        }
    });
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>ARUS</title></head><body>{body}</body></html>"
    )
}

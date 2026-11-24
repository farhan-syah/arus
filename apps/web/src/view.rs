//! Bilingual (Bahasa Malaysia / English) Dioxus components and SSR helpers for ARUS web views.

use arus_api_types::{ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus, ApiSourceSummary};
use arus_domain::RunClassification;
use dioxus::prelude::*;

use crate::styles::ARUS_STYLESHEET;

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
            "Onset: {} ({:.0} s) | Peak: {:.3} m at {:.0} s | Duration: {:.0} s | Recession: {}",
            episode.onset_status,
            episode.onset_time_s,
            episode.peak_depth_m,
            episode.peak_time_s,
            episode.exceedance_duration_s,
            episode.recession_status,
        ),
        Locale::BahasaMalaysia => format!(
            "Mula: {} ({:.0} s) | Puncak: {:.3} m pada {:.0} s | Tempoh: {:.0} s | Surut: {}",
            episode.onset_status,
            episode.onset_time_s,
            episode.peak_depth_m,
            episode.peak_time_s,
            episode.exceedance_duration_s,
            episode.recession_status,
        ),
    }
}

#[component]
fn TopBar(locale: Locale, current_path: String) -> Element {
    let lang = locale.code();
    let runs_label = match locale {
        Locale::English => "Runs",
        Locale::BahasaMalaysia => "Larian",
    };
    let sources_label = match locale {
        Locale::English => "Data Sources",
        Locale::BahasaMalaysia => "Sumber Data",
    };
    let en_class = if locale == Locale::English {
        "arus-pill arus-pill-active"
    } else {
        "arus-pill"
    };
    let bm_class = if locale == Locale::BahasaMalaysia {
        "arus-pill arus-pill-active"
    } else {
        "arus-pill"
    };

    rsx! {
        header { class: "arus-topbar",
            a { class: "arus-brand", href: "/?lang={lang}",
                span { class: "arus-brand-mark", "A" }
                span { "ARUS" }
            }
            nav { class: "arus-nav",
                a { class: "arus-pill", href: "/?lang={lang}", "{runs_label}" }
                a { class: "arus-pill", href: "/sources?lang={lang}", "{sources_label}" }
                a { class: "arus-pill", href: "/api/v1/runs", "API JSON" }
                a { class: "{en_class}", href: "{current_path}?lang=en", "EN" }
                a { class: "{bm_class}", href: "{current_path}?lang=bm", "BM" }
            }
        }
    }
}

/// Dioxus component rendering one `ApiLocationOutlook` with explicit support and classification banners.
#[component]
pub fn OutlookCard(outlook: ApiLocationOutlook, locale: Locale) -> Element {
    let banner = classification_label(outlook.classification, locale);
    let class_id = outlook.classification.id();
    let banner_class = if outlook.classification == RunClassification::OperationalForecast {
        "arus-banner arus-banner-operational"
    } else {
        "arus-banner"
    };

    rsx! {
        section {
            class: "arus-outlook",
            "data-location": "{outlook.location_id}",
            "data-classification": "{class_id}",
            h2 { "{outlook.location_id}" }
            p { class: "{banner_class}", "{banner}" }
            match outlook.support {
                ApiLocationStatus::Unsupported => {
                    let msg = match locale {
                        Locale::English => "Location is outside supported model coverage. Never treat as dry.",
                        Locale::BahasaMalaysia => "Lokasi di luar liputan model yang disokong. Jangan anggap kering.",
                    };
                    rsx! {
                        p { class: "arus-unsupported", "data-status": "unsupported", "{msg}" }
                    }
                }
                ApiLocationStatus::Supported => {
                    let depth_val = outlook.max_depth_m.unwrap_or(0.0);
                    let max_depth = outlook
                        .max_depth_m
                        .map_or_else(|| "n/a".to_owned(), |d| format!("{d:.3} m"));
                    let episode_count = outlook.episodes.len();
                    let denom = (outlook.threshold_m * 1.5).max(depth_val).max(1e-6);
                    let pct = ((depth_val / denom) * 100.0).clamp(0.0, 100.0);
                    let meter_class = if depth_val >= outlook.threshold_m {
                        "arus-meter-fill arus-meter-exceeded"
                    } else {
                        "arus-meter-fill"
                    };
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
                        div { class: "arus-meter-track",
                            div { class: "{meter_class}", style: "width: {pct:.1}%;" }
                        }
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
    let subtitle = match locale {
        Locale::English => {
            "Location-specific hydraulic depth, threshold crossing, and recession outlook."
        }
        Locale::BahasaMalaysia => {
            "Ramalan kedalaman hidraulik, pelampauan ambang, dan surutan mengikut lokasi."
        }
    };
    let json_label = match locale {
        Locale::English => "Raw Outlook JSON",
        Locale::BahasaMalaysia => "JSON Ramalan Mentah",
    };

    rsx! {
        div { class: "arus-shell",
            TopBar { locale, current_path: format!("/runs/{run_id}") }
            section { class: "arus-hero",
                h1 { "ARUS — {run_id}" }
                p { class: "arus-subtitle", "{subtitle}" }
                p {
                    a { href: "/api/v1/runs/{run_id}/outlook", "{json_label}" }
                }
            }
            main { class: "arus-grid",
                for outlook in outlooks {
                    OutlookCard { outlook, locale }
                }
            }
            footer { class: "arus-footer",
                span { "ARUS • Physics-first flood forecasting & drainage scenario platform" }
                span { "hydr + nodedb-lite + axum + dioxus" }
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
    let subtitle = match locale {
        Locale::English => {
            "Open-source physics-first flood depth, onset, recession, and drainage intervention analysis."
        }
        Locale::BahasaMalaysia => {
            "Platform sumber terbuka untuk analisis kedalaman banjir, masa mula, surutan, dan intervensi saliran."
        }
    };
    let pub_label = match locale {
        Locale::English => "Published Operational Forecast",
        Locale::BahasaMalaysia => "Ramalan Operasi Diterbitkan",
    };
    let latest_label = match locale {
        Locale::English => "Latest Archived Run",
        Locale::BahasaMalaysia => "Larian Arkib Terkini",
    };
    let count_label = match locale {
        Locale::English => "Archived Runs",
        Locale::BahasaMalaysia => "Jumlah Larian",
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
        (Some(id), _) => id.to_owned(),
        (None, Locale::English) => "None".to_owned(),
        (None, Locale::BahasaMalaysia) => "Tiada".to_owned(),
    };
    let lang = locale.code();
    let run_count = run_ids.len();

    rsx! {
        div { class: "arus-shell",
            TopBar { locale, current_path: "/".to_owned() }
            section { class: "arus-hero",
                h1 { "{heading}" }
                p { class: "arus-subtitle", "{subtitle}" }
            }
            section { class: "arus-kpi-grid",
                div { class: "arus-kpi",
                    div { class: "arus-kpi-label", "{count_label}" }
                    div { class: "arus-kpi-value", "{run_count}" }
                }
                div { class: "arus-kpi",
                    div { class: "arus-kpi-label", "{latest_label}" }
                    div { class: "arus-kpi-value", "{latest_text}" }
                }
                div { class: "arus-kpi",
                    div { class: "arus-kpi-label", "{pub_label}" }
                    div { class: "arus-kpi-value", "{pub_text}" }
                }
            }
            main {
                if !run_ids.is_empty() {
                    ul { class: "arus-run-list",
                        for id in run_ids {
                            li { class: "arus-run-item",
                                a { href: "/runs/{id}?lang={lang}",
                                    span { "{id}" }
                                    span { "→" }
                                }
                            }
                        }
                    }
                }
            }
            footer { class: "arus-footer",
                span { "ARUS • Malaysia & Southeast Asia Flood Intelligence" }
                a { href: "/sources?lang={lang}", "Catalog & Coverage" }
            }
        }
    }
}

/// Dioxus page component listing registered Malaysian and global data sources and their access gates.
#[component]
pub fn SourcesPage(sources: Vec<ApiSourceSummary>, locale: Locale) -> Element {
    let heading = match locale {
        Locale::English => "Public Data Sources & Coverage",
        Locale::BahasaMalaysia => "Sumber Data Awam & Liputan",
    };
    let subtitle = match locale {
        Locale::English => {
            "Every external rainfall, terrain, imagery, and gauge source records explicit access class, licence, and limitations."
        }
        Locale::BahasaMalaysia => {
            "Setiap sumber hujan, rupa bumi, imej satelit, dan tolok merekodkan kelas akses, lesen, dan had penggunaan secara telus."
        }
    };

    rsx! {
        div { class: "arus-shell",
            TopBar { locale, current_path: "/sources".to_owned() }
            section { class: "arus-hero",
                h1 { "{heading}" }
                p { class: "arus-subtitle", "{subtitle}" }
            }
            main { class: "arus-table-wrap",
                table { class: "arus-table",
                    thead {
                        tr {
                            th { "ID" }
                            th { "Source" }
                            th { "Access" }
                            th { "Class" }
                            th { "Licence" }
                            th { "Coverage" }
                        }
                    }
                    tbody {
                        for src in sources {
                            tr {
                                td { code { "{src.id}" } }
                                td {
                                    strong { "{src.title}" }
                                    br {}
                                    span { "{src.provider}" }
                                }
                                td {
                                    span {
                                        class: if src.access == "public" { "arus-badge arus-badge-public" } else { "arus-badge" },
                                        "{src.access}"
                                    }
                                }
                                td { span { class: "arus-badge", "{src.data_class}" } }
                                td { "{src.license_name}" }
                                td { "{src.coverage}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn wrap_document(title: &str, body: &str) -> String {
    let safe_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{safe_title}</title><style>{ARUS_STYLESHEET}</style></head>\
         <body>{body}</body></html>"
    )
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
    wrap_document(&format!("ARUS — {run_id}"), &body)
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
    wrap_document("ARUS Flood Intelligence", &body)
}

/// Renders the ARUS data sources & coverage HTML document via Dioxus SSR.
#[must_use]
pub fn render_sources_page_html(sources: &[ApiSourceSummary], locale: Locale) -> String {
    let owned = sources.to_vec();
    let body = dioxus_ssr::render_element(rsx! {
        SourcesPage {
            sources: owned,
            locale,
        }
    });
    wrap_document("ARUS — Data Sources & Coverage", &body)
}

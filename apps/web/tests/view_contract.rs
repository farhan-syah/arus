use arus_api_types::{
    API_SCHEMA_VERSION, ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus, ApiSourceSummary,
};
use arus_domain::RunClassification;
use arus_web::{
    Locale, render_index_page_html, render_outlook_html, render_run_page_html,
    render_sources_page_html,
};

#[test]
fn bilingual_dioxus_renderer_preserves_unsupported_and_escapes_html() {
    let outlook = ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: "site-<1>".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        support: ApiLocationStatus::Unsupported,
        threshold_m: 0.15,
        horizon_start_s: Some(0.0),
        horizon_end_s: Some(60.0),
        max_depth_m: None,
        episodes: Vec::new(),
    };

    let en = render_outlook_html(&outlook, Locale::English);
    assert!(!en.contains("<1>"));
    assert!(en.contains("site-&lt;1&gt;") || en.contains("site-&#60;1&#62;"));
    assert!(en.contains("data-status=\"unsupported\""));
    assert!(en.contains("not an operational forecast"));

    let bm = render_outlook_html(&outlook, Locale::BahasaMalaysia);
    assert!(bm.contains("Demonstrasi berangka"));
    assert!(bm.contains("Jangan anggap kering"));
}

#[test]
fn dioxus_pages_render_episodes_and_index_navigation() {
    let outlook = ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: "basin-center".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        support: ApiLocationStatus::Supported,
        threshold_m: 0.10,
        horizon_start_s: Some(0.0),
        horizon_end_s: Some(120.0),
        max_depth_m: Some(0.25),
        episodes: vec![ApiEpisodeSummary {
            onset_status: "crossed".to_owned(),
            onset_time_s: 30.0,
            initial_inundation: false,
            peak_depth_m: 0.25,
            peak_time_s: 90.0,
            peak_at_horizon_end: false,
            exceedance_duration_s: 90.0,
            recession_status: "not_receded_within_horizon".to_owned(),
            recession_crossing_s: None,
            recession_confirmation_s: None,
            rebound_count: 0,
        }],
    };

    let page = render_run_page_html("run-42", &[outlook], Locale::English);
    assert!(page.contains("ARUS — run-42"));
    assert!(page.contains("not_receded_within_horizon"));
    assert!(page.contains("/runs/run-42?lang=bm"));

    let index = render_index_page_html(
        &["run-42".to_owned()],
        Some("run-42"),
        None,
        Locale::BahasaMalaysia,
    );
    assert!(index.contains("Tiada ramalan operasi diterbitkan"));
    assert!(index.contains("/runs/run-42?lang=bm"));

    let sources_page = render_sources_page_html(
        &[ApiSourceSummary {
            id: "met-weather-forecast".to_owned(),
            title: "METMalaysia general weather forecasts".to_owned(),
            provider: "data-gov-weather".to_owned(),
            access: "public".to_owned(),
            data_class: "general_guidance".to_owned(),
            license_name: "CC BY 4.0".to_owned(),
            coverage: "Malaysia".to_owned(),
        }],
        Locale::English,
    );
    assert!(sources_page.contains("Public Data Sources"));
    assert!(sources_page.contains("Coverage"));
    assert!(sources_page.contains("met-weather-forecast"));
}

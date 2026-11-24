use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook, ApiLocationStatus};
use arus_domain::RunClassification;
use arus_web::{Locale, render_outlook_html};

#[test]
fn bilingual_renderer_preserves_unsupported_and_escapes_html() {
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
    assert!(en.contains("site-&lt;1&gt;"));
    assert!(en.contains("data-status=\"unsupported\""));
    assert!(en.contains("not an operational forecast"));

    let bm = render_outlook_html(&outlook, Locale::BahasaMalaysia);
    assert!(bm.contains("Demonstrasi berangka"));
    assert!(bm.contains("Jangan anggap kering"));
}

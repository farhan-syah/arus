//! Browser-compatible ARUS web application entrypoint.

use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook, ApiLocationStatus};
use arus_domain::RunClassification;
use arus_web::{Locale, render_outlook_html};

fn main() {
    let sample = ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: "demo-location".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        support: ApiLocationStatus::Unsupported,
        threshold_m: 0.15,
        horizon_start_s: None,
        horizon_end_s: None,
        max_depth_m: None,
        episodes: Vec::new(),
    };
    println!("{}", render_outlook_html(&sample, Locale::BahasaMalaysia));
}

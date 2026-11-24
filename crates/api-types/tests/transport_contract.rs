use arus_api_types::{
    API_SCHEMA_VERSION, ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus,
};
use arus_domain::RunClassification;

#[test]
fn location_outlook_json_round_trip_preserves_classification_and_unsupported() -> Result<(), String>
{
    let outlook = ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: "puncak-alam-probe-1".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        support: ApiLocationStatus::Unsupported,
        threshold_m: 0.15,
        horizon_start_s: Some(0.0),
        horizon_end_s: Some(60.0),
        max_depth_m: None,
        episodes: vec![ApiEpisodeSummary {
            onset_status: "observed".to_owned(),
            onset_time_s: 15.0,
            initial_inundation: false,
            peak_depth_m: 0.30,
            peak_time_s: 45.0,
            peak_at_horizon_end: false,
            exceedance_duration_s: 30.0,
            recession_status: "right-censored".to_owned(),
            recession_crossing_s: None,
            recession_confirmation_s: None,
            rebound_count: 0,
        }],
    };
    let json = serde_json::to_string(&outlook).map_err(|e| e.to_string())?;
    assert!(json.contains("\"numerical-demonstration\""));
    assert!(json.contains("\"unsupported\""));
    let decoded: ApiLocationOutlook = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    assert_eq!(decoded, outlook);
    Ok(())
}

use super::*;
use crate::{parameters::ResponseKind, response};

#[test]
fn failed_immutable_collection_remains_due() -> TestResult {
    let (mut catalog, collections) = config()?;
    catalog.sources[0].refresh_seconds = None;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([
            Response {
                status: 200,
                body: b"{}".to_vec(),
                ..Default::default()
            },
            forecast()?,
        ]),
        ..Default::default()
    };
    let mut clock = FakeClock::new();
    let options = SyncOptions::default();
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options,
            &mut transport,
            &mut clock
        )?
        .failed,
        1
    );
    assert!(!planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    clock.sleep(Duration::from_secs(300));
    assert!(planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options,
            &mut transport,
            &mut clock
        )?
        .succeeded,
        1
    );
    assert!(!planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    Ok(())
}

#[test]
fn deferred_provider_does_not_hold_lock_or_request_early() -> TestResult {
    let (mut catalog, collections) = config()?;
    catalog.sources[0].refresh_seconds = None;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([Response {
            status: 429,
            retry_after: Some("600".into()),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let mut clock = FakeClock::new();
    sync_with(
        &catalog,
        &collections,
        &archive,
        &options(),
        &mut transport,
        &mut clock,
    )?;
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options(),
            &mut transport,
            &mut clock
        )?
        .failed,
        1
    );
    assert_eq!(transport.requests.len(), 1);
    assert!(clock.waited.is_empty());
    assert!(
        planning::plan(&catalog, &collections, &archive, clock.now())?[0]
            .provider_deferred_until
            .is_some()
    );
    let _lock = archive.lock()?;
    Ok(())
}

fn hourly(times: Vec<String>, values: Vec<serde_json::Value>) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(
        &json!({"latitude":3.2,"longitude":101.4,"timezone":"GMT","utc_offset_seconds":0,"hourly_units":{"precipitation":"mm"},"hourly":{"time":times,"precipitation":values}}),
    )?)
}

fn history_kind() -> TestResult<ResponseKind> {
    let start = crate::parameters::date("2026-11-04")?.midnight();
    let end = start + time::Duration::hours(23);
    Ok(ResponseKind::Hourly {
        window: Some((start, end)),
        requested_run: None,
        models: vec!["gfs_global".into()],
        hourly: vec!["precipitation".into()],
    })
}

#[test]
fn unrelated_history_period_is_rejected() -> TestResult {
    let bytes = hourly(vec!["2026-11-05T00:00".into()], vec![json!(1)])?;
    assert!(response::check(&bytes, &history_kind()?).is_err());
    Ok(())
}

#[test]
fn partial_history_is_explicit_and_complete_history_is_checked() -> TestResult {
    let bytes = hourly(
        vec!["2026-11-04T00:00".into(), "2026-11-04T01:00".into()],
        vec![json!(1), json!(null)],
    )?;
    let partial = response::check(&bytes, &history_kind()?)?;
    assert_eq!(partial.coverage_complete, Some(false));
    assert_eq!(partial.missing_values, 1);
    let times = (0..24)
        .map(|hour| format!("2026-11-04T{hour:02}:00"))
        .collect();
    let full = response::check(&hourly(times, vec![json!(0); 24])?, &history_kind()?)?;
    assert_eq!(full.coverage_complete, Some(true));
    Ok(())
}

#[test]
fn single_run_initialization_is_not_claimed_as_checked() -> TestResult {
    let bytes = hourly(vec!["2026-11-04T00:00".into()], vec![json!(1)])?;
    let summary = response::check(
        &bytes,
        &ResponseKind::Hourly {
            window: None,
            requested_run: Some("2026-11-03T18:00".into()),
            models: vec!["gfs_global".into()],
            hourly: vec!["precipitation".into()],
        },
    )?;
    assert_eq!(summary.coverage_complete, None);
    assert_eq!(
        summary.requested_initialization.as_deref(),
        Some("2026-11-03T18:00")
    );
    assert!(
        summary
            .coverage_assessment
            .contains("not_independently_confirmed")
    );
    Ok(())
}

#[test]
fn empty_warning_is_checked_but_empty_forecast_is_not() -> TestResult {
    assert_eq!(
        response::check(b"[]", &ResponseKind::Warning { limit: 100 })?.records,
        0
    );
    assert!(response::check(b"[]", &ResponseKind::Forecast { limit: 100 }).is_err());
    Ok(())
}

#[test]
fn warning_null_validity_is_preserved_without_accepting_missing_fields() -> TestResult {
    let active = json!({"warning_issue":{"issued":"2026-11-08T08:00:00"},"valid_from":"2026-11-08T08:00:00","valid_to":"2026-11-08T12:00:00","heading_en":"Thunderstorm","text_en":"Rain"});
    let advisory = json!({"warning_issue":{"issued":"2026-11-08T10:30:00"},"valid_from":null,"valid_to":null,"heading_en":"No Advisory","text_en":"No cyclone observed"});
    let bytes = serde_json::to_vec(&json!([active, advisory.clone()]))?;
    let summary = response::check(&bytes, &ResponseKind::Warning { limit: 100 })?;
    assert_eq!(summary.records, 2);
    assert_eq!(summary.unknown_validity_records, 1);
    let mut missing = advisory.clone();
    if let Some(object) = missing.as_object_mut() {
        object.remove("valid_from");
    }
    assert!(
        response::check(
            &serde_json::to_vec(&json!([missing]))?,
            &ResponseKind::Warning { limit: 100 }
        )
        .is_err()
    );
    let mut wrong = advisory;
    wrong["valid_to"] = json!(42);
    assert!(
        response::check(
            &serde_json::to_vec(&json!([wrong]))?,
            &ResponseKind::Warning { limit: 100 }
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn forced_sync_still_requires_licence_and_retries_after_restart_obey_rate() -> TestResult {
    let (catalog, collections) = config()?;
    let directory = tempfile::tempdir()?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([forecast()?, forecast()?]),
        ..Default::default()
    };
    let mut clock = FakeClock::new();
    {
        let archive = Archive::open(directory.path())?;
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options(),
            &mut transport,
            &mut clock,
        )?;
    }
    let archive = Archive::open(directory.path())?;
    let start = clock.now();
    sync_with(
        &catalog,
        &collections,
        &archive,
        &options(),
        &mut transport,
        &mut clock,
    )?;
    assert!((clock.now() - start).whole_seconds() >= 15);
    Ok(())
}

#[test]
fn body_error_keeps_server_deferral_and_marks_partial_evidence() -> TestResult {
    for status in [429, 503] {
        let (catalog, collections) = config()?;
        let directory = tempfile::tempdir()?;
        let archive = Archive::open(directory.path())?;
        let response = Response {
            status,
            body: b"partial".to_vec(),
            body_error: Some("interrupted or oversized body".into()),
            retry_after: Some("3600".into()),
            ..Default::default()
        };
        let mut transport = FakeTransport {
            responses: VecDeque::from([response]),
            ..Default::default()
        };
        let mut clock = FakeClock::new();
        assert_eq!(
            sync_with(
                &catalog,
                &collections,
                &archive,
                &options(),
                &mut transport,
                &mut clock
            )?
            .failed,
            1
        );
        assert_eq!(transport.requests.len(), 1);
        assert!(clock.waited.is_empty());
        let entry = std::fs::read_dir(directory.path().join("receipts/selangor"))?
            .next()
            .ok_or("missing receipt")??;
        let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(entry.path())?)?;
        assert_eq!(receipt["http_status"], json!(status));
        assert_eq!(receipt["body_complete"], json!(false));
        assert!(receipt["raw_blob"].is_object());
        assert!(
            planning::plan(&catalog, &collections, &archive, clock.now())?[0]
                .provider_deferred_until
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn failed_daily_collection_recovers_before_normal_cadence() -> TestResult {
    let (mut catalog, collections) = config()?;
    catalog.sources[0].refresh_seconds = Some(86400);
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([
            Response {
                status: 200,
                body: b"{}".to_vec(),
                ..Default::default()
            },
            forecast()?,
        ]),
        ..Default::default()
    };
    let mut clock = FakeClock::new();
    let options = SyncOptions::default();
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options,
            &mut transport,
            &mut clock
        )?
        .failed,
        1
    );
    assert!(!planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    clock.sleep(Duration::from_secs(300));
    assert!(planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options,
            &mut transport,
            &mut clock
        )?
        .succeeded,
        1
    );
    Ok(())
}

#[test]
fn catalogue_window_rejects_fraction_over_thirty_one_days() -> TestResult {
    let (mut catalog, mut collections) = config()?;
    let source = &mut catalog.sources[0];
    source.connector = crate::ConnectorKind::CdseCatalogue;
    source.provider = "copernicus-cdse".into();
    source.data_class = crate::DataClass::Imagery;
    collections.collections[0].parameters = json!({"latitude":3.2,"longitude":101.4,"start":"2026-10-01T00:00:00Z","end":"2026-11-01T00:00:01Z","limit":20});
    assert!(crate::validate(&catalog, &collections).is_err());
    collections.collections[0].parameters["end"] = json!("2026-11-01T00:00:00Z");
    crate::validate(&catalog, &collections)?;
    Ok(())
}

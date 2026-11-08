use super::*;
use crate::{http::Response, planning, state};
use serde_json::json;
use std::collections::VecDeque;
use time::OffsetDateTime;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "lifecycle_tests.rs"]
mod lifecycle;

struct FakeClock {
    now: OffsetDateTime,
    waited: Vec<Duration>,
}
impl FakeClock {
    fn new() -> Self {
        Self {
            now: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_800_000_000),
            waited: vec![],
        }
    }
}
impl Clock for FakeClock {
    fn now(&self) -> OffsetDateTime {
        self.now
    }
    fn sleep(&mut self, duration: Duration) {
        self.waited.push(duration);
        self.now = self
            .now
            .saturating_add(time::Duration::seconds(duration.as_secs() as i64));
    }
}

#[derive(Default)]
struct FakeTransport {
    responses: VecDeque<Response>,
    requests: Vec<(String, Option<String>)>,
}
impl Transport for FakeTransport {
    fn get(&mut self, request: Request<'_>) -> Result<Response> {
        self.requests
            .push((request.url.into(), request.etag.map(str::to_owned)));
        self.responses
            .pop_front()
            .ok_or_else(|| Error::Acquisition("no fake response".into()))
    }
}

fn config() -> TestResult<(Catalog, Collections)> {
    let catalog = serde_json::from_value(json!({"schema_version":1,"sources":[{
        "id":"government-forecast","title":"Forecast","provider":"data-gov-weather","description":"Categorical outlook",
        "data_class":"general_guidance","access":"public","connector":"data_gov_weather",
        "documentation_urls":["https://developer.data.gov.my/realtime-api/weather"],
        "license":{"name":"CC BY 4.0","url":"https://creativecommons.org/licenses/by/4.0/","noncommercial_only":false,"redistribution":"Attribution required"},
        "coverage":"Malaysia","limitations":["Categorical forecast"],"reviewed_on":"2026-11-08",
        "refresh_seconds":3600,"provider_min_interval_seconds":15,"max_response_bytes":100000
    }]}))?;
    let collections = serde_json::from_value(json!({"schema_version":1,"collections":[{
        "id":"selangor","source_id":"government-forecast","description":"Selangor","enabled":true,"accept_noncommercial":false,
        "parameters":{"product":"forecast","limit":100,"location_name":"Selangor"}
    }]}))?;
    Ok((catalog, collections))
}

fn forecast() -> TestResult<Response> {
    Ok(Response {
        status: 200,
        body: serde_json::to_vec(
            &json!([{"location":{"location_id":"Ds001","location_name":"Selangor"},"date":"2026-11-08","morning_forecast":"Rain","afternoon_forecast":"Rain","night_forecast":"Clear","min_temp":24,"max_temp":30}]),
        )?,
        etag: Some("version-one".into()),
        ..Default::default()
    })
}

fn options() -> SyncOptions {
    SyncOptions {
        collection_ids: vec![],
        force: true,
    }
}

#[test]
fn conditional_response_reuses_checked_blob() -> TestResult {
    let (catalog, collections) = config()?;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([
            forecast()?,
            Response {
                status: 304,
                ..Default::default()
            },
        ]),
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
        .succeeded,
        1
    );
    let first = planning::status(&catalog, &collections, &archive, clock.now())?;
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options(),
            &mut transport,
            &mut clock
        )?
        .succeeded,
        1
    );
    let second = planning::status(&catalog, &collections, &archive, clock.now())?;
    assert_eq!(
        first[0].last_good_blob.as_ref().map(|blob| &blob.sha256),
        second[0].last_good_blob.as_ref().map(|blob| &blob.sha256)
    );
    assert_eq!(transport.requests[1].1.as_deref(), Some("version-one"));
    assert!(!clock.waited.is_empty());
    Ok(())
}

#[test]
fn schema_error_preserves_last_good_and_failure_evidence() -> TestResult {
    let (catalog, collections) = config()?;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let bad = Response {
        status: 200,
        body: b"<html>outage</html>".to_vec(),
        ..Default::default()
    };
    let mut transport = FakeTransport {
        responses: VecDeque::from([forecast()?, bad]),
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
    let first = planning::status(&catalog, &collections, &archive, clock.now())?;
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
    let after = planning::status(&catalog, &collections, &archive, clock.now())?;
    assert_eq!(after[0].health, "failed");
    assert_eq!(first[0].last_success_at, after[0].last_success_at);
    assert_eq!(
        first[0].last_good_blob.as_ref().map(|blob| &blob.sha256),
        after[0].last_good_blob.as_ref().map(|blob| &blob.sha256)
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("receipts/selangor"))?.count(),
        2
    );
    Ok(())
}

#[test]
fn rate_limit_and_retry_after_survive_forced_refresh() -> TestResult {
    let (catalog, collections) = config()?;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let limited = Response {
        status: 429,
        body: b"{}".to_vec(),
        retry_after: Some("20".into()),
        ..Default::default()
    };
    let mut transport = FakeTransport {
        responses: VecDeque::from([limited, forecast()?]),
        ..Default::default()
    };
    let mut clock = FakeClock::new();
    let start = clock.now();
    assert_eq!(
        sync_with(
            &catalog,
            &collections,
            &archive,
            &options(),
            &mut transport,
            &mut clock
        )?
        .succeeded,
        1
    );
    assert!((clock.now() - start).whole_seconds() >= 20);
    assert_eq!(transport.requests.len(), 2);
    Ok(())
}

#[test]
fn long_retry_after_defers_without_early_retry() -> TestResult {
    let (catalog, collections) = config()?;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let limited = Response {
        status: 429,
        body: b"{}".to_vec(),
        retry_after: Some("600".into()),
        ..Default::default()
    };
    let mut transport = FakeTransport {
        responses: VecDeque::from([limited]),
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
    Ok(())
}

#[test]
fn changed_request_discards_cache_validators() -> TestResult {
    let (catalog, mut collections) = config()?;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([forecast()?, forecast()?]),
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
    collections.collections[0].parameters["location_name"] = json!("Perak");
    assert!(planning::plan(&catalog, &collections, &archive, clock.now())?[0].due);
    sync_with(
        &catalog,
        &collections,
        &archive,
        &options(),
        &mut transport,
        &mut clock,
    )?;
    assert!(transport.requests[1].1.is_none());
    assert_ne!(transport.requests[0].0, transport.requests[1].0);
    Ok(())
}

#[test]
fn licence_and_access_blocks_prevent_network_calls() -> TestResult {
    let (mut catalog, collections) = config()?;
    catalog.sources[0].license.noncommercial_only = true;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport::default();
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
    catalog.sources[0].license.noncommercial_only = false;
    catalog.sources[0].access = crate::AccessClass::PublicViewOnly;
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
    assert!(transport.requests.is_empty());
    Ok(())
}

#[test]
fn distinct_collections_keep_separate_state() -> TestResult {
    let (catalog, mut collections) = config()?;
    let mut second = collections.collections[0].clone();
    second.id = "perak".into();
    second.parameters["location_name"] = json!("Perak");
    collections.collections.push(second);
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([forecast()?, forecast()?]),
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
        .succeeded,
        2
    );
    assert!(
        archive
            .read_json::<State>(&state::path("selangor"))?
            .is_some()
    );
    assert!(archive.read_json::<State>(&state::path("perak"))?.is_some());
    assert!(!clock.waited.is_empty());
    Ok(())
}

#[test]
fn oversized_response_never_becomes_successful() -> TestResult {
    let (mut catalog, collections) = config()?;
    catalog.sources[0].max_response_bytes = 1;
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let mut transport = FakeTransport {
        responses: VecDeque::from([forecast()?]),
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
    assert!(
        planning::status(&catalog, &collections, &archive, clock.now())?[0]
            .last_good_blob
            .is_none()
    );
    Ok(())
}

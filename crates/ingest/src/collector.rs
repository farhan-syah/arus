use crate::{
    Catalog, Collection, Collections, Error, Result, Source,
    catalog::blocked,
    http::{HttpTransport, Request, Response, Transport},
    parameters::{Prepared, prepare},
    rate::{Clock, SystemClock},
    response::{ValidationSummary, check},
    state::{self, State, stamp},
};
use arus_assets::{Archive, BlobRef};
use serde::Serialize;
use std::{collections::HashSet, time::Duration};

#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    pub collection_ids: Vec<String>,
    pub force: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub items: Vec<SyncItem>,
}

#[derive(Debug, Serialize)]
pub struct SyncItem {
    pub collection_id: String,
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Serialize)]
struct Receipt<'a> {
    schema_version: u32,
    collection_id: &'a str,
    source_id: &'a str,
    provider: &'a str,
    data_class: crate::DataClass,
    request_fingerprint: &'a str,
    request_url: &'a str,
    requested_parameters: &'a serde_json::Value,
    attempted_at: String,
    retrieved_at: String,
    attempt: u8,
    http_status: Option<u16>,
    body_complete: bool,
    outcome: &'a str,
    raw_blob: Option<&'a BlobRef>,
    validation: Option<&'a ValidationSummary>,
    error: Option<&'a str>,
    license: &'a crate::License,
    noncommercial_accepted: bool,
}

static RECEIPT_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn sync(
    catalog: &Catalog,
    collections: &Collections,
    archive: &Archive,
    options: &SyncOptions,
) -> Result<SyncReport> {
    let mut http = HttpTransport::new()?;
    sync_with(
        catalog,
        collections,
        archive,
        options,
        &mut http,
        &mut SystemClock,
    )
}

pub(crate) fn sync_with(
    catalog: &Catalog,
    collections: &Collections,
    archive: &Archive,
    options: &SyncOptions,
    transport: &mut dyn Transport,
    clock: &mut dyn Clock,
) -> Result<SyncReport> {
    crate::validate(catalog, collections)?;
    let selected: HashSet<&str> = options.collection_ids.iter().map(String::as_str).collect();
    if selected.iter().any(|id| {
        !collections
            .collections
            .iter()
            .any(|collection| collection.id.as_str() == *id)
    }) {
        return Err(Error::Config("selected collection does not exist".into()));
    }
    let _lock = archive.lock()?;
    let mut report = SyncReport::default();
    for collection in &collections.collections {
        if (!selected.is_empty() && !selected.contains(collection.id.as_str()))
            || (selected.is_empty() && !collection.enabled)
        {
            continue;
        }
        let source = catalog
            .sources
            .iter()
            .find(|source| source.id == collection.source_id)
            .ok_or_else(|| Error::Config(format!("unknown source {}", collection.source_id)))?;
        if let Some(reason) = blocked(source, collection) {
            report.failed += 1;
            report.items.push(SyncItem {
                collection_id: collection.id.clone(),
                outcome: "blocked".into(),
                detail: Some(reason),
            });
            continue;
        }
        let prepared = prepare(source, collection)?
            .ok_or_else(|| Error::Config("automatic source lacks a request".into()))?;
        let state = state::read(archive, &collection.id, &prepared.fingerprint)?;
        if !options.force && !state::due(&state, source.refresh_seconds, clock.now())? {
            report.skipped += 1;
            report.items.push(SyncItem {
                collection_id: collection.id.clone(),
                outcome: "not_due".into(),
                detail: None,
            });
            continue;
        }
        report.attempted += 1;
        let interval = catalog
            .sources
            .iter()
            .filter(|item| item.provider == source.provider)
            .map(|item| item.provider_min_interval_seconds)
            .max()
            .unwrap_or(source.provider_min_interval_seconds);
        let result = collect(
            source, collection, &prepared, state, archive, interval, transport, clock,
        );
        match result {
            Ok(outcome) => {
                report.succeeded += 1;
                report.items.push(SyncItem {
                    collection_id: collection.id.clone(),
                    outcome,
                    detail: None,
                });
            }
            Err(error) => {
                report.failed += 1;
                report.items.push(SyncItem {
                    collection_id: collection.id.clone(),
                    outcome: "failed".into(),
                    detail: Some(error.to_string()),
                });
            }
        }
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn collect(
    source: &Source,
    collection: &Collection,
    prepared: &Prepared,
    mut state: State,
    archive: &Archive,
    interval: u64,
    transport: &mut dyn Transport,
    clock: &mut dyn Clock,
) -> Result<String> {
    let mut last_error = None;
    for attempt in 1..=3 {
        if let Err(error) = crate::rate::reserve(archive, &source.provider, interval, clock) {
            state.last_error = Some(error.to_string());
            state.failures = state.failures.saturating_add(1);
            archive.write_json_atomic(&state::path(&collection.id), &state)?;
            return Err(error);
        }
        let start = clock.now();
        state.last_attempt_at = Some(stamp(start)?);
        archive.write_json_atomic(&state::path(&collection.id), &state)?;
        let cache = match &state.blob {
            Some(blob) => archive.read_blob(&blob.sha256).ok(),
            None => None,
        };
        let conditional = cache.is_some();
        let response = transport.get(Request {
            url: &prepared.url,
            etag: conditional.then_some(state.etag.as_deref()).flatten(),
            last_modified: conditional
                .then_some(state.last_modified.as_deref())
                .flatten(),
            max_bytes: source.max_response_bytes,
        });
        let mut raw_blob = None;
        let mut summary = None;
        let mut http_status = None;
        let mut retry;
        let mut retry_seconds = u64::from(attempt);
        let mut body_complete = false;
        let result = match response {
            Err(error) => {
                retry = true;
                Err(error)
            }
            Ok(response) => {
                http_status = Some(response.status);
                retry = response.status == 429 || matches!(response.status, 500 | 502 | 503 | 504);
                if let Some(delay) = retry_after(&response, clock.now()) {
                    crate::rate::penalize(archive, &source.provider, delay, clock)?;
                    if delay > 60 {
                        retry = false;
                    }
                    retry_seconds = delay.min(60);
                }
                body_complete = response.body_error.is_none()
                    && response.body.len() as u64 <= source.max_response_bytes;
                if !response.body.is_empty() {
                    let bound = response.body.len().min(source.max_response_bytes as usize);
                    raw_blob = Some(archive.put_blob(&response.body[..bound])?);
                }
                if response.body.len() as u64 > source.max_response_bytes {
                    Err(Error::Acquisition(format!(
                        "response exceeds {} bytes. Inspect provider changes before raising the budget",
                        source.max_response_bytes
                    )))
                } else {
                    process_response(
                        &response,
                        cache.as_deref(),
                        prepared,
                        &mut state,
                        &mut raw_blob,
                        &mut summary,
                    )
                }
            }
        };
        let finish = clock.now();
        let outcome = match &result {
            Ok(value) => value.as_str(),
            Err(_) if retry && attempt < 3 => "retrying",
            Err(_) => "failed",
        };
        let error_text = result.as_ref().err().map(ToString::to_string);
        let receipt = Receipt {
            schema_version: 1,
            collection_id: &collection.id,
            source_id: &source.id,
            provider: &source.provider,
            data_class: source.data_class,
            request_fingerprint: &prepared.fingerprint,
            request_url: &prepared.url,
            requested_parameters: &collection.parameters,
            attempted_at: stamp(start)?,
            retrieved_at: stamp(finish)?,
            attempt,
            http_status,
            body_complete,
            outcome,
            raw_blob: raw_blob.as_ref(),
            validation: summary.as_ref(),
            error: error_text.as_deref(),
            license: &source.license,
            noncommercial_accepted: collection.accept_noncommercial,
        };
        let sequence = RECEIPT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let receipt_id = format!(
            "{}-{}-{}-{sequence}",
            finish.unix_timestamp_nanos(),
            &prepared.fingerprint[..16],
            attempt
        );
        archive.append_receipt(&collection.id, &receipt_id, &receipt)?;
        match result {
            Ok(outcome) => {
                state.last_success_at = Some(stamp(finish)?);
                state.last_error = None;
                state.failures = 0;
                state.validation = summary;
                archive.write_json_atomic(&state::path(&collection.id), &state)?;
                return Ok(outcome);
            }
            Err(error) => {
                state.last_error = Some(error.to_string());
                last_error = Some(error);
                archive.write_json_atomic(&state::path(&collection.id), &state)?;
            }
        }
        if !retry || attempt == 3 {
            break;
        }
        clock.sleep(Duration::from_secs(retry_seconds));
    }
    state.failures = state.failures.saturating_add(1);
    archive.write_json_atomic(&state::path(&collection.id), &state)?;
    Err(last_error.unwrap_or_else(|| {
        Error::Acquisition("request attempts exhausted. Inspect acquisition receipts".into())
    }))
}

fn process_response(
    response: &Response,
    cached: Option<&[u8]>,
    prepared: &Prepared,
    state: &mut State,
    raw: &mut Option<BlobRef>,
    summary: &mut Option<ValidationSummary>,
) -> Result<String> {
    if let Some(error) = &response.body_error {
        return Err(Error::Acquisition(format!(
            "HTTP {} for {}: {error}. Inspect the incomplete archived response",
            response.status, prepared.url
        )));
    }
    if response.status == 304 {
        let bytes = cached.ok_or_else(|| {
            Error::Acquisition(
                "HTTP 304 has no checked cached body. Retry an unconditional request".into(),
            )
        })?;
        *summary = Some(check(bytes, &prepared.response)?);
        *raw = state.blob.clone();
        if response.etag.is_some() {
            state.etag = response.etag.clone();
        }
        if response.last_modified.is_some() {
            state.last_modified = response.last_modified.clone();
        }
        return Ok("not_modified".into());
    }
    if response.status != 200 {
        return Err(Error::Acquisition(format!(
            "HTTP {} for {}. Inspect provider status and the archived response",
            response.status, prepared.url
        )));
    }
    let checked = check(&response.body, &prepared.response)?;
    state.blob = raw.clone();
    state.etag = response.etag.clone();
    state.last_modified = response.last_modified.clone();
    *summary = Some(checked);
    Ok("collected".into())
}

fn retry_after(response: &Response, now: time::OffsetDateTime) -> Option<u64> {
    let value = response.retry_after.as_deref()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc2822)
        .ok()
        .and_then(|until| u64::try_from((until - now).whole_seconds().max(0)).ok())
}

#[cfg(test)]
mod tests;

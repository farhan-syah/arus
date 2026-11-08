use crate::{Catalog, Collections, Result, catalog::blocked, parameters::prepare, state};
use arus_assets::{Archive, BlobRef};
use serde::Serialize;
use time::OffsetDateTime;

#[derive(Debug, Serialize)]
pub struct PlanItem {
    pub collection_id: String,
    pub source_id: String,
    pub enabled: bool,
    pub due: bool,
    pub blocked: Option<String>,
    pub request_url: Option<String>,
    pub request_fingerprint: Option<String>,
    pub refresh_seconds: Option<u64>,
    pub provider_deferred_until: Option<String>,
    pub next_attempt_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CollectionStatus {
    pub collection_id: String,
    pub source_id: String,
    pub enabled: bool,
    pub due: bool,
    pub blocked: Option<String>,
    pub health: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub failures: u32,
    pub last_good_blob: Option<BlobRef>,
    pub validation: Option<crate::response::ValidationSummary>,
    pub provider_deferred_until: Option<String>,
    pub next_attempt_at: Option<String>,
}

pub fn plan(
    catalog: &Catalog,
    collections: &Collections,
    archive: &Archive,
    now: OffsetDateTime,
) -> Result<Vec<PlanItem>> {
    crate::validate(catalog, collections)?;
    collections
        .collections
        .iter()
        .map(|collection| {
            let source = catalog
                .sources
                .iter()
                .find(|source| source.id == collection.source_id)
                .ok_or_else(|| {
                    crate::Error::Config(format!("unknown source {}", collection.source_id))
                })?;
            let prepared = prepare(source, collection)?;
            let state = match &prepared {
                Some(request) => state::read(archive, &collection.id, &request.fingerprint)?,
                None => state::State::default(),
            };
            let due = prepared.is_some() && state::due(&state, source.refresh_seconds, now)?;
            let next_attempt_at = state::next_attempt(&state, source.refresh_seconds)?
                .map(state::stamp)
                .transpose()?;
            Ok(PlanItem {
                collection_id: collection.id.clone(),
                source_id: source.id.clone(),
                enabled: collection.enabled,
                due,
                blocked: blocked(source, collection),
                request_url: prepared.as_ref().map(|request| request.url.clone()),
                request_fingerprint: prepared.map(|request| request.fingerprint),
                refresh_seconds: source.refresh_seconds,
                next_attempt_at,
                provider_deferred_until: crate::rate::deferred_until(
                    archive,
                    &source.provider,
                    now,
                )?,
            })
        })
        .collect()
}

pub fn status(
    catalog: &Catalog,
    collections: &Collections,
    archive: &Archive,
    now: OffsetDateTime,
) -> Result<Vec<CollectionStatus>> {
    let planned = plan(catalog, collections, archive, now)?;
    planned
        .into_iter()
        .map(|item| {
            let state = match item.request_fingerprint.as_deref() {
                Some(fingerprint) => state::read(archive, &item.collection_id, fingerprint)?,
                None => state::State::default(),
            };
            let health = if item.blocked.is_some() {
                "blocked"
            } else if item.due && item.provider_deferred_until.is_some() {
                "provider_deferred"
            } else if state.last_error.is_some() {
                "failed"
            } else if state.last_success_at.is_none() {
                "never_collected"
            } else if item.due {
                "refresh_due"
            } else {
                "retrieval_current"
            };
            Ok(CollectionStatus {
                collection_id: item.collection_id,
                source_id: item.source_id,
                enabled: item.enabled,
                due: item.due,
                blocked: item.blocked,
                health: health.into(),
                last_attempt_at: state.last_attempt_at,
                last_success_at: state.last_success_at,
                last_error: state.last_error,
                failures: state.failures,
                last_good_blob: state.blob,
                validation: state.validation,
                provider_deferred_until: item.provider_deferred_until,
                next_attempt_at: item.next_attempt_at,
            })
        })
        .collect()
}

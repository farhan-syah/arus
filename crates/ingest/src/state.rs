use crate::{Error, Result, response::ValidationSummary};
use arus_assets::{Archive, BlobRef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct State {
    pub fingerprint: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub failures: u32,
    pub blob: Option<BlobRef>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub validation: Option<ValidationSummary>,
}

pub(crate) fn path(id: &str) -> PathBuf {
    PathBuf::from("state/collections").join(format!("{id}.json"))
}

pub(crate) fn read(archive: &Archive, id: &str, fingerprint: &str) -> Result<State> {
    let state = archive.read_json::<State>(&path(id))?;
    Ok(state
        .filter(|state| state.fingerprint == fingerprint)
        .unwrap_or_else(|| State {
            fingerprint: fingerprint.into(),
            ..State::default()
        }))
}

pub(crate) fn stamp(now: OffsetDateTime) -> Result<String> {
    now.format(&Rfc3339).map_err(|error| {
        Error::Acquisition(format!(
            "cannot format clock {now}: {error}. Check the system clock"
        ))
    })
}

pub(crate) fn parse_stamp(raw: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339).map_err(|error| {
        Error::Acquisition(format!(
            "cannot parse archived timestamp {raw}: {error}. Inspect archive state"
        ))
    })
}

pub(crate) fn due(state: &State, interval: Option<u64>, now: OffsetDateTime) -> Result<bool> {
    if state.last_attempt_at.is_none() {
        return Ok(true);
    }
    Ok(next_attempt(state, interval)?.is_some_and(|next| now >= next))
}

pub(crate) fn next_attempt(state: &State, interval: Option<u64>) -> Result<Option<OffsetDateTime>> {
    let Some(last) = state.last_attempt_at.as_deref() else {
        return Ok(None);
    };
    let attempted = parse_stamp(last)?;
    let success = state
        .last_success_at
        .as_deref()
        .map(parse_stamp)
        .transpose()?;
    let unsuccessful =
        state.last_error.is_some() || success.is_none_or(|success| success < attempted);
    let seconds = if unsuccessful {
        let exponent = state.failures.saturating_sub(1).min(4);
        let backoff = 300_u64.saturating_mul(1_u64 << exponent).min(3600);
        backoff.min(interval.unwrap_or(3600))
    } else {
        let Some(interval) = interval else {
            return Ok(None);
        };
        interval
    };
    Ok(Some(attempted.saturating_add(time::Duration::seconds(
        i64::try_from(seconds).unwrap_or(i64::MAX),
    ))))
}

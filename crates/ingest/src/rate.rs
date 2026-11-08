use crate::{
    Error, Result,
    state::{parse_stamp, stamp},
};
use arus_assets::Archive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
use time::OffsetDateTime;

pub(crate) trait Clock {
    fn now(&self) -> OffsetDateTime;
    fn sleep(&mut self, duration: Duration);
}

pub(crate) struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Serialize, Deserialize)]
struct RateState {
    next_allowed_at: String,
}

fn path(provider: &str) -> PathBuf {
    PathBuf::from("state/providers").join(format!("{:x}.json", Sha256::digest(provider.as_bytes())))
}

pub(crate) fn deferred_until(
    archive: &Archive,
    provider: &str,
    now: OffsetDateTime,
) -> Result<Option<String>> {
    let state = archive.read_json::<RateState>(&path(provider))?;
    match state {
        Some(state) if parse_stamp(&state.next_allowed_at)? > now => {
            Ok(Some(state.next_allowed_at))
        }
        _ => Ok(None),
    }
}

pub(crate) fn reserve(
    archive: &Archive,
    provider: &str,
    interval: u64,
    clock: &mut dyn Clock,
) -> Result<()> {
    if let Some(state) = archive.read_json::<RateState>(&path(provider))? {
        let wait = parse_stamp(&state.next_allowed_at)? - clock.now();
        if wait > time::Duration::seconds(60) {
            return Err(Error::Acquisition(format!(
                "provider {provider} is deferred until {}. Run refresh after that time",
                state.next_allowed_at
            )));
        }
        if wait.is_positive() {
            clock.sleep(wait.unsigned_abs());
        }
    }
    let next = clock.now().saturating_add(time::Duration::seconds(
        i64::try_from(interval).unwrap_or(i64::MAX),
    ));
    archive.write_json_atomic(
        &path(provider),
        &RateState {
            next_allowed_at: stamp(next)?,
        },
    )?;
    Ok(())
}

pub(crate) fn penalize(
    archive: &Archive,
    provider: &str,
    seconds: u64,
    clock: &dyn Clock,
) -> Result<()> {
    let target = clock.now().saturating_add(time::Duration::seconds(
        i64::try_from(seconds).unwrap_or(i64::MAX),
    ));
    let old = archive.read_json::<RateState>(&path(provider))?;
    let next = match old {
        Some(old) => parse_stamp(&old.next_allowed_at)?.max(target),
        None => target,
    };
    archive.write_json_atomic(
        &path(provider),
        &RateState {
            next_allowed_at: stamp(next)?,
        },
    )?;
    Ok(())
}

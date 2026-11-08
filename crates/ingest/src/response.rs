use crate::{Error, Result, parameters::ResponseKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationSummary {
    pub records: usize,
    pub missing_values: usize,
    pub time_gaps: usize,
    pub truncated: bool,
    pub first_valid_time: Option<String>,
    pub last_valid_time: Option<String>,
    pub metadata_discovery_only: bool,
    pub requested_start: Option<String>,
    pub requested_end: Option<String>,
    pub requested_initialization: Option<String>,
    pub coverage_complete: Option<bool>,
    pub coverage_assessment: String,
    pub unknown_validity_records: usize,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Acquisition(format!(
        "response schema: {}. Inspect the archived response",
        message.into()
    ))
}

fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("missing string {pointer}")))
}

pub(crate) fn check(bytes: &[u8], kind: &ResponseKind) -> Result<ValidationSummary> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| invalid(format!("invalid JSON: {error}")))?;
    match kind {
        ResponseKind::Forecast { limit } | ResponseKind::Warning { limit } => {
            let rows = value
                .as_array()
                .ok_or_else(|| invalid("expected an array"))?;
            if rows.len() > *limit
                || (rows.is_empty() && matches!(kind, ResponseKind::Forecast { .. }))
            {
                return Err(invalid("record count outside request bounds"));
            }
            let mut unknown_validity = 0;
            for row in rows {
                if matches!(kind, ResponseKind::Forecast { .. }) {
                    for key in [
                        "/location/location_id",
                        "/location/location_name",
                        "/date",
                        "/morning_forecast",
                        "/afternoon_forecast",
                        "/night_forecast",
                    ] {
                        text(row, key)?;
                    }
                    crate::parameters::date(text(row, "/date")?)
                        .map_err(|_| invalid("invalid forecast date"))?;
                    for key in ["/min_temp", "/max_temp"] {
                        if !row.pointer(key).is_some_and(Value::is_number) {
                            return Err(invalid(format!("missing numeric {key}")));
                        }
                    }
                } else {
                    for key in ["/warning_issue/issued", "/heading_en", "/text_en"] {
                        text(row, key)?;
                    }
                    let mut unknown = false;
                    for key in ["valid_from", "valid_to"] {
                        match row.get(key) {
                            Some(Value::Null) => unknown = true,
                            Some(Value::String(_)) => {}
                            _ => return Err(invalid(format!("missing string-or-null {key}"))),
                        }
                    }
                    if unknown {
                        unknown_validity += 1;
                    }
                }
            }
            Ok(ValidationSummary {
                records: rows.len(),
                truncated: rows.len() == *limit,
                unknown_validity_records: unknown_validity,
                ..Default::default()
            })
        }
        ResponseKind::Hourly {
            window,
            requested_run,
        } => hourly(&value, *window, requested_run.as_deref()),
        ResponseKind::Catalogue { limit } => {
            let rows = value
                .get("value")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("missing value array"))?;
            if rows.len() > *limit {
                return Err(invalid("catalogue count exceeds request limit"));
            }
            for row in rows {
                for key in ["/Id", "/Name", "/ContentDate/Start"] {
                    text(row, key)?;
                }
                if !row.get("GeoFootprint").is_some_and(Value::is_object) {
                    return Err(invalid("missing GeoFootprint object"));
                }
            }
            Ok(ValidationSummary {
                records: rows.len(),
                truncated: rows.len() == *limit || value.get("@odata.nextLink").is_some(),
                metadata_discovery_only: true,
                ..Default::default()
            })
        }
    }
}

fn hourly(
    value: &Value,
    window: Option<(time::PrimitiveDateTime, time::PrimitiveDateTime)>,
    requested_run: Option<&str>,
) -> Result<ValidationSummary> {
    for (key, low, high) in [("latitude", -90.0, 90.0), ("longitude", -180.0, 180.0)] {
        if !value
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(|number| number.is_finite() && (low..=high).contains(&number))
        {
            return Err(invalid(format!("invalid {key}")));
        }
    }
    if value.get("utc_offset_seconds").and_then(Value::as_i64) != Some(0)
        || !matches!(
            value.get("timezone").and_then(Value::as_str),
            Some("GMT" | "UTC")
        )
        || value
            .pointer("/hourly_units/precipitation")
            .and_then(Value::as_str)
            != Some("mm")
    {
        return Err(invalid("expected UTC timestamps and precipitation in mm"));
    }
    let times = value
        .pointer("/hourly/time")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("missing hourly time array"))?;
    let rain = value
        .pointer("/hourly/precipitation")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("missing hourly precipitation array"))?;
    if times.is_empty() || times.len() != rain.len() {
        return Err(invalid("empty or misaligned hourly arrays"));
    }
    let format =
        time::format_description::parse_borrowed::<2>("[year]-[month]-[day]T[hour]:[minute]")
            .map_err(|error| invalid(error.to_string()))?;
    let mut previous = None;
    let mut first = None;
    let mut gaps = 0;
    for entry in times {
        let raw = entry
            .as_str()
            .ok_or_else(|| invalid("time entry is not a string"))?;
        let parsed = time::PrimitiveDateTime::parse(raw, &format)
            .map_err(|_| invalid(format!("invalid hourly time {raw}")))?;
        if let Some((start, end)) = window
            && (parsed < start || parsed > end)
        {
            return Err(invalid(
                "hourly timestamps fall outside the requested historical window",
            ));
        }
        if first.is_none() {
            first = Some(parsed);
        }
        if let Some(before) = previous {
            if parsed <= before {
                return Err(invalid("hourly times are not strictly ascending"));
            }
            if (parsed - before).whole_seconds() != 3600 {
                gaps += 1;
            }
        }
        previous = Some(parsed);
    }
    let mut missing = 0;
    for entry in rain {
        if entry.is_null() {
            missing += 1;
        } else if !entry
            .as_f64()
            .is_some_and(|number| number.is_finite() && number >= 0.0)
        {
            return Err(invalid(
                "precipitation must be nonnegative or explicit null",
            ));
        }
    }
    if missing == rain.len() {
        return Err(invalid("all precipitation values are missing"));
    }
    let coverage_complete = window.map(|(start, end)| {
        first == Some(start)
            && previous == Some(end)
            && times.len() as i64 == (end - start).whole_hours() + 1
            && gaps == 0
            && missing == 0
    });
    let requested_start = window.map(|(start, _)| start.to_string());
    let requested_end = window.map(|(_, end)| end.to_string());
    Ok(ValidationSummary {
        requested_start,
        requested_end,
        requested_initialization: requested_run.map(str::to_owned),
        coverage_complete,
        coverage_assessment: if window.is_some() {
            "requested_window_checked"
        } else if requested_run.is_some() {
            "requested_initialization_not_independently_confirmed"
        } else {
            "provider_times_only"
        }
        .into(),
        records: times.len(),
        missing_values: missing,
        time_gaps: gaps,
        first_valid_time: times.first().and_then(Value::as_str).map(str::to_owned),
        last_valid_time: times.last().and_then(Value::as_str).map(str::to_owned),
        ..Default::default()
    })
}

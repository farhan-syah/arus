use crate::{Collection, ConnectorKind, Error, Result, Source};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::{Date, OffsetDateTime, PrimitiveDateTime, format_description::well_known::Rfc3339};

#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub url: String,
    pub fingerprint: String,
    pub response: ResponseKind,
}

#[derive(Debug, Clone)]
pub(crate) enum ResponseKind {
    Forecast {
        limit: usize,
    },
    Warning {
        limit: usize,
    },
    Hourly {
        window: Option<(PrimitiveDateTime, PrimitiveDateTime)>,
        requested_run: Option<String>,
        models: Vec<String>,
        hourly: Vec<String>,
    },
    Marine {
        hourly: Vec<String>,
    },
    Flood {
        daily: Vec<String>,
    },
    Catalogue {
        limit: usize,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Government {
    product: Product,
    #[serde(default = "default_limit")]
    limit: u32,
    location_name: Option<String>,
}

fn default_limit() -> u32 {
    100
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Product {
    Forecast,
    Warning,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Forecast {
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    hourly: Vec<String>,
    forecast_days: u8,
    #[serde(default)]
    past_days: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SingleRun {
    latitude: f64,
    longitude: f64,
    model: String,
    hourly: Vec<String>,
    forecast_days: u8,
    run: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    latitude: f64,
    longitude: f64,
    model: String,
    hourly: Vec<String>,
    start_date: String,
    end_date: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marine {
    latitude: f64,
    longitude: f64,
    hourly: Vec<String>,
    forecast_days: u8,
    #[serde(default)]
    past_days: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Flood {
    latitude: f64,
    longitude: f64,
    daily: Vec<String>,
    forecast_days: u16,
    #[serde(default)]
    past_days: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    latitude: f64,
    longitude: f64,
    start: String,
    end: String,
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(collection: &Collection) -> Result<T> {
    serde_json::from_value(collection.parameters.clone())
        .map_err(|error| Error::Config(format!("collection {} parameters: {error}", collection.id)))
}

fn coordinates(latitude: f64, longitude: f64) -> Result<()> {
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(Error::Config(
            "latitude/longitude outside valid finite bounds".into(),
        ));
    }
    Ok(())
}

const ALLOWED_OPEN_METEO_MODELS: &[&str] = &[
    "gfs_global",
    "ecmwf_ifs025",
    "icon_seamless",
    "gem_global",
    "jma_seamless",
];

const ALLOWED_OPEN_METEO_HOURLY: &[&str] = &[
    "precipitation",
    "soil_moisture_0_to_7cm",
    "soil_moisture_0_to_1cm",
];

const ALLOWED_MARINE_HOURLY: &[&str] = &[
    "wave_height",
    "sea_level_height_msl",
    "ocean_current_velocity",
];

const ALLOWED_FLOOD_DAILY: &[&str] = &[
    "river_discharge",
    "river_discharge_mean",
    "river_discharge_median",
    "river_discharge_max",
    "river_discharge_min",
    "river_discharge_p25",
    "river_discharge_p75",
];

fn resolve_models(model: Option<String>, models: Vec<String>) -> Result<Vec<String>> {
    let list = if !models.is_empty() {
        if model.is_some() {
            return Err(Error::Config(
                "specify either `model` or `models`, not both".into(),
            ));
        }
        models
    } else if let Some(single) = model {
        vec![single]
    } else {
        return Err(Error::Config(
            "Open-Meteo collection requires `model` or non-empty `models`".into(),
        ));
    };
    for m in &list {
        if !ALLOWED_OPEN_METEO_MODELS.contains(&m.as_str()) {
            return Err(Error::Config(format!(
                "unsupported Open-Meteo model `{m}`; allowed: {}",
                ALLOWED_OPEN_METEO_MODELS.join(", ")
            )));
        }
    }
    Ok(list)
}

fn meteo(
    latitude: f64,
    longitude: f64,
    models: &[String],
    hourly: &[String],
) -> Result<Vec<(String, String)>> {
    coordinates(latitude, longitude)?;
    if models.is_empty() {
        return Err(Error::Config(
            "Open-Meteo requires at least one model".into(),
        ));
    }
    for m in models {
        if !ALLOWED_OPEN_METEO_MODELS.contains(&m.as_str()) {
            return Err(Error::Config(format!(
                "unsupported Open-Meteo model `{m}`; allowed: {}",
                ALLOWED_OPEN_METEO_MODELS.join(", ")
            )));
        }
    }
    if hourly.is_empty() || !hourly.iter().any(|v| v == "precipitation") {
        return Err(Error::Config(
            "Open-Meteo hourly variables must include `precipitation`".into(),
        ));
    }
    for var in hourly {
        if !ALLOWED_OPEN_METEO_HOURLY.contains(&var.as_str()) {
            return Err(Error::Config(format!(
                "unsupported Open-Meteo hourly variable `{var}`; allowed: {}",
                ALLOWED_OPEN_METEO_HOURLY.join(", ")
            )));
        }
    }
    Ok(vec![
        ("latitude".into(), latitude.to_string()),
        ("longitude".into(), longitude.to_string()),
        ("models".into(), models.join(",")),
        ("hourly".into(), hourly.join(",")),
        ("timezone".into(), "GMT".into()),
    ])
}

fn days(value: u8) -> Result<()> {
    if !(1..=7).contains(&value) {
        return Err(Error::Config(
            "forecast_days must be between 1 and 7".into(),
        ));
    }
    Ok(())
}

fn url(endpoint: &str, query: Vec<(String, String)>) -> Result<String> {
    let mut url =
        reqwest::Url::parse(endpoint).map_err(|error| Error::Config(error.to_string()))?;
    let mut query = query;
    query.sort();
    url.query_pairs_mut().extend_pairs(query);
    Ok(url.into())
}

pub(crate) fn date(value: &str) -> Result<Date> {
    let format = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")
        .map_err(|error| Error::Config(error.to_string()))?;
    Date::parse(value, &format).map_err(|error| Error::Config(format!("date {value}: {error}")))
}

pub(crate) fn prepare(source: &Source, collection: &Collection) -> Result<Option<Prepared>> {
    let (request, response) = match source.connector {
        ConnectorKind::CatalogOnly => {
            let _: Empty = parse(collection)?;
            return Ok(None);
        }
        ConnectorKind::DataGovWeather => {
            let params: Government = parse(collection)?;
            if !(1..=1000).contains(&params.limit) {
                return Err(Error::Config(
                    "government limit must be between 1 and 1000".into(),
                ));
            }
            let mut query = vec![("limit".into(), params.limit.to_string())];
            let (product, response) = match params.product {
                Product::Forecast => (
                    "forecast",
                    ResponseKind::Forecast {
                        limit: params.limit as usize,
                    },
                ),
                Product::Warning => (
                    "warning",
                    ResponseKind::Warning {
                        limit: params.limit as usize,
                    },
                ),
            };
            if let Some(name) = params.location_name {
                if !matches!(params.product, Product::Forecast)
                    || name.trim().is_empty()
                    || name.len() > 100
                    || name.contains('@')
                    || name.chars().any(char::is_control)
                {
                    return Err(Error::Config("location_name requires a bounded forecast location without @ or control characters".into()));
                }
                query.push(("contains".into(), format!("{name}@location__location_name")));
            }
            (
                url(
                    &format!("https://api.data.gov.my/weather/{product}/"),
                    query,
                )?,
                response,
            )
        }
        ConnectorKind::OpenMeteoForecast => {
            let params: Forecast = parse(collection)?;
            days(params.forecast_days)?;
            let models = resolve_models(params.model, params.models)?;
            let mut query = meteo(params.latitude, params.longitude, &models, &params.hourly)?;
            query.push(("forecast_days".into(), params.forecast_days.to_string()));
            if let Some(past) = params.past_days {
                if past > 7 {
                    return Err(Error::Config("past_days must be between 0 and 7".into()));
                }
                query.push(("past_days".into(), past.to_string()));
            }
            (
                url("https://api.open-meteo.com/v1/forecast", query)?,
                ResponseKind::Hourly {
                    window: None,
                    requested_run: None,
                    models,
                    hourly: params.hourly,
                },
            )
        }
        ConnectorKind::OpenMeteoSingleRun => {
            let params: SingleRun = parse(collection)?;
            days(params.forecast_days)?;
            let run_format = time::format_description::parse_borrowed::<2>(
                "[year]-[month]-[day]T[hour]:[minute]",
            )
            .map_err(|error| Error::Config(error.to_string()))?;
            let run = PrimitiveDateTime::parse(&params.run, &run_format)
                .map_err(|error| Error::Config(format!("run {}: {error}", params.run)))?;
            if run.minute() != 0 || ![0, 6, 12, 18].contains(&run.hour()) {
                return Err(Error::Config(
                    "GFS run must identify a 00, 06, 12, or 18 UTC cycle".into(),
                ));
            }
            let models = vec![params.model];
            let mut query = meteo(params.latitude, params.longitude, &models, &params.hourly)?;
            query.push(("run".into(), params.run.clone()));
            query.push(("forecast_days".into(), params.forecast_days.to_string()));
            (
                url("https://single-runs-api.open-meteo.com/v1/forecast", query)?,
                ResponseKind::Hourly {
                    window: None,
                    requested_run: Some(params.run),
                    models,
                    hourly: params.hourly,
                },
            )
        }
        ConnectorKind::OpenMeteoHistory => {
            let params: History = parse(collection)?;
            let start = date(&params.start_date)?;
            let end = date(&params.end_date)?;
            if end < start || (end - start).whole_days() >= 7 {
                return Err(Error::Config(
                    "historical window must contain between 1 and 7 calendar days".into(),
                ));
            }
            let models = vec![params.model];
            let mut query = meteo(params.latitude, params.longitude, &models, &params.hourly)?;
            query.extend([
                ("start_date".into(), params.start_date),
                ("end_date".into(), params.end_date),
            ]);
            (
                url(
                    "https://historical-forecast-api.open-meteo.com/v1/forecast",
                    query,
                )?,
                ResponseKind::Hourly {
                    window: Some((
                        start.midnight(),
                        end.with_hms(23, 0, 0)
                            .map_err(|error| Error::Config(error.to_string()))?,
                    )),
                    requested_run: None,
                    models,
                    hourly: params.hourly,
                },
            )
        }
        ConnectorKind::OpenMeteoMarine => {
            let params: Marine = parse(collection)?;
            coordinates(params.latitude, params.longitude)?;
            days(params.forecast_days)?;
            if params.hourly.is_empty() {
                return Err(Error::Config(
                    "Open-Meteo Marine requires at least one hourly variable".into(),
                ));
            }
            for var in &params.hourly {
                if !ALLOWED_MARINE_HOURLY.contains(&var.as_str()) {
                    return Err(Error::Config(format!(
                        "unsupported marine hourly variable `{var}`; allowed: {}",
                        ALLOWED_MARINE_HOURLY.join(", ")
                    )));
                }
            }
            let mut query = vec![
                ("latitude".into(), params.latitude.to_string()),
                ("longitude".into(), params.longitude.to_string()),
                ("hourly".into(), params.hourly.join(",")),
                ("forecast_days".into(), params.forecast_days.to_string()),
                ("timezone".into(), "GMT".into()),
            ];
            if let Some(past) = params.past_days {
                if past > 7 {
                    return Err(Error::Config("past_days must be between 0 and 7".into()));
                }
                query.push(("past_days".into(), past.to_string()));
            }
            (
                url("https://marine-api.open-meteo.com/v1/marine", query)?,
                ResponseKind::Marine {
                    hourly: params.hourly,
                },
            )
        }
        ConnectorKind::OpenMeteoFlood => {
            let params: Flood = parse(collection)?;
            coordinates(params.latitude, params.longitude)?;
            if !(1..=183).contains(&params.forecast_days) {
                return Err(Error::Config(
                    "GloFAS flood forecast_days must be between 1 and 183".into(),
                ));
            }
            if params.daily.is_empty() {
                return Err(Error::Config(
                    "Open-Meteo GloFAS requires at least one daily variable".into(),
                ));
            }
            for var in &params.daily {
                if !ALLOWED_FLOOD_DAILY.contains(&var.as_str()) {
                    return Err(Error::Config(format!(
                        "unsupported flood daily variable `{var}`; allowed: {}",
                        ALLOWED_FLOOD_DAILY.join(", ")
                    )));
                }
            }
            let mut query = vec![
                ("latitude".into(), params.latitude.to_string()),
                ("longitude".into(), params.longitude.to_string()),
                ("daily".into(), params.daily.join(",")),
                ("forecast_days".into(), params.forecast_days.to_string()),
            ];
            if let Some(past) = params.past_days {
                if past > 92 {
                    return Err(Error::Config(
                        "GloFAS past_days must be between 0 and 92".into(),
                    ));
                }
                query.push(("past_days".into(), past.to_string()));
            }
            (
                url("https://flood-api.open-meteo.com/v1/flood", query)?,
                ResponseKind::Flood {
                    daily: params.daily,
                },
            )
        }
        ConnectorKind::CdseCatalogue => {
            let params: Catalogue = parse(collection)?;
            coordinates(params.latitude, params.longitude)?;
            let start = OffsetDateTime::parse(&params.start, &Rfc3339)
                .map_err(|error| Error::Config(format!("catalogue start: {error}")))?;
            let end = OffsetDateTime::parse(&params.end, &Rfc3339)
                .map_err(|error| Error::Config(format!("catalogue end: {error}")))?;
            if end <= start
                || (end - start) > time::Duration::days(31)
                || !(1..=20).contains(&params.limit)
            {
                return Err(Error::Config(
                    "catalogue query needs a positive window at most 31 days and limit 1..20"
                        .into(),
                ));
            }
            let filter = format!(
                "Collection/Name eq 'SENTINEL-1' and ContentDate/Start ge {} and ContentDate/Start lt {} and OData.CSC.Intersects(area=geography'SRID=4326;POINT({} {})')",
                params.start, params.end, params.longitude, params.latitude
            );
            (
                url(
                    "https://catalogue.dataspace.copernicus.eu/odata/v1/Products",
                    vec![
                        ("$filter".into(), filter),
                        ("$orderby".into(), "ContentDate/Start".into()),
                        ("$top".into(), params.limit.to_string()),
                    ],
                )?,
                ResponseKind::Catalogue {
                    limit: params.limit as usize,
                },
            )
        }
    };
    let identity = format!("v1\n{}\n{:?}\n{}", source.id, source.connector, request);
    Ok(Some(Prepared {
        url: request,
        fingerprint: format!("{:x}", Sha256::digest(identity.as_bytes())),
        response,
    }))
}

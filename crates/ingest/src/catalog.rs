use crate::{AccessClass, Catalog, Collections, ConnectorKind, Error, Result};
use serde::de::DeserializeOwned;
use std::{collections::HashSet, fs, path::Path};

fn load<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|source| Error::Read {
        path: path.into(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        load(path)
    }
}

impl Collections {
    pub fn load(path: &Path) -> Result<Self> {
        load(path)
    }
}

pub(crate) fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn https(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && !url.query_pairs().any(|(key, _)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "token"
                        | "api_key"
                        | "apikey"
                        | "password"
                        | "secret"
                        | "access_token"
                        | "key"
                        | "auth"
                )
            })
            && url.fragment().is_none()
    })
}

pub fn validate(catalog: &Catalog, collections: &Collections) -> Result<()> {
    if catalog.schema_version != 1 || collections.schema_version != 1 {
        return Err(Error::Config(
            "unsupported schema_version (expected 1)".into(),
        ));
    }
    let mut ids = HashSet::new();
    for source in &catalog.sources {
        if !slug(&source.id) || !ids.insert(source.id.as_str()) {
            return Err(Error::Config(format!(
                "source ID {:?} is invalid or duplicated",
                source.id
            )));
        }
        if source.title.trim().is_empty()
            || source.provider.trim().is_empty()
            || source.description.trim().is_empty()
            || source.coverage.trim().is_empty()
            || source.license.name.trim().is_empty()
            || source.license.redistribution.trim().is_empty()
        {
            return Err(Error::Config(format!(
                "source {} has empty metadata",
                source.id
            )));
        }
        if !https(&source.license.url)
            || source.documentation_urls.is_empty()
            || source.documentation_urls.iter().any(|url| !https(url))
        {
            return Err(Error::Config(format!(
                "source {} needs HTTPS documentation and licence URLs without query credentials",
                source.id
            )));
        }
        if crate::parameters::date(&source.reviewed_on).is_err() {
            return Err(Error::Config(format!(
                "source {} reviewed_on must use YYYY-MM-DD",
                source.id
            )));
        }
        if source.refresh_seconds == Some(0)
            || source.provider_min_interval_seconds == 0
            || source.provider_min_interval_seconds > 3600
            || source.max_response_bytes == 0
            || source.max_response_bytes > 64 * 1024 * 1024
        {
            return Err(Error::Config(format!(
                "source {} has invalid refresh, rate, or response bounds",
                source.id
            )));
        }
        if source.connector == ConnectorKind::DataGovWeather
            && source.provider_min_interval_seconds < 15
        {
            return Err(Error::Config(format!(
                "source {} must allow at least 15 seconds between weather requests",
                source.id
            )));
        }
        let expected = match source.connector {
            ConnectorKind::DataGovWeather => {
                Some(("data-gov-weather", crate::DataClass::GeneralGuidance))
            }
            ConnectorKind::OpenMeteoForecast
            | ConnectorKind::OpenMeteoSingleRun
            | ConnectorKind::OpenMeteoMarine
            | ConnectorKind::OpenMeteoFlood => Some(("open-meteo", crate::DataClass::Forecast)),
            ConnectorKind::OpenMeteoHistory => {
                Some(("open-meteo", crate::DataClass::StitchedForecastHistory))
            }
            ConnectorKind::CdseCatalogue => Some(("copernicus-cdse", crate::DataClass::Imagery)),
            ConnectorKind::CatalogOnly => None,
        };
        if let Some((provider, class)) = expected {
            if source.provider != provider || source.data_class != class {
                return Err(Error::Config(format!(
                    "source {} must use provider {provider} and data class {class:?} for this fixed adapter",
                    source.id
                )));
            }
            if provider == "open-meteo" && !source.license.noncommercial_only {
                return Err(Error::Config(format!(
                    "source {} must retain the free-service noncommercial gate",
                    source.id
                )));
            }
        }
    }
    let mut collection_ids = HashSet::new();
    for collection in &collections.collections {
        if !slug(&collection.id) || !collection_ids.insert(collection.id.as_str()) {
            return Err(Error::Config(format!(
                "collection ID {:?} is invalid or duplicated",
                collection.id
            )));
        }
        let source = catalog
            .sources
            .iter()
            .find(|source| source.id == collection.source_id)
            .ok_or_else(|| {
                Error::Config(format!(
                    "collection {} references unknown source {}",
                    collection.id, collection.source_id
                ))
            })?;
        if collection.description.trim().is_empty() {
            return Err(Error::Config(format!(
                "collection {} needs a description",
                collection.id
            )));
        }
        crate::parameters::prepare(source, collection)?;
    }
    Ok(())
}

pub(crate) fn blocked(source: &crate::Source, collection: &crate::Collection) -> Option<String> {
    if source.connector == ConnectorKind::CatalogOnly {
        Some("Catalog-only source has no acquisition adapter".into())
    } else if source.access != AccessClass::Public {
        Some(format!(
            "Access {:?} requires a separate acquisition arrangement",
            source.access
        ))
    } else if source.license.noncommercial_only && !collection.accept_noncommercial {
        Some("Noncommercial service use requires explicit acceptance".into())
    } else {
        None
    }
}

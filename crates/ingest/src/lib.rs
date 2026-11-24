//! Public source catalogs and bounded raw acquisition.

mod catalog;
mod collector;
mod error;
mod http;
pub mod meteo;
mod models;
mod parameters;
mod planning;
mod rate;
mod response;
mod state;

pub use catalog::validate;
pub use collector::{SyncItem, SyncOptions, SyncReport, sync};
pub use error::{Error, Result};
pub use meteo::{HourlyModelSeries, MultiModelHourlyForecast, parse_open_meteo_hourly};
pub use models::{
    AccessClass, Catalog, Collection, Collections, ConnectorKind, DataClass, License, Source,
};
pub use planning::{CollectionStatus, PlanItem, plan, status};
pub use response::ValidationSummary;

use crate::{Error, Result};
use std::{io::Read, time::Duration};

pub(crate) struct Request<'a> {
    pub url: &'a str,
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
    pub max_bytes: u64,
}

#[derive(Default)]
pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub body_error: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub retry_after: Option<String>,
}

pub(crate) trait Transport {
    fn get(&mut self, request: Request<'_>) -> Result<Response>;
}

pub(crate) struct HttpTransport {
    client: reqwest::blocking::Client,
}

impl HttpTransport {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .user_agent("ARUS/0.1 public-data-collector")
            .build()
            .map_err(|error| {
                Error::Acquisition(format!(
                    "cannot create HTTP client: {error}. Check TLS and network config"
                ))
            })?;
        Ok(Self { client })
    }
}

impl Transport for HttpTransport {
    fn get(&mut self, request: Request<'_>) -> Result<Response> {
        let mut builder = self.client.get(request.url);
        if let Some(value) = request.etag {
            builder = builder.header(reqwest::header::IF_NONE_MATCH, value);
        }
        if let Some(value) = request.last_modified {
            builder = builder.header(reqwest::header::IF_MODIFIED_SINCE, value);
        }
        let response = builder.send().map_err(|error| {
            Error::Acquisition(format!(
                "GET {}: {error}. Check provider availability and network access",
                request.url
            ))
        })?;
        let status = response.status().as_u16();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let etag = header(reqwest::header::ETAG);
        let last_modified = header(reqwest::header::LAST_MODIFIED);
        let retry_after = header(reqwest::header::RETRY_AFTER);
        if response
            .content_length()
            .is_some_and(|size| size > request.max_bytes)
        {
            return Ok(Response {
                status,
                body: Vec::new(),
                body_error: Some(format!("declared body exceeds {} bytes", request.max_bytes)),
                etag,
                last_modified,
                retry_after,
            });
        }
        let mut body = Vec::new();
        let read = response
            .take(request.max_bytes.saturating_add(1))
            .read_to_end(&mut body);
        let mut body_error = read
            .err()
            .map(|error| format!("body read interrupted: {error}"));
        if body.len() as u64 > request.max_bytes {
            body.truncate(request.max_bytes as usize);
            body_error = Some(format!("body exceeds {} bytes", request.max_bytes));
        }
        Ok(Response {
            status,
            body,
            body_error,
            etag,
            last_modified,
            retry_after,
        })
    }
}

use std::fmt;

use futures_util::StreamExt;
use http::{HeaderValue, header};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{
    ApiError, ApiResponse, BodyPreview, Error, ResponseMeta,
    operation::{AuthScope, Operation, RequestEncoding},
};

const JSON_MIME: &str = "application/json";
const SSE_MIME: &str = "text/event-stream";
const DECODE_PREVIEW_BYTES: usize = 8 * 1024;

/// One safely encoded component in an operation route.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PathSegment<'a> {
    Literal(&'static str),
    Parameter(&'a str),
}

impl<'a> PathSegment<'a> {
    pub(crate) const fn literal(value: &'static str) -> Self {
        Self::Literal(value)
    }

    pub(crate) fn parameter(name: &'static str, value: &'a str) -> Result<Self, Error> {
        if value.is_empty() {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not be empty",
            });
        }
        if value == "." || value == ".." {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not be a dot segment",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(Error::InvalidPathParameter {
                name,
                reason: "must not contain control characters",
            });
        }
        Ok(Self::Parameter(value))
    }
}

/// The shared authenticated JSON transport.
pub(crate) struct Transport {
    http: reqwest::Client,
    base_url: Url,
    authorization: HeaderValue,
    organization: Option<HeaderValue>,
    project: Option<HeaderValue>,
    max_json_body_bytes: usize,
    max_error_body_bytes: usize,
}

impl Transport {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        http: reqwest::Client,
        base_url: Url,
        authorization: HeaderValue,
        organization: Option<HeaderValue>,
        project: Option<HeaderValue>,
        max_json_body_bytes: usize,
        max_error_body_bytes: usize,
    ) -> Self {
        Self {
            http,
            base_url,
            authorization,
            organization,
            project,
            max_json_body_bytes,
            max_error_body_bytes,
        }
    }

    pub(crate) const fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub(crate) async fn execute_json<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<ApiResponse<O::Response>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::Json {
            return Err(Error::InvalidConfiguration(
                "JSON decoder used for a non-JSON operation".into(),
            ));
        }
        let response = self.send::<O, Q>(path, query, body).await?;
        self.decode_json(response).await
    }

    pub(crate) async fn execute_optional_json<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<ApiResponse<Option<O::Response>>, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        if O::META.response_mode != crate::operation::ResponseMode::EmptyOrJson {
            return Err(Error::InvalidConfiguration(
                "empty-or-JSON decoder used for an incompatible operation".into(),
            ));
        }
        let response = self.send::<O, Q>(path, query, body).await?;
        self.decode_optional_json(response).await
    }

    /// Sends an operation after validating its static contract. Kept separate
    /// from decoding so the streaming layer can reuse authentication, safe URL
    /// construction, status handling, and metadata extraction.
    pub(crate) async fn send<O, Q>(
        &self,
        path: &[PathSegment<'_>],
        query: Option<&Q>,
        body: Option<&O::Request>,
    ) -> Result<reqwest::Response, Error>
    where
        O: Operation,
        Q: Serialize + ?Sized,
    {
        let meta = &O::META;
        if meta.id.is_empty() || !meta.route.starts_with('/') {
            return Err(Error::InvalidConfiguration(
                "operation metadata has an invalid identifier or route template".into(),
            ));
        }
        if meta.auth != AuthScope::Platform {
            return Err(Error::InvalidConfiguration(
                "operation is not authorized for Platform credentials".into(),
            ));
        }
        match (meta.request_encoding, body) {
            (RequestEncoding::Json, None) => {
                return Err(Error::InvalidConfiguration(
                    "JSON operation is missing its request body".into(),
                ));
            }
            (RequestEncoding::None, Some(_)) => {
                return Err(Error::InvalidConfiguration(
                    "bodyless operation unexpectedly received a request body".into(),
                ));
            }
            (RequestEncoding::None, None) | (RequestEncoding::Json, Some(_)) => {}
        }

        let mut url = self.operation_url(path)?;
        if let Some(query) = query {
            append_query(&mut url, query)?;
        }
        let accept = match meta.response_mode {
            crate::operation::ResponseMode::Json | crate::operation::ResponseMode::EmptyOrJson => {
                JSON_MIME
            }
            crate::operation::ResponseMode::Sse => SSE_MIME,
        };
        let mut request = self
            .http
            .request(meta.method.clone(), url)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::ACCEPT, accept);
        if let Some(organization) = &self.organization {
            request = request.header("OpenAI-Organization", organization.clone());
        }
        if let Some(project) = &self.project {
            request = request.header("OpenAI-Project", project.clone());
        }
        if let Some(body) = body {
            let encoded = serde_json::to_vec(body).map_err(Error::Encode)?;
            request = request
                .header(header::CONTENT_TYPE, JSON_MIME)
                .body(encoded);
        }

        let request = request.build().map_err(Error::from_reqwest)?;
        if !same_origin(request.url(), &self.base_url) {
            return Err(Error::InvalidConfiguration(
                "operation URL escaped the configured authentication origin".into(),
            ));
        }
        let response = self
            .http
            .execute(request)
            .await
            .map_err(Error::from_reqwest)?;

        if meta.success_statuses.contains(&response.status()) {
            Ok(response)
        } else {
            let response_meta = ResponseMeta::from_headers(response.status(), response.headers());
            let (body, truncated) = read_up_to(response, self.max_error_body_bytes).await?;
            Err(ApiError::from_body(response_meta, &body, truncated).into())
        }
    }

    pub(crate) async fn decode_json<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<ApiResponse<T>, Error>
    where
        T: DeserializeOwned,
    {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let decoded = serde_json::from_slice(&body).map_err(|source| Error::Decode {
            source,
            meta_status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
            body: BodyPreview::from_bytes(
                &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                body.len() > DECODE_PREVIEW_BYTES,
            ),
        })?;
        Ok(ApiResponse::new(decoded, meta))
    }

    pub(crate) async fn decode_optional_json<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<ApiResponse<Option<T>>, Error>
    where
        T: DeserializeOwned,
    {
        let meta = ResponseMeta::from_headers(response.status(), response.headers());
        let body = read_success(response, self.max_json_body_bytes, &meta).await?;
        let decoded = if body.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            Some(
                serde_json::from_slice(&body).map_err(|source| Error::Decode {
                    source,
                    meta_status: meta.status(),
                    request_id: meta.request_id().map(Box::<str>::from),
                    body: BodyPreview::from_bytes(
                        &body[..body.len().min(DECODE_PREVIEW_BYTES)],
                        body.len() > DECODE_PREVIEW_BYTES,
                    ),
                })?,
            )
        };
        Ok(ApiResponse::new(decoded, meta))
    }

    fn operation_url(&self, path: &[PathSegment<'_>]) -> Result<Url, Error> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|()| {
                Error::InvalidConfiguration("base URL cannot contain path segments".into())
            })?;
            segments.pop_if_empty();
            for segment in path {
                match segment {
                    PathSegment::Literal(value) => segments.push(value),
                    PathSegment::Parameter(value) => segments.push(value),
                };
            }
        }
        if !same_origin(&url, &self.base_url) {
            return Err(Error::InvalidConfiguration(
                "operation path escaped the configured authentication origin".into(),
            ));
        }
        Ok(url)
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Transport")
            .field("base_url", &self.base_url.as_str())
            .field("authorization", &"[REDACTED]")
            .field(
                "organization",
                &self.organization.as_ref().map(|_| "[REDACTED]"),
            )
            .field("project", &self.project.as_ref().map(|_| "[REDACTED]"))
            .field("max_json_body_bytes", &self.max_json_body_bytes)
            .field("max_error_body_bytes", &self.max_error_body_bytes)
            .finish_non_exhaustive()
    }
}

async fn read_success(
    response: reqwest::Response,
    limit: usize,
    meta: &ResponseMeta,
) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        });
    }
    let (body, truncated) = read_up_to(response, limit).await?;
    if truncated {
        Err(Error::BodyTooLarge {
            limit,
            status: meta.status(),
            request_id: meta.request_id().map(Box::<str>::from),
        })
    } else {
        Ok(body)
    }
}

async fn read_up_to(response: reqwest::Response, limit: usize) -> Result<(Vec<u8>, bool), Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(limit.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(Error::from_reqwest)?;
        let remaining = limit.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn append_query<T>(url: &mut Url, query: &T) -> Result<(), Error>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(query)
        .map_err(|error| Error::EncodeQuery(error.to_string().into()))?;
    let serde_json::Value::Object(fields) = value else {
        return Err(Error::EncodeQuery(
            "operation query must serialize as an object".into(),
        ));
    };
    let mut serializer = url.query_pairs_mut();
    for (name, value) in fields {
        match value {
            serde_json::Value::Null => {
                serializer.append_pair(&name, "");
            }
            serde_json::Value::Bool(value) => {
                serializer.append_pair(&name, if value { "true" } else { "false" });
            }
            serde_json::Value::Number(value) => {
                serializer.append_pair(&name, &value.to_string());
            }
            serde_json::Value::String(value) => {
                serializer.append_pair(&name, &value);
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    let value = query_scalar(&name, value)?;
                    serializer.append_pair(&name, &value);
                }
            }
            serde_json::Value::Object(_) => {
                return Err(Error::EncodeQuery(
                    format!("query field `{name}` requires an unsupported object encoding").into(),
                ));
            }
        }
    }
    Ok(())
}

fn query_scalar(name: &str, value: serde_json::Value) -> Result<String, Error> {
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(Error::EncodeQuery(
            format!("query array field `{name}` contains a non-scalar value").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parameters_are_single_percent_encoded_segments() {
        let base = Url::parse("https://api.openai.com/v1/").expect("test URL");
        let transport = Transport::new(
            reqwest::Client::new(),
            base,
            HeaderValue::from_static("Bearer test-placeholder-key"),
            None,
            None,
            1024,
            1024,
        );
        let path = [
            PathSegment::literal("responses"),
            PathSegment::parameter("response_id", "resp/a b").expect("valid ID"),
        ];
        let url = transport.operation_url(&path).expect("operation URL");
        assert_eq!(
            url.as_str(),
            "https://api.openai.com/v1/responses/resp%2Fa%20b"
        );
    }

    #[test]
    fn dot_segments_are_rejected() {
        assert!(PathSegment::parameter("response_id", "..").is_err());
    }
}

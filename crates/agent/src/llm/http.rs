//! One HTTPS POST helper, shared by the three providers.
//!
//! This is the same stack `crates/deploy` uses for GitHub: `hyper` +
//! `hyper-rustls` with the ring provider and webpki roots. Reusing it keeps the
//! workspace on a single TLS stack; adding `reqwest` would have pulled a second
//! one in for a handful of requests.

use std::time::Duration;

use http::{Method, Request, StatusCode, Uri};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;

use crate::error::AgentError;

type Connector = hyper_rustls::HttpsConnector<HttpConnector>;
type HyperClient = Client<Connector, Full<Bytes>>;

/// Total tries per request, including the first. Two is deliberate: a provider
/// that is down should fail the run quickly, not burn the wall-clock budget.
const MAX_ATTEMPTS: u32 = 2;

/// A tiny JSON-over-HTTPS client.
pub struct HttpClient {
    client: HyperClient,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            // `https_or_http` keeps a local Ollama (`http://127.0.0.1:11434`)
            // working without a second client type.
            .https_or_http()
            .enable_http1()
            .build();
        Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
        }
    }

    /// POST `body` as JSON and parse the JSON response.
    ///
    /// **No provider calls this any more.** Since every provider streams, they
    /// all go through [`HttpClient::post_stream`], which is the single parse
    /// path: `complete` delegates to `stream` with a null sink. This method
    /// stays as the transport for a future non-chat endpoint — do not build a
    /// second response parser on top of it, because two parsers drift.
    ///
    /// A non-2xx status is returned as `Provider`, carrying a bounded slice of
    /// the response body — never the request, which holds the API key.
    ///
    /// Rate limits and 5xx responses are retried once. Chat completions are
    /// safe to repeat: the worst case is paying for the same tokens twice,
    /// whereas failing the user's whole build over a transient 503 is worse.
    #[allow(dead_code)]
    pub async fn post_json(
        &self,
        provider: &'static str,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, AgentError> {
        let mut attempt = 1;
        loop {
            match self.attempt(provider, url, headers, body, timeout).await {
                Ok(value) => return Ok(value),
                Err(failure) if failure.retryable && attempt < MAX_ATTEMPTS => {
                    tracing::debug!(
                        provider,
                        attempt,
                        "retrying after a transient provider error"
                    );
                    tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
                    attempt += 1;
                }
                Err(failure) => return Err(failure.error),
            }
        }
    }

    /// POST `body` as JSON and read the response as a line stream.
    ///
    /// Used for SSE (`text/event-stream`) and NDJSON, which are both line
    /// protocols. The returned reader yields lines as they arrive, so the
    /// caller can show text before the response is complete.
    ///
    /// Only failures that happen *before* the first line are retried: once a
    /// body is being consumed a second attempt would duplicate text that has
    /// already been displayed.
    pub async fn post_stream(
        &self,
        provider: &'static str,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        idle: Duration,
    ) -> Result<LineReader, AgentError> {
        let mut attempt = 1;
        loop {
            match self.open_stream(provider, url, headers, body, idle).await {
                Ok(reader) => return Ok(reader),
                Err(failure) if failure.retryable && attempt < MAX_ATTEMPTS => {
                    tracing::debug!(
                        provider,
                        attempt,
                        "retrying a stream that failed before its first line"
                    );
                    tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
                    attempt += 1;
                }
                Err(failure) => return Err(failure.error),
            }
        }
    }

    /// Open the stream and hand back the body. Nothing is consumed here, which
    /// is what makes the retry in `post_stream` safe.
    async fn open_stream(
        &self,
        provider: &'static str,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        idle: Duration,
    ) -> Result<LineReader, AttemptError> {
        let uri: Uri = url
            .parse()
            .map_err(|error: http::uri::InvalidUri| AttemptError {
                error: AgentError::Internal(format!("invalid provider URL: {error}")),
                retryable: false,
            })?;
        let payload = serde_json::to_vec(body).map_err(|error| AttemptError {
            error: AgentError::from(error),
            retryable: false,
        })?;
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json")
            // Ollama ignores it; the two hosted APIs use it to pick a mode.
            .header("accept", "text/event-stream, application/x-ndjson");
        for (name, value) in headers {
            builder = builder.header(*name, value);
        }
        let request = builder
            .body(Full::new(Bytes::from(payload)))
            .map_err(|error| AttemptError {
                error: AgentError::Internal(format!("could not build request: {error}")),
                retryable: false,
            })?;

        // The timeout covers the response *head* only. A long generation must
        // not be cut off here; the per-read idle timeout in `LineReader` is what
        // bounds a stalled stream.
        let response = tokio::time::timeout(idle, self.client.request(request))
            .await
            .map_err(|_| AttemptError {
                error: AgentError::Timeout(idle.as_secs()),
                retryable: false,
            })?
            .map_err(|error| AttemptError {
                error: AgentError::Provider {
                    provider,
                    message: error.to_string(),
                },
                retryable: false,
            })?;

        let status = response.status();
        if !status.is_success() {
            // A refused request is small and JSON; read it so the message names
            // the real problem instead of "status 401".
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|error| AttemptError {
                    error: AgentError::Provider {
                        provider,
                        message: error.to_string(),
                    },
                    retryable: true,
                })?
                .to_bytes();
            let text = String::from_utf8_lossy(&bytes);
            let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Err(AttemptError {
                error: AgentError::Provider {
                    provider,
                    message: format!("HTTP {}: {}", status.as_u16(), error_message(&parsed)),
                },
                retryable: is_retryable(status),
            });
        }

        let body = response
            .into_body()
            .map_err(move |error| AgentError::Provider {
                provider,
                message: error.to_string(),
            });
        Ok(LineReader {
            body: UnsyncBoxBody::new(body),
            buffer: Vec::new(),
            idle,
        })
    }

    /// One request. Failures carry whether a retry could plausibly help.
    async fn attempt(
        &self,
        provider: &'static str,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, AttemptError> {
        let uri: Uri = url
            .parse()
            .map_err(|error: http::uri::InvalidUri| AttemptError {
                error: AgentError::Internal(format!("invalid provider URL: {error}")),
                retryable: false,
            })?;

        let payload = serde_json::to_vec(body).map_err(|error| AttemptError {
            error: AgentError::from(error),
            retryable: false,
        })?;
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json")
            .header("accept", "application/json");
        for (name, value) in headers {
            builder = builder.header(*name, value);
        }
        let request = builder
            .body(Full::new(Bytes::from(payload)))
            .map_err(|error| AttemptError {
                error: AgentError::Internal(format!("could not build request: {error}")),
                retryable: false,
            })?;

        let response = tokio::time::timeout(timeout, self.client.request(request))
            .await
            .map_err(|_| AttemptError {
                error: AgentError::Timeout(timeout.as_secs()),
                // A timeout may be a slow model rather than a broken network;
                // retrying doubles the wall clock the user waits. Do not.
                retryable: false,
            })?
            .map_err(|error| AttemptError {
                error: AgentError::Provider {
                    provider,
                    message: error.to_string(),
                },
                retryable: false,
            })?;

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| AttemptError {
                error: AgentError::Provider {
                    provider,
                    message: error.to_string(),
                },
                retryable: true,
            })?
            .to_bytes();

        let text = String::from_utf8_lossy(&bytes);
        let parsed: Value = serde_json::from_str(&text).map_err(|_| AttemptError {
            error: AgentError::BadResponse {
                provider,
                message: format!("status {status} with a non-JSON body"),
            },
            retryable: is_retryable(status),
        })?;

        if !status.is_success() {
            return Err(AttemptError {
                error: AgentError::Provider {
                    provider,
                    message: format!("HTTP {}: {}", status.as_u16(), error_message(&parsed)),
                },
                retryable: is_retryable(status),
            });
        }
        Ok(parsed)
    }
}

/// One failed request plus whether a second attempt could help.
struct AttemptError {
    error: AgentError,
    retryable: bool,
}

/// A response body read one line at a time.
///
/// A network chunk can end anywhere — including inside a JSON string — so bytes
/// are buffered until their line terminator arrives, and only then decoded. A
/// line boundary is always a UTF-8 boundary, which is why decoding per line is
/// safe where decoding per chunk would not be.
///
/// `idle` bounds a stalled stream: a provider that stops sending for that long
/// is reported as a timeout instead of hanging the run until the wall clock
/// runs out.
pub struct LineReader {
    body: UnsyncBoxBody<Bytes, AgentError>,
    buffer: Vec<u8>,
    idle: Duration,
}

impl LineReader {
    /// The next line, without its terminator. `None` at end of body.
    pub async fn next_line(&mut self) -> Result<Option<String>, AgentError> {
        loop {
            if let Some(line) = take_line(&mut self.buffer) {
                return Ok(Some(line));
            }

            let frame = tokio::time::timeout(self.idle, self.body.frame())
                .await
                .map_err(|_| AgentError::Timeout(self.idle.as_secs()))?;
            match frame {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.buffer.extend_from_slice(&data);
                    }
                }
                Some(Err(error)) => return Err(error),
                None => {
                    if self.buffer.is_empty() {
                        return Ok(None);
                    }
                    // A last line without a terminator is still a line.
                    let rest = std::mem::take(&mut self.buffer);
                    return Ok(Some(String::from_utf8_lossy(&rest).into_owned()));
                }
            }
        }
    }
}

/// Remove one complete line from `buffer`, if it holds a terminator.
///
/// Split out so the chunk-boundary case — a frame arriving in two reads — is
/// testable without an HTTP body.
fn take_line(buffer: &mut Vec<u8>) -> Option<String> {
    let index = buffer.iter().position(|byte| *byte == b'\n')?;
    let mut line: Vec<u8> = buffer.drain(..=index).collect();
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Some(String::from_utf8_lossy(&line).into_owned())
}

/// Best-effort extraction of a provider's error message.
fn error_message(body: &Value) -> String {
    let message = body
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .or_else(|| body.get("message").and_then(Value::as_str))
        .unwrap_or("request failed");
    // Provider errors can echo the request; bound what reaches a log or the UI.
    message.chars().take(300).collect()
}

/// Whether a status is worth retrying at the call site.
pub fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_errors_are_extracted_and_bounded() {
        let body = json!({ "error": { "message": "x".repeat(1000) } });
        assert_eq!(error_message(&body).len(), 300);
        assert_eq!(error_message(&json!({"error": "plain"})), "plain");
    }

    #[test]
    fn retryable_statuses_are_rate_limits_and_5xx() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn a_line_split_across_chunks_is_only_emitted_once_complete() {
        // The chunk boundary falls inside a JSON string, which is the case that
        // a per-chunk parser gets wrong.
        let mut buffer = b"data: {\"text\":\"hel".to_vec();
        assert_eq!(take_line(&mut buffer), None, "no terminator yet");

        buffer.extend_from_slice(b"lo\"}\n");
        assert_eq!(
            take_line(&mut buffer).as_deref(),
            Some("data: {\"text\":\"hello\"}")
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn carriage_returns_are_stripped() {
        let mut buffer = b"data: x\r\ndata: y\n".to_vec();
        assert_eq!(take_line(&mut buffer).as_deref(), Some("data: x"));
        assert_eq!(take_line(&mut buffer).as_deref(), Some("data: y"));
        assert_eq!(take_line(&mut buffer), None);
    }

    #[test]
    fn a_multibyte_character_survives_the_split() {
        // The emoji is four bytes; splitting the *line* is safe because the
        // terminator is ASCII, so a line boundary is always a char boundary.
        let mut buffer = Vec::new();
        buffer.extend_from_slice("data: 🎉".as_bytes());
        assert_eq!(take_line(&mut buffer), None);
        buffer.push(b'\n');
        assert_eq!(take_line(&mut buffer).as_deref(), Some("data: 🎉"));
    }
}

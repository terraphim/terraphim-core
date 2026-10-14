//! Hardened remote fetching for thesaurus and artefact loaders.
//!
//! Centralises HTTP request policy so remote loaders never treat a response
//! body as acceptable merely because a request completed:
//!
//! - Only documented successful statuses (2xx) are accepted; error bodies are
//!   rejected with typed errors before any parsing occurs.
//! - HTTPS is required by default. Plain HTTP is only permitted when the
//!   caller explicitly opts in (`RemoteFetchPolicy::allow_http`), which is
//!   intended for local development and tests.
//! - Redirects are handled manually: a small maximum, every destination is
//!   revalidated, HTTPS-to-HTTP downgrades are rejected and sensitive headers
//!   are stripped when a redirect crosses origins.
//! - Both the declared `Content-Length` and the streamed byte count are
//!   checked against an explicit limit.
//! - Connect, per-request and total deadlines are enforced.

use std::time::{Duration, Instant};

use crate::{Result, TerraphimAutomataError};

/// Default maximum response body size (64 MiB), aligned with the artefact
/// decoded-size budgets established for shared loaders.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

/// Default maximum number of redirect hops permitted for one fetch.
pub const DEFAULT_MAX_REDIRECTS: usize = 5;

/// Default total deadline for a fetch, including all redirects.
pub const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Default connect deadline for each individual request.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Policy governing a remote fetch.
#[derive(Debug, Clone)]
pub struct RemoteFetchPolicy {
    /// Permit plain HTTP URLs. Off by default: production remote sources must
    /// use HTTPS. Intended only for explicit, narrowly scoped local
    /// development and testing.
    pub allow_http: bool,
    /// Maximum number of redirect hops before failing.
    pub max_redirects: usize,
    /// Maximum accepted response body size, checked both against the declared
    /// `Content-Length` and the streamed byte count.
    pub max_response_bytes: u64,
    /// Connect deadline per request.
    pub connect_timeout: Duration,
    /// Total deadline for the whole fetch, across all redirects.
    pub total_timeout: Duration,
}

impl Default for RemoteFetchPolicy {
    fn default() -> Self {
        Self {
            allow_http: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
        }
    }
}

impl RemoteFetchPolicy {
    /// Policy that permits plain HTTP for local development and tests.
    pub fn allow_http_local() -> Self {
        Self {
            allow_http: true,
            ..Self::default()
        }
    }

    fn scheme_allowed(&self, scheme: &str) -> bool {
        match scheme {
            "https" => true,
            "http" => self.allow_http,
            _ => false,
        }
    }
}

/// Headers that must never be forwarded to a different origin after a
/// redirect.
const SENSITIVE_HEADERS: [&str; 6] = [
    "authorization",
    "cookie",
    "x-api-key",
    "proxy-authorization",
    "www-authenticate",
    "x-terraphim-token",
];

/// Validate a redirect hop against the policy.
///
/// Returns the resolved destination URL. Rejects disallowed schemes,
/// HTTPS-to-HTTP downgrades and malformed `Location` values.
pub fn validate_redirect(
    from: &reqwest::Url,
    location: &str,
    policy: &RemoteFetchPolicy,
) -> Result<reqwest::Url> {
    let destination = from
        .join(location)
        .map_err(|e| TerraphimAutomataError::InvalidRedirect {
            url: from.to_string(),
            reason: format!("malformed Location header {location:?}: {e}"),
        })?;

    match destination.scheme() {
        "https" => Ok(destination),
        "http" if policy.allow_http => {
            if from.scheme() == "https" {
                Err(TerraphimAutomataError::InsecureRedirect {
                    from: from.to_string(),
                    to: destination.to_string(),
                })
            } else {
                Ok(destination)
            }
        }
        scheme => Err(TerraphimAutomataError::SchemeNotAllowed {
            url: destination.to_string(),
            scheme: scheme.to_string(),
        }),
    }
}

/// Whether two URLs differ in origin (scheme, host, effective port).
fn is_cross_origin(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    let origin = |u: &reqwest::Url| {
        (
            u.scheme().to_string(),
            u.host_str().map(|h| h.to_lowercase()),
            u.port_or_known_default(),
        )
    };
    origin(from) != origin(to)
}

fn fetch_error(url: &reqwest::Url, e: reqwest::Error) -> TerraphimAutomataError {
    TerraphimAutomataError::HttpTransport(format!("Failed to fetch {url}: {e}"))
}

/// Fetch `url` as bytes under the given policy.
///
/// The request never follows redirects automatically; every hop is validated
/// by [`validate_redirect`]. Non-success statuses yield typed errors and the
/// error body is never returned for parsing. The declared `Content-Length`
/// and the streamed byte count are both bounded by
/// `policy.max_response_bytes`.
pub async fn fetch_bytes(
    url: &str,
    extra_headers: &[(String, String)],
    policy: &RemoteFetchPolicy,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    fetch_bytes_inner(url, extra_headers, policy, started, 0).await
}

async fn fetch_bytes_inner(
    url: &str,
    extra_headers: &[(String, String)],
    policy: &RemoteFetchPolicy,
    started: Instant,
    hops_used: usize,
) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(policy.connect_timeout)
        .timeout(policy.total_timeout)
        .user_agent("Terraphim-Automata/1.0")
        .build()
        .map_err(|e| TerraphimAutomataError::HttpTransport(format!("client build failed: {e}")))?;

    let mut hops_used = hops_used;
    let mut current: reqwest::Url =
        url.parse()
            .map_err(|_| TerraphimAutomataError::SchemeNotAllowed {
                url: url.to_string(),
                scheme: "(unparsable)".to_string(),
            })?;

    loop {
        if !policy.scheme_allowed(current.scheme()) {
            return Err(TerraphimAutomataError::SchemeNotAllowed {
                url: current.to_string(),
                scheme: current.scheme().to_string(),
            });
        }

        let mut request = client
            .get(current.clone())
            .header("Accept", "application/json");
        for (name, value) in extra_headers {
            request = request.header(name, value);
        }

        let response = request.send().await.map_err(|e| fetch_error(&current, e))?;

        let status = response.status();
        if status.is_redirection() {
            if hops_used >= policy.max_redirects {
                return Err(TerraphimAutomataError::TooManyRedirects {
                    url: current.to_string(),
                    max: policy.max_redirects,
                });
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| TerraphimAutomataError::InvalidRedirect {
                    url: current.to_string(),
                    reason: "redirect response without a Location header".to_string(),
                })?
                .to_string();

            let destination = validate_redirect(&current, &location, policy)?;
            if is_cross_origin(&current, &destination) {
                // Drop sensitive headers before continuing at the new origin.
                let retained: Vec<(String, String)> = extra_headers
                    .iter()
                    .filter(|(name, _)| {
                        !SENSITIVE_HEADERS.contains(&name.to_ascii_lowercase().as_str())
                    })
                    .cloned()
                    .collect();
                return Box::pin(fetch_bytes_inner(
                    destination.as_str(),
                    &retained,
                    policy,
                    started,
                    hops_used + 1,
                ))
                .await;
            }
            hops_used += 1;
            current = destination;
            continue;
        }

        if !status.is_success() {
            return Err(TerraphimAutomataError::HttpStatus {
                url: current.to_string(),
                status: status.as_u16(),
            });
        }

        if let Some(length) = response.content_length() {
            if length > policy.max_response_bytes {
                return Err(TerraphimAutomataError::BodyTooLarge {
                    limit: policy.max_response_bytes,
                });
            }
        }

        // Stream with an explicit byte counter so chunked responses, missing
        // or lying Content-Length values cannot exceed the budget.
        let mut bytes: Vec<u8> = Vec::new();
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| fetch_error(&current, e))?
        {
            if bytes.len() as u64 + chunk.len() as u64 > policy.max_response_bytes {
                return Err(TerraphimAutomataError::BodyTooLarge {
                    limit: policy.max_response_bytes,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
}

/// Fetch `url` as UTF-8 text under the given policy.
pub async fn fetch_text(
    url: &str,
    extra_headers: &[(String, String)],
    policy: &RemoteFetchPolicy,
) -> Result<String> {
    let bytes = fetch_bytes(url, extra_headers, policy).await?;
    String::from_utf8(bytes).map_err(|e| {
        TerraphimAutomataError::InvalidThesaurus(format!(
            "Remote response from {url} was not valid UTF-8: {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_requires_https() {
        let policy = RemoteFetchPolicy::default();
        assert!(!policy.allow_http);
    }

    #[test]
    fn validate_redirect_rejects_https_to_http_downgrade() {
        let policy = RemoteFetchPolicy::allow_http_local();
        let from: reqwest::Url = "https://example.com/a".parse().unwrap();
        let result = validate_redirect(&from, "http://example.com/b", &policy);
        assert!(matches!(
            result,
            Err(TerraphimAutomataError::InsecureRedirect { .. })
        ));
    }

    #[test]
    fn validate_redirect_rejects_disallowed_scheme() {
        let policy = RemoteFetchPolicy::default();
        let from: reqwest::Url = "https://example.com/a".parse().unwrap();
        let result = validate_redirect(&from, "http://example.com/b", &policy);
        assert!(matches!(
            result,
            Err(TerraphimAutomataError::SchemeNotAllowed { .. })
        ));
        let result = validate_redirect(&from, "ftp://example.com/b", &policy);
        assert!(matches!(
            result,
            Err(TerraphimAutomataError::SchemeNotAllowed { .. })
        ));
    }

    #[test]
    fn validate_redirect_resolves_relative_locations() {
        let policy = RemoteFetchPolicy::allow_http_local();
        let from: reqwest::Url = "http://localhost:1/dir/page".parse().unwrap();
        let dest = validate_redirect(&from, "../target.json", &policy).unwrap();
        assert_eq!(dest.as_str(), "http://localhost:1/target.json");
    }

    #[test]
    fn validate_redirect_rejects_malformed_location() {
        let policy = RemoteFetchPolicy::allow_http_local();
        let from: reqwest::Url = "http://localhost:1/a".parse().unwrap();
        let result = validate_redirect(&from, "http://[::1:99999", &policy);
        assert!(result.is_err());
    }

    #[test]
    fn cross_origin_detection_includes_port() {
        let a: reqwest::Url = "http://127.0.0.1:9001/x".parse().unwrap();
        let b: reqwest::Url = "http://127.0.0.1:9002/x".parse().unwrap();
        let c: reqwest::Url = "http://127.0.0.1:9001/y".parse().unwrap();
        assert!(is_cross_origin(&a, &b));
        assert!(!is_cross_origin(&a, &c));
    }
}

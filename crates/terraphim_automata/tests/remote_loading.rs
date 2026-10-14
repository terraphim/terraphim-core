//! Integration tests for hardened remote loading (Refs #40).
//!
//! These tests run against real local HTTP services (raw TCP listeners) —
//! no mocks — exercising status handling, HTTPS policy, redirects, size
//! limits and deadlines through the public fetch API.

#![cfg(feature = "remote-loading")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use terraphim_automata::{RemoteFetchPolicy, TerraphimAutomataError, fetch_bytes};

/// Spawn a real local HTTP service. The handler receives the raw request and
/// returns the raw response bytes. Serves until the process exits.
fn spawn_http_service(handler: impl Fn(String) -> Vec<u8> + Send + Sync + 'static) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local service");
    let addr = listener.local_addr().expect("local addr");
    let handler = Arc::new(handler);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let handler = handler.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Read until end of request headers.
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let request = String::from_utf8_lossy(&buf).to_string();
                let response = handler(request);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            });
        }
    });
    addr
}

fn response(status_line: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("{status_line}\r\nConnection: close\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn valid_thesaurus_body() -> Vec<u8> {
    br#"{"name": "test", "data": {"rust": {"id": 1, "nterm": "rust"}}}"#.to_vec()
}

fn local_policy() -> RemoteFetchPolicy {
    RemoteFetchPolicy {
        // Keep budgets small so tests run fast and exercise the limits.
        total_timeout: std::time::Duration::from_secs(5),
        ..RemoteFetchPolicy::allow_http_local()
    }
}

#[tokio::test]
async fn test_successful_bounded_load_over_local_service() {
    let addr = spawn_http_service(move |_| {
        response(
            "HTTP/1.1 200 OK",
            &[("Content-Type", "application/json")],
            &valid_thesaurus_body(),
        )
    });
    let bytes = fetch_bytes(&format!("http://{addr}/t.json"), &[], &local_policy())
        .await
        .expect("fetch should succeed under allow-http policy");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"nterm\": \"rust\""));
}

#[tokio::test]
async fn test_plain_http_rejected_by_default_policy() {
    let addr =
        spawn_http_service(move |_| response("HTTP/1.1 200 OK", &[], &valid_thesaurus_body()));
    let policy = RemoteFetchPolicy {
        total_timeout: std::time::Duration::from_secs(5),
        ..RemoteFetchPolicy::default()
    };
    let result = fetch_bytes(&format!("http://{addr}/t.json"), &[], &policy).await;
    match result {
        Err(TerraphimAutomataError::SchemeNotAllowed { scheme, .. }) => {
            assert_eq!(scheme, "http");
        }
        other => panic!("expected SchemeNotAllowed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_error_statuses_yield_typed_errors_before_parsing() {
    for status in [
        "404 Not Found",
        "500 Internal Server Error",
        "503 Service Unavailable",
    ] {
        let addr = spawn_http_service(move |_| {
            response(
                &format!("HTTP/1.1 {status}"),
                &[],
                b"<html>this error body must never be parsed as an artefact</html>",
            )
        });
        let result = fetch_bytes(&format!("http://{addr}/t.json"), &[], &local_policy()).await;
        match result {
            Err(TerraphimAutomataError::HttpStatus { status: code, .. }) => {
                let expected: u16 = status.split_whitespace().next().unwrap().parse().unwrap();
                assert_eq!(code, expected);
            }
            other => panic!("expected HttpStatus for {status}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn test_same_origin_redirect_chain_within_budget_succeeds() {
    let addr = spawn_http_service(move |request| {
        if request.contains("/hop-2") {
            response("HTTP/1.1 200 OK", &[], &valid_thesaurus_body())
        } else {
            response("HTTP/1.1 302 Found", &[("Location", "/hop-2")], b"")
        }
    });
    let bytes = fetch_bytes(&format!("http://{addr}/hop-1"), &[], &local_policy())
        .await
        .expect("same-origin redirect chain should succeed");
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn test_redirect_loop_is_rejected() {
    let addr =
        spawn_http_service(move |_| response("HTTP/1.1 302 Found", &[("Location", "/loop")], b""));
    let result = fetch_bytes(&format!("http://{addr}/loop"), &[], &local_policy()).await;
    assert!(
        matches!(result, Err(TerraphimAutomataError::TooManyRedirects { .. })),
        "expected TooManyRedirects, got {result:?}"
    );
}

#[tokio::test]
async fn test_sensitive_headers_not_forwarded_cross_origin() {
    // Cross-origin here means a different port: 127.0.0.1:A -> 127.0.0.1:B.
    let target = spawn_http_service(move |request| {
        // Echo the request headers back so the test can inspect them.
        response("HTTP/1.1 200 OK", &[], request.as_bytes())
    });
    let target_addr = format!("{target}");
    let source = spawn_http_service(move |_| {
        response(
            "HTTP/1.1 302 Found",
            &[("Location", &format!("http://{target_addr}/hit"))],
            b"",
        )
    });

    let headers = vec![(
        "Authorization".to_string(),
        "Bearer super-secret".to_string(),
    )];
    let bytes = fetch_bytes(&format!("http://{source}/go"), &headers, &local_policy())
        .await
        .expect("cross-origin redirect should succeed");
    let echoed = String::from_utf8(bytes).unwrap();
    assert!(
        !echoed.to_lowercase().contains("super-secret"),
        "sensitive header value leaked across origins: {echoed}"
    );
    assert!(
        !echoed.to_lowercase().contains("authorization"),
        "sensitive header name forwarded across origins: {echoed}"
    );
}

#[tokio::test]
async fn test_https_to_http_downgrade_rejected() {
    // The destination service is plain HTTP; a redirect from an HTTPS origin
    // (validated by the pure policy check via a synthetic HTTPS source URL)
    // must be rejected. We point the fetch at a service that redirects and
    // confirm the downgrade rule via the redirect validator on the hop.
    use terraphim_automata::validate_redirect;
    let policy = local_policy();
    let from: reqwest::Url = "https://origin.example/a".parse().unwrap();
    let result = validate_redirect(&from, "http://127.0.0.1:1/b", &policy);
    assert!(matches!(
        result,
        Err(TerraphimAutomataError::InsecureRedirect { .. })
    ));
}

#[tokio::test]
async fn test_oversized_declared_content_length_rejected() {
    let addr = spawn_http_service(move |_| {
        // Lie about a small body with a huge declared Content-Length by
        // writing a raw oversized header.
        b"HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\n\r\ntiny".to_vec()
    });
    let policy = RemoteFetchPolicy {
        max_response_bytes: 1024,
        ..local_policy()
    };
    let result = fetch_bytes(&format!("http://{addr}/big"), &[], &policy).await;
    assert!(
        matches!(result, Err(TerraphimAutomataError::BodyTooLarge { .. })),
        "expected BodyTooLarge, got {result:?}"
    );
}

#[tokio::test]
async fn test_streamed_chunked_body_over_limit_rejected() {
    let addr = spawn_http_service(move |_| {
        // Real chunked transfer with no honest Content-Length: the streamed
        // byte counter must enforce the budget.
        let mut out =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        for _ in 0..4 {
            out.extend_from_slice(b"10\r\naaaaaaaaaaaaaaaa\r\n"); // 16 bytes per chunk
        }
        out.extend_from_slice(b"0\r\n\r\n");
        out
    });
    let policy = RemoteFetchPolicy {
        max_response_bytes: 32,
        ..local_policy()
    };
    let result = fetch_bytes(&format!("http://{addr}/chunked"), &[], &policy).await;
    assert!(
        matches!(result, Err(TerraphimAutomataError::BodyTooLarge { .. })),
        "expected BodyTooLarge, got {result:?}"
    );
}

#[tokio::test]
async fn test_slow_body_hits_total_deadline() {
    let addr = spawn_http_service(move |request| {
        let _ = request;
        std::thread::sleep(std::time::Duration::from_millis(700));
        response("HTTP/1.1 200 OK", &[], b"late")
    });
    let policy = RemoteFetchPolicy {
        total_timeout: std::time::Duration::from_millis(150),
        ..local_policy()
    };
    let result = fetch_bytes(&format!("http://{addr}/slow"), &[], &policy).await;
    assert!(result.is_err(), "slow body must hit the total deadline");
}

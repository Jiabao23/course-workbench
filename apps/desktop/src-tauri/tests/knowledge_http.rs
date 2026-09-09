//! Real loopback HTTP, mocked provider replies. No cloud requests or user keys.
use course_core::Segment;
use course_workbench_lib::{knowledge::OpenAiCompatible, settings::AppSettings};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

fn server(
    status: u16,
    body: String,
    delay: Duration,
    chunked: bool,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/v1", listener.local_addr().unwrap());
    let (send, receive) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let size = socket.read(&mut buf).unwrap();
            if size == 0 {
                break;
            }
            request.extend_from_slice(&buf[..size]);
            if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        let _ = send.send(String::from_utf8(request).unwrap());
        thread::sleep(delay);
        let response = if chunked {
            format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",body.len(),body)
        } else {
            format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body)
        };
        let _ = socket.write_all(response.as_bytes());
    });
    (address, receive, handle)
}

fn segments() -> Vec<Segment> {
    vec![Segment {
        id: "s1".into(),
        start_ms: 1000,
        end_ms: 2000,
        text: "协议规定通信双方的行为。".into(),
    }]
}
fn response(value: Value) -> String {
    json!({"choices":[{"message":{"content":value.to_string()}}]}).to_string()
}

#[test]
fn successful_http_request_sends_only_selected_text_and_binds_real_citations() {
    let (url, request, handle) = server(
        200,
        response(json!({"content":"协议约定通信行为。[引用:s1]","citations":["s1"]})),
        Duration::ZERO,
        false,
    );
    let settings = AppSettings {
        llm_base_url: url,
        llm_model: "mock-model".into(),
        cookie_file: "PRIVATE_COOKIE_PATH".into(),
        ..Default::default()
    };
    let result = OpenAiCompatible
        .generate_with_timeout(
            &settings,
            Some("test-only-key"),
            "summary",
            None,
            &segments(),
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(result.1[0].segment_id, "s1");
    assert_eq!(result.1[0].start_ms, 1000);
    let raw = request.recv().unwrap();
    assert!(raw.starts_with("POST /v1/chat/completions "));
    assert!(raw.contains("协议规定通信双方的行为"));
    assert!(!raw.contains("PRIVATE_COOKIE_PATH"));
    assert!(!raw.contains("audioPath"));
    handle.join().unwrap();
}

#[test]
fn unknown_citations_are_rejected_and_insufficient_evidence_is_explicit() {
    for (body, valid) in [
        (
            json!({"content":"编造。[引用:unknown]","citations":["unknown"]}),
            false,
        ),
        (
            json!({"content":"现有材料不足以回答这个问题。","citations":[],"insufficientEvidence":true}),
            true,
        ),
    ] {
        let (url, _, handle) = server(200, response(body), Duration::ZERO, false);
        let settings = AppSettings {
            llm_base_url: url,
            llm_model: "mock".into(),
            ..Default::default()
        };
        let result = OpenAiCompatible.generate_with_timeout(
            &settings,
            None,
            "answer",
            Some("资料未涉及的问题"),
            &segments(),
            Duration::from_secs(3),
        );
        assert_eq!(result.is_ok(), valid);
        if valid {
            let (content, cites) = result.unwrap();
            assert!(content.contains("不足"));
            assert!(cites.is_empty());
        }
        handle.join().unwrap();
    }
}

#[test]
fn auth_rate_limit_timeout_and_invalid_json_have_bounded_failures() {
    for (status, body, delay, expected) in [
        (401, "{}", 0, "密钥"),
        (429, "{}", 0, "限流"),
        (200, "bad-json", 0, "JSON"),
        (200, "{}", 400, "超时"),
    ] {
        let (url, _, handle) = server(status, body.into(), Duration::from_millis(delay), false);
        let settings = AppSettings {
            llm_base_url: url,
            llm_model: "mock".into(),
            ..Default::default()
        };
        let result = OpenAiCompatible.generate_with_timeout(
            &settings,
            None,
            "summary",
            None,
            &segments(),
            Duration::from_millis(200),
        );
        assert!(format!("{:#}", result.unwrap_err()).contains(expected));
        handle.join().unwrap();
    }
}

#[test]
fn response_limit_applies_even_without_content_length() {
    let (url, _, handle) = server(
        200,
        "x".repeat(8 * 1024 * 1024 + 4096),
        Duration::ZERO,
        true,
    );
    let settings = AppSettings {
        llm_base_url: url,
        llm_model: "mock".into(),
        ..Default::default()
    };
    let error = OpenAiCompatible
        .generate_with_timeout(
            &settings,
            None,
            "summary",
            None,
            &segments(),
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert!(error.to_string().contains("响应过大"));
    handle.join().unwrap();
}

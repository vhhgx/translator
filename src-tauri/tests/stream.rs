//! Verifies the SSE streaming path against the live llama-server.

use std::net::ToSocketAddrs;

use hy_mt2_translator_lib::client::{self, TranslateRequest};
use hy_mt2_translator_lib::config::Backend;
use hy_mt2_translator_lib::prompt::Mode;

fn base_url() -> String {
    std::env::var("HYMT_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn backend() -> Backend {
    Backend {
        id: "local".into(),
        name: "local".into(),
        base_url: base_url(),
        model: "hy-mt2".into(),
        has_api_key: false,
    }
}

fn online() -> bool {
    let hp = base_url()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    match hp.to_socket_addrs() {
        Ok(mut a) => match a.next() {
            Some(addr) => std::net::TcpStream::connect_timeout(
                &addr,
                std::time::Duration::from_secs(2),
            )
            .is_ok(),
            None => false,
        },
        Err(_) => false,
    }
}

#[tokio::test]
async fn streaming_yields_incremental_deltas() {
    if !online() {
        eprintln!("skipping: no model service");
        return;
    }
    let req = TranslateRequest {
        text: "人工智能正在深刻改变软件开发的方式。".into(),
        target_lang: "英语".into(),
        mode: Mode::Basic,
        terms: vec![],
        auto_filter_terms: false,
        style: String::new(),
        format_type: String::new(),
        context: String::new(),
        ignore_glossary: true,
    };

    let mut chunks = 0usize;
    let mut first_at: Option<std::time::Instant> = None;
    let t0 = std::time::Instant::now();
    let mut last = t0;
    let mut max_gap = std::time::Duration::ZERO;

    let full = client::translate_stream(&backend(), None, 0.1, 120, &req, |_d| {
        let now = std::time::Instant::now();
        if first_at.is_none() {
            first_at = Some(now);
        }
        let gap = now - last;
        if gap > max_gap {
            max_gap = gap;
        }
        last = now;
        chunks += 1;
    })
    .await
    .expect("stream failed");

    assert!(chunks > 1, "expected multiple SSE deltas, got {chunks}");
    assert!(!full.trim().is_empty());
    eprintln!("chunks={chunks} first_token={:?} total={:?}", first_at.unwrap() - t0, t0.elapsed());
    eprintln!("out: {full}");
}

#[tokio::test]
async fn streamed_output_matches_non_streaming() {
    if !online() {
        return;
    }
    let mk = || TranslateRequest {
        text: "今天天气真好。".into(),
        target_lang: "英语".into(),
        mode: Mode::Basic,
        terms: vec![],
        auto_filter_terms: false,
        style: String::new(),
        format_type: String::new(),
        context: String::new(),
        ignore_glossary: true,
    };
    let streamed = client::translate_stream(&backend(), None, 0.0, 120, &mk(), |_| {})
        .await
        .expect("stream failed");
    let plain = client::translate(&backend(), None, 0.0, 120, &mk())
        .await
        .expect("plain failed");
    eprintln!("streamed: {streamed}");
    eprintln!("plain   : {}", plain.text);
    assert_eq!(streamed.trim(), plain.text.trim(), "streams diverged");
}
//! End-to-end check against the live llama-server.
//! Skipped automatically when nothing is listening on the configured URL.

use std::net::{SocketAddr, ToSocketAddrs};

use zephyr_translator_lib::client::{self, TranslateRequest};
use zephyr_translator_lib::config::Backend;
use zephyr_translator_lib::prompt::Mode;

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

/// Cheap TCP probe so tests skip fast when the model service is down.
fn online() -> bool {
    let url = base_url();
    let hostport = url.trim_start_matches("http://").trim_start_matches("https://");
    let hostport = hostport.split('/').next().unwrap_or(hostport);
    let _ = hostport.to_socket_addrs().map(|mut addrs: std::vec::IntoIter<SocketAddr>| {
        addrs.next()
    });
    match hostport.to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(2)).is_ok(),
            None => false,
        },
        Err(_) => false,
    }
}

#[tokio::test]
async fn translates_zh_to_en() {
    if !online() {
        eprintln!("skipping: no model service on {}", base_url());
        return;
    }
    let req = TranslateRequest {
        text: "今天天气真好，我们一起去公园散步吧。".into(),
        target_lang: "英语".into(),
        mode: Mode::Basic,
        terms: vec![],
        auto_filter_terms: false,
        style: String::new(),
        format_type: String::new(),
        context: String::new(),
        ignore_glossary: true,
    };
    let out = client::translate(&backend(), None, 0.1, 120, &req)
        .await
        .expect("translation failed");
    assert!(!out.text.trim().is_empty(), "empty translation");
    eprintln!("zh->en: {}", out.text);
    assert!(
        out.text.chars().any(|c| c.is_ascii_alphabetic()),
        "expected latin output, got: {}",
        out.text
    );
}

#[tokio::test]
async fn glossary_terms_are_honoured() {
    if !online() {
        eprintln!("skipping: no model service");
        return;
    }
    let req = TranslateRequest {
        text: "请提交这个订单到仓库。".into(),
        target_lang: "英语".into(),
        mode: Mode::Terminology,
        terms: vec![
            ["订单".into(), "order".into()],
            ["仓库".into(), "repository".into()],
        ],
        auto_filter_terms: false,
        style: String::new(),
        format_type: String::new(),
        context: String::new(),
        ignore_glossary: false,
    };
    let out = client::translate(&backend(), None, 0.1, 120, &req)
        .await
        .expect("translation failed");
    eprintln!("glossary: {}", out.text);
    let lower = out.text.to_lowercase();
    assert!(lower.contains("order"), "glossary term 'order' missing: {}", out.text);
}

#[tokio::test]
async fn health_check_reports_online() {
    if !online() {
        return;
    }
    let models = client::check_health(&backend()).await;
    assert!(models.is_ok(), "health check failed: {:?}", models);
    eprintln!("models: {:?}", models.unwrap());
}
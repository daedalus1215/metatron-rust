//! Acceptance tests for `specs/11-workbench.md`: the server.
//!
//! Each test starts a workbench on port 0 against its own copy of the
//! `wiring` fixture, because the watching tests write into it.

use metatron::clock::Clock;
use metatron::serve::{Options, Workbench};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct FixedClock;

impl Clock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_761_234_567
    }
}

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(tag: &str) -> Self {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wiring");
        let dst = std::env::temp_dir().join(format!("metatron-serve-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        copy(&src, &dst);
        Sandbox(dst)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

fn start(dir: &Path, watch: bool) -> Workbench {
    let opts = Options {
        port: 0,
        watch,
        poll: Duration::from_millis(40),
    };
    Workbench::start(dir, opts, FixedClock, |_| {}).expect("workbench")
}

fn port(wb: &Workbench) -> u16 {
    wb.url
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// A raw HTTP/1.1 request: `(status, body)`. The server closes after one.
fn get_as(wb: &Workbench, method: &str, path: &str, host: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port(wb))).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, body.to_string())
}

fn get(wb: &Workbench, path: &str) -> (u16, String) {
    get_as(wb, "GET", path, &format!("127.0.0.1:{}", port(wb)))
}

fn model(wb: &Workbench) -> Value {
    let (code, body) = get(wb, "/api/model");
    assert_eq!(code, 200);
    serde_json::from_str(&body).unwrap()
}

fn has_brick(m: &Value, id: &str) -> bool {
    m["bricks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == id)
}

// ---------------------------------------------------------------- routes

#[test]
fn it_binds_the_loopback_address_and_serves_the_page_and_the_model() {
    let sb = Sandbox::new("routes");
    let wb = start(&sb.0, false);
    assert!(wb.url.starts_with("http://127.0.0.1:"), "{}", wb.url);

    let (code, html) = get(&wb, "/");
    assert_eq!(code, 200);
    assert!(html.contains("metatron workbench"));

    let m = model(&wb);
    assert_eq!(m["version"], 1);
    assert_eq!(m["project"], "wiring");
    assert_eq!(
        m["changes"], false,
        "the page must hide a change form it cannot serve"
    );
    assert!(has_brick(&m, "app::App"));
    // The keys the page reads, spelled the way it reads them.
    let app = m["bricks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "app::App")
        .unwrap();
    assert!(app["sockets"][0].get("boundTo").is_some());
    assert!(m["calls"][0].get("toMethod").is_some());
    assert!(m["wiringMeta"]["studs"]["unseen"].is_number());
    wb.close();
}

#[test]
fn source_is_served_from_the_scan_and_nothing_else() {
    let sb = Sandbox::new("source");
    let wb = start(&sb.0, false);
    let (code, text) = get(&wb, "/api/source?file=app.rs");
    assert_eq!(code, 200);
    assert_eq!(
        text,
        std::fs::read_to_string(sb.0.join("src/app.rs")).unwrap()
    );
    let (code, _) = get(&wb, "/api/source?file=ports%2Ers");
    assert_eq!(code, 200, "a percent-encoded name is the same file");

    for bad in [
        "../Cargo.toml",
        "%2E%2E%2FCargo.toml",
        "/etc/passwd",
        "src/app.rs",
        "",
    ] {
        let (code, _) = get(&wb, &format!("/api/source?file={bad}"));
        assert_eq!(code, 404, "{bad} was served");
    }
    wb.close();
}

#[test]
fn it_refuses_another_host_a_write_and_a_change() {
    let sb = Sandbox::new("refuse");
    let wb = start(&sb.0, false);
    let p = port(&wb);
    // A page elsewhere that resolved its own name to 127.0.0.1.
    let (code, _) = get_as(&wb, "GET", "/api/model", &format!("evil.example:{p}"));
    assert_eq!(code, 403);
    let (code, _) = get_as(&wb, "GET", "/api/model", &format!("localhost:{p}"));
    assert_eq!(code, 200);
    let (code, _) = get_as(&wb, "POST", "/api/model", &format!("127.0.0.1:{p}"));
    assert_eq!(code, 405);
    let (code, body) = get(&wb, "/api/change?range=main...HEAD");
    assert_eq!(code, 501);
    assert!(body.contains("spec 10"), "{body}");
    assert_eq!(get(&wb, "/nope").0, 404);
    wb.close();
}

// -------------------------------------------------------------- watching

#[test]
fn a_save_rescans_once_and_the_event_stream_says_so() {
    let sb = Sandbox::new("watch");
    let wb = start(&sb.0, true);

    let mut ev = TcpStream::connect(("127.0.0.1", port(&wb))).unwrap();
    ev.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        ev,
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
        port(&wb)
    )
    .unwrap();
    let mut ev = BufReader::new(ev);
    let mut next_event = || loop {
        let mut line = String::new();
        ev.read_line(&mut line).unwrap();
        if let Some(d) = line.strip_prefix("data: ") {
            return serde_json::from_str::<Value>(d).unwrap();
        }
    };
    assert_eq!(next_event()["version"], 1);

    let app = sb.0.join("src/app.rs");
    let mut s = std::fs::read_to_string(&app).unwrap();
    s.push_str("\npub struct Added;\n");
    std::fs::write(&app, s).unwrap();

    assert_eq!(next_event()["version"], 2);
    let m = model(&wb);
    assert!(has_brick(&m, "app::Added"));
    assert_eq!(m["watching"], true);
    // Nothing else changed, so nothing else re-scans.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(wb.version(), 2);
    wb.close();
}

#[test]
fn a_rescan_that_fails_keeps_the_last_good_model_and_says_why() {
    let sb = Sandbox::new("fail");
    let wb = start(&sb.0, false);
    assert_eq!(wb.rescan(), 2);

    let manifest = sb.0.join("Cargo.toml");
    let kept = std::fs::read_to_string(&manifest).unwrap();
    std::fs::remove_file(&manifest).unwrap();
    assert_eq!(wb.rescan(), 2, "a failed scan must not advance the version");
    let m = model(&wb);
    assert!(has_brick(&m, "app::App"), "the last good model is gone");
    assert!(
        m["error"].as_str().unwrap_or("").contains("Cargo.toml"),
        "{}",
        m["error"]
    );

    std::fs::write(&manifest, kept).unwrap();
    assert_eq!(wb.rescan(), 3);
    assert!(model(&wb)["error"].is_null());
    wb.close();
}

#[test]
fn with_no_watch_the_model_is_static() {
    let sb = Sandbox::new("static");
    let wb = start(&sb.0, false);
    std::fs::write(sb.0.join("src/app.rs"), "pub struct Only;\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(wb.version(), 1);
    let m = model(&wb);
    assert_eq!(m["watching"], false);
    assert!(has_brick(&m, "app::App"));
    wb.close();
}

#[test]
fn a_first_scan_that_fails_is_an_error_not_an_empty_bench() {
    let dir = std::env::temp_dir().join(format!("metatron-serve-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let r = Workbench::start(
        &dir,
        Options {
            port: 0,
            ..Default::default()
        },
        FixedClock,
        |_| {},
    );
    assert!(r.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

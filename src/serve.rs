//! `metatron serve` — the workbench. See `specs/11-workbench.md`.
//!
//! The first thing metatron runs that does not exit. It scans once, keeps
//! the payload in memory, serves the page and a small JSON API, and
//! re-scans when a file under the crate changes.
//!
//! `std::net` only, as metatron-nestjs uses `node:http` only: a workbench is
//! no reason to take a dependency.
//!
//! It binds `127.0.0.1` and nothing else, because the page serves source
//! code, and it answers only a request whose `Host` is that address (or
//! `localhost`) — so a page on another site cannot reach it by pointing a
//! DNS name at the loopback address. Source is served from the scan's own
//! text map, never from a joined path: there is nothing to traverse, and what
//! is shown always matches the model it is drawn beside.
//!
//! Watching is polling. A poll that finds a change waits for the next one to
//! find nothing new, so a save that touches several files re-scans once.
//! That debounce counts polls rather than reading a clock: the crate's clock
//! is a port, and a debounce has no business asking it what time it is.

use crate::classify::{self, Config};
use crate::ports::Clock;
use crate::views::workbench::{self, Payload};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const HOST: &str = "127.0.0.1";
const PAGE: &str = include_str!("../templates/serve/workbench.html");

/// How long an idle event stream waits before writing a comment line. A
/// write is the only way to find out the browser went away.
const KEEPALIVE: Duration = Duration::from_secs(15);

pub struct Options {
    /// `0` asks the OS for a free port.
    pub port: u16,
    pub watch: bool,
    pub poll: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            port: 4477,
            watch: true,
            poll: Duration::from_millis(250),
        }
    }
}

struct State {
    payload: Payload,
    text: Arc<BTreeMap<String, String>>,
    /// Bumped on every event — a re-scan that worked or one that failed —
    /// so a waiting event stream wakes for both.
    seq: u64,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

pub struct Workbench {
    pub url: String,
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    rescan: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Workbench {
    /// Scan, bind, and start serving. A first scan that fails is an error:
    /// there is no last good model to show instead.
    pub fn start(
        dir: &Path,
        opts: Options,
        clock: impl Clock + Send + Sync + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<Workbench> {
        let dir = dir
            .canonicalize()
            .with_context(|| format!("no such directory: {}", dir.display()))?;
        let (payload, text) = build(&dir, &clock)?;
        let listener = TcpListener::bind((HOST, opts.port))
            .with_context(|| format!("cannot listen on {HOST}:{}", opts.port))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                payload: Payload {
                    version: 1,
                    watching: opts.watch,
                    ..payload
                },
                text: Arc::new(text),
                seq: 1,
                closed: false,
            }),
            changed: Condvar::new(),
        });
        let stop = Arc::new(AtomicBool::new(false));

        let rescan: Arc<dyn Fn() -> u64 + Send + Sync> = {
            let (shared, dir) = (shared.clone(), dir.clone());
            Arc::new(move || {
                let next = build(&dir, &clock);
                let mut st = shared.state.lock().unwrap();
                match next {
                    Ok((p, text)) => {
                        let (version, watching) = (st.payload.version + 1, st.payload.watching);
                        st.payload = Payload {
                            version,
                            watching,
                            ..p
                        };
                        st.text = Arc::new(text);
                    }
                    // Keep the last good model, and say why it is stale.
                    Err(e) => st.payload.error = Some(format!("{e:#}")),
                }
                st.seq += 1;
                let v = st.payload.version;
                let line = match &st.payload.error {
                    Some(e) => format!("  re-scan failed (still v{v}): {e}"),
                    None => format!("  re-scanned (v{v})"),
                };
                drop(st);
                shared.changed.notify_all();
                log(&line);
                v
            })
        };

        let mut threads = Vec::new();
        {
            let (shared, stop) = (shared.clone(), stop.clone());
            let port = addr.port();
            threads.push(thread::spawn(move || {
                for conn in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let shared = shared.clone();
                    thread::spawn(move || {
                        let _ = handle(conn, &shared, port);
                    });
                }
            }));
        }
        if opts.watch {
            let (stop, rescan, dir) = (stop.clone(), rescan.clone(), dir.clone());
            threads.push(thread::spawn(move || {
                let mut seen = fingerprint(&dir);
                let mut pending: Option<u64> = None;
                while !stop.load(Ordering::SeqCst) {
                    thread::sleep(opts.poll);
                    let now = fingerprint(&dir);
                    match pending {
                        // Quiet for one whole poll since the last change.
                        Some(p) if p == now => {
                            pending = None;
                            seen = now;
                            rescan();
                        }
                        _ if now != seen => pending = Some(now),
                        _ => pending = None,
                    }
                }
            }));
        }

        Ok(Workbench {
            url: format!("http://{HOST}:{}/", addr.port()),
            addr,
            shared,
            stop,
            threads,
            rescan,
        })
    }

    /// Re-scan now, as a save would. Returns the version being served.
    pub fn rescan(&self) -> u64 {
        (self.rescan)()
    }

    pub fn version(&self) -> u64 {
        self.shared.state.lock().unwrap().payload.version
    }

    pub fn meta(&self) -> crate::wiring::WiringMeta {
        self.shared
            .state
            .lock()
            .unwrap()
            .payload
            .wiring_meta
            .clone()
    }

    pub fn bricks(&self) -> usize {
        self.shared.state.lock().unwrap().payload.bricks.len()
    }

    /// Block until the server stops — for the CLI, until it is killed.
    pub fn wait(mut self) {
        if let Some(t) = self.threads.drain(..).next() {
            let _ = t.join();
        }
    }

    /// Stop serving: wake the accept loop and every open event stream.
    pub fn close(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.shared.state.lock().unwrap().closed = true;
        self.shared.changed.notify_all();
        let _ = TcpStream::connect(self.addr);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Everything the page reads, and the text of every file it came from.
fn build(dir: &Path, clock: &impl Clock) -> Result<(Payload, BTreeMap<String, String>)> {
    let (model, text) = crate::scan::scan_with_text(dir)?;
    let config = Config::load(dir)?;
    let classified = classify::classify(&model, &config);
    Ok((
        workbench::payload(&model, &config, &classified, clock.now_secs()),
        text,
    ))
}

// ------------------------------------------------------------------ http

fn handle(conn: TcpStream, shared: &Shared, port: u16) -> std::io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut host = None;
    let mut head = first.len();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        head += line.len();
        if head > 16 * 1024 {
            return respond(
                conn,
                431,
                "application/json",
                br#"{"error":"headers too large"}"#,
            );
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("host") {
                host = Some(v.trim().to_string());
            }
        }
    }
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

    // A browser always names the host it thinks it reached. Anything but
    // this server's own address is a page elsewhere that pointed a name at
    // the loopback address to read source through it.
    let ok_host = host
        .as_deref()
        .is_some_and(|h| h == format!("{HOST}:{port}") || h == format!("localhost:{port}"));
    if !ok_host {
        return respond(conn, 403, "application/json", br#"{"error":"wrong host"}"#);
    }
    if method != "GET" {
        return respond(conn, 405, "application/json", br#"{"error":"read-only"}"#);
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/" => respond(conn, 200, "text/html; charset=utf-8", PAGE.as_bytes()),
        "/api/model" => {
            let body = {
                let st = shared.state.lock().unwrap();
                serde_json::to_vec(&st.payload).unwrap_or_default()
            };
            respond(conn, 200, "application/json; charset=utf-8", &body)
        }
        "/api/source" => {
            let file = param(query, "file").unwrap_or_default();
            let text = shared.state.lock().unwrap().text.clone();
            match text.get(&file) {
                Some(t) => respond(conn, 200, "text/plain; charset=utf-8", t.as_bytes()),
                None => respond(
                    conn,
                    404,
                    "application/json",
                    br#"{"error":"not a scanned file"}"#,
                ),
            }
        }
        "/api/change" => respond(
            conn,
            501,
            "application/json",
            br#"{"error":"metatron-rust cannot show a change yet: that needs `metatron diff` (spec 10)"}"#,
        ),
        "/api/events" => events(conn, shared),
        // The browser asks on every load; an answer keeps the console clean.
        "/favicon.ico" => respond(conn, 204, "image/x-icon", b""),
        _ => respond(conn, 404, "application/json", br#"{"error":"no such route"}"#),
    }
}

fn respond(mut conn: TcpStream, code: u16, ty: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "",
    };
    write!(
        conn,
        "HTTP/1.1 {code} {reason}\r\ncontent-type: {ty}\r\ncontent-length: {}\r\n\
         cache-control: no-store\r\nx-content-type-options: nosniff\r\nconnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body)?;
    conn.flush()
}

/// Server-sent events: `{ version, error }` now, and after every re-scan.
fn events(mut conn: TcpStream, shared: &Shared) -> std::io::Result<()> {
    conn.write_all(
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-store\r\n\
          connection: keep-alive\r\n\r\n",
    )?;
    let mut st = shared.state.lock().unwrap();
    let mut seq = 0;
    loop {
        if st.closed {
            return Ok(());
        }
        let line = if st.seq != seq {
            seq = st.seq;
            format!(
                "data: {}\n\n",
                serde_json::json!({ "version": st.payload.version, "error": st.payload.error })
            )
        } else {
            ": keepalive\n\n".to_string()
        };
        // Never write with the lock held: a stalled browser would stall
        // every request behind it.
        drop(st);
        conn.write_all(line.as_bytes())?;
        conn.flush()?;
        st = shared.state.lock().unwrap();
        if st.seq == seq && !st.closed {
            st = shared.changed.wait_timeout(st, KEEPALIVE).unwrap().0;
        }
    }
}

/// One query parameter, percent-decoded.
fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| decode(v))
    })
}

fn decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi << 4 | lo);
                i += 3;
            }
            (b'+', ..) => {
                out.push(b' ');
                i += 1;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// --------------------------------------------------------------- watching

/// Every `.rs` file under the crate, plus the two manifests metatron reads,
/// by size and modification time. Build output, VCS state and metatron's own
/// output are skipped: a scan writes into one of them.
fn fingerprint(dir: &Path) -> u64 {
    let mut files: Vec<(PathBuf, u64, u128)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    stack.push(p);
                }
                continue;
            }
            let watched = name.ends_with(".rs") || name == "Cargo.toml" || name == "metatron.toml";
            if !watched {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            files.push((p, md.len(), mtime));
        }
    }
    files.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    files.hash(&mut h);
    h.finish()
}

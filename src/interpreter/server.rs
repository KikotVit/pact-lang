use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tiny_http::{Header, Response, Server, StatusCode};

use crate::interpreter::Interpreter;
use crate::interpreter::db::sse_query_new_rows;
use crate::interpreter::json::{json_to_value, value_to_json};
use crate::interpreter::value::Value;

// ── Rate limiting ──────────────────────────────────────────────────

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_DEFAULT: u32 = 200; // 200 requests per minute per IP

struct RateLimiter {
    clients: HashMap<String, (u32, Instant)>,
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter {
            clients: HashMap::new(),
        }
    }

    fn check(&mut self, ip: &str) -> bool {
        let max = std::env::var("RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(RATE_LIMIT_DEFAULT);
        if max == 0 {
            return true; // disabled
        }
        let now = Instant::now();
        let entry = self.clients.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) > RATE_LIMIT_WINDOW {
            *entry = (1, now);
            true
        } else {
            entry.0 += 1;
            entry.0 <= max
        }
    }
}

#[derive(Debug, Clone)]
enum PathSegment {
    Literal(String),
    Param(String),
}

fn parse_path_template(template: &str) -> Vec<PathSegment> {
    template
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with('{') && s.ends_with('}') {
                PathSegment::Param(s[1..s.len() - 1].to_string())
            } else {
                PathSegment::Literal(s.to_string())
            }
        })
        .collect()
}

fn match_path(segments: &[PathSegment], path: &str) -> Option<HashMap<String, String>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() != segments.len() {
        return None;
    }
    let mut params = HashMap::new();
    for (seg, part) in segments.iter().zip(parts.iter()) {
        match seg {
            PathSegment::Literal(s) => {
                if s != part {
                    return None;
                }
            }
            PathSegment::Param(name) => {
                params.insert(name.clone(), part.to_string());
            }
        }
    }
    Some(params)
}

// ── SSE support ────────────────────────────────────────────────────

/// Custom Read impl that blocks on a channel receiver.
/// Used to stream SSE events through tiny_http's Response.
struct SseReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    pos: usize,
}

impl SseReader {
    fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        SseReader {
            rx,
            buffer: Vec::new(),
            pos: 0,
        }
    }
}

impl io::Read for SseReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.buffer.len() {
            match self.rx.recv() {
                Ok(data) => {
                    self.buffer = data;
                    self.pos = 0;
                }
                Err(_) => return Ok(0), // Channel closed = end stream
            }
        }
        let available = &self.buffer[self.pos..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

fn format_sse_event(id: i64, data: &str) -> Vec<u8> {
    let event = format!("id: {}\ndata: {}\n\n", id, data);
    // Pad with SSE comment to flush tiny_http's internal BufWriter (~8KB).
    // SSE comments (lines starting with ':') are ignored by EventSource clients.
    let padding_needed = 8192_usize.saturating_sub(event.len());
    if padding_needed > 0 {
        format!("{}:{}\n", event, " ".repeat(padding_needed)).into_bytes()
    } else {
        event.into_bytes()
    }
}

fn handle_sse(
    request: tiny_http::Request,
    table: String,
    filter: Option<Box<Value>>,
    db_path: Option<String>,
    last_event_id: i64,
) {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // Spawn polling thread
    let poll_tx = tx.clone();
    thread::spawn(move || {
        let conn = match &db_path {
            Some(path) => match Connection::open(path) {
                Ok(c) => {
                    let _ = c.execute_batch("PRAGMA journal_mode=WAL;");
                    c
                }
                Err(_) => return,
            },
            None => return,
        };

        let mut last_rowid = last_event_id;

        loop {
            match sse_query_new_rows(&conn, &table, &filter, last_rowid) {
                Ok(rows) => {
                    for (rowid, json) in rows {
                        if poll_tx.send(format_sse_event(rowid, &json)).is_err() {
                            return; // Client disconnected
                        }
                        last_rowid = rowid;
                    }
                }
                Err(_) => return,
            }
            thread::sleep(Duration::from_millis(500));
        }
    });

    // Send SSE response — blocks until channel closes (client disconnect)
    // Pre-seed with a large SSE comment to flush tiny_http's ~8KB BufWriter.
    // SSE comments (lines starting with ':') are ignored by EventSource clients.
    let mut sse_reader = SseReader::new(rx);
    let padding = format!(": {}\n\n", " ".repeat(8192));
    sse_reader.buffer = padding.into_bytes();
    let mut headers = vec![
        Header::from_bytes("Content-Type", "text/event-stream").unwrap(),
        Header::from_bytes("Cache-Control", "no-cache").unwrap(),
        Header::from_bytes("Connection", "keep-alive").unwrap(),
    ];
    add_cors_headers(&mut headers);
    let response = Response::new(StatusCode(200), headers, sse_reader, None, None);
    let _ = request.respond(response);
    drop(tx); // Signal polling thread to stop
}

// ── CORS ───────────────────────────────────────────────────────────

/// Add CORS headers. Controlled via CORS_ORIGIN env var:
/// - Not set or "*" → allow all origins (dev mode)
/// - "none" → no CORS headers at all (disable CORS)
/// - "https://myapp.com" → allow only that origin
fn add_cors_headers(headers: &mut Vec<Header>) {
    let origin = std::env::var("CORS_ORIGIN").unwrap_or_else(|_| "*".to_string());
    if origin == "none" {
        return;
    }
    headers.push(Header::from_bytes("Access-Control-Allow-Origin", origin.as_str()).unwrap());
    headers.push(
        Header::from_bytes(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, Last-Event-ID",
        )
        .unwrap(),
    );
    headers.push(
        Header::from_bytes(
            "Access-Control-Allow-Methods",
            "GET, POST, PUT, DELETE, OPTIONS",
        )
        .unwrap(),
    );
}

// ── Schedule helpers ───────────────────────────────────────────────

fn format_schedule_interval(ms: u64) -> String {
    if ms % 86_400_000 == 0 {
        format!("{}d", ms / 86_400_000)
    } else if ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms % 60_000 == 0 {
        format!("{}m", ms / 60_000)
    } else if ms % 1000 == 0 {
        format!("{}s", ms / 1000)
    } else {
        format!("{}ms", ms)
    }
}

// ── Server ─────────────────────────────────────────────────────────

pub fn start_server(interpreter: &mut Interpreter, name: &str, port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to start server: {}", e);
            return;
        }
    };

    println!("{} listening on http://{}", name, addr);

    // Pre-compile path matchers for routes and streams
    let routes: Vec<(String, Vec<PathSegment>, usize)> = interpreter
        .routes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.method.clone(), parse_path_template(&r.path), i))
        .collect();

    let stream_routes: Vec<(String, Vec<PathSegment>, usize)> = interpreter
        .streams
        .iter()
        .enumerate()
        .map(|(i, s)| (s.method.clone(), parse_path_template(&s.path), i))
        .collect();

    let db_path = interpreter.get_db_path();

    // Spawn schedule threads
    for schedule in &interpreter.schedules {
        let interval = Duration::from_millis(schedule.interval_ms);
        let intent = schedule.intent.clone();
        let body = schedule.body.clone();
        let effects = schedule.effects.clone();
        let source = interpreter.source_code().to_string();
        let db_path_clone = db_path.clone();

        thread::spawn(move || {
            loop {
                let mut sched_interp = Interpreter::new(&source);
                sched_interp.setup_test_effects();
                if let Some(ref path) = db_path_clone {
                    let _ = sched_interp.open_sqlite(path);
                }
                sched_interp.blocked_effects = vec!["db", "time", "rng", "log", "auth", "http"]
                    .into_iter()
                    .filter(|e| !effects.contains(&e.to_string()))
                    .map(String::from)
                    .collect();

                let mut env = crate::interpreter::environment::Environment::new();
                for stmt in &body {
                    if let Err(e) = sched_interp.eval_statement(stmt, &mut env) {
                        eprintln!("[schedule:{}] Error: {}", intent, e.message);
                    }
                }
                thread::sleep(interval);
            }
        });
        let dur = format_schedule_interval(schedule.interval_ms);
        println!("  Schedule '{}' started (every {})", schedule.intent, dur);
    }

    let mut rate_limiter = RateLimiter::new();

    for mut request in server.incoming_requests() {
        // Rate limiting
        let peer_ip = request
            .remote_addr()
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        if !rate_limiter.check(&peer_ip) {
            let _ = request.respond(make_json_response(
                429,
                r#"{"error":"Too many requests. Try again later."}"#,
            ));
            continue;
        }

        let method = request.method().to_string().to_uppercase();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or(&url);

        // Parse query string
        let query_params = parse_query_string(&url);

        // OPTIONS preflight
        if method == "OPTIONS" {
            let mut headers = vec![];
            add_cors_headers(&mut headers);
            let response = Response::new(
                StatusCode(204),
                headers,
                std::io::Cursor::new(vec![]),
                Some(0),
                None,
            );
            let _ = request.respond(response);
            continue;
        }

        // Check stream routes first
        let stream_matched = stream_routes
            .iter()
            .find(|(m, segs, _)| m.to_uppercase() == method && match_path(segs, path).is_some());

        if let Some((_, segs, stream_idx)) = stream_matched {
            let path_params = match_path(segs, path).unwrap_or_default();

            // Extract headers before consuming the request
            let last_event_id: i64 = request
                .headers()
                .iter()
                .find(|h| h.field.to_string().to_lowercase() == "last-event-id")
                .and_then(|h| h.value.as_str().parse().ok())
                .unwrap_or(0);

            let mut headers_map = HashMap::new();
            for header in request.headers() {
                headers_map.insert(
                    header.field.to_string().to_lowercase(),
                    header.value.to_string(),
                );
            }

            let req_value = build_request_value(
                &method,
                path,
                path_params,
                query_params,
                headers_map,
                Value::Nothing,
            );

            let stream = interpreter.streams[*stream_idx].clone();
            match interpreter.execute_stream(&stream, req_value) {
                Ok(Value::DbWatch { table, filter }) => {
                    let db_path_clone = db_path.clone();
                    thread::spawn(move || {
                        handle_sse(request, table, filter, db_path_clone, last_event_id);
                    });
                }
                Ok(Value::Struct { ref fields, .. }) if fields.contains_key("status") => {
                    // Auth or permission error — return normal HTTP response
                    let status = match fields.get("status") {
                        Some(Value::Int(n)) => *n as i32,
                        _ => 500,
                    };
                    let custom_ct = fields.get("content_type").and_then(|v| {
                        if let Value::String(ct) = v {
                            Some(ct.clone())
                        } else {
                            None
                        }
                    });
                    let body = fields.get("body").unwrap_or(&Value::Nothing);
                    if let Some(ct) = custom_ct {
                        let body_str = match body {
                            Value::String(s) => s.clone(),
                            other => {
                                serde_json::to_string(&value_to_json(other)).unwrap_or_default()
                            }
                        };
                        let _ = request
                            .respond(make_response_with_content_type(status, &body_str, &ct));
                    } else {
                        let json_body =
                            serde_json::to_string(&value_to_json(body)).unwrap_or_default();
                        let _ = request.respond(make_json_response(status, &json_body));
                    }
                }
                Ok(_) => {
                    let _ = request.respond(make_json_response(
                        500,
                        r#"{"error":"Stream must use send db.watch()"}"#,
                    ));
                }
                Err(err) => {
                    let error_json = serde_json::json!({"error": err.message}).to_string();
                    let _ = request.respond(make_json_response(500, &error_json));
                }
            }
            continue;
        }

        // Find matching regular route
        let matched = routes
            .iter()
            .find(|(m, segs, _)| m.to_uppercase() == method && match_path(segs, path).is_some());

        let response = if let Some((_, segs, route_idx)) = matched {
            let path_params = match_path(segs, path).unwrap_or_default();

            // Parse body
            let mut body_str = String::new();
            let _ = std::io::Read::read_to_string(request.as_reader(), &mut body_str);
            let body_value = if body_str.is_empty() {
                Value::Nothing
            } else {
                match serde_json::from_str::<serde_json::Value>(&body_str) {
                    Ok(json) => json_to_value(&json),
                    Err(_) => Value::String(body_str),
                }
            };

            // Extract headers
            let mut headers_map = HashMap::new();
            for header in request.headers() {
                headers_map.insert(
                    header.field.to_string().to_lowercase(),
                    header.value.to_string(),
                );
            }

            // Build request Value
            let req_value = build_request_value(
                &method,
                path,
                path_params,
                query_params,
                headers_map,
                body_value,
            );

            // Execute route
            let route = interpreter.routes[*route_idx].clone();
            match interpreter.execute_route(&route, req_value) {
                Ok(Value::Struct { ref fields, .. }) => {
                    let status = match fields.get("status") {
                        Some(Value::Int(n)) => *n as i32,
                        _ => 200,
                    };
                    // Handle redirects (301, 302, 307, 308)
                    if matches!(status, 301 | 302 | 307 | 308) {
                        let location = fields.get("location").or_else(|| {
                            fields.get("body").and_then(|b| {
                                if let Value::Struct { fields: bf, .. } = b {
                                    bf.get("location")
                                } else {
                                    None
                                }
                            })
                        });
                        if let Some(Value::String(loc)) = location {
                            make_redirect_response(status, loc)
                        } else {
                            let body = fields.get("body").unwrap_or(&Value::Nothing);
                            let json_body =
                                serde_json::to_string(&value_to_json(body)).unwrap_or_default();
                            make_json_response(status, &json_body)
                        }
                    } else {
                        let custom_ct = fields.get("content_type").and_then(|v| {
                            if let Value::String(ct) = v {
                                Some(ct.clone())
                            } else {
                                None
                            }
                        });
                        let body = fields.get("body").unwrap_or(&Value::Nothing);
                        if let Some(ct) = custom_ct {
                            let body_str = match body {
                                Value::String(s) => s.clone(),
                                other => {
                                    serde_json::to_string(&value_to_json(other)).unwrap_or_default()
                                }
                            };
                            make_response_with_content_type(status, &body_str, &ct)
                        } else {
                            let json_body =
                                serde_json::to_string(&value_to_json(body)).unwrap_or_default();
                            make_json_response(status, &json_body)
                        }
                    }
                }
                Ok(other) => {
                    let json_body =
                        serde_json::to_string(&value_to_json(&other)).unwrap_or_default();
                    make_json_response(200, &json_body)
                }
                Err(err) => {
                    let error_json = serde_json::json!({"error": err.message}).to_string();
                    make_json_response(500, &error_json)
                }
            }
        } else {
            make_json_response(404, r#"{"error":"Not found"}"#)
        };

        let _ = request.respond(response);
    }
}

fn build_request_value(
    method: &str,
    path: &str,
    path_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Value,
) -> Value {
    let params_fields: HashMap<String, Value> = path_params
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    let query_fields: HashMap<String, Value> = query_params
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    let mut fields = HashMap::new();
    fields.insert("method".to_string(), Value::String(method.to_string()));
    fields.insert("path".to_string(), Value::String(path.to_string()));
    fields.insert(
        "params".to_string(),
        Value::Struct {
            type_name: "Params".to_string(),
            fields: params_fields,
        },
    );
    fields.insert(
        "query".to_string(),
        Value::Struct {
            type_name: "Query".to_string(),
            fields: query_fields,
        },
    );
    let headers_fields: HashMap<String, Value> = headers
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    fields.insert(
        "headers".to_string(),
        Value::Struct {
            type_name: "Headers".to_string(),
            fields: headers_fields,
        },
    );
    fields.insert("body".to_string(), body);

    Value::Struct {
        type_name: "Request".to_string(),
        fields,
    }
}

fn parse_query_string(url: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = url.split('?').nth(1) {
        for pair in query.split('&') {
            let mut kv = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                params.insert(k.to_string(), v.to_string());
            }
        }
    }
    params
}

fn make_redirect_response(status: i32, location: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"redirect":"{}"}}"#, location);
    let data = body.as_bytes().to_vec();
    let location_header = Header::from_bytes("Location", location).unwrap();
    let content_type = Header::from_bytes("Content-Type", "application/json").unwrap();
    let mut headers = vec![location_header, content_type];
    add_cors_headers(&mut headers);
    Response::new(
        StatusCode(status as u16),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

fn make_response_with_content_type(
    status: i32,
    body: &str,
    content_type: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let ct_header = Header::from_bytes("Content-Type", content_type).unwrap();
    let mut headers = vec![ct_header];
    add_cors_headers(&mut headers);
    Response::new(
        StatusCode(status as u16),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

fn make_json_response(status: i32, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let content_type = Header::from_bytes("Content-Type", "application/json").unwrap();
    let mut headers = vec![content_type];
    add_cors_headers(&mut headers);
    Response::new(
        StatusCode(status as u16),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_literal_path() {
        let segs = parse_path_template("/health");
        assert!(match_path(&segs, "/health").is_some());
        assert!(match_path(&segs, "/other").is_none());
    }

    #[test]
    fn match_param_path() {
        let segs = parse_path_template("/users/{id}");
        let params = match_path(&segs, "/users/123").unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn match_multi_param() {
        let segs = parse_path_template("/users/{id}/posts/{post_id}");
        let params = match_path(&segs, "/users/1/posts/42").unwrap();
        assert_eq!(params.get("id"), Some(&"1".to_string()));
        assert_eq!(params.get("post_id"), Some(&"42".to_string()));
    }

    #[test]
    fn no_match_different_length() {
        let segs = parse_path_template("/users/{id}");
        assert!(match_path(&segs, "/users").is_none());
        assert!(match_path(&segs, "/users/1/extra").is_none());
    }

    #[test]
    fn parse_query_params() {
        let params = parse_query_string("/users?name=Alice&age=30");
        assert_eq!(params.get("name"), Some(&"Alice".to_string()));
        assert_eq!(params.get("age"), Some(&"30".to_string()));
    }

    #[test]
    fn parse_empty_query() {
        let params = parse_query_string("/users");
        assert!(params.is_empty());
    }

    #[test]
    fn sse_event_format() {
        let event = format_sse_event(42, r#"{"msg":"hello"}"#);
        let s = String::from_utf8(event).unwrap();
        assert!(s.contains("id: 42\n"));
        assert!(s.contains("data: {\"msg\":\"hello\"}\n"));
        // Event is padded with SSE comment to flush tiny_http buffer
        assert!(s.len() >= 8192);
        assert!(s.contains(":"));
    }

    #[test]
    fn sse_reader_delivers_data() {
        let (tx, rx) = mpsc::channel();
        let mut reader = SseReader::new(rx);

        tx.send(b"hello".to_vec()).unwrap();
        tx.send(b"world".to_vec()).unwrap();
        drop(tx);

        let mut buf = [0u8; 5];
        let n = io::Read::read(&mut reader, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello");

        let n = io::Read::read(&mut reader, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"world");

        // Channel closed
        let n = io::Read::read(&mut reader, &mut buf).unwrap();
        assert_eq!(n, 0);
    }
}

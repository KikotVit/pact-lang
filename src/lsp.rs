use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use crate::checker::{self, Severity, Symbol, SymbolKind};
use crate::lexer::Lexer;
use crate::parser::Parser;

struct LspServer {
    documents: HashMap<String, String>,
    initialized: bool,
}

impl LspServer {
    fn new() -> Self {
        LspServer {
            documents: HashMap::new(),
            initialized: false,
        }
    }
}

// --- JSON-RPC helpers ---

fn make_response(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn make_notification(method: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    })
}

// --- Content-Length framing ---

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;

    // Read headers
    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            return Ok(None); // EOF
        }
        let header = header.trim();
        if header.is_empty() {
            break; // End of headers
        }
        if let Some(len_str) = header.strip_prefix("Content-Length: ") {
            if let Ok(len) = len_str.parse::<usize>() {
                content_length = Some(len);
            }
        }
    }

    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).to_string()))
}

fn write_message(writer: &mut impl Write, msg: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_string(msg).unwrap();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()
}

// --- Analysis ---

fn analyze(source: &str, uri: &str) -> (Vec<serde_json::Value>, Vec<Symbol>) {
    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            return (
                vec![serde_json::json!({
                    "range": lsp_range(e.line, e.column, e.line, e.column + 1),
                    "severity": 1,
                    "source": "pact",
                    "message": e.message,
                })],
                vec![],
            );
        }
    };

    let mut parser = Parser::new(tokens, source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(errors) => {
            let diags: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "range": lsp_range(e.line, e.column, e.line, e.column + 1),
                        "severity": 1,
                        "source": "pact",
                        "message": &e.message,
                    })
                })
                .collect();
            return (diags, vec![]);
        }
    };

    let base_dir = if uri.starts_with("file://") {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        std::path::Path::new(path).parent()
    } else {
        None
    };

    let result = checker::check_with_symbols(&program, source, base_dir);

    let diags: Vec<serde_json::Value> = result
        .diagnostics
        .iter()
        .map(|d| {
            let severity = match d.severity {
                Severity::Error => 1,
                Severity::Warning => 2,
            };
            let mut diag = serde_json::json!({
                "range": lsp_range(d.line, d.column, d.line, d.column + 1),
                "severity": severity,
                "source": "pact",
                "message": &d.message,
            });
            if let Some(ref hint) = d.hint {
                diag["message"] =
                    serde_json::Value::String(format!("{}\nHint: {}", d.message, hint));
            }
            diag
        })
        .collect();

    (diags, result.symbols)
}

fn lsp_range(line: usize, col: usize, end_line: usize, end_col: usize) -> serde_json::Value {
    // LSP uses 0-based positions
    serde_json::json!({
        "start": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) },
        "end": { "line": end_line.saturating_sub(1), "character": end_col.saturating_sub(1) }
    })
}

// --- Completion items ---

fn keyword_completions() -> Vec<serde_json::Value> {
    let keywords = [
        ("fn", "Function declaration"),
        ("let", "Variable binding"),
        ("var", "Mutable variable"),
        ("type", "Type declaration"),
        ("if", "Conditional"),
        ("else", "Else branch"),
        ("match", "Pattern matching"),
        ("return", "Return value"),
        ("use", "Import"),
        ("intent", "Intent declaration"),
        ("needs", "Effect declaration"),
        ("route", "HTTP route"),
        ("stream", "SSE stream route"),
        ("test", "Test block"),
        ("app", "App declaration"),
        ("assert", "Test assertion"),
        ("respond", "HTTP response"),
        ("send", "Stream send"),
    ];

    keywords
        .iter()
        .map(|(kw, desc)| {
            serde_json::json!({
                "label": kw,
                "kind": 14, // Keyword
                "detail": desc,
            })
        })
        .collect()
}

fn pipeline_completions() -> Vec<serde_json::Value> {
    let steps = [
        ("filter where", "Filter items by predicate"),
        ("map to", "Transform items"),
        ("sort by", "Sort items by field"),
        ("take first", "Take first N items"),
        ("take last", "Take last N items"),
        ("skip", "Skip N items"),
        ("group by", "Group by field"),
        ("flatten", "Flatten nested lists"),
        ("unique", "Remove duplicates"),
        ("count", "Count items"),
        ("sum", "Sum values"),
        ("find first where", "Find first matching"),
        ("expect one or raise", "Expect exactly one"),
        ("expect any or raise", "Expect at least one"),
        ("or default", "Default value if empty"),
        ("on success:", "Handle success"),
        ("validate as", "Validate against type"),
    ];

    steps
        .iter()
        .map(|(step, desc)| {
            serde_json::json!({
                "label": step,
                "kind": 15, // Snippet
                "detail": desc,
            })
        })
        .collect()
}

fn effect_completions() -> Vec<serde_json::Value> {
    let effects = [
        ("db.insert", "Insert record"),
        ("db.query", "Query records"),
        ("db.find", "Find single record"),
        ("db.update", "Update record"),
        ("db.delete", "Delete record"),
        ("db.watch", "Watch table for changes (SSE)"),
        ("time.now", "Current timestamp"),
        ("rng.uuid", "Generate UUID"),
        ("rng.hex", "Generate hex string"),
        ("auth.require", "Require JWT auth"),
        ("auth.sign", "Sign JWT token"),
        ("auth.verify", "Verify JWT token"),
        ("log.info", "Log info message"),
        ("log.warn", "Log warning"),
        ("log.error", "Log error"),
        ("env.get", "Get env variable"),
        ("env.require", "Require env variable"),
        ("http.get", "HTTP GET request"),
        ("http.post", "HTTP POST request"),
    ];

    effects
        .iter()
        .map(|(name, desc)| {
            serde_json::json!({
                "label": name,
                "kind": 3, // Function
                "detail": desc,
            })
        })
        .collect()
}

// --- LSP handlers ---

fn handle_initialize(id: &serde_json::Value) -> serde_json::Value {
    make_response(
        id,
        serde_json::json!({
            "capabilities": {
                "textDocumentSync": {
                    "openClose": true,
                    "change": 1, // Full sync
                },
                "hoverProvider": true,
                "completionProvider": {
                    "triggerCharacters": [".", "|"],
                },
                "definitionProvider": false,
            },
            "serverInfo": {
                "name": "pact-lsp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )
}

fn handle_hover(
    id: &serde_json::Value,
    params: &serde_json::Value,
    server: &LspServer,
) -> serde_json::Value {
    let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
    let line = params["position"]["line"].as_u64().unwrap_or(0) as usize + 1; // 1-based
    let col = params["position"]["character"].as_u64().unwrap_or(0) as usize + 1;

    let source = match server.documents.get(uri) {
        Some(s) => s,
        None => return make_response(id, serde_json::Value::Null),
    };

    // Find the word at position
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || line > lines.len() {
        return make_response(id, serde_json::Value::Null);
    }
    let line_text = lines[line - 1];
    let word = extract_word_at(line_text, col.saturating_sub(1));

    if word.is_empty() {
        return make_response(id, serde_json::Value::Null);
    }

    // Check symbols
    let (_, symbols) = analyze(source, uri);
    for sym in &symbols {
        if sym.name == word {
            let kind_str = match sym.kind {
                SymbolKind::Function => "function",
                SymbolKind::Type => "type",
            };
            return make_response(
                id,
                serde_json::json!({
                    "contents": {
                        "kind": "markdown",
                        "value": format!("**{}** ({})\n\n`{}`", sym.name, kind_str, sym.type_info),
                    }
                }),
            );
        }
    }

    // Check builtins
    let builtin_docs = get_builtin_doc(&word);
    if let Some(doc) = builtin_docs {
        return make_response(
            id,
            serde_json::json!({
                "contents": {
                    "kind": "markdown",
                    "value": doc,
                }
            }),
        );
    }

    make_response(id, serde_json::Value::Null)
}

fn handle_completion(
    id: &serde_json::Value,
    params: &serde_json::Value,
    server: &LspServer,
) -> serde_json::Value {
    let uri = params["textDocument"]["uri"].as_str().unwrap_or("");

    let mut items: Vec<serde_json::Value> = Vec::new();

    // Keywords
    items.extend(keyword_completions());

    // Pipeline steps
    items.extend(pipeline_completions());

    // Effect builtins
    items.extend(effect_completions());

    // User-defined symbols
    if let Some(source) = server.documents.get(uri) {
        let (_, symbols) = analyze(source, uri);
        for sym in symbols {
            let kind = match sym.kind {
                SymbolKind::Function => 3,
                SymbolKind::Type => 22,
            };
            items.push(serde_json::json!({
                "label": sym.name,
                "kind": kind,
                "detail": sym.type_info,
            }));
        }
    }

    make_response(id, serde_json::json!({ "items": items }))
}

fn extract_word_at(line: &str, col: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if col >= chars.len() {
        return String::new();
    }

    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '.';

    let mut start = col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }

    let mut end = col;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }

    chars[start..end].iter().collect()
}

fn get_builtin_doc(name: &str) -> Option<String> {
    match name {
        "db" => Some("**db** (effect)\n\nDatabase operations: `insert`, `query`, `find`, `update`, `delete`, `watch`".to_string()),
        "auth" => Some("**auth** (effect)\n\n`require(request)` — validate JWT\n`sign(payload)` — create JWT\n`verify(token)` — verify raw JWT".to_string()),
        "time" => Some("**time** (effect)\n\n`now()` — current ISO timestamp".to_string()),
        "rng" => Some("**rng** (effect)\n\n`uuid()` — random UUID\n`hex(n)` — random hex string".to_string()),
        "log" => Some("**log** (effect)\n\n`info(msg)`, `warn(msg)`, `error(msg)`".to_string()),
        "env" => Some("**env** (effect)\n\n`get(key)` — read env var\n`require(key)` — require env var".to_string()),
        "http" => Some("**http** (effect)\n\n`get(url)`, `post(url, body)`, `put(url, body)`, `delete(url)`".to_string()),
        "respond" => Some("**respond** status **with** body\n\nSend HTTP response from a route handler".to_string()),
        "send" => Some("**send** expr\n\nSend SSE event from a stream handler".to_string()),
        "needs" => Some("**needs** effect1, effect2, ...\n\nDeclare required effects for a function or route".to_string()),
        "intent" => Some("**intent** \"description\"\n\nDeclare the purpose of the next function, route, or stream".to_string()),
        _ => None,
    }
}

// --- Main loop ---

pub fn run_lsp_server() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());

    let mut server = LspServer::new();

    loop {
        let body = match read_message(&mut reader) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(_) => break,
        };

        let msg: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.get("id");
        let params = msg.get("params").cloned().unwrap_or(serde_json::json!({}));

        match method {
            "initialize" => {
                if let Some(id) = id {
                    let resp = handle_initialize(id);
                    let _ = write_message(&mut writer, &resp);
                    server.initialized = true;
                }
            }
            "initialized" => {
                // Client acknowledged, nothing to do
            }
            "shutdown" => {
                if let Some(id) = id {
                    let resp = make_response(id, serde_json::Value::Null);
                    let _ = write_message(&mut writer, &resp);
                }
            }
            "exit" => break,

            "textDocument/didOpen" => {
                if let Some(doc) = params.get("textDocument") {
                    let uri = doc["uri"].as_str().unwrap_or("").to_string();
                    let text = doc["text"].as_str().unwrap_or("").to_string();
                    server.documents.insert(uri.clone(), text.clone());

                    // Publish diagnostics
                    let (diags, _) = analyze(&text, &uri);
                    let notif = make_notification(
                        "textDocument/publishDiagnostics",
                        serde_json::json!({
                            "uri": uri,
                            "diagnostics": diags,
                        }),
                    );
                    let _ = write_message(&mut writer, &notif);
                }
            }

            "textDocument/didChange" => {
                if let Some(doc) = params.get("textDocument") {
                    let uri = doc["uri"].as_str().unwrap_or("").to_string();
                    if let Some(changes) = params.get("contentChanges").and_then(|c| c.as_array()) {
                        if let Some(change) = changes.first() {
                            let text = change["text"].as_str().unwrap_or("").to_string();
                            server.documents.insert(uri.clone(), text.clone());

                            let (diags, _) = analyze(&text, &uri);
                            let notif = make_notification(
                                "textDocument/publishDiagnostics",
                                serde_json::json!({
                                    "uri": uri,
                                    "diagnostics": diags,
                                }),
                            );
                            let _ = write_message(&mut writer, &notif);
                        }
                    }
                }
            }

            "textDocument/didClose" => {
                if let Some(doc) = params.get("textDocument") {
                    let uri = doc["uri"].as_str().unwrap_or("");
                    server.documents.remove(uri);
                    // Clear diagnostics
                    let notif = make_notification(
                        "textDocument/publishDiagnostics",
                        serde_json::json!({
                            "uri": uri,
                            "diagnostics": [],
                        }),
                    );
                    let _ = write_message(&mut writer, &notif);
                }
            }

            "textDocument/hover" => {
                if let Some(id) = id {
                    let resp = handle_hover(id, &params, &server);
                    let _ = write_message(&mut writer, &resp);
                }
            }

            "textDocument/completion" => {
                if let Some(id) = id {
                    let resp = handle_completion(id, &params, &server);
                    let _ = write_message(&mut writer, &resp);
                }
            }

            _ => {
                // Unknown method — respond with error if it has an id
                if let Some(id) = id {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("Method not found: '{}'", method),
                        }
                    });
                    let _ = write_message(&mut writer, &resp);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_write_message() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();
        let written = String::from_utf8(buf.clone()).unwrap();
        assert!(written.starts_with("Content-Length: "));
        assert!(written.contains("initialize"));

        let mut reader = io::BufReader::new(&buf[..]);
        let body = read_message(&mut reader).unwrap().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["method"], "initialize");
    }

    #[test]
    fn test_handle_initialize() {
        let id = serde_json::json!(1);
        let resp = handle_initialize(&id);
        assert!(
            resp["result"]["capabilities"]["hoverProvider"]
                .as_bool()
                .unwrap()
        );
        assert!(resp["result"]["capabilities"]["completionProvider"].is_object());
    }

    #[test]
    fn test_analyze_valid_source() {
        let source = "let x: Int = 42\n";
        let (diags, _) = analyze(source, "file:///test.pact");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_analyze_syntax_error() {
        let source = "let !!!\n";
        let (diags, _) = analyze(source, "file:///test.pact");
        assert!(!diags.is_empty());
    }

    #[test]
    fn test_analyze_returns_symbols() {
        let source = r#"type User { name: String, age: Int }
intent "find"
fn find_user(id: String) -> String { id }
"#;
        let (_, symbols) = analyze(source, "file:///test.pact");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"User"));
        assert!(names.contains(&"find_user"));
    }

    #[test]
    fn test_keyword_completions() {
        let items = keyword_completions();
        let labels: Vec<&str> = items.iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"fn"));
        assert!(labels.contains(&"route"));
        assert!(labels.contains(&"intent"));
    }

    #[test]
    fn test_extract_word_at() {
        assert_eq!(extract_word_at("let name: String", 4), "name");
        assert_eq!(extract_word_at("db.query", 0), "db.query");
        assert_eq!(extract_word_at("  x + y", 2), "x");
    }

    #[test]
    fn test_hover_builtin() {
        let id = serde_json::json!(1);
        let mut server = LspServer::new();
        server
            .documents
            .insert("file:///test.pact".to_string(), "needs db\n".to_string());
        let params = serde_json::json!({
            "textDocument": {"uri": "file:///test.pact"},
            "position": {"line": 0, "character": 6}
        });
        let resp = handle_hover(&id, &params, &server);
        let contents = resp["result"]["contents"]["value"].as_str().unwrap_or("");
        assert!(contents.contains("db"));
    }
}

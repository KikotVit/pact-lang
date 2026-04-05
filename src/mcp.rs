use std::io::{self, BufRead, Write};

use crate::interpreter::Interpreter;
use crate::interpreter::json::value_to_json;
use crate::lexer::Lexer;
use crate::parser::Parser;

// --- JSON-RPC helpers ---

fn make_response(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn make_error_response(id: &serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn make_tool_result(text: &str, is_error: bool) -> serde_json::Value {
    let mut result = serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    });
    if is_error {
        result["isError"] = serde_json::Value::Bool(true);
    }
    result
}

// --- Error conversion ---

fn pact_error_json(
    phase: &str,
    line: usize,
    column: usize,
    message: &str,
    hint: &Option<String>,
    source_line: &str,
) -> serde_json::Value {
    serde_json::json!({
        "phase": phase,
        "line": line,
        "column": column,
        "message": message,
        "hint": hint,
        "source_line": source_line
    })
}

// --- Source resolution ---

fn get_source(params: &serde_json::Value) -> Result<String, String> {
    let code = params.get("code").and_then(|v| v.as_str());
    let file = params.get("file").and_then(|v| v.as_str());

    match (code, file) {
        (Some(c), None) => Ok(c.to_string()),
        (None, Some(f)) => {
            std::fs::read_to_string(f).map_err(|e| format!("Cannot read '{}': {}", f, e))
        }
        (Some(_), Some(_)) => Err("Provide either 'code' or 'file', not both".to_string()),
        (None, None) => Err("Provide 'code' (inline source) or 'file' (path)".to_string()),
    }
}

// --- Tool execution ---

fn execute_pact_check(params: &serde_json::Value) -> serde_json::Value {
    let source = match get_source(params) {
        Ok(s) => s,
        Err(msg) => {
            return make_tool_result(
                &serde_json::json!({"errors": [{"phase": "input", "message": msg}]}).to_string(),
                true,
            );
        }
    };

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            let errors = vec![pact_error_json(
                "lexer",
                e.line,
                e.column,
                &e.message,
                &e.hint,
                &e.source_line,
            )];
            return make_tool_result(&serde_json::json!({"errors": errors}).to_string(), true);
        }
    };

    let mut parser = Parser::new(tokens, &source);
    match parser.parse() {
        Ok(program) => {
            let stmt_count = program.statements.len();
            let base_dir = params
                .get("file")
                .and_then(|v| v.as_str())
                .and_then(|f| std::path::Path::new(f).parent());
            let diagnostics = crate::checker::check(&program, &source, base_dir);
            let errors: Vec<serde_json::Value> = diagnostics
                .iter()
                .filter(|d| d.severity == crate::checker::Severity::Error)
                .map(|d| {
                    pact_error_json(
                        "checker",
                        d.line,
                        d.column,
                        &d.message,
                        &d.hint,
                        &d.source_line,
                    )
                })
                .collect();
            let warnings: Vec<serde_json::Value> = diagnostics
                .iter()
                .filter(|d| d.severity == crate::checker::Severity::Warning)
                .map(|d| {
                    pact_error_json(
                        "checker",
                        d.line,
                        d.column,
                        &d.message,
                        &d.hint,
                        &d.source_line,
                    )
                })
                .collect();

            if !errors.is_empty() {
                let mut result = serde_json::json!({
                    "valid": false,
                    "statements": stmt_count,
                    "errors": errors
                });
                if !warnings.is_empty() {
                    result["warnings"] = serde_json::json!(warnings);
                }
                make_tool_result(&result.to_string(), true)
            } else if !warnings.is_empty() {
                make_tool_result(
                    &serde_json::json!({
                        "valid": true,
                        "statements": stmt_count,
                        "warnings": warnings
                    })
                    .to_string(),
                    false,
                )
            } else {
                make_tool_result(
                    &serde_json::json!({
                        "valid": true,
                        "statements": stmt_count
                    })
                    .to_string(),
                    false,
                )
            }
        }
        Err(errors) => {
            let err_list: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| {
                    pact_error_json(
                        "parser",
                        e.line,
                        e.column,
                        &e.message,
                        &e.hint,
                        &e.source_line,
                    )
                })
                .collect();
            make_tool_result(&serde_json::json!({"errors": err_list}).to_string(), true)
        }
    }
}

fn execute_pact_run(params: &serde_json::Value) -> serde_json::Value {
    let source = match get_source(params) {
        Ok(s) => s,
        Err(msg) => {
            return make_tool_result(
                &serde_json::json!({"errors": [{"phase": "input", "message": msg}]}).to_string(),
                true,
            );
        }
    };

    let file_path = params.get("file").and_then(|v| v.as_str());

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            let errors = vec![pact_error_json(
                "lexer",
                e.line,
                e.column,
                &e.message,
                &e.hint,
                &e.source_line,
            )];
            return make_tool_result(&serde_json::json!({"errors": errors}).to_string(), true);
        }
    };

    let mut parser = Parser::new(tokens, &source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(errors) => {
            let err_list: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| {
                    pact_error_json(
                        "parser",
                        e.line,
                        e.column,
                        &e.message,
                        &e.hint,
                        &e.source_line,
                    )
                })
                .collect();
            return make_tool_result(&serde_json::json!({"errors": err_list}).to_string(), true);
        }
    };

    let mut interp = Interpreter::new(&source);
    if let Some(path) = file_path {
        interp.set_base_dir(path);
    }
    interp.setup_test_effects();

    match interp.interpret(&program) {
        Ok(value) => {
            if interp.app_config.is_some() {
                return make_tool_result(
                    &serde_json::json!({
                        "errors": [{
                            "phase": "runtime",
                            "message": "Cannot start HTTP server via MCP. Use 'pact run <file>' from the command line instead."
                        }]
                    })
                    .to_string(),
                    true,
                );
            }
            let json_value = value_to_json(&value);
            make_tool_result(
                &serde_json::json!({"result": json_value}).to_string(),
                false,
            )
        }
        Err(e) => {
            let errors = vec![pact_error_json(
                "runtime",
                e.line,
                e.column,
                &e.message,
                &e.hint,
                &e.source_line,
            )];
            make_tool_result(&serde_json::json!({"errors": errors}).to_string(), true)
        }
    }
}

fn execute_pact_docs(params: &serde_json::Value) -> serde_json::Value {
    let topic = params.get("topic").and_then(|v| v.as_str());

    match topic {
        Some(t) if !t.is_empty() => match crate::docs::get_doc(t) {
            Some(content) => make_tool_result(content, false),
            None => {
                let topics = crate::docs::list_topics();
                let names: Vec<&str> = topics.iter().map(|(n, _)| *n).collect();
                let hint = match crate::docs::suggest_topic(t) {
                    Some(s) => format!(" Did you mean '{}'?", s),
                    None => String::new(),
                };
                make_tool_result(
                    &format!(
                        "Unknown topic '{}'.{} Available topics: {}",
                        t,
                        hint,
                        names.join(", ")
                    ),
                    true,
                )
            }
        },
        _ => {
            // No topic — list all topics
            let topics = crate::docs::list_topics();
            let mut text = String::from("Available PACT documentation topics:\n\n");
            for (name, desc) in &topics {
                text.push_str(&format!("  {:<12} {}\n", name, desc));
            }
            text.push_str("\nCall pact_docs with a topic name for full documentation.");
            make_tool_result(&text, false)
        }
    }
}

// --- Protocol handlers ---

fn handle_initialize(id: &serde_json::Value) -> serde_json::Value {
    make_response(
        id,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "pact",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(id: &serde_json::Value) -> serde_json::Value {
    make_response(
        id,
        serde_json::json!({
            "tools": [
                {
                    "name": "pact_run",
                    "description": "Execute PACT code and return the result. Provide either inline code or a file path. Returns the evaluated result as JSON, or structured error details if execution fails.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "Inline PACT source code to execute"
                            },
                            "file": {
                                "type": "string",
                                "description": "Path to a .pact file to execute"
                            }
                        },
                        "additionalProperties": false
                    }
                },
                {
                    "name": "pact_check",
                    "description": "Parse, validate syntax, and check types of PACT code without executing it. Returns validity status with any type errors or warnings, or structured error details with line numbers, columns, and hints.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "Inline PACT source code to validate"
                            },
                            "file": {
                                "type": "string",
                                "description": "Path to a .pact file to validate"
                            }
                        },
                        "additionalProperties": false
                    }
                },
                {
                    "name": "pact_docs",
                    "description": "Get PACT language documentation. Returns markdown reference for a topic, or lists all available topics if no topic is specified.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "topic": {
                                "type": "string",
                                "description": "Topic name (e.g. 'quickstart', 'pipeline', 'route', 'db'). Omit to list all topics."
                            }
                        },
                        "additionalProperties": false
                    }
                }
            ]
        }),
    )
}

fn handle_tools_call(id: &serde_json::Value, params: &serde_json::Value) -> serde_json::Value {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let result = match tool_name {
        "pact_run" => execute_pact_run(&arguments),
        "pact_check" => execute_pact_check(&arguments),
        "pact_docs" => execute_pact_docs(&arguments),
        _ => make_tool_result(&format!("Unknown tool: '{}'", tool_name), true),
    };

    make_response(id, result)
}

// --- Main dispatch ---

pub fn handle_message(msg: &serde_json::Value) -> Option<serde_json::Value> {
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = msg.get("id");

    // Notifications (no id) — process silently
    if id.is_none() {
        return None;
    }

    let id = id.unwrap();

    match method {
        "initialize" => Some(handle_initialize(id)),
        "tools/list" => Some(handle_tools_list(id)),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(serde_json::json!({}));
            Some(handle_tools_call(id, &params))
        }
        "" => Some(make_error_response(id, -32600, "Missing 'method' field")),
        _ => Some(make_error_response(
            id,
            -32601,
            &format!("Method not found: '{}'", method),
        )),
    }
}

pub fn run_mcp_server() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut writer = io::BufWriter::new(stdout.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let error = make_error_response(
                    &serde_json::Value::Null,
                    -32700,
                    "Parse error: invalid JSON",
                );
                let _ = writeln!(writer, "{}", error);
                let _ = writer.flush();
                continue;
            }
        };

        if let Some(response) = handle_message(&msg) {
            let _ = writeln!(writer, "{}", response);
            let _ = writer.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_initialize() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "pact");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn test_handle_tools_list() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let resp = handle_message(&msg).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "pact_run");
        assert_eq!(tools[1]["name"], "pact_check");
        assert_eq!(tools[2]["name"], "pact_docs");
        assert!(tools[0]["inputSchema"].is_object());
        assert!(tools[1]["inputSchema"].is_object());
        assert!(tools[2]["inputSchema"].is_object());
    }

    #[test]
    fn test_handle_notification() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_message(&msg).is_none());
    }

    #[test]
    fn test_handle_unknown_method() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "unknown/thing"});
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert!(
            resp["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown/thing")
        );
    }

    #[test]
    fn test_pact_check_valid() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "pact_check",
                "arguments": {"code": "let x: Int = 42"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["valid"], true);
        assert!(text["statements"].as_u64().unwrap() > 0);
        assert!(resp["result"].get("isError").is_none());
    }

    #[test]
    fn test_pact_check_error() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "pact_check",
                "arguments": {"code": "let x: = 42"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        let errors = text["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0]["line"].is_number());
        assert!(errors[0]["message"].is_string());
    }

    #[test]
    fn test_pact_run_expression() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "pact_run",
                "arguments": {"code": "1 + 2"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["result"], 3);
        assert!(resp["result"].get("isError").is_none());
    }

    #[test]
    fn test_pact_run_runtime_error() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "pact_run",
                "arguments": {"code": "x"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        let errors = text["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
        assert_eq!(errors[0]["phase"], "runtime");
    }

    #[test]
    fn test_pact_run_both_params() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "pact_run",
                "arguments": {"code": "1", "file": "test.pact"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn test_pact_run_no_params() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {
                "name": "pact_run",
                "arguments": {}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn test_pact_docs_with_topic() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "pact_docs",
                "arguments": {"topic": "quickstart"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert!(
            resp["result"].get("isError").is_none(),
            "pact_docs with valid topic should not return isError"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("pact"),
            "quickstart docs should contain 'pact'"
        );
    }

    #[test]
    fn test_pact_docs_list() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "pact_docs",
                "arguments": {}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert!(
            resp["result"].get("isError").is_none(),
            "pact_docs list should not return isError"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("quickstart"),
            "topic list should contain 'quickstart'"
        );
    }

    #[test]
    fn test_pact_docs_unknown() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "pact_docs",
                "arguments": {"topic": "nonexistent"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(
            resp["result"]["isError"], true,
            "pact_docs with unknown topic should return isError: true"
        );
    }

    #[test]
    fn test_pact_check_type_error() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "pact_check",
                "arguments": {"code": "let x: Int = \"hello\""}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["valid"], false);
        let errors = text["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
        assert_eq!(errors[0]["phase"], "checker");
    }

    #[test]
    fn test_pact_check_warning_only() {
        let code = "type Status = Active | Inactive | Banned\nlet s: Status = Active\nmatch s {\n  Active => 1\n  Inactive => 2\n}";
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "pact_check",
                "arguments": {"code": code}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert!(
            resp["result"].get("isError").is_none(),
            "warnings-only should not return isError"
        );
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["valid"], true);
        assert!(text["warnings"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_pact_check_clean() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/call",
            "params": {
                "name": "pact_check",
                "arguments": {"code": "let x: Int = 42"}
            }
        });
        let resp = handle_message(&msg).unwrap();
        assert!(resp["result"].get("isError").is_none());
        let text: serde_json::Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["valid"], true);
        assert!(text.get("errors").is_none());
        assert!(text.get("warnings").is_none());
    }
}

use std::collections::HashMap;

use tiny_http::{Header, Response, Server, StatusCode};

use crate::interpreter::Interpreter;
use crate::interpreter::json::{json_to_value, value_to_json};
use crate::interpreter::value::Value;

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

    // Pre-compile path matchers
    let routes: Vec<(String, Vec<PathSegment>, usize)> = interpreter
        .routes
        .iter()
        .enumerate()
        .map(|(i, r)| (r.method.clone(), parse_path_template(&r.path), i))
        .collect();

    for mut request in server.incoming_requests() {
        let method = request.method().to_string().to_uppercase();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or(&url);

        // Parse query string
        let query_params = parse_query_string(&url);

        // Find matching route
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

            // Build request Value
            let req_value =
                build_request_value(&method, path, path_params, query_params, body_value);

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
                        let body = fields.get("body").unwrap_or(&Value::Nothing);
                        let json_body =
                            serde_json::to_string(&value_to_json(body)).unwrap_or_default();
                        make_json_response(status, &json_body)
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
    Response::new(
        StatusCode(status as u16),
        vec![location_header, content_type],
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

fn make_json_response(status: i32, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let header = Header::from_bytes("Content-Type", "application/json").unwrap();
    Response::new(
        StatusCode(status as u16),
        vec![header],
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
}

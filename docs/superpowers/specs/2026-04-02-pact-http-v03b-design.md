# PACT HTTP Server v0.3b Design

## Overview

Add HTTP server to PACT via `tiny_http`. Routes declared in `.pact` files are served as real HTTP endpoints. `app Name { port: N }` starts the server.

## Scope

- Parser: `app Name { port: N }`
- Path matching: `/users/{id}` → extract params
- HTTP server loop: listen, match, execute, respond
- JSON serialization: Value ↔ serde_json
- Request body parsing: automatic JSON → Value
- Response serialization: Value → JSON, always application/json

## Parser Addition

```rust
// Add to Statement enum:
App {
    name: String,
    port: u16,
}
```

Parse rule: `app` keyword → read identifier (name) → expect `{` → expect identifier `"port"` → expect `:` → read IntLiteral → optional comma → expect `}`.

`intent` is NOT required for `app` — it's a configuration block, not a function.

## Path Matching

```rust
enum PathSegment {
    Literal(String),    // "users"
    Param(String),      // "{id}" → "id"
}
```

Compile route path `"/users/{id}"` into `Vec<PathSegment>` by splitting on `/` and checking for `{...}` pattern.

Match incoming URL path against segments:
- Literal segments must match exactly
- Param segments match any non-empty string and capture the value

Returns `Option<HashMap<String, String>>` — None if no match, Some(params) if match.

## HTTP Server

### New file: `src/interpreter/server.rs`

```rust
pub fn start_server(interpreter: &mut Interpreter, name: &str, port: u16) -> Result<(), RuntimeError>
```

Flow:
1. Build path matchers for all stored routes
2. `tiny_http::Server::http(format!("0.0.0.0:{}", port))`
3. Print `{name} listening on http://0.0.0.0:{port}`
4. Loop: receive request
5. Find matching route (method + path)
6. If no match → 404 JSON response
7. Extract path params
8. Parse query string into params
9. Parse JSON body (if Content-Type: application/json)
10. Build request Value::Struct
11. `execute_route(route, request)`
12. Convert Response struct to HTTP response
13. Send response with Content-Type: application/json

### Request struct construction

```rust
Value::Struct {
    type_name: "Request",
    fields: {
        "method": Value::String("GET"),
        "path": Value::String("/users/123"),
        "params": Value::Struct { type_name: "Params", fields: { "id": "123" } },
        "query": Value::Struct { type_name: "Query", fields: { "page": "1" } },
        "body": Value::Nothing | Value::Struct { ... },  // parsed JSON or Nothing
    }
}
```

### Response conversion

`execute_route` returns `Value::Struct { type_name: "Response", fields: { status: Int, body: Value } }`.

Extract `status` as HTTP status code, convert `body` to JSON string, send with Content-Type: application/json.

### Error responses

- Route not found → `{ "error": "Not found" }` with 404
- Route execution error (RuntimeError) → `{ "error": "<message>" }` with 500
- JSON parse error → `{ "error": "Invalid JSON body" }` with 400

## JSON Conversion

### New file: `src/interpreter/json.rs`

```rust
pub fn value_to_json(value: &Value) -> serde_json::Value
pub fn json_to_value(json: &serde_json::Value) -> Value
```

Mapping:
- `Value::Int(n)` ↔ `Number(n)`
- `Value::Float(n)` ↔ `Number(n)`
- `Value::String(s)` ↔ `String(s)`
- `Value::Bool(b)` ↔ `Bool(b)`
- `Value::Nothing` ↔ `Null`
- `Value::List(items)` ↔ `Array`
- `Value::Struct { fields, .. }` ↔ `Object`
- `Value::Variant { variant, fields: None, .. }` → `String(variant)`
- `Value::Variant { variant, fields: Some(f), .. }` → `Object { "type": variant, ...f }`
- `Value::Ok(v)` → converts inner value
- `Value::Error { variant, fields }` → `Object { "error": variant, ...fields }`

## Dependencies

```toml
[dependencies]
tiny_http = "0.12"
serde_json = "1"
```

## CLI Change

`pact run file.pact`:
- If program contains `Statement::App` → start HTTP server (blocks until Ctrl+C)
- If no `App` → execute normally (eval and print result, exit)

## Module Structure

```
src/interpreter/
  server.rs     — start_server, path matching, request/response HTTP handling
  json.rs       — value_to_json, json_to_value
  mod.rs        — add pub mod server, json
```

## Testing Strategy

- `json.rs`: unit tests for Value ↔ JSON roundtrip
- `server.rs`: path matching unit tests (segment parsing, URL matching)
- Integration: manual `curl` testing (HTTP server tests are inherently manual for v0.3b)
- Parser: test `app Name { port: N }` parsing

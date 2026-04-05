# PACT HTTP Client (`http` effect) — Design Spec

## Goal

Add a sync HTTP client to PACT as the `http` effect. Without it PACT is a calculator — with it, it's an instrument. Agents pull external data, pipeline it, respond. "5 lines in PACT = 30 in Python."

## API

### Effect declaration

```pact
route GET "/proxy" {
  intent "fetch external data"
  needs http
  // http.get, http.post, etc. available here
}
```

Same `needs` pattern as `db`, `time`, `rng`. Without `needs http`, the effect is blocked.

### Methods

Four methods: `http.get`, `http.post`, `http.put`, `http.delete`.

```pact
// GET — no options
http.get("https://api.example.com/users")

// GET — with headers
http.get("https://api.example.com/users", {
  headers: { "Authorization": "Bearer ${token}" }
})

// POST — with body
http.post("https://api.example.com/users", {
  body: { name: "Alice", email: "alice@example.com" }
})

// POST — with body + headers + timeout
http.post("https://api.example.com/users", {
  body: { name: "Alice" },
  headers: { "Authorization": "Bearer ${token}" },
  timeout: 5000
})

// PUT
http.put("https://api.example.com/users/1", {
  body: { name: "Alice Updated" }
})

// DELETE
http.delete("https://api.example.com/users/1")
```

**Signature:** `http.method(url: String)` or `http.method(url: String, options: Struct)`

**Options struct fields:**
- `body` — request body, serialized as JSON. Only for POST/PUT/DELETE.
- `headers` — key-value struct of HTTP headers. Optional.
- `timeout` — request timeout in milliseconds. Optional. Default: 30000 (30s).

One options struct, not positional args. Extensible — adding `retry`, `cache`, etc. later won't break anything.

### Response

Every method returns a `Struct`:

```pact
{
  status: 200,            // Int — HTTP status code
  body: [...],            // Value — auto-parsed JSON, or String if not JSON
  headers: {              // Struct — response headers (lowercase keys)
    content_type: "application/json",
    ...
  }
}
```

Response is always a Struct, never an error for HTTP 4xx/5xx. Status codes are data, not errors.

Pipeline shorthand for the common case (80% only need body):

```pact
http.get("https://api.example.com/users")
  | .body
  | filter where .active
  | map to .name
```

### Error handling

One error type: `HttpError`. Returned when the request fails entirely (network, DNS, timeout). NOT for HTTP 4xx/5xx — those are normal responses with a status code.

```pact
http.get("https://api.example.com/users")
  | on HttpError: respond 502 with { error: .message }
  | .body
  | filter where .active
```

`HttpError` fields:
- `message: String` — human-readable error description

Status-based handling through `match`:

```pact
let res: Struct = http.get("https://api.example.com/users")
  | on HttpError: respond 502 with { error: .message }

match res.status {
  200 => res.body | filter where .active,
  404 => respond 404 with { error: "not found" },
  429 => respond 429 with { error: "rate limited" },
  _   => respond 502 with { error: "upstream error" }
}
```

### Testing with mocks

Mocks use PACT's `using` pattern — scoped dependency injection, no global state:

```pact
test "fetch and filter active users" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: [
        { name: "Alice", active: true },
        { name: "Bob", active: false }
      ]},
    "https://api.example.com/roles":
      { status: 200, body: [{ role: "admin" }] }
  })

  let users: List = http.get("https://api.example.com/users")
    | .body
    | filter where .active

  assert users.length() == 1
  assert users.first().name == "Alice"
}
```

`http.mock(url_map)` — takes a struct where keys are URLs, values are response structs `{ status, body, headers }`. Returns an `http` effect that matches URLs against the map. Unmocked URLs return HttpError.

## Architecture

### New dependency

Add `ureq` to `Cargo.toml` — sync, blocking HTTP client. No async, no tokio. Fits PACT's single-threaded tree-walking interpreter.

```toml
ureq = "3"
```

### Implementation pattern

Follows the exact same pattern as existing effects (`db`, `time`, `rng`):

1. **Effect registration** — `setup_test_effects()` and `make_http_effect()` create `Value::Effect { name: "http", methods: {...} }` with BuiltinFn entries for get/post/put/delete/mock
2. **Effect blocking** — add `"http"` to `known_effects` list
3. **Builtin dispatch** — add `"http.get"`, `"http.post"`, etc. to `call_builtin()` match
4. **Implementation** — `builtin_http_get(args)` etc. parse URL + options, make request via ureq, convert response to Value::Struct
5. **Mock state** — `http_mock_responses: Option<HashMap<String, Value>>` on Interpreter. `http.mock()` builtin sets it and returns the effect. When mock is active, builtins check mock map before making real request.
6. **JSON conversion** — response body parsed via `serde_json`, converted to PACT values via existing `json_to_value()` from `src/interpreter/json.rs`. Non-JSON bodies become `Value::String`.

### Response header normalization

HTTP headers are case-insensitive. Normalize to lowercase, replace `-` with `_` for PACT field access:

`Content-Type` → `content_type`
`X-Request-Id` → `x_request_id`

### Request body serialization

POST/PUT/DELETE body is serialized to JSON via existing `value_to_json()`. Content-Type defaults to `application/json` unless overridden in headers.

## Integration

### Effect availability

- `pact run` — real HTTP requests via ureq
- `pact test` — `setup_test_effects()` registers http effect. Without `using http = http.mock(...)`, real requests go through. With mock, requests are intercepted.
- `pact mcp` (pact_run) — `setup_test_effects()` already called. HTTP available but sandboxed (test effects use in-memory db etc.)

### MCP safety

`pact_run` via MCP already uses `setup_test_effects()` for sandboxing. HTTP requests from MCP are real network calls. This is intentional — agents need to test API integrations. The mock system exists for controlled tests.

### pact docs

Add `http` topic to `src/docs/http.md` — methods, options, response format, error handling, testing with mocks. Update `src/docs.rs` and `src/docs/effects.md` cross-references.

## Testing

### Unit tests in `src/interpreter/`

**Mock-based (no real network):**
- `test_http_mock_get` — mock URL, http.get returns mocked response
- `test_http_mock_post` — mock URL, http.post returns mocked response
- `test_http_mock_unknown_url` — unmocked URL returns HttpError
- `test_http_mock_multiple_urls` — mock map with 2+ URLs
- `test_http_response_fields` — response has status, body, headers
- `test_http_get_body_pipeline` — `http.get(url) | .body | filter where ...` works
- `test_http_error_handling` — `on HttpError:` catches mock errors
- `test_http_needs_declaration` — without `needs http`, access blocked
- `test_http_post_with_options` — body and headers from options struct
- `test_http_mock_scoped` — mock in test block doesn't leak to other tests

**Integration (real network, optional):**
- `test_http_real_get` — GET to a known stable URL (httpbin.org or similar). Mark `#[ignore]` so it doesn't run in CI by default.

### pact test files

Create `examples/http_test.pact`:
```pact
test "mock http get" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: [{ name: "Alice", active: true }] }
  })

  let res: Struct = http.get("https://api.example.com/users")
  assert res.status == 200
  assert res.body.length() == 1
}
```

## File structure

### New files
- `src/docs/http.md` — HTTP client documentation topic

### Modified files
- `Cargo.toml` — add `ureq = "3"`
- `src/interpreter/interpreter.rs` — add http effect registration, builtin methods, mock state
- `src/interpreter/mod.rs` — no changes needed (interpreter.rs already has all effect code)
- `src/docs.rs` — add `http` topic
- `src/docs/effects.md` — add http to effects list

## Out of scope

- PATCH, HEAD methods (add when needed)
- Streaming / SSE / WebSocket (needs async runtime)
- Request retry logic (can be added to options later)
- Cookie management
- Redirect following configuration (ureq follows redirects by default — fine for v1)
- File upload / multipart

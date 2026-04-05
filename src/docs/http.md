# HTTP Client

The `http` effect provides GET, POST, PUT, DELETE methods for calling external APIs.

## Syntax

Declare `needs http` in a route or function, then call methods:

```pact
intent "fetch users"
route GET "/proxy" {
  needs http
  http.get("https://api.example.com/users")
    | .body
    | respond 200 with .
}
```

## Methods

```pact
http.get("https://api.example.com/users")

http.post("https://api.example.com/users", {
  body: { name: "Alice" }
})

http.put("https://api.example.com/users/1", {
  body: { name: "Updated" }
})

http.delete("https://api.example.com/users/1")
```

All methods accept `(url)` or `(url, options)`. Options is a struct with optional fields:

- `body` — request body, serialized as JSON (POST/PUT)
- `headers` — key-value struct of HTTP headers
- `timeout` — timeout in milliseconds (default: 30000)

```pact
http.get("https://api.example.com/data", {
  headers: { authorization: "Bearer token123" },
  timeout: 5000
})
```

## Response

Every method returns a struct:

```pact
let res: Struct = http.get("https://api.example.com/data")
res.status
res.body
res.headers
```

- `status` — HTTP status code (Int)
- `body` — parsed JSON value, or String if not JSON
- `headers` — struct with normalized keys (lowercase, hyphens become underscores)

HTTP 4xx/5xx are normal responses, not errors.

## Error handling

`HttpError` is returned only when the request fails entirely (network, DNS, timeout):

```pact
http.get("https://api.example.com/data")
  | on HttpError: respond 502 with { error: .message }
  | .body
```

## Testing with mocks

Use `http.mock()` with `using` for scoped dependency injection:

```pact
test "fetch users" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: list({ name: "Alice", active: true }) }
  })

  let users: List = http.get("https://api.example.com/users")
    | .body
    | filter where .active

  assert users.length() == 1
}
```

Unmocked URLs return `HttpError`. Mocks are scoped to the test block — no global state.

> See also: effects (all built-in effects), pipeline (process response data), route (HTTP routes), test (mock patterns)

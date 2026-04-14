# Effects

Effects declare what side effects a function or route uses. Declare them with `needs`.

## Syntax in functions

```pact
intent "save a record"
fn save(data: Struct) -> Struct
  needs db, time
{
  data
}
```

## Syntax in routes

```pact
intent "create item"
route POST "/items" {
  needs db, rng
  let item: Struct = { id: rng.hex(8), name: request.body.name }
  db.insert("items", item)
  respond 201 with item
}
```

## db — database access

Provides `db.insert`, `db.query`, `db.find`, `db.update`, `db.delete`, `db.delete_where`, `db.watch`.

```pact
db.insert("users", { id: "1", name: "Alice" })
db.query("users") | filter where .active == true
db.find("users", { id: "1" })
db.update("users", "1", { id: "1", name: "Bob" })
db.delete("users", "1")
db.watch("users")
```

`db.watch` returns a stream descriptor for SSE. See `pact docs stream`.

## time — timestamps and date arithmetic

```pact
let now: String = time.now()
let week_ago: String = time.days_ago(7)
```

## rng — random values

```pact
let id: String = rng.uuid()
let hex: String = rng.hex(8)
let code: String = rng.short_id()
```

## auth — JWT authentication

`auth.require(request)` validates the JWT token from the Authorization header. `auth.sign(payload)` creates a new JWT token.

Set `JWT_SECRET` environment variable to enable real JWT validation. Without it, auth runs in dev mode (accepts any Bearer token).

```pact
intent "login"
route POST "/login" {
  needs auth
  let token: String = auth.sign({ id: "user-1", role: "admin" })
  respond 200 with { token: token }
}

intent "protected endpoint"
route GET "/me" {
  needs auth
  let user: User = auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  respond 200 with user
}
```

Run with: `JWT_SECRET=mysecret pact run app.pact`

## log — logging

```pact
log.info("request received")
log.warn("deprecated endpoint")
log.error("something went wrong")
```

## env — environment variables

```pact
let key: String = env.get("API_KEY")
let secret: String = env.require("SECRET")
```

## http — HTTP client

```pact
http.get("https://api.example.com/users")
http.post("https://api.example.com/users", { body: { name: "Alice" } })
http.put("https://api.example.com/users/1", { body: { name: "Bob" } })
http.delete("https://api.example.com/users/1")
```

## Mock effects in tests

```pact
test "mock all effects" {
  using db = db.memory()
  using rng = rng.deterministic(42)
  using time = time.fixed("2026-04-04T12:00:00Z")

  assert rng.hex(4).length() > 0
}
```

> See also: fn, route, test, db

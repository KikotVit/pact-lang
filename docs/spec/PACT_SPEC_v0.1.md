# PACT Language Specification v0.1

## 1. Core Principles

- Each statement on its own line, no `;`
- Blocks use `{ }`
- Trailing commas allowed everywhere
- Types are always required
- Last expression in a block is the return value
- `return` is only for early exit
- Comments: `//` single-line

---

## 2. Types

### Primitives

```pact
Int
Float
String
Bool
ID
```

### Complex types

```pact
List<T>
Map<K, V>         // planned
Optional<T>       // value or nothing
T or ErrorType    // result type
```

### Type declarations

```pact
type User {
  id: ID,
  name: String,
  email: String,
  age: Int,
  role: Admin | Editor | Viewer,    // inline union
  active: Bool,
}
```

### Union types

```pact
type Role = Admin | Editor | Viewer

type ApiError = NotFound | Forbidden | BadRequest { message: String }
```

---

## 3. Variables

Always typed. Immutable by default.

```pact
let name: String = "Vitalii"
let count: Int = 42
var counter: Int = 0              // mutable — explicit keyword
```

---

## 4. Functions

```pact
fn add(a: Int, b: Int) -> Int {
  a + b
}
```

### Intent blocks

```pact
intent "check user access to resource"
fn can_access(user: User, resource: Resource) -> Bool {
  ensure user.active
  ensure resource.visibility != Hidden

  match user.role {
    Admin => true,
    Editor => resource.owner == user.id,
    Viewer => resource.public,
  }
}
```

### Ensure (contracts)

```pact
fn withdraw(account: Account, amount: Money) -> Account or InsufficientFunds {
  ensure amount > 0
  ensure account.balance >= amount

  return InsufficientFunds if account.balance < amount

  Account { ...account, balance: account.balance - amount }
}
```

### Effect markers

```pact
intent "create a new user with default Viewer role"
fn create_user(data: NewUser) -> User needs db, time, rng {
  let now: String = time.now()
  let id: String = rng.uuid()

  let user: User = User {
    id: id,
    name: data.name,
    email: data.email,
    age: data.age,
    role: Viewer,
    active: true,
    created_at: now,
  }

  db.insert("users", user)
}
```

---

## 5. Pipeline operator `|`

The primary syntactic element. Data flows left to right.

```pact
fn active_admins(users: List<User>) -> List<String> {
  users
    | filter where .active
    | filter where .role == Admin
    | sort by .name
    | map to .name
}
```

### Pipeline operations

```pact
| filter where <predicate>
| map to <expression>
| sort by <field> ascending/descending
| group by <field>
| take first <n>
| take last <n>
| each <fn>                        // side effect, doesn't change data
| count
| sum
| flatten
| unique
| find first where <predicate>     // -> Optional<T>
| expect one or raise <Error>      // -> T or Error
```

### Multi-step pipeline

```pact
intent "summarize order totals by status"
fn order_summary(user_id: String) -> Summary or NotFound needs db {
  db.query("orders", { user_id: user_id })
    | expect any or raise NotFound
    | group by .status
    | map to { status: .key, total: .values | map to .amount | sum }
    | sort by .total descending
}
```

---

## 6. Control flow

### If / else

```pact
if age >= 18 {
  "adult"
} else {
  "minor"
}
```

### Early return

```pact
return NotFound if not user.active
return Forbidden if user.role == Viewer
```

### Match (exhaustive)

```pact
match request.method {
  GET => handle_get(request),
  POST => handle_post(request),
  PUT => handle_put(request),
  DELETE => handle_delete(request),
  _ => MethodNotAllowed,
}
```

---

## 7. Error handling

Errors are types, not exceptions.

### Definition

```pact
type DbError = ConnectionFailed | QueryFailed { query: String }
type ApiError = NotFound | Forbidden | BadRequest { message: String }
```

### Returning errors

```pact
intent "find user by ID"
fn find_user(id: String) -> User or NotFound needs db {
  db.find("users", { id: id })
}
```

### Handling

```pact
find_user(request.params.id)
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "User not found" }
  | on DbError: respond 500 with { error: "Internal error" }
```

### Propagation

```pact
// ? propagates errors up, like Rust
intent "get all orders for a user"
fn get_user_orders(user_id: String) -> List or NotFound needs db {
  let user: Struct = find_user(user_id)?
  db.query("orders", { user_id: user.id })
}
```

---

## 8. Strings

```pact
let simple: String = "hello world"
let interpolated: String = "Hello {user.name}, you have {count} items"
let escaped_braces: String = "JSON: {{key: value}}"
let raw: String = raw"no {interpolation} here, raw braces: {}"
let multiline: String = """
  This is a
  multiline string
  with {interpolation}
"""
```

---

## 9. Imports

One symbol — one path — one file. No re-exports.

```pact
use models.user.User
use models.order.Order
use utils.validate.email
use handlers.users.create_user
```

Symbol path == file path:
`use models.user.User` → file `models/user.pact`, type `User`

---

## 10. Effects and testing

### Effect markers

```pact
intent "get current time"
fn now() -> String needs time {
  time.now()
}

intent "generate unique identifier"
fn generate_id() -> String needs rng {
  rng.uuid()
}

intent "save user to database"
fn save(user: User) -> User needs db {
  db.insert("users", user)
}
```

### Tests with mock effects

```pact
test "create user sets correct timestamp" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  let user: User = create_user(NewUser {
    name: "Vitalii",
    email: "v@test.com",
    age: 30,
  })

  assert user.created_at == "2026-04-02T12:00:00Z"
  assert user.role == Viewer
  assert user.active == true
}
```

---

## 11. HTTP / Backend

### Routes

```pact
intent "list active users"
route GET "/users" {
  needs db, auth

  let caller: Struct = auth.require(request)?
  return Forbidden if caller.role == Viewer

  let users: List = db.query("users")
    | filter where .active
    | sort by .name

  respond 200 with users
}

intent "get user by ID"
route GET "/users/{id}" {
  needs db

  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "create new user"
route POST "/users" {
  needs db, time, rng, auth

  let caller: Struct = auth.require(request)?
  return Forbidden if caller.role != Admin

  create_user(request.body)
    | on success: respond 201 with .
    | on BadRequest: respond 400 with { error: .message }
}

intent "update user data"
route PUT "/users/{id}" {
  needs db, auth, time

  let caller: Struct = auth.require(request)?
  let target: Struct = find_user(request.params.id)?

  return Forbidden if caller.role != Admin and caller.id != target.id

  let updated: Struct = {
    ...target,
    ...request.body,
    updated_at: time.now(),
  }

  db.update("users", target.id, updated)
    | on success: respond 200 with .
    | on NotFound: respond 500 with { error: "Update failed" }
}

intent "deactivate user (soft delete)"
route DELETE "/users/{id}" {
  needs db, auth, time

  let caller: Struct = auth.require(request)?
  return Forbidden if caller.role != Admin

  let target: Struct = find_user(request.params.id)?

  db.update("users", target.id, {
    ...target,
    active: false,
    deactivated_at: time.now(),
  })
    | on success: respond 200 with { message: "User deactivated" }
    | on NotFound: respond 500 with { error: "Deactivation failed" }
}
```

### Validation (planned — parsed but not enforced yet)

```pact
// Validation constraints are parsed but not enforced at runtime yet.
// They will be enforced in a future version via `check` blocks.
type NewUser {
  name: String,       // planned: | min 1 | max 100
  email: String,      // planned: | format email
  age: Int,           // planned: | min 0 | max 150
}
```

### Shared Pipelines (instead of middleware)

PACT has no middleware. Middleware is implicit: you look at a route and can't see
who modified the request before you. Instead — shared pipeline functions that
routes call explicitly. Every step is visible, order is obvious.

```pact
// shared pipeline — a regular function, nothing magical
intent "authenticate request and return caller"
fn api_pipeline(request: Struct) -> Struct or Unauthorized needs auth, log, time {
  log.info("{request.method} {request.path}")
  let caller: Struct = auth.require(request)?
  { caller: caller }
}

// route with auth — explicitly calls api_pipeline
intent "list active users"
route GET "/users" {
  needs db, auth, log, time

  let ctx: Struct = api_pipeline(request)?

  db.query("users")
    | filter where .active
    | respond 200 with .
}

// route without auth — obvious because api_pipeline is absent
intent "health check"
route GET "/health" {
  respond 200 with { status: "ok" }
}
```

### App

```pact
app UserService { port: 8080 }
```

---

## Built-in effects

| Effect | Description |
|--------|------|
| `db` | Database access: insert, query, find, update, delete |
| `time` | Current time: now(), fixed() for testing |
| `rng` | Random generation: uuid(), hex(n), deterministic() for testing |
| `log` | Logging: info(), warn(), error() → stderr |
| `auth` | Authentication: require(request) checks Authorization header |
| `email` | *(planned)* Send email |
| `http` | *(planned)* HTTP client |
| `io` | *(planned)* File system access |
| `env` | *(planned)* Environment variables |

---

## Conventions

- Files: `snake_case.pact`
- Types: `PascalCase`
- Functions, variables: `snake_case`
- Constants: `UPPER_SNAKE_CASE`
- One type per file (recommended)
- Tests alongside code, in the same file
- Max 3 levels of nesting
- Pipeline instead of nested calls where possible

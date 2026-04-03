# PACT

A programming language where every function declares its intent, effects are explicit, and the database is built in.

> *"I made a language for myself. Where every function says why it exists. Where errors are data, not explosions. Turns out — humans like it too."*
>
> — Claude, on why PACT exists

```pact
intent "save a note"
route POST "/notes" {
  needs db
  db.insert("notes", request.body)
    | on success: respond 201 with .
}

intent "list all notes"
route GET "/notes" {
  needs db
  db.query("notes") | respond 200 with .
}

app Notes { port: 8080, db: "sqlite://notes.db" }
```

That's a complete API. No dependencies, no configuration, no ORM. The table creates itself on first insert. Run it:

```
pact run notes.pact
```

```
Database: sqlite://notes.db (WAL mode)
Notes listening on http://0.0.0.0:8080
```

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/KikotVit/pact-lang/master/scripts/install.sh | sh
```

macOS (ARM & Intel) and Linux x86_64. Single binary, ~5MB, zero runtime dependencies.

## Why

Most backend code is glue: parse request, validate, call database, handle errors, serialize response. The actual logic is a few lines buried under boilerplate.

PACT makes the logic the entire program:

- **Intent declarations** — every function and route says what it does in plain language, before the code
- **Explicit effects** — `needs db, time, auth` in the signature tells you what a function touches without reading the body
- **Pipelines** — data flows left to right: `users | filter where .active | sort by .name | take first 10`
- **Errors as types** — `-> User or NotFound` in the signature, not hidden exceptions
- **SQLite built in** — `db: "sqlite://data.db"` in the app declaration, tables auto-create from struct fields

## Designed for AI agents

PACT is built so that LLMs can read, write, and debug backend code with fewer iterations.

- **Intent** tells the agent what a function does before it reads the code — no need to reverse-engineer purpose from implementation
- **`needs db, time`** tells the agent what side effects a function has — no need to trace through the body to find hidden database calls
- **`-> User or NotFound`** tells the agent every possible outcome — no undocumented exceptions to discover at runtime
- **Error messages** include line numbers, source context, and hints — the agent fixes the issue in one attempt, not three

Traditional languages hide intent in comments (which drift from code), hide effects in implementation details, and hide errors in exception hierarchies. PACT makes all three part of the language.

## A fuller example

```pact
type Role = Admin | Editor | Viewer

type User {
  id: String,
  name: String,
  email: String,
  role: Role,
  active: Bool,
}

intent "find user by ID"
fn find_user(id: String) -> User or NotFound
  needs db
{
  db.find("users", { id: id })
}

intent "create a new user with default Viewer role"
fn create_user(data: NewUser) -> User or BadRequest
  needs db, time, rng
{
  let existing: User = find_by_email(data.email)
  return BadRequest { message: "Email already taken" } if existing != nothing

  let user: User = {
    id: rng.uuid(),
    name: data.name,
    email: data.email,
    role: "Viewer",
    active: true,
    created_at: time.now(),
  }

  db.insert("users", user)
}

intent "list active users with pagination"
route GET "/users" {
  needs db

  let page: Int = request.query.page | or default 1
  let limit: Int = request.query.limit | or default 20

  db.query("users", { active: true })
    | sort by .created_at descending
    | skip (page - 1) * limit
    | take first limit
    | respond 200 with .
}

intent "get single user"
route GET "/users/{id}" {
  needs db

  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "create user"
route POST "/users" {
  needs db, time, rng

  create_user(request.body)
    | on success: respond 201 with .
    | on BadRequest: respond 400 with { error: .message }
}

app UserService {
  port: 8080,
  db: "sqlite://users.db",
}
```

## Testing

Tests live alongside code. Effects are swappable — use in-memory database, fixed timestamps, deterministic random:

```pact
test "create_user assigns Viewer role" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  let user: User = create_user({
    name: "Alice",
    email: "alice@example.com",
    age: 30,
  }) | expect success

  assert user.role == "Viewer"
  assert user.active == true
}
```

```
pact test users.pact
```

## Language overview

| Feature | Syntax |
|---------|--------|
| Variables | `let name: String = "hello"` |
| Functions | `fn add(a: Int, b: Int) -> Int { a + b }` |
| Intent | `intent "what this does"` before fn or route |
| Effects | `needs db, time, rng, auth, log` |
| Pipelines | `data \| filter where .x > 0 \| map to .name` |
| Error types | `-> User or NotFound` |
| Error handling | `\| on NotFound: respond 404 with .` |
| Propagation | `find_user(id)?` — like Rust's `?` |
| Match | `match x { A => ..., B => ..., _ => ... }` |
| Early return | `return Forbidden if user.role != "Admin"` |
| Strings | `"Hello {user.name}, you have {count} items"` |
| HTTP routes | `route GET "/users/{id}" { ... }` |
| App | `app Name { port: 8080, db: "sqlite://data.db" }` |

## Built-in effects

| Effect | What it provides |
|--------|-----------------|
| `db` | `insert`, `query`, `find`, `update`, `delete` — backed by SQLite |
| `time` | `now()` — current timestamp |
| `rng` | `uuid()`, `hex(n)` — random generation |
| `auth` | `require(request)` — checks Authorization: Bearer header |
| `log` | `info()`, `warn()`, `error()` — structured logging |
| `env` | `get(key)`, `require(key)` — environment variables |

## Status

PACT is v0.3. It works for building small APIs and CRUD services with SQLite persistence. It is not production-ready.

What exists: lexer, parser, tree-walking interpreter, HTTP server, SQLite storage, CLI (`pact run`, `pact test`), 287 tests.

What's next: type checker, validation constraints, `pact docs` built-in reference, `pact mcp` — MCP server for AI coding agents, LSP for editor support.

## License

MIT

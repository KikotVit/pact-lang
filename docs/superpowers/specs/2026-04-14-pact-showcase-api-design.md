# PACT Showcase API — Design Spec

**Date:** 2026-04-14
**Goal:** Self-documenting API that demonstrates PACT's features, deployed on Coolify.

## Overview

One file (`showcase.pact`, ~150 lines) that serves:
- An HTML landing page with docs, curl examples, and PACT source snippets
- A paste bin (create + read)
- A URL shortener (create + redirect)
- An SVG avatar generator
- Usage statistics
- Automatic daily cleanup of old records

Every endpoint showcases a different PACT feature. Every content-type is covered: JSON, HTML, plain text, SVG, redirect.

## New Language Features Required

### 1. Custom Content-Type in `respond`

**Syntax:** `respond <status> with <expr> as "<content-type>"`

```pact
respond 200 with html as "text/html"
respond 200 with svg as "image/svg+xml"
respond 200 with text as "text/plain"
```

Without `as`, default remains `application/json` (current behavior).

**Implementation:** Parser recognizes optional `as <string>` after `respond ... with ...`. Server reads the content-type from the AST and sets the header accordingly. When content-type is not `application/json`, the body is sent as raw string bytes (not JSON-serialized).

### 2. `schedule` blocks

**Syntax:**

```pact
intent "description"
schedule every <duration> {
  needs <effects>
  <body>
}
```

**Duration format:** `<number><unit>` where unit is one of: `ms`, `s`, `m`, `h`, `d`.

Values must be positive integers. Examples: `500ms`, `30s`, `5m`, `24h`, `1d`

**Behavior:**
- First execution: immediately on app start
- Subsequent executions: every `<duration>` after the previous execution completes
- Runs in a background thread, does not block the HTTP server
- Has access to effects (`needs db, time`, etc.) — same as routes
- If the body errors, log the error and continue the schedule (don't crash)

**Implementation:** New AST node `Schedule { interval, needs, body }`. On `app` startup, spawn a thread per schedule block. Each thread: run body → sleep(duration) → loop.

### 3. `time.days_ago(n)`

**Signature:** `time.days_ago(n: Int) -> String`

Returns an ISO 8601 timestamp for `n` days before `time.now()`. Used for cleanup queries.

### 4. `db.delete_where(table, filter)`

**Signature:** `db.delete_where(table: String, filter: Struct) -> Int`

Deletes rows matching the filter. Returns number of deleted rows.

Filter supports a `before` key for timestamp comparison: `{ before: "2026-04-07T00:00:00Z" }` deletes rows where `created_at < value`.

Works with both Memory and SQLite backends.

## Endpoints

### `GET /` — Landing Page

- Returns HTML (`text/html`)
- Shows: project name, list of endpoints with descriptions, curl examples, PACT source snippets
- The HTML is built as a string in PACT code
- **PACT features demonstrated:** string concatenation, custom content-type

### `GET /health` — Health Check

```pact
intent "health check"
route GET "/health" {
  respond 200 with { status: "ok", version: "0.5.0" }
}
```

- **PACT features demonstrated:** simplest possible route, struct literals

### `POST /paste` — Create Paste

```pact
intent "store a text snippet"
route POST "/paste" {
  needs db, rng, time
  let id: String = rng.uuid()
  db.insert("pastes", {
    id: id,
    content: request.body.content,
    created_at: time.now(),
  })
  respond 201 with { id: id }
}
```

- Request body: `{ "content": "some text" }`
- Response: `{ "id": "abc-123" }`
- **PACT features demonstrated:** DB insert, rng, time, needs (explicit effects)

### `GET /paste/:id` — Read Paste

```pact
intent "retrieve a stored snippet"
route GET "/paste/{id}" {
  needs db
  db.find("pastes", { id: request.params.id })
    | on success: respond 200 with .content as "text/plain"
    | on NotFound: respond 404 with { error: "Paste not found" }
}
```

- Returns raw text (`text/plain`)
- **PACT features demonstrated:** DB lookup, error handling (errors as values), pipelines, custom content-type

### `POST /shorten` — Create Short Link

```pact
intent "shorten a URL"
route POST "/shorten" {
  needs db, rng, time
  let code: String = rng.short_id()
  db.insert("links", {
    code: code,
    url: request.body.url,
    created_at: time.now(),
  })
  respond 201 with { code: code, short_url: "/s/" + code }
}
```

- Request body: `{ "url": "https://example.com" }`
- Response: `{ "code": "x7k2", "short_url": "/s/x7k2" }`
- **PACT features demonstrated:** DB, string concatenation, rng

### `GET /s/:code` — Redirect

```pact
intent "redirect to original URL"
route GET "/s/{code}" {
  needs db
  db.find("links", { code: request.params.code })
    | on success: respond 302 with { location: .url }
    | on NotFound: respond 404 with { error: "Link not found" }
}
```

- 302 redirect to original URL
- **PACT features demonstrated:** redirect responses, error handling, pipelines

### `GET /avatar/:name` — SVG Identicon

```pact
intent "generate avatar from name"
fn make_avatar(name: String) -> String {
  let colors: List<String> = list("#e74c3c", "#3498db", "#2ecc71", "#f39c12", "#9b59b6", "#1abc9c")
  let seed: Int = name | chars | map .code | sum
  let color: String = colors | get (seed % colors.length)

  let cells: List<Int> = list(
    seed % 2, (seed / 2) % 2, (seed / 4) % 2,
    (seed / 8) % 2, (seed / 16) % 2, (seed / 32) % 2,
    (seed / 64) % 2, (seed / 128) % 2, (seed / 256) % 2
  )

  // Build 5x5 mirrored grid SVG from 3x3 seed cells
  // (implementation details in code — generates <rect> elements)
  "<svg ...>" + rects + "</svg>"
}

intent "serve avatar as SVG"
route GET "/avatar/{name}" {
  let svg: String = make_avatar(request.params.name)
  respond 200 with svg as "image/svg+xml"
}
```

- Returns SVG image
- Deterministic: same name → same avatar
- 5x5 mirrored grid (like GitHub identicons but simpler)
- **PACT features demonstrated:** pipelines, list operations, map, string building, custom content-type, pure functions

**Required new primitives for avatar:**
- `name.chars()` — returns `List<String>` of single characters (e.g., `"abc".chars()` → `list("a", "b", "c")`)
- `char.code()` — returns `Int` Unicode code point of first character (e.g., `"a".code()` → `97`)
- `list | sum` — pipeline operator that sums a `List<Int>` (e.g., `list(1, 2, 3) | sum` → `6`)

These are small, generally useful primitives — not avatar-specific.

### `GET /stats` — Usage Statistics

```pact
intent "show API usage stats"
route GET "/stats" {
  needs db
  let pastes: List = db.query("pastes")
  let links: List = db.query("links")
  respond 200 with {
    total_pastes: pastes | count,
    total_links: links | count,
    recent_pastes: pastes | sort by .created_at descending | take first 5,
  }
}
```

- **PACT features demonstrated:** DB queries, pipelines (sort, take, count), struct building

### Scheduled Cleanup

```pact
intent "clean up records older than 7 days"
schedule every 1d {
  needs db, time
  let cutoff: String = time.days_ago(7)
  db.delete_where("pastes", { before: cutoff })
  db.delete_where("links", { before: cutoff })
}
```

- Runs immediately on app start, then every 24h
- **PACT features demonstrated:** `schedule` (new), background tasks, DB cleanup

## App Declaration

```pact
app Showcase { port: 8080 }
```

## DB Schema

Two tables, auto-created on first insert (existing PACT behavior):

**pastes:**
| Column | Type | Description |
|---|---|---|
| id | String | UUID, primary key |
| content | String | Paste content |
| created_at | String | ISO 8601 timestamp |

**links:**
| Column | Type | Description |
|---|---|---|
| code | String | Short code, primary key |
| url | String | Original URL |
| created_at | String | ISO 8601 timestamp |

## Deployment

- **Platform:** Coolify (self-hosted PaaS on user's VPS)
- **Docker:** Simple Dockerfile — build Rust binary, copy `showcase.pact`, run `pact run showcase.pact`
- **DB:** SQLite file, persisted via Coolify volume
- **Env vars:** `ADMIN_KEY` (optional, for future admin endpoints)

## Testing

- Rust tests for each new language feature (content-type, schedule, time.days_ago, db.delete_where)
- PACT test blocks for showcase business logic (paste creation, link shortening, cleanup)
- Manual curl tests against running server

## New Language Primitives Summary

| Primitive | Type | Purpose |
|---|---|---|
| `respond ... as "type"` | Syntax | Custom Content-Type |
| `schedule every <dur> {}` | Syntax | Background scheduled tasks |
| Duration literals (`ms`, `s`, `m`, `h`, `d`) | Syntax | Time durations |
| `time.days_ago(n)` | Effect method | Date arithmetic |
| `db.delete_where(table, filter)` | Effect method | Filtered row deletion |
| `rng.short_id()` | Effect method | 8-char alphanumeric random ID |
| `str.chars()` | String method | Split string into list of characters |
| `str.code()` | String method | Unicode code point of first char |
| `list \| sum` | Pipeline op | Sum a List<Int> |

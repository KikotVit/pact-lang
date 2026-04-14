# PACT Showcase API — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a self-documenting PACT Showcase API with 7 endpoints, scheduled cleanup, and Dockerfile — requiring 6 new language features.

**Architecture:** Each language feature is added TDD-style to the existing tree-walking interpreter. Features touch 4 layers: lexer/token → parser/AST → interpreter → server. The showcase app is a single `showcase.pact` file that exercises all new features.

**Tech Stack:** Rust, tiny_http, rusqlite, chrono (new dependency), Docker

**Spec:** `docs/superpowers/specs/2026-04-14-pact-showcase-api-design.md`

---

## File Map

| File | Changes |
|---|---|
| `Cargo.toml` | Add `chrono` dependency |
| `src/lexer/token.rs:16-54` | Add `Schedule` keyword variant |
| `src/lexer/token.rs:162-190` | Add `"schedule"` to `keyword_from_str` |
| `src/parser/ast.rs:47-66` | Add `Schedule` variant to `Statement` enum |
| `src/parser/ast.rs:161-164` | Add `content_type` field to `Expr::Respond` |
| `src/parser/parser.rs:616-625` | Parse `respond ... as "type"` |
| `src/parser/parser.rs:874-885` | Dispatch `schedule` after `intent` |
| `src/parser/parser.rs` (after 1486) | Add `parse_schedule_with_intent()` |
| `src/interpreter/interpreter.rs:21-36` | Add `StoredSchedule` struct |
| `src/interpreter/interpreter.rs:38-63` | Add `schedules: Vec<StoredSchedule>` field |
| `src/interpreter/interpreter.rs:370-412` | Handle `Statement::Schedule` |
| `src/interpreter/interpreter.rs:792-816` | Pass `content_type` through Respond |
| `src/interpreter/interpreter.rs:1345-1399` | Add `chars()`, `code()` to String methods |
| `src/interpreter/interpreter.rs:1497-1601` | Add `db.delete_where`, `time.days_ago`, `rng.short_id` builtins |
| `src/interpreter/interpreter.rs:2117-2123` | Fix `time.now()` to use real time |
| `src/interpreter/db.rs:158+` | Add `delete_where()` method |
| `src/interpreter/server.rs:231+` | Spawn schedule threads on app start |
| `src/interpreter/server.rs:536-548` | Support custom content-type in responses |
| `src/formatter.rs:440-446` | Format `respond ... as "type"` |
| `src/formatter.rs:225-263` | Format `Schedule` statement |
| `src/checker.rs:927-934` | Handle `Schedule` in type checker |
| `examples/showcase.pact` | **Create:** The showcase application |
| `Dockerfile` | **Create:** Multi-stage Docker build |

---

### Task 1: Add `chrono` dependency and fix `time.now()`

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/interpreter/interpreter.rs:2117-2123`
- Test: `src/interpreter/interpreter.rs` (existing test module)

- [ ] **Step 1: Write failing test for real `time.now()`**

In `src/interpreter/interpreter.rs`, in the `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn test_time_now_returns_current_time() {
    let mut interp = Interpreter::new("");
    let result = interp.builtin_time_now().unwrap();
    if let Value::String(s) = result {
        // Should be a real ISO 8601 timestamp, not the old hardcoded one
        assert!(s.contains("T"), "Expected ISO 8601 format, got: {}", s);
        assert!(s.ends_with("Z"), "Expected UTC timezone, got: {}", s);
        assert_ne!(s, "2026-04-02T12:00:00Z", "Should not be hardcoded");
    } else {
        panic!("Expected String, got: {:?}", result);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_time_now_returns_current_time -- --nocapture`
Expected: FAIL — currently returns hardcoded `"2026-04-02T12:00:00Z"`

- [ ] **Step 3: Add chrono dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
chrono = "0.4"
```

- [ ] **Step 4: Fix `builtin_time_now` to use real time**

In `src/interpreter/interpreter.rs`, replace the `builtin_time_now` method (line ~2117):

```rust
fn builtin_time_now(&self) -> Result<Value, RuntimeError> {
    let time_str = self.fixed_time.clone().unwrap_or_else(|| {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    });
    Ok(Value::String(time_str))
}
```

Add at top of file:

```rust
use chrono;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_time_now_returns_current_time -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write test for `time.days_ago()`**

```rust
#[test]
fn test_time_days_ago() {
    let mut interp = Interpreter::new("");
    interp.fixed_time = Some("2026-04-14T12:00:00Z".to_string());
    let result = interp
        .builtin_time_days_ago(vec![Value::Int(7)])
        .unwrap();
    assert_eq!(result, Value::String("2026-04-07T12:00:00Z".to_string()));
}

#[test]
fn test_time_days_ago_no_args() {
    let mut interp = Interpreter::new("");
    let result = interp.builtin_time_days_ago(vec![]);
    assert!(result.is_err());
}
```

- [ ] **Step 7: Run test to verify it fails**

Run: `cargo test test_time_days_ago -- --nocapture`
Expected: FAIL — method does not exist

- [ ] **Step 8: Implement `builtin_time_days_ago`**

In `src/interpreter/interpreter.rs`, after `builtin_time_now`:

```rust
fn builtin_time_days_ago(&self, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let days = match args.first() {
        Some(Value::Int(n)) => *n,
        _ => return Err(self.error("time.days_ago expects 1 argument: days (Int)")),
    };
    let base = self.fixed_time.clone().unwrap_or_else(|| {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    });
    let dt = chrono::NaiveDateTime::parse_from_str(&base, "%Y-%m-%dT%H:%M:%SZ")
        .map_err(|e| self.error(&format!("Failed to parse time: {}", e)))?;
    let result = dt - chrono::Duration::days(days);
    Ok(Value::String(result.format("%Y-%m-%dT%H:%M:%SZ").to_string()))
}
```

Add dispatch in the `match name` block (after `"time.now"`):

```rust
"time.days_ago" => self.builtin_time_days_ago(args),
```

Update the hint string at line ~1599 to include `time.days_ago()`.

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test test_time_days_ago -- --nocapture`
Expected: PASS (both tests)

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: All ~496 tests pass

- [ ] **Step 11: Commit**

```bash
cargo fmt
git add Cargo.toml Cargo.lock src/interpreter/interpreter.rs
git commit -m "feat: fix time.now() to return real time, add time.days_ago(n)"
```

---

### Task 2: Add `respond ... as "content-type"`

**Files:**
- Modify: `src/parser/ast.rs:161-164`
- Modify: `src/parser/parser.rs:616-625`
- Modify: `src/interpreter/interpreter.rs:792-816`
- Modify: `src/interpreter/server.rs:536-548`
- Modify: `src/formatter.rs:440-446`
- Test: `src/parser/parser.rs` (test module), `src/interpreter/interpreter.rs` (test module)

- [ ] **Step 1: Write failing parser test**

In `src/parser/parser.rs` test module, add:

```rust
#[test]
fn parse_respond_with_content_type() {
    let expr = parse_expr("respond 200 with html as \"text/html\"");
    match expr {
        Expr::Respond { status, body, content_type } => {
            assert!(matches!(*status, Expr::IntLiteral(200)));
            assert!(matches!(*body, Expr::Identifier(ref n) if n == "html"));
            assert_eq!(content_type, Some("text/html".to_string()));
        }
        other => panic!("Expected Respond, got: {:?}", other),
    }
}

#[test]
fn parse_respond_without_content_type() {
    let expr = parse_expr("respond 200 with data");
    match expr {
        Expr::Respond { content_type, .. } => {
            assert_eq!(content_type, None);
        }
        other => panic!("Expected Respond, got: {:?}", other),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_respond_with_content_type -- --nocapture`
Expected: FAIL — `Respond` has no `content_type` field

- [ ] **Step 3: Add `content_type` to AST**

In `src/parser/ast.rs`, change `Expr::Respond`:

```rust
Respond {
    status: Box<Expr>,
    body: Box<Expr>,
    content_type: Option<String>,
},
```

- [ ] **Step 4: Fix all compilation errors from the new field**

After adding the field, the compiler will report every place that constructs or pattern-matches `Expr::Respond`. Fix each one:

**`src/parser/parser.rs:616-625`** — parser construction:
```rust
TokenKind::Identifier(ref name) if name == "respond" => {
    self.advance();
    let status = self.parse_or()?;
    self.expect_contextual("with")?;
    let body = self.parse_or()?;
    // Optional: as "content-type"
    let content_type = if self.eat(&TokenKind::As) {
        match self.current_token().kind {
            TokenKind::RawStringLiteral(ref s) => {
                let ct = s.clone();
                self.advance();
                Some(ct)
            }
            _ => return Err(self.error(
                "Expected string after 'as' in respond",
                Some("Example: respond 200 with body as \"text/html\""),
            )),
        }
    } else {
        None
    };
    Ok(Expr::Respond {
        status: Box::new(status),
        body: Box::new(body),
        content_type,
    })
}
```

**`src/interpreter/interpreter.rs:792-816`** — interpreter evaluation:
```rust
Expr::Respond { status, body, content_type } => {
    let status_val = self.eval_expr(status, env)?;
    let body_val = self.eval_expr(body, env)?;
    let mut fields = HashMap::new();
    fields.insert("status".to_string(), status_val.clone());
    fields.insert("body".to_string(), body_val.clone());
    if let Some(ct) = content_type {
        fields.insert("content_type".to_string(), Value::String(ct.clone()));
    }
    // For redirects, extract location to top level
    if let Value::Int(code) = &status_val {
        if matches!(code, 301 | 302 | 307 | 308) {
            if let Value::Struct { fields: body_fields, .. } = &body_val {
                if let Some(loc) = body_fields.get("location") {
                    fields.insert("location".to_string(), loc.clone());
                }
            }
        }
    }
    Ok(Value::Struct {
        type_name: "Response".to_string(),
        fields,
    })
}
```

**`src/formatter.rs:440-446`** — formatter:
```rust
Expr::Respond { status, body, content_type } => {
    let base = format!(
        "respond {} with {}",
        self.format_expr(status),
        self.format_expr(body)
    );
    match content_type {
        Some(ct) => format!("{} as \"{}\"", base, ct),
        None => base,
    }
}
```

**`src/checker.rs`** — find and fix any pattern matches on `Expr::Respond`. Use `..` to ignore the new field if the checker doesn't need it.

- [ ] **Step 5: Run parser test to verify it passes**

Run: `cargo test parse_respond_with_content_type parse_respond_without_content_type -- --nocapture`
Expected: PASS

- [ ] **Step 6: Write server test for custom content-type**

In `src/interpreter/interpreter.rs` test module:

```rust
#[test]
fn test_respond_with_content_type() {
    let result = run("respond 200 with \"<h1>Hello</h1>\" as \"text/html\"");
    if let Value::Struct { fields, .. } = result {
        assert_eq!(fields.get("content_type"), Some(&Value::String("text/html".to_string())));
    } else {
        panic!("Expected Struct, got: {:?}", result);
    }
}
```

- [ ] **Step 7: Run test to verify it passes** (should already pass from Step 4)

Run: `cargo test test_respond_with_content_type -- --nocapture`
Expected: PASS

- [ ] **Step 8: Update server to use custom content-type**

In `src/interpreter/server.rs`, in the route result handler (around line 428-432), before calling `make_json_response`, check for custom content-type:

```rust
// After extracting status and body from the Response struct:
let custom_ct = fields.get("content_type").and_then(|v| {
    if let Value::String(ct) = v { Some(ct.clone()) } else { None }
});

if let Some(ct) = custom_ct {
    let body_str = match fields.get("body") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(&value_to_json(other)).unwrap_or_default(),
        None => String::new(),
    };
    make_response_with_content_type(status, &body_str, &ct)
} else {
    let json_body = serde_json::to_string(&value_to_json(body)).unwrap_or_default();
    make_json_response(status, &json_body)
}
```

Add the new function:

```rust
fn make_response_with_content_type(status: i32, body: &str, content_type: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let ct_header = Header::from_bytes("Content-Type", content_type).unwrap();
    let mut headers = vec![ct_header];
    add_cors_headers(&mut headers);
    Response::new(
        StatusCode(status as u16),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}
```

Apply the same change to the stream result handler (~line 338-346).

- [ ] **Step 9: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 10: Commit**

```bash
cargo fmt
git add src/parser/ast.rs src/parser/parser.rs src/interpreter/interpreter.rs src/interpreter/server.rs src/formatter.rs src/checker.rs
git commit -m "feat: add custom content-type support in respond (as \"type\")"
```

---

### Task 3: Add `str.chars()` and `str.code()` string methods

**Files:**
- Modify: `src/interpreter/interpreter.rs:1352-1399`
- Test: `src/interpreter/interpreter.rs` (test module)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_string_chars() {
    let result = run("\"abc\".chars()");
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ])
    );
}

#[test]
fn test_string_chars_empty() {
    let result = run("\"\".chars()");
    assert_eq!(result, Value::List(vec![]));
}

#[test]
fn test_string_code() {
    let result = run("\"a\".code()");
    assert_eq!(result, Value::Int(97));
}

#[test]
fn test_string_code_unicode() {
    let result = run("\"é\".code()");
    assert_eq!(result, Value::Int(233));
}

#[test]
fn test_chars_then_code_pipeline() {
    let result = run("\"ab\" | chars | map .code() | sum");
    // 'a' = 97, 'b' = 98, sum = 195
    assert_eq!(result, Value::Int(195));
}
```

Note: The pipeline test (`chars_then_code_pipeline`) uses `| chars` as a pipeline operator. But `chars` currently only exists as a method. We need it to work as `| chars` too. Check if it should be a pipeline step or just a method. Looking at the spec: `name | chars | map .code | sum`. This means `| chars` is a pipeline step. We'll need to add it as a PipelineStep as well.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_string_chars test_string_code -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement `chars()` and `code()` in call_method**

In `src/interpreter/interpreter.rs`, in the `Value::String(s) => match method` block (after line ~1377), add before the fallback `_` arm:

```rust
"chars" => Ok(Value::List(
    s.chars().map(|c| Value::String(c.to_string())).collect(),
)),
"code" => {
    if let Some(c) = s.chars().next() {
        Ok(Value::Int(c as i64))
    } else {
        Err(self.error("Cannot get code of empty string"))
    }
}
```

Update the `string_methods` vec (line ~1385) to include `"chars"` and `"code"`.

- [ ] **Step 4: Add `| chars` pipeline step**

In `src/parser/ast.rs`, add `Chars` to the `PipelineStep` enum (near `Count`, `Sum`):

```rust
Chars,
```

In `src/parser/parser.rs`, in `parse_pipeline_step` (near `"count"` / `"sum"` handling, around line 360):

```rust
"chars" => {
    self.advance();
    PipelineStep::Chars
}
```

In `src/interpreter/interpreter.rs`, in `eval_pipeline_step` (near the `Sum` handler, around line 979), add:

```rust
PipelineStep::Chars => {
    match &current {
        Value::String(s) => {
            Ok(Value::List(s.chars().map(|c| Value::String(c.to_string())).collect()))
        }
        _ => Err(self.error(&format!("'chars' expects a String, got {}", current.type_name()))),
    }
}
```

In `src/formatter.rs`, in the pipeline step formatter, add:

```rust
PipelineStep::Chars => "chars".to_string(),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_string_chars test_string_code test_chars_then_code -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add src/parser/ast.rs src/parser/parser.rs src/interpreter/interpreter.rs src/formatter.rs
git commit -m "feat: add str.chars(), str.code() methods and | chars pipeline"
```

---

### Task 4: Add `rng.short_id()`

**Files:**
- Modify: `src/interpreter/interpreter.rs:1497-1601` (builtin dispatch + new method)
- Test: `src/interpreter/interpreter.rs` (test module)

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_rng_short_id_deterministic() {
    let result = run("using rng = rng.deterministic(42)\nrng.short_id()");
    if let Value::String(s) = result {
        assert_eq!(s.len(), 8, "short_id should be 8 chars, got: {}", s);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()), "Should be alphanumeric: {}", s);
    } else {
        panic!("Expected String, got: {:?}", result);
    }
}

#[test]
fn test_rng_short_id_sequence() {
    let result = run("using rng = rng.sequence(list(\"abc123\"))\nrng.short_id()");
    assert_eq!(result, Value::String("abc123".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_rng_short_id -- --nocapture`
Expected: FAIL — unknown builtin

- [ ] **Step 3: Implement `builtin_rng_short_id`**

In `src/interpreter/interpreter.rs`, after `builtin_rng_hex` (~line 2168):

```rust
fn builtin_rng_short_id(&mut self) -> Result<Value, RuntimeError> {
    if let Some(val) = self.next_from_sequence() {
        return Ok(Value::String(val));
    }
    self.rng_counter += 1;
    let seed = self.rng_seed.unwrap_or(42);
    let mut value = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(self.rng_counter);
    let chars: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut id = String::with_capacity(8);
    for _ in 0..8 {
        value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
        id.push(chars[((value >> 32) as usize) % chars.len()] as char);
    }
    Ok(Value::String(id))
}
```

Add dispatch in `match name` (after `"rng.hex"`):

```rust
"rng.short_id" => self.builtin_rng_short_id(),
```

Update the hint string at line ~1599 to include `rng.short_id()`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_rng_short_id -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/interpreter/interpreter.rs
git commit -m "feat: add rng.short_id() for 8-char alphanumeric IDs"
```

---

### Task 5: Add `db.delete_where(table, filter)`

**Files:**
- Modify: `src/interpreter/db.rs`
- Modify: `src/interpreter/interpreter.rs:1497-1503`
- Test: `src/interpreter/interpreter.rs` (test module)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_db_delete_where_before() {
    let result = run(r#"
        using db = db.memory()
        using time = time.fixed("2026-04-14T12:00:00Z")
        db.insert("items", { id: "1", name: "old", created_at: "2026-04-01T00:00:00Z" })
        db.insert("items", { id: "2", name: "new", created_at: "2026-04-13T00:00:00Z" })
        db.delete_where("items", { before: "2026-04-10T00:00:00Z" })
    "#);
    // Should return count of deleted rows
    assert_eq!(result, Value::Int(1));
}

#[test]
fn test_db_delete_where_keeps_recent() {
    let result = run(r#"
        using db = db.memory()
        db.insert("items", { id: "1", name: "old", created_at: "2026-04-01T00:00:00Z" })
        db.insert("items", { id: "2", name: "new", created_at: "2026-04-13T00:00:00Z" })
        db.delete_where("items", { before: "2026-04-10T00:00:00Z" })
        db.query("items") | count
    "#);
    assert_eq!(result, Value::Int(1));
}

#[test]
fn test_db_delete_where_empty_table() {
    let result = run(r#"
        using db = db.memory()
        db.delete_where("nonexistent", { before: "2026-04-10T00:00:00Z" })
    "#);
    assert_eq!(result, Value::Int(0));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_db_delete_where -- --nocapture`
Expected: FAIL — unknown builtin

- [ ] **Step 3: Implement `delete_where` on `DbBackend`**

In `src/interpreter/db.rs`, after the `delete` method (~line 522), add:

```rust
/// Delete rows where `created_at` < the `before` value in the filter.
/// Returns the count of deleted rows.
pub fn delete_where(&mut self, table: &str, filter: &Value) -> Result<Value, RuntimeError> {
    let before = match filter {
        Value::Struct { fields, .. } => {
            match fields.get("before") {
                Some(Value::String(s)) => s.clone(),
                _ => return Err(RuntimeError {
                    message: "db.delete_where filter must have a 'before' key with String value".to_string(),
                    line: 0, column: 0, source_line: String::new(), hint: None,
                }),
            }
        }
        _ => return Err(RuntimeError {
            message: "db.delete_where second argument must be a Struct filter".to_string(),
            line: 0, column: 0, source_line: String::new(), hint: None,
        }),
    };

    match self {
        DbBackend::Memory { tables } => {
            if let Some(rows) = tables.get_mut(table) {
                let before_count = rows.len();
                rows.retain(|row| {
                    if let Value::Struct { fields, .. } = row {
                        if let Some(Value::String(created)) = fields.get("created_at") {
                            return created.as_str() >= before.as_str();
                        }
                    }
                    true // keep rows without created_at
                });
                Ok(Value::Int((before_count - rows.len()) as i64))
            } else {
                Ok(Value::Int(0))
            }
        }
        DbBackend::Sqlite { conn, schemas } => {
            if schemas.get(table).is_none() {
                return Ok(Value::Int(0));
            }
            let sql = format!("DELETE FROM \"{}\" WHERE \"created_at\" < ?", table);
            match conn.execute(&sql, rusqlite::params![before]) {
                Ok(count) => Ok(Value::Int(count as i64)),
                Err(e) => Ok(db_value_error(&format!("delete_where on '{}'", table), e)),
            }
        }
    }
}
```

- [ ] **Step 4: Add interpreter dispatch**

In `src/interpreter/interpreter.rs`, add `builtin_db_delete_where` method:

```rust
fn builtin_db_delete_where(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        return Err(self.error("db.delete_where expects 2 arguments: table name, filter struct"));
    }
    let table_name = match &args[0] {
        Value::String(s) => s.clone(),
        _ => return Err(self.error("db.delete_where first argument must be a String table name")),
    };
    self.db.delete_where(&table_name, &args[1])
}
```

Add to `match name` block (after `"db.delete"`):

```rust
"db.delete_where" => self.builtin_db_delete_where(args),
```

Update the hint string at line ~1599 to include `db.delete_where()`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_db_delete_where -- --nocapture`
Expected: PASS (all 3 tests)

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add src/interpreter/db.rs src/interpreter/interpreter.rs
git commit -m "feat: add db.delete_where(table, filter) with before: timestamp"
```

---

### Task 6: Add `schedule every <duration> { }` blocks

**Files:**
- Modify: `src/lexer/token.rs:16-54, 92-190`
- Modify: `src/parser/ast.rs:47-66`
- Modify: `src/parser/parser.rs:874-885`
- Modify: `src/interpreter/interpreter.rs:21-63, 370-412`
- Modify: `src/interpreter/server.rs:231+`
- Modify: `src/formatter.rs:225-263`
- Modify: `src/checker.rs:927-934`
- Test: parser, interpreter, lexer test modules

- [ ] **Step 1: Add `Schedule` keyword to lexer**

In `src/lexer/token.rs`, add `Schedule` to `TokenKind` enum (after `App` on line 46):

```rust
Schedule,
```

In `keyword_from_str` (after `"app"` on line 179):

```rust
"schedule" => Some(TokenKind::Schedule),
```

In the `Display` impl (after `App`):

```rust
TokenKind::Schedule => write!(f, "'schedule'"),
```

Update the comment from "Keywords (24 reserved)" to "Keywords (25 reserved)".

- [ ] **Step 2: Run lexer tests**

Run: `cargo test --lib lexer -- --nocapture`
Expected: PASS (may need to update the `all_24_keywords` test to `all_25_keywords` and add `Schedule`)

- [ ] **Step 3: Add `Schedule` to AST**

In `src/parser/ast.rs`, add to `Statement` enum (after `Stream`, before `App`):

```rust
Schedule {
    intent: String,
    interval_ms: u64,
    effects: Vec<String>,
    body: Vec<Statement>,
},
```

- [ ] **Step 4: Write failing parser test**

In `src/parser/parser.rs` test module:

```rust
#[test]
fn parse_schedule_block() {
    let input = "intent \"cleanup\"\nschedule every 24h {\n  needs db, time\n}";
    let program = parse(input).unwrap();
    match &program.statements[0] {
        Statement::Schedule { intent, interval_ms, effects, .. } => {
            assert_eq!(intent, "cleanup");
            assert_eq!(*interval_ms, 24 * 60 * 60 * 1000); // 24h in ms
            assert_eq!(effects, &vec!["db".to_string(), "time".to_string()]);
        }
        other => panic!("Expected Schedule, got: {:?}", other),
    }
}

#[test]
fn parse_schedule_duration_formats() {
    // Test all duration units
    for (input, expected_ms) in [
        ("500ms", 500u64),
        ("30s", 30_000),
        ("5m", 300_000),
        ("2h", 7_200_000),
        ("1d", 86_400_000),
    ] {
        let code = format!("intent \"test\"\nschedule every {} {{\n}}", input);
        let program = parse(&code).unwrap();
        match &program.statements[0] {
            Statement::Schedule { interval_ms, .. } => {
                assert_eq!(*interval_ms, expected_ms, "Failed for input: {}", input);
            }
            other => panic!("Expected Schedule for '{}', got: {:?}", input, other),
        }
    }
}
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test parse_schedule -- --nocapture`
Expected: FAIL — parser doesn't know `schedule`

- [ ] **Step 6: Implement parser**

In `src/parser/parser.rs`, in the `intent` dispatch block (line ~878), add a branch for `Schedule`:

```rust
TokenKind::Intent => {
    self.advance();
    let intent = self.parse_intent_string()?;
    self.skip_newlines();
    if self.at(&TokenKind::Route) {
        self.parse_route_with_intent(intent)?
    } else if self.at(&TokenKind::Stream) {
        self.parse_stream_with_intent(intent)?
    } else if self.at(&TokenKind::Schedule) {
        self.parse_schedule_with_intent(intent)?
    } else {
        self.parse_fn_decl(Some(intent))?
    }
}
```

Add error for bare `schedule` without `intent` (after the `Stream` error, ~line 892):

```rust
TokenKind::Schedule => {
    return Err(self.error(
        "Missing 'intent' block before schedule declaration",
        Some("Write: intent \"description\" on the line before schedule"),
    ));
}
```

Add the `parse_schedule_with_intent` method (after `parse_stream_with_intent`):

```rust
fn parse_schedule_with_intent(&mut self, intent: String) -> Result<Statement, ParseError> {
    self.advance(); // consume `schedule`

    // Expect "every"
    self.expect_contextual("every")?;

    // Parse duration: <number><unit>
    let interval_ms = self.parse_duration()?;

    self.push_block("schedule");
    self.expect(&TokenKind::LBrace)?;
    self.skip_newlines();

    // Optional: needs
    let mut effects = Vec::new();
    if self.eat(&TokenKind::Needs)
        && !self.at(&TokenKind::LBrace)
        && !self.at(&TokenKind::Newline)
    {
        effects.push(self.expect_identifier()?);
        while self.eat(&TokenKind::Comma) {
            if self.at(&TokenKind::LBrace) || self.at(&TokenKind::Newline) {
                break;
            }
            effects.push(self.expect_identifier()?);
        }
    }
    self.skip_newlines();

    let body = self.parse_block_body()?;
    self.expect_closing_brace()?;

    Ok(Statement::Schedule {
        intent,
        interval_ms,
        effects,
        body,
    })
}

fn parse_duration(&mut self) -> Result<u64, ParseError> {
    // Duration is a single token like "24h", "500ms", "30s", "5m", "1d"
    // The lexer sees it as IntLiteral followed by Identifier
    let number = match self.current_token().kind {
        TokenKind::IntLiteral(n) => {
            self.advance();
            n as u64
        }
        _ => return Err(self.error(
            "Expected duration number (e.g., 24h, 500ms)",
            Some("Valid units: ms, s, m, h, d"),
        )),
    };
    let unit = self.expect_identifier()?;
    match unit.as_str() {
        "ms" => Ok(number),
        "s" => Ok(number * 1000),
        "m" => Ok(number * 60 * 1000),
        "h" => Ok(number * 60 * 60 * 1000),
        "d" => Ok(number * 24 * 60 * 60 * 1000),
        _ => Err(self.error(
            &format!("Unknown duration unit '{}'. Valid: ms, s, m, h, d", unit),
            None,
        )),
    }
}
```

- [ ] **Step 7: Run parser tests to verify they pass**

Run: `cargo test parse_schedule -- --nocapture`
Expected: PASS

- [ ] **Step 8: Add `StoredSchedule` and interpreter handling**

In `src/interpreter/interpreter.rs`, add struct (after `StoredStream`, ~line 36):

```rust
#[derive(Debug, Clone)]
pub struct StoredSchedule {
    pub intent: String,
    pub interval_ms: u64,
    pub effects: Vec<String>,
    pub body: Vec<Statement>,
}
```

Add field to `Interpreter` struct (after `streams`):

```rust
pub schedules: Vec<StoredSchedule>,
```

Initialize in `Interpreter::new()`:

```rust
schedules: Vec::new(),
```

Handle `Statement::Schedule` in `eval_statement` (after `Statement::Stream` handler):

```rust
Statement::Schedule {
    intent,
    interval_ms,
    effects,
    body,
} => {
    self.schedules.push(StoredSchedule {
        intent: intent.clone(),
        interval_ms: *interval_ms,
        effects: effects.clone(),
        body: body.clone(),
    });
    Ok(StmtResult::Value(Value::Nothing))
}
```

- [ ] **Step 9: Add formatter support**

In `src/formatter.rs`, add `Statement::Schedule` handling (after `Statement::Stream` block):

```rust
Statement::Schedule {
    intent,
    interval_ms,
    effects,
    body,
} => {
    if !intent.is_empty() {
        self.writeln(&format!("intent \"{}\"", intent));
    }
    let dur = format_duration(*interval_ms);
    self.writeln(&format!("schedule every {} {{", dur));
    self.indent += 1;
    if !effects.is_empty() {
        self.writeln(&format!("needs {}", effects.join(", ")));
    }
    self.format_body(body);
    self.indent -= 1;
    self.writeln("}");
}
```

Add helper function:

```rust
fn format_duration(ms: u64) -> String {
    if ms % 86_400_000 == 0 { format!("{}d", ms / 86_400_000) }
    else if ms % 3_600_000 == 0 { format!("{}h", ms / 3_600_000) }
    else if ms % 60_000 == 0 { format!("{}m", ms / 60_000) }
    else if ms % 1000 == 0 { format!("{}s", ms / 1000) }
    else { format!("{}ms", ms) }
}
```

- [ ] **Step 10: Add checker support**

In `src/checker.rs`, extend the `Statement::Route | Statement::Stream` match (line ~927) to include `Schedule`:

```rust
Statement::Route { effects, body, .. }
| Statement::Stream { effects, body, .. }
| Statement::Schedule { effects, body, .. } => {
```

- [ ] **Step 11: Spawn schedule threads in server**

In `src/interpreter/server.rs`, in `start_server` (after route compilation, before the request loop at line ~261):

```rust
// Spawn schedule threads
for schedule in &interpreter.schedules {
    let interval = std::time::Duration::from_millis(schedule.interval_ms);
    let intent = schedule.intent.clone();
    let body = schedule.body.clone();
    let effects = schedule.effects.clone();
    let source = interpreter.source_code().to_string();
    let db_path_clone = db_path.clone();

    std::thread::spawn(move || {
        loop {
            // Execute the schedule body
            let mut sched_interp = Interpreter::new(&source);
            sched_interp.setup_test_effects();
            if let Some(ref path) = db_path_clone {
                let _ = sched_interp.open_sqlite(path);
            }
            sched_interp.blocked_effects = vec!["db", "time", "rng", "log", "auth", "http"]
                .into_iter()
                .filter(|e| !effects.contains(&e.to_string()))
                .map(String::from)
                .collect();

            let mut env = Environment::new();
            for stmt in &body {
                if let Err(e) = sched_interp.eval_statement(stmt, &mut env) {
                    eprintln!("[schedule:{}] Error: {}", intent, e.message);
                }
            }
            // Sleep until next run
            std::thread::sleep(interval);
        }
    });
    println!("  Schedule '{}' started (every {})", intent, format_schedule_interval(schedule.interval_ms));
}
```

Add helper:

```rust
fn format_schedule_interval(ms: u64) -> String {
    if ms % 86_400_000 == 0 { format!("{}d", ms / 86_400_000) }
    else if ms % 3_600_000 == 0 { format!("{}h", ms / 3_600_000) }
    else if ms % 60_000 == 0 { format!("{}m", ms / 60_000) }
    else if ms % 1000 == 0 { format!("{}s", ms / 1000) }
    else { format!("{}ms", ms) }
}
```

Note: The schedule thread creates a fresh Interpreter each iteration because the main interpreter owns the server loop and can't be shared across threads. Each schedule iteration opens its own DB connection. This is correct for SQLite WAL mode.

Also need to expose `source_code()` getter on Interpreter if not already available:

```rust
pub fn source_code(&self) -> &str {
    &self.source
}
```

- [ ] **Step 12: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 13: Commit**

```bash
cargo fmt
git add src/lexer/token.rs src/parser/ast.rs src/parser/parser.rs src/interpreter/interpreter.rs src/interpreter/server.rs src/formatter.rs src/checker.rs
git commit -m "feat: add schedule every <duration> { } blocks with background execution"
```

---

### Task 7: Write `showcase.pact`

**Files:**
- Create: `examples/showcase.pact`

This task creates the showcase application that exercises all new features.

- [ ] **Step 1: Create the showcase file**

Create `examples/showcase.pact`:

```pact
// ============================================
// PACT Showcase API
// One file. Seven endpoints. Every feature.
// ============================================

// --- Health Check ---

intent "health check"
route GET "/health" {
  respond 200 with { status: "ok", version: "0.5.0" }
}

// --- Paste Bin ---

intent "store a text snippet"
route POST "/paste" {
  needs db, rng, time

  return BadRequest { message: "content is required" } if request.body.content == nothing
  return BadRequest { message: "content too large (max 64KB)" } if request.body.content.length() > 65536

  let id: String = rng.uuid()
  db.insert("pastes", {
    id: id,
    content: request.body.content,
    created_at: time.now(),
  })
  respond 201 with { id: id }
}

intent "retrieve a stored snippet"
route GET "/paste/{id}" {
  needs db

  db.find("pastes", { id: request.params.id })
    | on success: respond 200 with .content as "text/plain"
    | on NotFound: respond 404 with { error: "Paste not found" }
}

// --- URL Shortener ---

intent "shorten a URL"
route POST "/shorten" {
  needs db, rng, time

  return BadRequest { message: "url is required" } if request.body.url == nothing

  let code: String = rng.short_id()
  db.insert("links", {
    code: code,
    url: request.body.url,
    created_at: time.now(),
  })
  respond 201 with { code: code, short_url: "/s/" + code }
}

intent "redirect to original URL"
route GET "/s/{code}" {
  needs db

  db.find("links", { code: request.params.code })
    | on success: respond 302 with { location: .url }
    | on NotFound: respond 404 with { error: "Link not found" }
}

// --- SVG Avatar ---

intent "generate avatar from name"
fn make_avatar(name: String) -> String {
  let colors: List<String> = list("#e74c3c", "#3498db", "#2ecc71", "#f39c12", "#9b59b6", "#1abc9c", "#e67e22", "#2c3e50")
  let seed: Int = name | chars | map .code() | sum
  let bg: String = colors | get (seed % 8)
  let fg: String = colors | get ((seed / 8) % 8)

  let svg: String = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"80\" height=\"80\" viewBox=\"0 0 5 5\">"
    + "<rect width=\"5\" height=\"5\" fill=\"" + bg + "\"/>"

  // Build 3x5 half-grid, mirror to get 5x5
  let grid: String = ""
  let row: Int = 0
  // Row 0
  let c0: Int = seed % 2
  let c1: Int = (seed / 2) % 2
  let c2: Int = (seed / 4) % 2
  // Row 1
  let c3: Int = (seed / 8) % 2
  let c4: Int = (seed / 16) % 2
  let c5: Int = (seed / 32) % 2
  // Row 2
  let c6: Int = (seed / 64) % 2
  let c7: Int = (seed / 128) % 2
  let c8: Int = (seed / 256) % 2
  // Row 3
  let c9: Int = (seed / 512) % 2
  let c10: Int = (seed / 1024) % 2
  let c11: Int = (seed / 2048) % 2
  // Row 4
  let c12: Int = (seed / 4096) % 2
  let c13: Int = (seed / 8192) % 2
  let c14: Int = (seed / 16384) % 2

  let rects: String = ""
  // Column 0 and 4 (mirrored)
  let rects: String = rects + (if c0 == 1 { "<rect x=\"0\" y=\"0\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"4\" y=\"0\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c3 == 1 { "<rect x=\"0\" y=\"1\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"4\" y=\"1\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c6 == 1 { "<rect x=\"0\" y=\"2\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"4\" y=\"2\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c9 == 1 { "<rect x=\"0\" y=\"3\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"4\" y=\"3\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c12 == 1 { "<rect x=\"0\" y=\"4\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"4\" y=\"4\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  // Column 1 and 3 (mirrored)
  let rects: String = rects + (if c1 == 1 { "<rect x=\"1\" y=\"0\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"3\" y=\"0\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c4 == 1 { "<rect x=\"1\" y=\"1\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"3\" y=\"1\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c7 == 1 { "<rect x=\"1\" y=\"2\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"3\" y=\"2\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c10 == 1 { "<rect x=\"1\" y=\"3\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"3\" y=\"3\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c13 == 1 { "<rect x=\"1\" y=\"4\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/><rect x=\"3\" y=\"4\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  // Column 2 (center, no mirror)
  let rects: String = rects + (if c2 == 1 { "<rect x=\"2\" y=\"0\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c5 == 1 { "<rect x=\"2\" y=\"1\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c8 == 1 { "<rect x=\"2\" y=\"2\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c11 == 1 { "<rect x=\"2\" y=\"3\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })
  let rects: String = rects + (if c14 == 1 { "<rect x=\"2\" y=\"4\" width=\"1\" height=\"1\" fill=\"" + fg + "\"/>" } else { "" })

  svg + rects + "</svg>"
}

intent "serve avatar as SVG"
route GET "/avatar/{name}" {
  let svg: String = make_avatar(request.params.name)
  respond 200 with svg as "image/svg+xml"
}

// --- Statistics ---

intent "show API usage stats"
route GET "/stats" {
  needs db

  let pastes: List = db.query("pastes")
  let links: List = db.query("links")

  respond 200 with {
    total_pastes: pastes | count,
    total_links: links | count,
  }
}

// --- Scheduled Cleanup ---

intent "clean up records older than 7 days"
schedule every 1d {
  needs db, time

  let cutoff: String = time.days_ago(7)
  let deleted_pastes: Int = db.delete_where("pastes", { before: cutoff })
  let deleted_links: Int = db.delete_where("links", { before: cutoff })
  print("Cleanup: removed " + deleted_pastes + " pastes, " + deleted_links + " links")
}

// --- Landing Page ---

intent "serve landing page"
route GET "/" {
  let html: String = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>PACT Showcase</title><style>body{font-family:system-ui,-apple-system,sans-serif;max-width:800px;margin:40px auto;padding:0 20px;color:#333;line-height:1.6}h1{color:#2c3e50}h2{color:#34495e;border-bottom:2px solid #ecf0f1;padding-bottom:8px}code{background:#f8f9fa;padding:2px 6px;border-radius:3px;font-size:0.9em}pre{background:#2c3e50;color:#ecf0f1;padding:16px;border-radius:8px;overflow-x:auto}a{color:#3498db}.endpoint{margin:20px 0;padding:16px;background:#f8f9fa;border-radius:8px;border-left:4px solid #3498db}.method{font-weight:bold;color:#e74c3c}.try{margin-top:8px;font-size:0.9em;color:#7f8c8d}</style></head><body>"
    + "<h1>PACT Showcase API</h1>"
    + "<p>One <code>.pact</code> file. Seven endpoints. Every feature demonstrated.</p>"
    + "<p>Built with <a href=\"https://github.com/KikotVit/pact-lang\">PACT</a> — a language designed for AI agents to build backend services.</p>"
    + "<h2>Endpoints</h2>"
    + "<div class=\"endpoint\"><span class=\"method\">GET</span> <code>/health</code> — Health check<div class=\"try\"><pre>curl /health</pre></div></div>"
    + "<div class=\"endpoint\"><span class=\"method\">POST</span> <code>/paste</code> — Store a text snippet<div class=\"try\"><pre>curl -X POST -H 'Content-Type: application/json' -d '{\"content\":\"Hello PACT!\"}' /paste</pre></div></div>"
    + "<div class=\"endpoint\"><span class=\"method\">GET</span> <code>/paste/{id}</code> — Retrieve snippet as plain text<div class=\"try\"><pre>curl /paste/&lt;id&gt;</pre></div></div>"
    + "<div class=\"endpoint\"><span class=\"method\">POST</span> <code>/shorten</code> — Shorten a URL<div class=\"try\"><pre>curl -X POST -H 'Content-Type: application/json' -d '{\"url\":\"https://example.com\"}' /shorten</pre></div></div>"
    + "<div class=\"endpoint\"><span class=\"method\">GET</span> <code>/s/{code}</code> — Redirect to original URL</div>"
    + "<div class=\"endpoint\"><span class=\"method\">GET</span> <code>/avatar/{name}</code> — Generate SVG identicon<div class=\"try\">Try: <a href=\"/avatar/pact\">/avatar/pact</a> <a href=\"/avatar/claude\">/avatar/claude</a> <a href=\"/avatar/your-name\">/avatar/your-name</a></div></div>"
    + "<div class=\"endpoint\"><span class=\"method\">GET</span> <code>/stats</code> — Usage statistics<div class=\"try\"><pre>curl /stats</pre></div></div>"
    + "<h2>Features Demonstrated</h2>"
    + "<ul><li><strong>Routes</strong> — GET, POST with path params</li>"
    + "<li><strong>Pipelines</strong> — <code>| on success:</code> <code>| on NotFound:</code> <code>| count</code></li>"
    + "<li><strong>Effects</strong> — <code>needs db, rng, time</code></li>"
    + "<li><strong>Content Types</strong> — JSON, HTML, plain text, SVG, redirect</li>"
    + "<li><strong>Database</strong> — SQLite with auto-schema</li>"
    + "<li><strong>Scheduled Tasks</strong> — <code>schedule every 1d</code> cleanup</li>"
    + "<li><strong>Error Handling</strong> — Errors as values, not exceptions</li></ul>"
    + "<p style=\"color:#95a5a6;font-size:0.85em\">This entire API is ~150 lines of PACT. Records auto-clean after 7 days.</p>"
    + "</body></html>"

  respond 200 with html as "text/html"
}

// --- App ---

app Showcase { port: 8080, db: "sqlite://data/showcase.db" }
```

- [ ] **Step 2: Verify it parses**

Run: `cargo run -- examples/showcase.pact --ast 2>&1 | head -20`
Expected: AST output without parse errors

- [ ] **Step 3: Run it and test manually**

Run: `cargo run -- run examples/showcase.pact &`

Then test each endpoint:

```bash
curl http://localhost:8080/health
curl http://localhost:8080/
curl -X POST -H 'Content-Type: application/json' -d '{"content":"hello"}' http://localhost:8080/paste
curl http://localhost:8080/paste/<id-from-above>
curl -X POST -H 'Content-Type: application/json' -d '{"url":"https://example.com"}' http://localhost:8080/shorten
curl -v http://localhost:8080/s/<code-from-above>
curl http://localhost:8080/avatar/pact
curl http://localhost:8080/stats
```

Kill the server after testing.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add examples/showcase.pact
git commit -m "feat: add showcase.pact — self-documenting API with 7 endpoints"
```

---

### Task 8: Add Dockerfile for Coolify deployment

**Files:**
- Create: `Dockerfile`

- [ ] **Step 1: Create Dockerfile**

```dockerfile
FROM rust:1.85-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/pact /usr/local/bin/pact
COPY examples/showcase.pact ./showcase.pact
RUN mkdir -p /app/data

EXPOSE 8080
VOLUME /app/data

CMD ["pact", "run", "showcase.pact"]
```

Note: The showcase.pact uses `db: "sqlite://data/showcase.db"` which resolves to `/app/data/showcase.db` inside the container. The volume mount at `/app/data` ensures persistence across deploys.

- [ ] **Step 2: Test Docker build**

Run: `docker build -t pact-showcase .`
Expected: Build succeeds

- [ ] **Step 3: Test Docker run**

Run: `docker run --rm -p 8080:8080 pact-showcase &`
Then: `curl http://localhost:8080/health`
Expected: `{"status":"ok","version":"0.5.0"}`

Kill the container after testing.

- [ ] **Step 4: Commit**

```bash
git add Dockerfile
git commit -m "feat: add Dockerfile for Coolify deployment"
```

---

## Dependency Graph

```
Task 1 (time.now + time.days_ago)  ──┐
Task 2 (respond as content-type)   ──┤
Task 3 (chars, code)               ──┤── Task 7 (showcase.pact) ── Task 8 (Dockerfile)
Task 4 (rng.short_id)              ──┤
Task 5 (db.delete_where)           ──┤
Task 6 (schedule)                  ──┘
```

Tasks 1-6 are independent and can be done in parallel. Task 7 depends on all of 1-6. Task 8 depends on Task 7.

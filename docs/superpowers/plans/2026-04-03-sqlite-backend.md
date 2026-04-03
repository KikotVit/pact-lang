# SQLite Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add persistent SQLite storage to PACT via `rusqlite` with bundled feature, so `app` declarations with `db: "sqlite://..."` store data on disk.

**Architecture:** New `src/interpreter/db.rs` module with `DbBackend` enum (Memory/Sqlite variants). Interpreter delegates all `builtin_db_*` calls to `DbBackend` methods. Parser extended to parse optional `db:` in `app` block. Server startup opens SQLite connection when configured.

**Tech Stack:** rusqlite 0.31 with `bundled` feature, existing serde_json for List/Struct serialization.

**Spec:** `docs/superpowers/specs/2026-04-03-sqlite-backend-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Add `rusqlite` dependency |
| `src/interpreter/db.rs` | **NEW** — `DbBackend` enum, `ColDef`, `PactType`, all CRUD methods for Memory and Sqlite |
| `src/interpreter/mod.rs` | Register `db` module |
| `src/interpreter/interpreter.rs` | Replace `db_storage: HashMap` with `db: DbBackend`, delegate builtins, update `AppConfig` |
| `src/parser/ast.rs` | Extend `Statement::App` with `db_url: Option<String>` |
| `src/parser/parser.rs` | Parse optional `db:` in `parse_app` |
| `src/main.rs` | Open SQLite connection before server start |

---

### Task 1: Add rusqlite dependency and create db.rs skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/interpreter/db.rs`
- Modify: `src/interpreter/mod.rs`

- [ ] **Step 1: Add rusqlite to Cargo.toml**

In `Cargo.toml`, add to `[dependencies]`:

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

- [ ] **Step 2: Create db.rs with types and empty DbBackend**

Create `src/interpreter/db.rs`:

```rust
use std::collections::HashMap;

use super::errors::RuntimeError;
use super::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum PactType {
    String,
    Int,
    Float,
    Bool,
    List,
    Struct,
}

#[derive(Debug, Clone)]
pub struct ColDef {
    pub name: String,
    pub pact_type: PactType,
}

pub enum DbBackend {
    Memory {
        storage: HashMap<String, Vec<Value>>,
    },
    Sqlite {
        conn: rusqlite::Connection,
        schemas: HashMap<String, Vec<ColDef>>,
    },
}

impl DbBackend {
    pub fn new_memory() -> Self {
        DbBackend::Memory {
            storage: HashMap::new(),
        }
    }

    pub fn new_sqlite(path: &str) -> Result<Self, RuntimeError> {
        let conn = rusqlite::Connection::open(path).map_err(|e| RuntimeError {
            line: 0,
            column: 0,
            message: format!("Cannot open database '{}': {}", path, e),
            hint: None,
            source_line: String::new(),
        })?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| RuntimeError {
                line: 0,
                column: 0,
                message: format!("Failed to set WAL mode: {}", e),
                hint: None,
                source_line: String::new(),
            })?;
        Ok(DbBackend::Sqlite {
            conn,
            schemas: HashMap::new(),
        })
    }

    pub fn clear(&mut self) {
        match self {
            DbBackend::Memory { storage } => storage.clear(),
            DbBackend::Sqlite { .. } => {
                // Sqlite clear not needed for db.memory() — we replace the whole backend
            }
        }
    }

    pub fn insert(&mut self, table: &str, value: Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                storage.entry(table.to_string()).or_default().push(value.clone());
                Ok(value)
            }
            DbBackend::Sqlite { .. } => {
                todo!("sqlite insert")
            }
        }
    }

    pub fn query(&self, table: &str, filter: Option<&Value>) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                let items = storage.get(table).cloned().unwrap_or_default();
                if let Some(f) = filter {
                    Ok(Value::List(filter_by_struct(&items, f)))
                } else {
                    Ok(Value::List(items))
                }
            }
            DbBackend::Sqlite { .. } => {
                todo!("sqlite query")
            }
        }
    }

    pub fn find(&self, table: &str, filter: &Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                let items = storage.get(table).cloned().unwrap_or_default();
                let matches = filter_by_struct(&items, filter);
                match matches.into_iter().next() {
                    Some(item) => Ok(item),
                    None => Ok(Value::Error {
                        variant: "NotFound".to_string(),
                        fields: None,
                    }),
                }
            }
            DbBackend::Sqlite { .. } => {
                todo!("sqlite find")
            }
        }
    }

    pub fn update(&mut self, table: &str, id: &str, new_value: Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                if let Some(items) = storage.get_mut(table) {
                    for item in items.iter_mut() {
                        if let Value::Struct { fields, .. } = item {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                *item = new_value.clone();
                                return Ok(new_value);
                            }
                        }
                    }
                }
                Ok(Value::Error {
                    variant: "NotFound".to_string(),
                    fields: None,
                })
            }
            DbBackend::Sqlite { .. } => {
                todo!("sqlite update")
            }
        }
    }

    pub fn delete(&mut self, table: &str, id: &str) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                if let Some(items) = storage.get_mut(table) {
                    let len_before = items.len();
                    let mut removed = Value::Nothing;
                    items.retain(|item| {
                        if let Value::Struct { fields, .. } = item {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                removed = item.clone();
                                return false;
                            }
                        }
                        true
                    });
                    if items.len() < len_before {
                        return Ok(removed);
                    }
                }
                Ok(Value::Error {
                    variant: "NotFound".to_string(),
                    fields: None,
                })
            }
            DbBackend::Sqlite { .. } => {
                todo!("sqlite delete")
            }
        }
    }
}

/// Match items against a struct filter — all fields in filter must match.
fn filter_by_struct(items: &[Value], filter: &Value) -> Vec<Value> {
    let filter_fields = match filter {
        Value::Struct { fields, .. } => fields,
        _ => return items.to_vec(),
    };
    items
        .iter()
        .filter(|item| {
            if let Value::Struct { fields, .. } = item {
                filter_fields.iter().all(|(k, v)| fields.get(k) == Some(v))
            } else {
                false
            }
        })
        .cloned()
        .collect()
}
```

- [ ] **Step 3: Register db module in mod.rs**

In `src/interpreter/mod.rs`, add `pub mod db;` line:

```rust
pub mod builtins;
pub mod db;
pub mod environment;
pub mod errors;
#[allow(clippy::module_inception)]
pub mod interpreter;
pub mod json;
pub mod pipeline;
pub mod server;
pub mod value;

pub use db::DbBackend;
pub use environment::Environment;
pub use errors::RuntimeError;
pub use interpreter::{Interpreter, StoredRoute, TestResult};
pub use value::Value;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles (with warnings about unused Sqlite variant — that's ok for now)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/interpreter/db.rs src/interpreter/mod.rs
git commit -m "feat: add db.rs module with DbBackend enum and Memory implementation"
```

---

### Task 2: Wire DbBackend into Interpreter (replace db_storage)

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Replace db_storage with db field in Interpreter struct**

In `src/interpreter/interpreter.rs`, change the struct definition (lines 27-45).

Replace:
```rust
    pub db_storage: HashMap<String, Vec<Value>>,
```

With:
```rust
    pub db: DbBackend,
```

Add import at top of file:
```rust
use super::db::DbBackend;
```

- [ ] **Step 2: Update Interpreter::new to use DbBackend::new_memory()**

In `Interpreter::new()` (lines 48-65), replace:
```rust
            db_storage: HashMap::new(),
```

With:
```rust
            db: DbBackend::new_memory(),
```

- [ ] **Step 3: Update builtin_db_insert to delegate to self.db**

Replace the `builtin_db_insert` method (lines 1183-1197):

```rust
    fn builtin_db_insert(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.insert expects 2 arguments: table name and value"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.insert first argument must be a String table name")),
        };
        let value = args[1].clone();
        self.db.insert(&table_name, value)
    }
```

- [ ] **Step 4: Update builtin_db_query to delegate to self.db**

Replace the `builtin_db_query` method (lines 1199-1220):

```rust
    fn builtin_db_query(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.is_empty() || args.len() > 2 {
            return Err(
                self.error("db.query expects 1-2 arguments: table name, optional filter struct")
            );
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.query first argument must be a String table name")),
        };
        let filter = args.get(1);
        self.db.query(&table_name, filter)
    }
```

- [ ] **Step 5: Update builtin_db_find to delegate to self.db**

Replace the `builtin_db_find` method (lines 1222-1243):

```rust
    fn builtin_db_find(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.find expects 2 arguments: table name, filter struct"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.find first argument must be a String table name")),
        };
        self.db.find(&table_name, &args[1])
    }
```

- [ ] **Step 6: Update builtin_db_update to delegate to self.db**

Replace the `builtin_db_update` method (lines 1264-1291):

```rust
    fn builtin_db_update(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 3 {
            return Err(self.error("db.update expects 3 arguments: table name, id, new value"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.update first argument must be a String table name")),
        };
        let id = match &args[1] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.update second argument must be a String id")),
        };
        let new_value = args[2].clone();
        self.db.update(&table_name, &id, new_value)
    }
```

- [ ] **Step 7: Update builtin_db_delete to delegate to self.db**

Replace the `builtin_db_delete` method (lines 1293-1325):

```rust
    fn builtin_db_delete(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.delete expects 2 arguments: table name, id"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.delete first argument must be a String table name")),
        };
        let id = match &args[1] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.delete second argument must be a String id")),
        };
        self.db.delete(&table_name, &id)
    }
```

- [ ] **Step 8: Update db.memory() in call_builtin**

In `call_builtin`, replace the `"db.memory"` arm (lines 1092-1095):

```rust
            "db.memory" => {
                self.db = DbBackend::new_memory();
                Ok(self.make_db_effect())
            }
```

- [ ] **Step 9: Remove filter_by_struct from interpreter.rs**

Delete the `filter_by_struct` method (lines 1245-1262) — it now lives in `db.rs`.

- [ ] **Step 10: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All 272+ tests pass. Memory behavior is identical.

- [ ] **Step 11: Commit**

```bash
git add src/interpreter/interpreter.rs
git commit -m "refactor: delegate db operations to DbBackend, remove db_storage from Interpreter"
```

---

### Task 3: Extend App parsing with optional db: parameter

**Files:**
- Modify: `src/parser/ast.rs`
- Modify: `src/parser/parser.rs`
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Add db_url to Statement::App in AST**

In `src/parser/ast.rs`, replace the App variant (lines 50-53):

```rust
    App {
        name: String,
        port: u16,
        db_url: Option<String>,
    },
```

- [ ] **Step 2: Update parse_app to accept optional db: parameter**

In `src/parser/parser.rs`, replace the `parse_app` method (lines 1251-1292):

```rust
    fn parse_app(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume `app`
        if self.at(&TokenKind::LBrace) {
            return Err(self.error(
                "app requires a name, e.g.: app MyService { port: 8080 }",
                None,
            ));
        }
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut port: Option<u16> = None;
        let mut db_url: Option<String> = None;

        // Parse key-value pairs until closing brace
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::EOF) {
            let key = self.expect_identifier()?;
            self.expect(&TokenKind::Colon)?;

            match key.as_str() {
                "port" => {
                    port = Some(match self.current_kind().clone() {
                        TokenKind::IntLiteral(n) => {
                            self.advance();
                            n as u16
                        }
                        _ => {
                            return self.fail(
                                &format!(
                                    "Expected integer for port, found {}",
                                    self.current_kind()
                                ),
                                Some("Syntax: app Name { port: 8080 }"),
                            );
                        }
                    });
                }
                "db" => {
                    db_url = Some(match self.current_kind().clone() {
                        TokenKind::StringLiteral(s) => {
                            self.advance();
                            s
                        }
                        _ => {
                            return self.fail(
                                &format!(
                                    "Expected string for db, found {}",
                                    self.current_kind()
                                ),
                                Some("Syntax: app Name { port: 8080, db: \"sqlite://data.db\" }"),
                            );
                        }
                    });
                }
                other => {
                    return self.fail(
                        &format!("Unknown app property '{}'", other),
                        Some("Known properties: port, db"),
                    );
                }
            }

            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }

        let port = port.ok_or_else(|| {
            self.error(
                "app declaration requires 'port'",
                Some("Syntax: app Name { port: 8080 }"),
            )
        })?;

        self.expect(&TokenKind::RBrace)?;

        Ok(Statement::App { name, port, db_url })
    }
```

- [ ] **Step 3: Update Statement::App handling in interpreter.rs**

In `src/interpreter/interpreter.rs`, replace the `Statement::App` match arm (lines 221-224):

```rust
            Statement::App { name, port, db_url } => {
                self.app_config = Some((name.clone(), *port, db_url.clone()));
                Ok(StmtResult::Value(Value::Nothing))
            }
```

- [ ] **Step 4: Update app_config type in Interpreter struct**

In `src/interpreter/interpreter.rs`, change the `app_config` field (line 40):

```rust
    pub app_config: Option<(String, u16, Option<String>)>,
```

- [ ] **Step 5: Fix existing parser test for App**

Search for the existing App test (line ~2468 in parser.rs) and update the pattern match to include `db_url`:

Find the test that matches `Statement::App { name, port }` and update it to:
```rust
            matches!(&prog.statements[0], Statement::App { name, port, db_url } if name == "UserService" && *port == 8080 && db_url.is_none())
```

- [ ] **Step 6: Add parser test for app with db**

Add test in `src/parser/parser.rs` tests section:

```rust
    #[test]
    fn parse_app_with_db() {
        let input = "app UserService {\n  port: 8080,\n  db: \"sqlite://data.db\",\n}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let prog = parser.parse().unwrap();
        assert!(
            matches!(&prog.statements[0], Statement::App { name, port, db_url }
                if name == "UserService" && *port == 8080 && db_url.as_deref() == Some("sqlite://data.db"))
        );
    }
```

- [ ] **Step 7: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass, including new parser test.

- [ ] **Step 8: Commit**

```bash
git add src/parser/ast.rs src/parser/parser.rs src/interpreter/interpreter.rs
git commit -m "feat: parse optional db: parameter in app declaration"
```

---

### Task 4: Implement SQLite CRUD operations in DbBackend

**Files:**
- Modify: `src/interpreter/db.rs`

- [ ] **Step 1: Add helper to determine PactType from Value**

Add to `src/interpreter/db.rs`, after the `PactType` enum:

```rust
impl PactType {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::String(_) => PactType::String,
            Value::Int(_) => PactType::Int,
            Value::Float(_) => PactType::Float,
            Value::Bool(_) => PactType::Bool,
            Value::List(_) => PactType::List,
            Value::Struct { .. } => PactType::Struct,
            _ => PactType::String, // fallback
        }
    }

    fn to_sql_type(&self) -> &str {
        match self {
            PactType::String => "TEXT",
            PactType::Int => "INTEGER",
            PactType::Float => "REAL",
            PactType::Bool => "INTEGER",
            PactType::List => "TEXT",
            PactType::Struct => "TEXT",
        }
    }
}
```

- [ ] **Step 2: Add helper to convert Value to rusqlite parameter**

Add to `src/interpreter/db.rs`:

```rust
use rusqlite::types::ToSql;

fn value_to_sql(value: &Value) -> Box<dyn ToSql> {
    match value {
        Value::String(s) => Box::new(s.clone()),
        Value::Int(n) => Box::new(*n),
        Value::Float(f) => Box::new(*f),
        Value::Bool(b) => Box::new(*b as i64),
        Value::Nothing => Box::new(rusqlite::types::Null),
        Value::List(_) | Value::Struct { .. } => {
            let json = super::json::value_to_json(value);
            Box::new(serde_json::to_string(&json).unwrap_or_default())
        }
        _ => Box::new(rusqlite::types::Null),
    }
}
```

- [ ] **Step 3: Add helper to read a row back into Value**

Add to `src/interpreter/db.rs`:

```rust
use rusqlite::Row;

fn row_to_value(row: &Row, schema: &[ColDef]) -> Result<Value, rusqlite::Error> {
    let mut fields = HashMap::new();
    for (i, col) in schema.iter().enumerate() {
        let value = match col.pact_type {
            PactType::String => {
                let v: Option<String> = row.get(i)?;
                match v {
                    Some(s) => Value::String(s),
                    None => Value::Nothing,
                }
            }
            PactType::Int => {
                let v: Option<i64> = row.get(i)?;
                match v {
                    Some(n) => Value::Int(n),
                    None => Value::Nothing,
                }
            }
            PactType::Float => {
                let v: Option<f64> = row.get(i)?;
                match v {
                    Some(f) => Value::Float(f),
                    None => Value::Nothing,
                }
            }
            PactType::Bool => {
                let v: Option<i64> = row.get(i)?;
                match v {
                    Some(n) => Value::Bool(n != 0),
                    None => Value::Nothing,
                }
            }
            PactType::List | PactType::Struct => {
                let v: Option<String> = row.get(i)?;
                match v {
                    Some(s) => {
                        match serde_json::from_str::<serde_json::Value>(&s) {
                            Ok(json) => super::json::json_to_value(&json),
                            Err(_) => Value::String(s),
                        }
                    }
                    None => Value::Nothing,
                }
            }
        };
        fields.insert(col.name.clone(), value);
    }
    Ok(Value::Struct {
        type_name: "Row".to_string(),
        fields,
    })
}
```

- [ ] **Step 4: Add ensure_table helper for auto-schema**

Add to `impl DbBackend`:

```rust
    fn ensure_table(&mut self, table: &str, value: &Value) -> Result<(), RuntimeError> {
        if let DbBackend::Sqlite { conn, schemas } = self {
            let fields = match value {
                Value::Struct { fields, .. } => fields,
                _ => return Ok(()),
            };

            if let Some(existing_schema) = schemas.get(table) {
                // Check for new columns
                let existing_names: Vec<&str> =
                    existing_schema.iter().map(|c| c.name.as_str()).collect();
                let mut new_cols = Vec::new();
                for (name, val) in fields {
                    if !existing_names.contains(&name.as_str()) {
                        new_cols.push(ColDef {
                            name: name.clone(),
                            pact_type: PactType::from_value(val),
                        });
                    }
                }
                for col in &new_cols {
                    let sql = format!(
                        "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {}",
                        table,
                        col.name,
                        col.pact_type.to_sql_type()
                    );
                    conn.execute(&sql, []).map_err(|e| RuntimeError {
                        line: 0,
                        column: 0,
                        message: format!(
                            "Database error adding column '{}' to table '{}': {}",
                            col.name, table, e
                        ),
                        hint: None,
                        source_line: String::new(),
                    })?;
                }
                if !new_cols.is_empty() {
                    let schema = schemas.get_mut(table).unwrap();
                    schema.extend(new_cols);
                }
            } else {
                // Create table
                let mut cols: Vec<ColDef> = Vec::new();
                let mut col_defs: Vec<String> = Vec::new();
                for (name, val) in fields {
                    let pact_type = PactType::from_value(val);
                    col_defs.push(format!(
                        "\"{}\" {}",
                        name,
                        pact_type.to_sql_type()
                    ));
                    cols.push(ColDef {
                        name: name.clone(),
                        pact_type,
                    });
                }
                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS \"{}\" ({})",
                    table,
                    col_defs.join(", ")
                );
                conn.execute(&sql, []).map_err(|e| RuntimeError {
                    line: 0,
                    column: 0,
                    message: format!(
                        "Database error creating table '{}': {}",
                        table, e
                    ),
                    hint: None,
                    source_line: String::new(),
                })?;
                schemas.insert(table.to_string(), cols);
            }
        }
        Ok(())
    }
```

- [ ] **Step 5: Implement SQLite insert**

Replace the `todo!("sqlite insert")` in the `insert` method:

```rust
            DbBackend::Sqlite { conn, schemas } => {
                // ensure_table needs &mut self, but we're inside match on &mut self
                // So we need to call it before the match. Restructure:
                drop(conn);
                drop(schemas);
                // Actually, restructure to call ensure_table first
                unreachable!() // placeholder — see restructured version below
            }
```

Actually, since `ensure_table` needs `&mut self` and we're inside a `match self`, we need to restructure the insert method. Replace the entire `insert` method:

```rust
    pub fn insert(&mut self, table: &str, value: Value) -> Result<Value, RuntimeError> {
        self.ensure_table(table, &value)?;
        match self {
            DbBackend::Memory { storage } => {
                storage.entry(table.to_string()).or_default().push(value.clone());
                Ok(value)
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = schemas.get(table).ok_or_else(|| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Table '{}' has no schema", table),
                    hint: None, source_line: String::new(),
                })?;
                let fields = match &value {
                    Value::Struct { fields, .. } => fields,
                    _ => return Ok(value),
                };
                let col_names: Vec<String> = schema.iter().map(|c| format!("\"{}\"", c.name)).collect();
                let placeholders: Vec<String> = schema.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({})",
                    table,
                    col_names.join(", "),
                    placeholders.join(", ")
                );
                let params: Vec<Box<dyn ToSql>> = schema
                    .iter()
                    .map(|col| {
                        let val = fields.get(&col.name).unwrap_or(&Value::Nothing);
                        value_to_sql(val)
                    })
                    .collect();
                let param_refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
                conn.execute(&sql, param_refs.as_slice()).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in insert on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                Ok(value)
            }
        }
    }
```

- [ ] **Step 6: Implement SQLite query**

Replace the entire `query` method:

```rust
    pub fn query(&self, table: &str, filter: Option<&Value>) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                let items = storage.get(table).cloned().unwrap_or_default();
                if let Some(f) = filter {
                    Ok(Value::List(filter_by_struct(&items, f)))
                } else {
                    Ok(Value::List(items))
                }
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = match schemas.get(table) {
                    Some(s) => s,
                    None => return Ok(Value::List(vec![])),
                };
                let (sql, params) = if let Some(filter) = filter {
                    if let Value::Struct { fields, .. } = filter {
                        let mut where_parts = Vec::new();
                        let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();
                        for (key, val) in fields {
                            where_parts.push(format!("\"{}\" = ?", key));
                            param_values.push(value_to_sql(val));
                        }
                        let sql = format!(
                            "SELECT * FROM \"{}\" WHERE {}",
                            table,
                            where_parts.join(" AND ")
                        );
                        (sql, param_values)
                    } else {
                        (format!("SELECT * FROM \"{}\"", table), vec![])
                    }
                } else {
                    (format!("SELECT * FROM \"{}\"", table), vec![])
                };
                let param_refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
                let mut stmt = conn.prepare(&sql).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in query on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| row_to_value(row, schema))
                    .map_err(|e| RuntimeError {
                        line: 0, column: 0,
                        message: format!("Database error in query on table '{}': {}", table, e),
                        hint: None, source_line: String::new(),
                    })?;
                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| RuntimeError {
                        line: 0, column: 0,
                        message: format!("Database error reading row from '{}': {}", table, e),
                        hint: None, source_line: String::new(),
                    })?);
                }
                Ok(Value::List(results))
            }
        }
    }
```

- [ ] **Step 7: Implement SQLite find**

Replace the entire `find` method:

```rust
    pub fn find(&self, table: &str, filter: &Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                let items = storage.get(table).cloned().unwrap_or_default();
                let matches = filter_by_struct(&items, filter);
                match matches.into_iter().next() {
                    Some(item) => Ok(item),
                    None => Ok(Value::Error {
                        variant: "NotFound".to_string(),
                        fields: None,
                    }),
                }
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = match schemas.get(table) {
                    Some(s) => s,
                    None => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                let (sql, params) = if let Value::Struct { fields, .. } = filter {
                    let mut where_parts = Vec::new();
                    let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();
                    for (key, val) in fields {
                        where_parts.push(format!("\"{}\" = ?", key));
                        param_values.push(value_to_sql(val));
                    }
                    let sql = format!(
                        "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
                        table,
                        where_parts.join(" AND ")
                    );
                    (sql, param_values)
                } else {
                    (format!("SELECT * FROM \"{}\" LIMIT 1", table), vec![])
                };
                let param_refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
                let mut stmt = conn.prepare(&sql).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in find on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                let mut rows = stmt
                    .query_map(param_refs.as_slice(), |row| row_to_value(row, schema))
                    .map_err(|e| RuntimeError {
                        line: 0, column: 0,
                        message: format!("Database error in find on table '{}': {}", table, e),
                        hint: None, source_line: String::new(),
                    })?;
                match rows.next() {
                    Some(Ok(val)) => Ok(val),
                    Some(Err(e)) => Err(RuntimeError {
                        line: 0, column: 0,
                        message: format!("Database error reading row from '{}': {}", table, e),
                        hint: None, source_line: String::new(),
                    }),
                    None => Ok(Value::Error {
                        variant: "NotFound".to_string(),
                        fields: None,
                    }),
                }
            }
        }
    }
```

- [ ] **Step 8: Implement SQLite update**

Replace the entire `update` method:

```rust
    pub fn update(&mut self, table: &str, id: &str, new_value: Value) -> Result<Value, RuntimeError> {
        self.ensure_table(table, &new_value)?;
        match self {
            DbBackend::Memory { storage } => {
                if let Some(items) = storage.get_mut(table) {
                    for item in items.iter_mut() {
                        if let Value::Struct { fields, .. } = item {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                *item = new_value.clone();
                                return Ok(new_value);
                            }
                        }
                    }
                }
                Ok(Value::Error {
                    variant: "NotFound".to_string(),
                    fields: None,
                })
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = match schemas.get(table) {
                    Some(s) => s,
                    None => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                let fields = match &new_value {
                    Value::Struct { fields, .. } => fields,
                    _ => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                let mut set_parts = Vec::new();
                let mut params: Vec<Box<dyn ToSql>> = Vec::new();
                for col in schema {
                    if col.name == "id" {
                        continue;
                    }
                    set_parts.push(format!("\"{}\" = ?", col.name));
                    let val = fields.get(&col.name).unwrap_or(&Value::Nothing);
                    params.push(value_to_sql(val));
                }
                params.push(Box::new(id.to_string()));
                let sql = format!(
                    "UPDATE \"{}\" SET {} WHERE \"id\" = ?",
                    table,
                    set_parts.join(", ")
                );
                let param_refs: Vec<&dyn ToSql> = params.iter().map(|p| p.as_ref()).collect();
                let rows_affected = conn.execute(&sql, param_refs.as_slice()).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in update on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                if rows_affected == 0 {
                    Ok(Value::Error {
                        variant: "NotFound".to_string(),
                        fields: None,
                    })
                } else {
                    Ok(new_value)
                }
            }
        }
    }
```

- [ ] **Step 9: Implement SQLite delete**

Replace the entire `delete` method:

```rust
    pub fn delete(&mut self, table: &str, id: &str) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { storage } => {
                if let Some(items) = storage.get_mut(table) {
                    let len_before = items.len();
                    let mut removed = Value::Nothing;
                    items.retain(|item| {
                        if let Value::Struct { fields, .. } = item {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                removed = item.clone();
                                return false;
                            }
                        }
                        true
                    });
                    if items.len() < len_before {
                        return Ok(removed);
                    }
                }
                Ok(Value::Error {
                    variant: "NotFound".to_string(),
                    fields: None,
                })
            }
            DbBackend::Sqlite { conn, schemas } => {
                // First, read the row so we can return it
                let schema = match schemas.get(table) {
                    Some(s) => s.clone(),
                    None => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                // Find the row first
                let select_sql = format!("SELECT * FROM \"{}\" WHERE \"id\" = ? LIMIT 1", table);
                let mut stmt = conn.prepare(&select_sql).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in delete on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                let mut rows = stmt
                    .query_map([id], |row| row_to_value(row, &schema))
                    .map_err(|e| RuntimeError {
                        line: 0, column: 0,
                        message: format!("Database error in delete on table '{}': {}", table, e),
                        hint: None, source_line: String::new(),
                    })?;
                let found = match rows.next() {
                    Some(Ok(val)) => val,
                    _ => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                drop(rows);
                drop(stmt);
                // Delete the row
                let delete_sql = format!("DELETE FROM \"{}\" WHERE \"id\" = ?", table);
                conn.execute(&delete_sql, [id]).map_err(|e| RuntimeError {
                    line: 0, column: 0,
                    message: format!("Database error in delete on table '{}': {}", table, e),
                    hint: None, source_line: String::new(),
                })?;
                Ok(found)
            }
        }
    }
```

- [ ] **Step 10: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compiles successfully.

- [ ] **Step 11: Commit**

```bash
git add src/interpreter/db.rs
git commit -m "feat: implement SQLite CRUD operations in DbBackend"
```

---

### Task 5: SQLite tests

**Files:**
- Modify: `src/interpreter/db.rs`

- [ ] **Step 1: Add test module to db.rs**

Add at the bottom of `src/interpreter/db.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(id: &str, name: &str, age: i64, active: bool) -> Value {
        let mut fields = HashMap::new();
        fields.insert("id".to_string(), Value::String(id.to_string()));
        fields.insert("name".to_string(), Value::String(name.to_string()));
        fields.insert("age".to_string(), Value::Int(age));
        fields.insert("active".to_string(), Value::Bool(active));
        Value::Struct {
            type_name: "User".to_string(),
            fields,
        }
    }

    fn make_filter(key: &str, val: Value) -> Value {
        let mut fields = HashMap::new();
        fields.insert(key.to_string(), val);
        Value::Struct {
            type_name: "Filter".to_string(),
            fields,
        }
    }

    fn new_sqlite() -> DbBackend {
        DbBackend::new_sqlite(":memory:").unwrap()
    }

    #[test]
    fn sqlite_insert_creates_table() {
        let mut db = new_sqlite();
        let user = make_user("1", "Alice", 30, true);
        db.insert("users", user).unwrap();
        // Verify table exists by querying
        let result = db.query("users", None).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn sqlite_insert_and_query() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        db.insert("users", make_user("2", "Bob", 25, true)).unwrap();
        let result = db.query("users", None).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 2);
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn sqlite_find_returns_value() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        let filter = make_filter("id", Value::String("1".to_string()));
        let result = db.find("users", &filter).unwrap();
        if let Value::Struct { fields, .. } = result {
            assert_eq!(fields.get("name"), Some(&Value::String("Alice".to_string())));
        } else {
            panic!("Expected Struct, got {:?}", result);
        }
    }

    #[test]
    fn sqlite_find_not_found() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        let filter = make_filter("id", Value::String("999".to_string()));
        let result = db.find("users", &filter).unwrap();
        assert!(matches!(result, Value::Error { variant, .. } if variant == "NotFound"));
    }

    #[test]
    fn sqlite_update() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        let updated = make_user("1", "Alice Updated", 31, true);
        let result = db.update("users", "1", updated).unwrap();
        if let Value::Struct { fields, .. } = result {
            assert_eq!(fields.get("name"), Some(&Value::String("Alice Updated".to_string())));
        } else {
            panic!("Expected Struct");
        }
        // Verify by reading back
        let filter = make_filter("id", Value::String("1".to_string()));
        let found = db.find("users", &filter).unwrap();
        if let Value::Struct { fields, .. } = found {
            assert_eq!(fields.get("name"), Some(&Value::String("Alice Updated".to_string())));
            assert_eq!(fields.get("age"), Some(&Value::Int(31)));
        } else {
            panic!("Expected Struct");
        }
    }

    #[test]
    fn sqlite_delete() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        let result = db.delete("users", "1").unwrap();
        if let Value::Struct { fields, .. } = result {
            assert_eq!(fields.get("name"), Some(&Value::String("Alice".to_string())));
        } else {
            panic!("Expected Struct");
        }
        // Verify deleted
        let all = db.query("users", None).unwrap();
        if let Value::List(items) = all {
            assert_eq!(items.len(), 0);
        }
    }

    #[test]
    fn sqlite_query_with_filter() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        db.insert("users", make_user("2", "Bob", 25, false)).unwrap();
        let filter = make_filter("active", Value::Bool(true));
        let result = db.query("users", Some(&filter)).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn sqlite_alter_table_new_field() {
        let mut db = new_sqlite();
        db.insert("users", make_user("1", "Alice", 30, true)).unwrap();
        // Insert with extra field
        let mut fields = HashMap::new();
        fields.insert("id".to_string(), Value::String("2".to_string()));
        fields.insert("name".to_string(), Value::String("Bob".to_string()));
        fields.insert("age".to_string(), Value::Int(25));
        fields.insert("active".to_string(), Value::Bool(true));
        fields.insert("email".to_string(), Value::String("bob@test.com".to_string()));
        let user_with_email = Value::Struct {
            type_name: "User".to_string(),
            fields,
        };
        db.insert("users", user_with_email).unwrap();
        // Query and check both rows
        let result = db.query("users", None).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 2);
            // First user should have Nothing for email
            if let Value::Struct { fields, .. } = &items[0] {
                assert_eq!(fields.get("email"), Some(&Value::Nothing));
            }
            // Second user should have email
            if let Value::Struct { fields, .. } = &items[1] {
                assert_eq!(fields.get("email"), Some(&Value::String("bob@test.com".to_string())));
            }
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn sqlite_value_roundtrip() {
        let mut db = new_sqlite();
        let mut fields = HashMap::new();
        fields.insert("id".to_string(), Value::String("1".to_string()));
        fields.insert("s".to_string(), Value::String("hello".to_string()));
        fields.insert("n".to_string(), Value::Int(42));
        fields.insert("f".to_string(), Value::Float(3.14));
        fields.insert("b".to_string(), Value::Bool(true));
        fields.insert("lst".to_string(), Value::List(vec![Value::Int(1), Value::Int(2)]));
        fields.insert("nothing".to_string(), Value::Nothing);
        let value = Value::Struct {
            type_name: "Test".to_string(),
            fields,
        };
        db.insert("test", value).unwrap();
        let filter = make_filter("id", Value::String("1".to_string()));
        let result = db.find("test", &filter).unwrap();
        if let Value::Struct { fields, .. } = result {
            assert_eq!(fields.get("s"), Some(&Value::String("hello".to_string())));
            assert_eq!(fields.get("n"), Some(&Value::Int(42)));
            assert_eq!(fields.get("f"), Some(&Value::Float(3.14)));
            assert_eq!(fields.get("b"), Some(&Value::Bool(true)));
            assert!(matches!(fields.get("lst"), Some(Value::List(_))));
            assert_eq!(fields.get("nothing"), Some(&Value::Nothing));
        } else {
            panic!("Expected Struct, got {:?}", result);
        }
    }

    #[test]
    fn sqlite_bool_roundtrip() {
        let mut db = new_sqlite();
        let mut fields = HashMap::new();
        fields.insert("id".to_string(), Value::String("1".to_string()));
        fields.insert("flag".to_string(), Value::Bool(false));
        fields.insert("count".to_string(), Value::Int(0));
        let value = Value::Struct {
            type_name: "Test".to_string(),
            fields,
        };
        db.insert("mixed", value).unwrap();
        let filter = make_filter("id", Value::String("1".to_string()));
        let result = db.find("mixed", &filter).unwrap();
        if let Value::Struct { fields, .. } = result {
            // Bool(false) and Int(0) both stored as INTEGER 0 — PactType disambiguates
            assert_eq!(fields.get("flag"), Some(&Value::Bool(false)));
            assert_eq!(fields.get("count"), Some(&Value::Int(0)));
        } else {
            panic!("Expected Struct");
        }
    }

    #[test]
    fn sqlite_prepared_statements() {
        let mut db = new_sqlite();
        let injection_name = "'; DROP TABLE users; --";
        db.insert("users", make_user("1", injection_name, 30, true)).unwrap();
        // Table should still exist and name should be stored as literal string
        let result = db.query("users", None).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
            if let Value::Struct { fields, .. } = &items[0] {
                assert_eq!(
                    fields.get("name"),
                    Some(&Value::String(injection_name.to_string()))
                );
            }
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn sqlite_wal_mode() {
        let db = new_sqlite();
        if let DbBackend::Sqlite { conn, .. } = &db {
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        } else {
            panic!("Expected Sqlite backend");
        }
    }

    #[test]
    fn sqlite_no_app_memory_works() {
        // Without app config, Memory backend works as before
        let mut db = DbBackend::new_memory();
        db.insert("test", make_user("1", "Alice", 30, true)).unwrap();
        let result = db.query("test", None).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
        }
    }
}
```

- [ ] **Step 2: Run SQLite tests**

Run: `cargo test db::tests 2>&1 | tail -20`
Expected: All 14 tests pass.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass (existing + new).

- [ ] **Step 4: Commit**

```bash
git add src/interpreter/db.rs
git commit -m "test: add 14 SQLite backend tests including injection and WAL"
```

---

### Task 6: Wire SQLite into server startup and add no-db error

**Files:**
- Modify: `src/main.rs`
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Add db_url validation helper to interpreter**

In `src/interpreter/interpreter.rs`, add a new method to `impl Interpreter`:

```rust
    pub fn open_sqlite(&mut self, url: &str) -> Result<(), RuntimeError> {
        if !url.starts_with("sqlite://") {
            let mut err = self.error(&format!("Invalid database URL '{}'", url));
            err.hint = Some("Expected format: sqlite://path/to/file.db".to_string());
            return Err(err);
        }
        let path = &url["sqlite://".len()..];
        self.db = DbBackend::new_sqlite(path)?;
        Ok(())
    }
```

- [ ] **Step 2: Add no-db-config check to call_builtin**

In `src/interpreter/interpreter.rs`, add a check at the start of `call_builtin` for db operations when app is configured without db:

```rust
    fn call_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Check for db operations without db config in app mode
        if name.starts_with("db.") && name != "db.memory" {
            if let Some((_, _, ref db_url)) = self.app_config {
                if db_url.is_none() {
                    if matches!(self.db, DbBackend::Memory { .. }) {
                        let mut err = self.error("Database not configured");
                        err.hint = Some(
                            "Add db to your app declaration:\n\n  app MyService {\n    port: 8080,\n    db: \"sqlite://data.db\",\n  }".to_string()
                        );
                        return Err(err);
                    }
                }
            }
        }
        match name {
            // ... rest unchanged
```

- [ ] **Step 3: Update main.rs to open SQLite before server start**

In `src/main.rs`, update the server startup section (lines 165-166):

Replace:
```rust
                if let Some((name, port)) = interp.app_config.clone() {
                    pact::interpreter::server::start_server(&mut interp, &name, port);
```

With:
```rust
                if let Some((name, port, db_url)) = interp.app_config.clone() {
                    if let Some(url) = &db_url {
                        if let Err(e) = interp.open_sqlite(url) {
                            eprintln!("{}", e);
                            process::exit(1);
                        }
                    }
                    pact::interpreter::server::start_server(&mut interp, &name, port);
```

- [ ] **Step 4: Add test for no-db error in app mode**

In `src/interpreter/interpreter.rs` test section, add:

```rust
    #[test]
    fn eval_db_no_config_with_app_errors() {
        let input = "app TestApp {\n  port: 8080,\n}\ndb.insert(\"users\", User { name: \"Alice\" })";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let result = interp.interpret(&program);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Database not configured"));
    }
```

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/interpreter/interpreter.rs
git commit -m "feat: wire SQLite into server startup, error on missing db config in app mode"
```

---

### Task 7: End-to-end verification

**Files:**
- No new files — verification only

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass (272+ existing + 14 new SQLite + 1 no-db-config + parser tests).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No errors. Fix any warnings.

- [ ] **Step 3: Verify the backend example still parses**

Run: `cargo run -- docs/spec/PACT_Examples_Backend.pact --ast 2>&1 | head -5`
Expected: Parses without errors.

- [ ] **Step 4: Test SQLite manually with a temp .pact file**

Create a temporary test file and run it:

```bash
cat > /tmp/test_sqlite.pact << 'EOF'
intent "list users"
route GET "/users" {
  needs db

  db.query("users")
    | respond 200 with .
}

intent "create user"
route POST "/users" {
  needs db

  db.insert("users", request.body)
    | on success: respond 201 with .
}

app TestDB {
  port: 9999,
  db: "sqlite:///tmp/test_pact.db",
}
EOF
cargo run -- /tmp/test_sqlite.pact &
sleep 1
# Test POST
curl -s -X POST http://localhost:9999/users -H "Content-Type: application/json" -d '{"id":"1","name":"Alice","age":30}'
# Test GET
curl -s http://localhost:9999/users
# Kill server
kill %1
# Verify db file exists
ls -la /tmp/test_pact.db
# Clean up
rm /tmp/test_sqlite.pact /tmp/test_pact.db
```

Expected: POST returns the created user, GET returns list with the user, db file exists on disk.

- [ ] **Step 5: Commit any fixes**

If clippy or manual testing found issues, fix and commit.

---

## Verification Summary

1. `cargo test` — all tests pass (existing + 15 new)
2. `cargo clippy` — no errors
3. `docs/spec/PACT_Examples_Backend.pact` — still parses
4. Manual SQLite test — data persists to disk, CRUD works via HTTP
5. `using db = db.memory()` in PACT tests — still works without SQLite

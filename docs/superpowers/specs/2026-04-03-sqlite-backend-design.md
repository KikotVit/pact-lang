# SQLite Backend for PACT

## Goal

Add persistent SQLite storage to PACT so that `pact run` with an `app` declaration stores data on disk. Zero external dependencies — SQLite is bundled into the binary via `rusqlite` with `bundled` feature.

## Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| SQLite bundling | `rusqlite` with `bundled` feature | Zero external deps, `brew install pact` and it works |
| Backend abstraction | `enum DbBackend` (not trait object) | Simpler, no lifetime issues, Rust-idiomatic |
| App config syntax | `db: "sqlite://data.db"` (flat) | YAGNI — nested config when we need options |
| Auto-schema | CREATE TABLE on first insert | Obvious behavior, like SQLite creating the file |
| New fields | ALTER TABLE ADD COLUMN | SQLite supports it, dynamic structs are normal |
| Test backend | HashMap stays for `db.memory()` | Don't break what works, fast tests |
| No db: with app | RuntimeError | Silent data loss is the worst bug |
| No app (script/test) | db.memory() works as before | Tests not affected |
| Concurrency | Single-threaded, one Connection | MVP, no pool |
| Journal mode | WAL | Crash safety, better concurrency later |
| SQL injection | Prepared statements only | From day one |

## Architecture

### New file: `src/interpreter/db.rs`

All DB logic moves out of `interpreter.rs` into a dedicated module.

```rust
pub enum DbBackend {
    Memory {
        storage: HashMap<String, Vec<Value>>,
    },
    Sqlite {
        conn: rusqlite::Connection,
        schemas: HashMap<String, Vec<ColDef>>,
    },
}

pub struct ColDef {
    pub name: String,
    pub col_type: ColType,
    pub pact_type: PactType,
}

pub enum ColType {
    Text,
    Integer,
    Real,
}

pub enum PactType {
    String,
    Int,
    Float,
    Bool,
    List,
    Struct,
}
```

### DbBackend methods

```rust
impl DbBackend {
    pub fn insert(&mut self, table: &str, value: Value) -> Result<Value, RuntimeError>;
    pub fn query(&self, table: &str, filter: Option<&Value>) -> Result<Value, RuntimeError>;
    pub fn find(&self, table: &str, filter: &Value) -> Result<Value, RuntimeError>;
    pub fn update(&mut self, table: &str, id: &str, value: Value) -> Result<Value, RuntimeError>;
    pub fn delete(&mut self, table: &str, id: &str) -> Result<Value, RuntimeError>;
    pub fn clear(&mut self);  // for db.memory() reset
}
```

Each method uses `match self` to dispatch to Memory (HashMap) or Sqlite (rusqlite) implementation.

### Interpreter changes

```rust
pub struct Interpreter {
    // REMOVE: pub db_storage: HashMap<String, Vec<Value>>,
    // ADD:
    pub db: DbBackend,
    pub app_config: Option<AppConfig>,
    // ... rest unchanged
}

pub struct AppConfig {
    pub name: String,
    pub port: u16,
    pub db_url: Option<String>,
}
```

- All `builtin_db_*` methods delegate to `self.db.insert()`, etc.
- `db.memory()` → `self.db = DbBackend::Memory { storage: HashMap::new() }`

### App parsing

```pact
app UserService {
  port: 8080,
  db: "sqlite://data.db",
}
```

Parser changes in `parse_app`:
- After parsing `port`, check for optional `db:` key
- Parse string literal as db_url
- Store in `AppConfig { name, port, db_url }`

### DB URL format

- `sqlite://data.db` — relative path, stripped to `data.db`
- `sqlite:///absolute/path/data.db` — absolute path, stripped to `/absolute/path/data.db`
- No prefix → RuntimeError: `Invalid database URL '<url>'. Expected format: sqlite://path/to/file.db`

### Server startup flow

1. Interpreter encounters `Statement::App` → stores `AppConfig`
2. `main.rs` sees `app_config` → before starting server:
   - If `db_url = Some("sqlite://data.db")`:
     - Strip `sqlite://` prefix
     - `Connection::open(path)`
     - `PRAGMA journal_mode=WAL`
     - Replace `self.db` with `DbBackend::Sqlite { conn, schemas: HashMap::new() }`
   - If `db_url = None` and route calls `db.*` → RuntimeError with hint
3. Start `start_server()`

### Error when no db: configured (app mode only)

The check happens at runtime when a `db.*` builtin is called. Logic:

- If `self.db` is `DbBackend::Memory` AND `self.app_config.is_some()` AND `self.app_config.db_url.is_none()` → RuntimeError
- If `self.app_config.is_none()` (script/test mode) → Memory works as before
- This ensures `using db = db.memory()` in tests is never affected

## Value <-> SQLite conversion

### Write (Value -> SQL parameter)

| Value | SQLite type | How |
|-------|------------|-----|
| `String` | TEXT | as-is |
| `Int` | INTEGER | as-is |
| `Float` | REAL | as-is |
| `Bool` | INTEGER | 0 / 1 |
| `List` | TEXT | serde_json serialization |
| `Struct` | TEXT | serde_json serialization |
| `Nothing` | NULL | SQL NULL |

### Read (SQL -> Value)

Uses `PactType` from `ColDef` to disambiguate:
- `PactType::Bool` + INTEGER → `Value::Bool(n != 0)`
- `PactType::Int` + INTEGER → `Value::Int(n)`
- `PactType::Float` + REAL → `Value::Float(f)`
- `PactType::String` + TEXT → `Value::String(s)`
- `PactType::List` + TEXT → `serde_json::from_str` → `Value::List`
- `PactType::Struct` + TEXT → `serde_json::from_str` → `Value::Struct`
- NULL → `Value::Nothing`

## Auto-schema

### First insert into a table

```sql
CREATE TABLE IF NOT EXISTS users (
  id TEXT,
  name TEXT,
  email TEXT,
  age INTEGER,
  active INTEGER,
  created_at TEXT
)
```

Column types derived from Value fields. Schema cached in `schemas` HashMap.

### Insert with new field

Compare struct fields against cached schema. New fields:

```sql
ALTER TABLE users ADD COLUMN deactivated_at TEXT
```

Update cached schema. Existing rows get NULL for new columns (SQLite default).

### Queries

```sql
-- db.query("users")
SELECT * FROM users

-- db.query("users", { active: true })
SELECT * FROM users WHERE active = ?    -- params: [1]

-- db.find("users", { id: "abc" })
SELECT * FROM users WHERE id = ? LIMIT 1  -- params: ["abc"]

-- db.update("users", "abc", updated_struct)
UPDATE users SET name = ?, email = ?, age = ? WHERE id = ?

-- db.delete("users", "abc")
DELETE FROM users WHERE id = ? RETURNING *
```

All queries use prepared statements with `?` parameters. No string interpolation of user data.

## Error messages

| Situation | Message |
|-----------|---------|
| `db.*` without config (app mode) | `Database not configured. Add db: "sqlite://data.db" to your app declaration` |
| Cannot open database file | `Cannot open database '<path>': <OS error>` |
| INSERT/UPDATE/DELETE failure | `Database error in <operation> on table '<table>': <sqlite error>` |
| Invalid db: URL | `Invalid database URL '<url>'. Expected format: sqlite://path/to/file.db` |

All errors include operation, table, and specific cause. LLM agents need this to understand what to fix.

## Testing

### Unchanged

- `using db = db.memory()` in PACT tests — HashMap, no SQLite
- `pact test` — no disk I/O
- All existing 272+ tests pass without changes

### New Rust tests (src/interpreter/db.rs)

| Test | What it verifies |
|------|-----------------|
| `sqlite_insert_creates_table` | First insert creates table with correct columns |
| `sqlite_insert_and_query` | Insert + query returns same data |
| `sqlite_find_returns_value` | Find with filter returns matching row |
| `sqlite_find_not_found` | Find returns NotFound for missing id |
| `sqlite_update` | Update changes row, returns updated |
| `sqlite_delete` | Delete removes row, returns deleted |
| `sqlite_query_with_filter` | Query with struct filter works |
| `sqlite_alter_table_new_field` | Insert with new field triggers ALTER TABLE |
| `sqlite_value_roundtrip` | String/Int/Float/Bool/List/Struct/Nothing survive write+read |
| `sqlite_bool_roundtrip` | Bool not confused with Int |
| `sqlite_no_db_config_with_app_errors` | App without db: + db.insert → RuntimeError |
| `sqlite_no_app_memory_works` | Script without app → db.memory() works |
| `sqlite_prepared_statements` | SQL injection stored as literal string, table intact |
| `sqlite_wal_mode` | PRAGMA journal_mode returns WAL after connection open |

All SQLite tests use `Connection::open_in_memory()` or temp files.

## Dependencies

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
```

Binary size impact: ~1-2MB increase (SQLite C library compiled in).

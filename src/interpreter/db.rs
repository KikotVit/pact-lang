use std::collections::HashMap;

use rusqlite::types::ToSql;
use rusqlite::Connection;

use super::errors::RuntimeError;
use super::value::Value;

// ── Schema helpers ──────────────────────────────────────────────────

/// PACT column type — used for CREATE TABLE mapping.
#[derive(Debug, Clone, PartialEq)]
pub enum PactType {
    String,
    Int,
    Float,
    Bool,
    List,
    Struct,
}

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

/// Column definition: name + PACT type.
#[derive(Debug, Clone)]
pub struct ColDef {
    pub name: String,
    pub pact_type: PactType,
}

// ── Value <-> SQL conversion helpers ────────────────────────────────

/// Convert a PACT Value to a boxed SQL parameter for prepared statements.
fn value_to_sql(value: &Value) -> Box<dyn ToSql> {
    match value {
        Value::String(s) => Box::new(s.clone()),
        Value::Int(n) => Box::new(*n),
        Value::Float(f) => Box::new(*f),
        Value::Bool(b) => Box::new(if *b { 1i64 } else { 0i64 }),
        Value::Nothing => Box::new(rusqlite::types::Null),
        Value::List(_) | Value::Struct { .. } => {
            let json = super::json::value_to_json(value);
            Box::new(serde_json::to_string(&json).unwrap_or_default())
        }
        _ => Box::new(rusqlite::types::Null),
    }
}

/// Convert a SQLite row back into a PACT Value::Struct using the cached schema.
fn row_to_value(row: &rusqlite::Row, schema: &[ColDef]) -> Result<Value, rusqlite::Error> {
    let mut fields = HashMap::new();
    for (i, col) in schema.iter().enumerate() {
        let value = match col.pact_type {
            PactType::String => {
                let v: Option<String> = row.get(i)?;
                v.map_or(Value::Nothing, Value::String)
            }
            PactType::Int => {
                let v: Option<i64> = row.get(i)?;
                v.map_or(Value::Nothing, Value::Int)
            }
            PactType::Float => {
                let v: Option<f64> = row.get(i)?;
                v.map_or(Value::Nothing, Value::Float)
            }
            PactType::Bool => {
                let v: Option<i64> = row.get(i)?;
                v.map_or(Value::Nothing, |n| Value::Bool(n != 0))
            }
            PactType::List | PactType::Struct => {
                let v: Option<String> = row.get(i)?;
                match v {
                    Some(s) => match serde_json::from_str::<serde_json::Value>(&s) {
                        Ok(json) => super::json::json_to_value(&json),
                        Err(_) => Value::String(s),
                    },
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

/// Wrap a rusqlite::Error into a RuntimeError with context.
fn db_error(context: &str, e: rusqlite::Error) -> RuntimeError {
    RuntimeError {
        line: 0,
        column: 0,
        message: format!("Database error in {}: {}", context, e),
        hint: None,
        source_line: String::new(),
    }
}

// ── DbBackend ───────────────────────────────────────────────────────

/// Unified database backend — Memory (HashMap) or Sqlite.
pub enum DbBackend {
    Memory {
        tables: HashMap<String, Vec<Value>>,
    },
    Sqlite {
        conn: Connection,
        schemas: HashMap<String, Vec<ColDef>>,
    },
}

impl DbBackend {
    /// Create an in-memory backend (replicates the old HashMap storage).
    pub fn new_memory() -> Self {
        DbBackend::Memory {
            tables: HashMap::new(),
        }
    }

    /// Create a SQLite-backed store at the given file path.
    pub fn new_sqlite(path: &str) -> Result<Self, RuntimeError> {
        let conn = Connection::open(path)
            .map_err(|e| db_error(&format!("opening database '{}'", path), e))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| db_error("setting WAL journal mode", e))?;
        Ok(DbBackend::Sqlite {
            conn,
            schemas: HashMap::new(),
        })
    }

    /// Auto-create or alter table to match the struct fields.
    fn ensure_table(&mut self, table: &str, value: &Value) -> Result<(), RuntimeError> {
        if let DbBackend::Sqlite { conn, schemas } = self {
            let fields = match value {
                Value::Struct { fields, .. } => fields,
                _ => return Ok(()),
            };
            if let Some(existing) = schemas.get(table) {
                // ALTER TABLE for new columns
                let existing_names: Vec<&str> =
                    existing.iter().map(|c| c.name.as_str()).collect();
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
                    conn.execute(
                        &format!(
                            "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {}",
                            table,
                            col.name,
                            col.pact_type.to_sql_type()
                        ),
                        [],
                    )
                    .map_err(|e| {
                        db_error(
                            &format!("adding column '{}' to '{}'", col.name, table),
                            e,
                        )
                    })?;
                }
                if !new_cols.is_empty() {
                    schemas.get_mut(table).unwrap().extend(new_cols);
                }
            } else {
                // CREATE TABLE
                let mut cols = Vec::new();
                let mut col_defs = Vec::new();
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
                conn.execute(
                    &format!(
                        "CREATE TABLE IF NOT EXISTS \"{}\" ({})",
                        table,
                        col_defs.join(", ")
                    ),
                    [],
                )
                .map_err(|e| db_error(&format!("creating table '{}'", table), e))?;
                schemas.insert(table.to_string(), cols);
            }
        }
        Ok(())
    }

    /// Drop all data (Memory: clear HashMap; Sqlite: no-op).
    pub fn clear(&mut self) {
        match self {
            DbBackend::Memory { tables } => tables.clear(),
            DbBackend::Sqlite { .. } => {
                // No-op: clear is used when switching to db.memory(),
                // which replaces the whole backend anyway.
            }
        }
    }

    // ── CRUD ────────────────────────────────────────────────────────

    /// Insert a value into a table. Returns the inserted value.
    pub fn insert(&mut self, table: &str, value: Value) -> Result<Value, RuntimeError> {
        self.ensure_table(table, &value)?;
        match self {
            DbBackend::Memory { tables } => {
                let ret = value.clone();
                tables
                    .entry(table.to_string())
                    .or_insert_with(Vec::new)
                    .push(value);
                Ok(ret)
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = match schemas.get(table) {
                    Some(s) => s,
                    None => return Ok(value), // shouldn't happen after ensure_table
                };
                let fields = match &value {
                    Value::Struct { fields, .. } => fields,
                    _ => return Ok(value),
                };
                let col_names: Vec<String> =
                    schema.iter().map(|c| format!("\"{}\"", c.name)).collect();
                let placeholders: Vec<&str> = schema.iter().map(|_| "?").collect();
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({})",
                    table,
                    col_names.join(", "),
                    placeholders.join(", ")
                );
                let params: Vec<Box<dyn ToSql>> = schema
                    .iter()
                    .map(|col| value_to_sql(fields.get(&col.name).unwrap_or(&Value::Nothing)))
                    .collect();
                let param_refs: Vec<&dyn ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                conn.execute(&sql, param_refs.as_slice())
                    .map_err(|e| db_error(&format!("insert on table '{}'", table), e))?;
                Ok(value)
            }
        }
    }

    /// Query all rows from a table, optionally filtered by a struct.
    /// Returns `Value::List`.
    pub fn query(&self, table: &str, filter: Option<&Value>) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { tables } => {
                let items = tables.get(table).cloned().unwrap_or_default();
                match filter {
                    Some(f) => Ok(Value::List(filter_by_struct(&items, f))),
                    None => Ok(Value::List(items)),
                }
            }
            DbBackend::Sqlite { conn, schemas } => {
                let schema = match schemas.get(table) {
                    Some(s) => s,
                    None => return Ok(Value::List(vec![])),
                };
                let (sql, params) = if let Some(Value::Struct { fields, .. }) = filter {
                    let mut where_parts = Vec::new();
                    let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();
                    for (key, val) in fields {
                        where_parts.push(format!("\"{}\" = ?", key));
                        param_values.push(value_to_sql(val));
                    }
                    (
                        format!(
                            "SELECT * FROM \"{}\" WHERE {}",
                            table,
                            where_parts.join(" AND ")
                        ),
                        param_values,
                    )
                } else {
                    (format!("SELECT * FROM \"{}\"", table), vec![])
                };
                let param_refs: Vec<&dyn ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| db_error(&format!("query on table '{}'", table), e))?;
                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| row_to_value(row, schema))
                    .map_err(|e| db_error(&format!("query on table '{}'", table), e))?;
                let mut results = Vec::new();
                for row in rows {
                    results.push(
                        row.map_err(|e| {
                            db_error(&format!("reading row from '{}'", table), e)
                        })?,
                    );
                }
                Ok(Value::List(results))
            }
        }
    }

    /// Find the first row matching a filter.
    /// Returns the item or `Value::Error { variant: "NotFound" }`.
    pub fn find(&self, table: &str, filter: &Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { tables } => {
                let items = tables.get(table).cloned().unwrap_or_default();
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
                    (
                        format!(
                            "SELECT * FROM \"{}\" WHERE {} LIMIT 1",
                            table,
                            where_parts.join(" AND ")
                        ),
                        param_values,
                    )
                } else {
                    (format!("SELECT * FROM \"{}\" LIMIT 1", table), vec![])
                };
                let param_refs: Vec<&dyn ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| db_error(&format!("find on table '{}'", table), e))?;
                let mut rows = stmt
                    .query_map(param_refs.as_slice(), |row| row_to_value(row, schema))
                    .map_err(|e| db_error(&format!("find on table '{}'", table), e))?;
                match rows.next() {
                    Some(row) => row.map_err(|e| {
                        db_error(&format!("reading row from '{}'", table), e)
                    }),
                    None => Ok(Value::Error {
                        variant: "NotFound".to_string(),
                        fields: None,
                    }),
                }
            }
        }
    }

    /// Update a row identified by `id` with `new_value`.
    /// Returns the updated value or `Value::Error { variant: "NotFound" }`.
    pub fn update(
        &mut self,
        table: &str,
        id: &str,
        new_value: Value,
    ) -> Result<Value, RuntimeError> {
        self.ensure_table(table, &new_value)?;
        match self {
            DbBackend::Memory { tables } => {
                if let Some(rows) = tables.get_mut(table) {
                    for row in rows.iter_mut() {
                        if let Value::Struct { fields, .. } = row {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                *row = new_value.clone();
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
                        continue; // don't SET the id column
                    }
                    set_parts.push(format!("\"{}\" = ?", col.name));
                    params.push(value_to_sql(
                        fields.get(&col.name).unwrap_or(&Value::Nothing),
                    ));
                }
                // WHERE id = ?
                params.push(value_to_sql(&Value::String(id.to_string())));
                let sql = format!(
                    "UPDATE \"{}\" SET {} WHERE \"id\" = ?",
                    table,
                    set_parts.join(", ")
                );
                let param_refs: Vec<&dyn ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let rows_affected = conn
                    .execute(&sql, param_refs.as_slice())
                    .map_err(|e| db_error(&format!("update on table '{}'", table), e))?;
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

    /// Delete a row identified by `id`.
    /// Returns the removed item or `Value::Error { variant: "NotFound" }`.
    pub fn delete(&mut self, table: &str, id: &str) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { tables } => {
                if let Some(rows) = tables.get_mut(table) {
                    let mut removed = None;
                    rows.retain(|row| {
                        if let Value::Struct { fields, .. } = row {
                            if fields.get("id") == Some(&Value::String(id.to_string())) {
                                removed = Some(row.clone());
                                return false;
                            }
                        }
                        true
                    });
                    if let Some(item) = removed {
                        return Ok(item);
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
                // SELECT the row first so we can return it
                let select_sql = format!(
                    "SELECT * FROM \"{}\" WHERE \"id\" = ? LIMIT 1",
                    table
                );
                let id_param = value_to_sql(&Value::String(id.to_string()));
                let mut stmt = conn
                    .prepare(&select_sql)
                    .map_err(|e| db_error(&format!("delete select on table '{}'", table), e))?;
                let mut rows = stmt
                    .query_map([id_param.as_ref()], |row| row_to_value(row, schema))
                    .map_err(|e| db_error(&format!("delete select on table '{}'", table), e))?;
                let found = match rows.next() {
                    Some(row) => row.map_err(|e| {
                        db_error(&format!("reading row from '{}'", table), e)
                    })?,
                    None => {
                        return Ok(Value::Error {
                            variant: "NotFound".to_string(),
                            fields: None,
                        })
                    }
                };
                // Drop the statement before executing DELETE (borrow rules)
                drop(rows);
                drop(stmt);
                // DELETE the row
                let delete_sql =
                    format!("DELETE FROM \"{}\" WHERE \"id\" = ?", table);
                let id_param2 = value_to_sql(&Value::String(id.to_string()));
                conn.execute(&delete_sql, [id_param2.as_ref()])
                    .map_err(|e| db_error(&format!("delete on table '{}'", table), e))?;
                Ok(found)
            }
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Match items against a struct filter — all fields in filter must match.
pub fn filter_by_struct(items: &[Value], filter: &Value) -> Vec<Value> {
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

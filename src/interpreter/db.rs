use std::collections::HashMap;

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

/// Column definition: name + PACT type.
#[derive(Debug, Clone)]
pub struct ColDef {
    pub name: String,
    pub pact_type: PactType,
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
    pub fn new_sqlite(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(DbBackend::Sqlite { conn, schemas: HashMap::new() })
    }

    /// Drop all data (Memory: clear HashMap; Sqlite: drop all tables).
    pub fn clear(&mut self) {
        match self {
            DbBackend::Memory { tables } => tables.clear(),
            DbBackend::Sqlite { .. } => todo!("Sqlite clear"),
        }
    }

    // ── CRUD ────────────────────────────────────────────────────────

    /// Insert a value into a table. Returns the inserted value.
    pub fn insert(&mut self, table: &str, value: Value) -> Result<Value, RuntimeError> {
        match self {
            DbBackend::Memory { tables } => {
                let ret = value.clone();
                tables
                    .entry(table.to_string())
                    .or_insert_with(Vec::new)
                    .push(value);
                Ok(ret)
            }
            DbBackend::Sqlite { .. } => todo!("Sqlite insert"),
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
            DbBackend::Sqlite { .. } => todo!("Sqlite query"),
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
            DbBackend::Sqlite { .. } => todo!("Sqlite find"),
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
            DbBackend::Sqlite { .. } => todo!("Sqlite update"),
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
            DbBackend::Sqlite { .. } => todo!("Sqlite delete"),
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

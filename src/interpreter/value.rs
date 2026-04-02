use std::collections::HashMap;
use std::fmt;

use crate::parser::ast::{Param, Statement};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nothing,
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Struct {
        type_name: String,
        fields: HashMap<String, Value>,
    },
    Variant {
        type_name: String,
        variant: String,
        fields: Option<HashMap<String, Value>>,
    },
    Ok(Box<Value>),
    Error {
        variant: String,
        fields: Option<HashMap<String, Value>>,
    },
    Function {
        name: String,
        params: Vec<Param>,
        body: Vec<Statement>,
    },
    BuiltinFn {
        name: String,
    },
    Effect {
        name: String,
        methods: HashMap<String, Value>,
    },
}

impl Value {
    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::String(_) => "String",
            Value::Bool(_) => "Bool",
            Value::Nothing => "Nothing",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Struct { type_name, .. } => type_name,
            Value::Variant { type_name, .. } => type_name,
            Value::Ok(_) => "Ok",
            Value::Error { .. } => "Error",
            Value::Function { .. } => "Function",
            Value::BuiltinFn { .. } => "BuiltinFn",
            Value::Effect { .. } => "Effect",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Nothing => false,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
            Value::Error { .. } => false,
            _ => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Nothing => write!(f, "nothing"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (key, val)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::Struct { type_name, fields } => {
                write!(f, "{} {{", type_name)?;
                for (i, (key, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::Variant {
                variant,
                fields: Some(fields),
                ..
            } => {
                write!(f, "{} {{", variant)?;
                for (i, (key, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::Variant {
                variant,
                fields: None,
                ..
            } => write!(f, "{}", variant),
            Value::Ok(val) => write!(f, "Ok({})", val),
            Value::Error {
                variant,
                fields: Some(fields),
            } => {
                write!(f, "Error.{} {{", variant)?;
                for (i, (key, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", key, val)?;
                }
                write!(f, "}}")
            }
            Value::Error {
                variant,
                fields: None,
            } => write!(f, "Error.{}", variant),
            Value::Function { name, .. } => write!(f, "<fn {}>", name),
            Value::BuiltinFn { name } => write!(f, "<builtin {}>", name),
            Value::Effect { name, .. } => write!(f, "<effect {}>", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_type_names() {
        assert_eq!(Value::Int(42).type_name(), "Int");
        assert_eq!(Value::Float(3.14).type_name(), "Float");
        assert_eq!(Value::String("hello".into()).type_name(), "String");
        assert_eq!(Value::Bool(true).type_name(), "Bool");
        assert_eq!(Value::Nothing.type_name(), "Nothing");
        assert_eq!(Value::List(vec![]).type_name(), "List");
        assert_eq!(Value::Map(HashMap::new()).type_name(), "Map");
    }

    #[test]
    fn value_truthiness() {
        assert!(Value::Bool(true).is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        assert!(!Value::Nothing.is_truthy());
        assert!(Value::Int(1).is_truthy());
        assert!(!Value::Int(0).is_truthy());
        assert!(Value::String("hello".into()).is_truthy());
        assert!(!Value::String("".into()).is_truthy());
        assert!(Value::List(vec![Value::Int(1)]).is_truthy());
        assert!(!Value::List(vec![]).is_truthy());
    }

    #[test]
    fn value_display() {
        assert_eq!(format!("{}", Value::Int(42)), "42");
        assert_eq!(format!("{}", Value::Float(3.14)), "3.14");
        assert_eq!(format!("{}", Value::String("hello".into())), "hello");
        assert_eq!(format!("{}", Value::Bool(true)), "true");
        assert_eq!(format!("{}", Value::Nothing), "nothing");
    }
}

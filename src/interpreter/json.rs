use crate::interpreter::value::Value;
use std::collections::HashMap;

pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::Float(n) => serde_json::json!(*n),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Nothing => serde_json::Value::Null,
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Struct { fields, .. } => {
            let obj: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Variant {
            variant,
            fields: None,
            ..
        } => serde_json::Value::String(variant.clone()),
        Value::Variant {
            variant,
            fields: Some(f),
            ..
        } => {
            let mut obj: serde_json::Map<String, serde_json::Value> = f
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            obj.insert(
                "type".to_string(),
                serde_json::Value::String(variant.clone()),
            );
            serde_json::Value::Object(obj)
        }
        Value::Ok(inner) => value_to_json(inner),
        Value::Error { variant, fields } => {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "error".to_string(),
                serde_json::Value::String(variant.clone()),
            );
            if let Some(f) = fields {
                for (k, v) in f {
                    obj.insert(k.clone(), value_to_json(v));
                }
            }
            serde_json::Value::Object(obj)
        }
        Value::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::DbWatch { table, .. } => {
            serde_json::json!({"__type": "DbWatch", "table": table})
        }
        // Functions, builtins, effects don't serialize
        _ => serde_json::Value::Null,
    }
}

pub fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Nothing,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Nothing
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            let fields: HashMap<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Struct {
                type_name: "Object".to_string(),
                fields,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_roundtrip() {
        let v = Value::Int(42);
        let json = value_to_json(&v);
        assert_eq!(json, serde_json::json!(42));
        assert_eq!(json_to_value(&json), v);
    }

    #[test]
    fn string_roundtrip() {
        let v = Value::String("hello".to_string());
        let json = value_to_json(&v);
        assert_eq!(json, serde_json::json!("hello"));
        assert_eq!(json_to_value(&json), v);
    }

    #[test]
    fn struct_to_json() {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), Value::String("Alice".to_string()));
        fields.insert("age".to_string(), Value::Int(30));
        let v = Value::Struct {
            type_name: "User".to_string(),
            fields,
        };
        let json = value_to_json(&v);
        assert_eq!(json["name"], "Alice");
        assert_eq!(json["age"], 30);
    }

    #[test]
    fn json_object_to_value() {
        let json = serde_json::json!({"name": "Bob", "active": true});
        let v = json_to_value(&json);
        if let Value::Struct { fields, .. } = &v {
            assert_eq!(fields.get("name"), Some(&Value::String("Bob".to_string())));
            assert_eq!(fields.get("active"), Some(&Value::Bool(true)));
        } else {
            panic!("Expected Struct");
        }
    }

    #[test]
    fn nothing_roundtrip() {
        assert_eq!(value_to_json(&Value::Nothing), serde_json::Value::Null);
        assert_eq!(json_to_value(&serde_json::Value::Null), Value::Nothing);
    }

    #[test]
    fn list_to_json() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let json = value_to_json(&v);
        assert_eq!(json, serde_json::json!([1, 2, 3]));
    }
}

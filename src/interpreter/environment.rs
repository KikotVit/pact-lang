use std::collections::{HashMap, HashSet};

use super::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    values: HashMap<String, Value>,
    mutables: HashSet<String>,
    parent: Option<Box<Environment>>,
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            values: HashMap::new(),
            mutables: HashSet::new(),
            parent: None,
        }
    }

    pub fn with_parent(parent: Environment) -> Self {
        Environment {
            values: HashMap::new(),
            mutables: HashSet::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn bind(&mut self, name: String, value: Value, mutable: bool) {
        self.values.insert(name.clone(), value);
        if mutable {
            self.mutables.insert(name);
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&Value> {
        if let Some(val) = self.values.get(name) {
            Some(val)
        } else if let Some(ref parent) = self.parent {
            parent.lookup(name)
        } else {
            None
        }
    }

    pub fn assign(&mut self, name: &str, value: Value) -> Result<(), String> {
        if self.values.contains_key(name) {
            if self.mutables.contains(name) {
                self.values.insert(name.to_string(), value);
                Ok(())
            } else {
                Err(format!(
                    "Cannot reassign immutable variable '{}'",
                    name
                ))
            }
        } else if let Some(ref mut parent) = self.parent {
            parent.assign(name, value)
        } else {
            Err(format!("Undefined variable '{}'", name))
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_lookup() {
        let mut env = Environment::new();
        env.bind("x".to_string(), Value::Int(42), false);
        assert_eq!(env.lookup("x"), Some(&Value::Int(42)));
        assert_eq!(env.lookup("y"), None);
    }

    #[test]
    fn parent_lookup() {
        let mut parent = Environment::new();
        parent.bind("x".to_string(), Value::Int(10), false);

        let child = Environment::with_parent(parent);
        assert_eq!(child.lookup("x"), Some(&Value::Int(10)));
    }

    #[test]
    fn child_shadows_parent() {
        let mut parent = Environment::new();
        parent.bind("x".to_string(), Value::Int(10), false);

        let mut child = Environment::with_parent(parent);
        child.bind("x".to_string(), Value::Int(20), false);
        assert_eq!(child.lookup("x"), Some(&Value::Int(20)));
    }

    #[test]
    fn assign_mutable() {
        let mut env = Environment::new();
        env.bind("x".to_string(), Value::Int(1), true);
        assert!(env.assign("x", Value::Int(2)).is_ok());
        assert_eq!(env.lookup("x"), Some(&Value::Int(2)));
    }

    #[test]
    fn assign_immutable_fails() {
        let mut env = Environment::new();
        env.bind("x".to_string(), Value::Int(1), false);
        let result = env.assign("x", Value::Int(2));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }
}

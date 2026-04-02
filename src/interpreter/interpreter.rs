use std::collections::HashMap;

use crate::parser::ast::Program;
use super::environment::Environment;
use super::errors::RuntimeError;
use super::value::Value;

pub struct Interpreter {
    pub global: Environment,
    source: String,
    db_storage: HashMap<String, Vec<Value>>,
    fixed_time: Option<String>,
    rng_seed: Option<u64>,
    rng_counter: u64,
}

impl Interpreter {
    pub fn new(source: &str) -> Self {
        Interpreter {
            global: Environment::new(),
            source: source.to_string(),
            db_storage: HashMap::new(),
            fixed_time: None,
            rng_seed: None,
            rng_counter: 0,
        }
    }

    pub fn interpret(&mut self, _program: &Program) -> Result<Value, RuntimeError> {
        // To be implemented in Task 2
        Ok(Value::Nothing)
    }
}

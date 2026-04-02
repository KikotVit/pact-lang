# PACT Interpreter v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a tree-walking interpreter that executes PACT AST — literals, arithmetic, functions, pipelines, error handling, struct literals, with in-memory effect stubs (db, time, rng).

**Architecture:** The interpreter walks AST nodes via `eval_expr`/`eval_statement`, uses `Environment` for lexical scoping (global → function → block), and `Value` enum for runtime values including explicit `Ok`/`Error` result types. Pipeline steps operate on lists with `_it` binding for per-element operations. Effects are dispatched by builtin name matching.

**Tech Stack:** Rust 1.94, no external dependencies. Consumes `Program` AST from existing parser.

**Design spec:** `docs/superpowers/specs/2026-04-02-pact-interpreter-v01-design.md`

---

## File Structure

```
src/
  lib.rs                — add: pub mod interpreter
  main.rs               — add: pact run <file> command
  interpreter/
    mod.rs              — pub mod, re-exports
    value.rs            — Value enum, Display impl, helper methods
    environment.rs      — Environment struct (lookup, bind, assign)
    errors.rs           — RuntimeError with Display impl
    interpreter.rs      — Interpreter struct, eval_statement, eval_expr
    pipeline.rs         — pipeline step execution
    builtins.rs         — builtin function dispatch (db, time, rng)
```

---

### Task 1: Value, RuntimeError, Environment

**Files:**
- Create: `src/interpreter/value.rs`
- Create: `src/interpreter/errors.rs`
- Create: `src/interpreter/environment.rs`
- Create: `src/interpreter/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/interpreter/value.rs`**

```rust
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
            Value::Error { variant, .. } => variant,
            Value::Function { name, .. } => name,
            Value::BuiltinFn { name, .. } => name,
            Value::Effect { name, .. } => name,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Nothing => false,
            Value::Int(0) => false,
            Value::String(s) if s.is_empty() => false,
            Value::List(items) if items.is_empty() => false,
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
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => write!(f, "Map({} entries)", map.len()),
            Value::Struct { type_name, fields } => {
                write!(f, "{} {{ ", type_name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Variant { variant, fields: None, .. } => write!(f, "{}", variant),
            Value::Variant { variant, fields: Some(flds), .. } => {
                write!(f, "{} {{ ", variant)?;
                for (i, (k, v)) in flds.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Ok(v) => write!(f, "Ok({})", v),
            Value::Error { variant, fields: None } => write!(f, "Error({})", variant),
            Value::Error { variant, fields: Some(flds) } => {
                write!(f, "Error({} {{ ", variant)?;
                for (i, (k, v)) in flds.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, " }})")
            }
            Value::Function { name, .. } => write!(f, "<fn {}>", name),
            Value::BuiltinFn { name } => write!(f, "<builtin {}>", name),
            Value::Effect { name, .. } => write!(f, "<effect {}>", name),
        }
    }
}
```

- [ ] **Step 2: Create `src/interpreter/errors.rs`**

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

impl std::error::Error for RuntimeError {}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Runtime error at line {}, col {}:", self.line, self.column)?;
        writeln!(f, "  {}", self.source_line)?;
        let padding = self.column - 1 + 2;
        writeln!(f, "{:>width$}^", "", width = padding)?;
        write!(f, "  {}", self.message)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_error_display() {
        let err = RuntimeError {
            line: 8,
            column: 3,
            message: "Unhandled error: NotFound".to_string(),
            hint: Some("add NotFound to function's error types".to_string()),
            source_line: "  let user: User = find_user(id)?".to_string(),
        };
        let output = format!("{}", err);
        assert!(output.contains("Runtime error at line 8"));
        assert!(output.contains("Unhandled error: NotFound"));
        assert!(output.contains("Hint:"));
    }
}
```

- [ ] **Step 3: Create `src/interpreter/environment.rs`**

```rust
use std::collections::{HashMap, HashSet};
use crate::interpreter::value::Value;

#[derive(Debug, Clone)]
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
            if !self.mutables.contains(name) {
                return Err(format!("Cannot assign to immutable variable '{}'", name));
            }
            self.values.insert(name.to_string(), value);
            Ok(())
        } else if let Some(ref mut parent) = self.parent {
            parent.assign(name, value)
        } else {
            Err(format!("Undefined variable '{}'", name))
        }
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
        parent.bind("x".to_string(), Value::Int(1), false);
        let child = Environment::with_parent(parent);
        assert_eq!(child.lookup("x"), Some(&Value::Int(1)));
    }

    #[test]
    fn child_shadows_parent() {
        let mut parent = Environment::new();
        parent.bind("x".to_string(), Value::Int(1), false);
        let mut child = Environment::with_parent(parent);
        child.bind("x".to_string(), Value::Int(2), false);
        assert_eq!(child.lookup("x"), Some(&Value::Int(2)));
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
        assert!(env.assign("x", Value::Int(2)).is_err());
    }
}
```

- [ ] **Step 4: Create `src/interpreter/mod.rs` and update `src/lib.rs`**

`src/interpreter/mod.rs`:
```rust
pub mod value;
pub mod environment;
pub mod errors;
pub mod interpreter;
pub mod pipeline;
pub mod builtins;

pub use value::Value;
pub use environment::Environment;
pub use errors::RuntimeError;
pub use interpreter::Interpreter;
```

Add to `src/lib.rs`:
```rust
pub mod lexer;
pub mod parser;
pub mod interpreter;
```

Create empty placeholder files so it compiles:
- `src/interpreter/interpreter.rs` — empty struct
- `src/interpreter/pipeline.rs` — empty
- `src/interpreter/builtins.rs` — empty

- [ ] **Step 5: Run tests, commit**

Run: `cargo test`
Expected: all pass (137 existing + ~7 new).

```bash
git add src/interpreter/ src/lib.rs
git commit -m "feat: add Value, RuntimeError, Environment for PACT interpreter"
```

---

### Task 2: Interpreter scaffold + literal evaluation + let/var

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Create Interpreter struct and basic eval**

```rust
use std::collections::HashMap;
use crate::parser::ast::*;
use crate::interpreter::value::Value;
use crate::interpreter::environment::Environment;
use crate::interpreter::errors::RuntimeError;

pub enum ControlFlow {
    Return(Value),
    RuntimeError(RuntimeError),
}

pub struct Interpreter {
    pub global: Environment,
    source: String,
    // Effect state
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

    pub fn interpret(&mut self, program: &Program) -> Result<Value, RuntimeError> {
        let mut env = Environment::with_parent(self.global.clone());
        let mut last = Value::Nothing;
        for stmt in &program.statements {
            last = self.eval_statement(stmt, &mut env)?;
        }
        Ok(last)
    }

    pub fn eval_statement(&mut self, stmt: &Statement, env: &mut Environment) -> Result<Value, RuntimeError> {
        match stmt {
            Statement::Let { name, mutable, value, .. } => {
                let val = self.eval_expr(value, env)?;
                env.bind(name.clone(), val.clone(), *mutable);
                Ok(val)
            }
            Statement::FnDecl { name, params, body, .. } => {
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                env.bind(name.clone(), func, false);
                Ok(Value::Nothing)
            }
            Statement::TypeDecl(_) => {
                // Type declarations don't produce runtime values in v0.1
                Ok(Value::Nothing)
            }
            Statement::Use { .. } => {
                // Use imports not implemented in v0.1
                Ok(Value::Nothing)
            }
            Statement::Return { value, condition } => {
                // Handle return X if Y
                if let Some(cond) = condition {
                    let cond_val = self.eval_expr(cond, env)?;
                    if !cond_val.is_truthy() {
                        return Ok(Value::Nothing); // condition false, don't return
                    }
                }
                let ret_val = match value {
                    Some(expr) => self.eval_expr(expr, env)?,
                    None => Value::Nothing,
                };
                Err(self.make_return(ret_val))
            }
            Statement::Expression(expr) => {
                self.eval_expr(expr, env)
            }
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr, env: &mut Environment) -> Result<Value, RuntimeError> {
        match expr {
            Expr::IntLiteral(n) => Ok(Value::Int(*n)),
            Expr::FloatLiteral(n) => Ok(Value::Float(*n)),
            Expr::BoolLiteral(b) => Ok(Value::Bool(*b)),
            Expr::Nothing => Ok(Value::Nothing),
            Expr::Identifier(name) => {
                env.lookup(name)
                    .cloned()
                    .ok_or_else(|| self.runtime_error(
                        &format!("Undefined variable '{}'", name), None
                    ))
            }

            // Placeholders for later tasks:
            Expr::StringLiteral(_) => self.eval_string(expr, env),
            Expr::BinaryOp { .. } => self.eval_binary_op(expr, env),
            Expr::UnaryOp { .. } => self.eval_unary_op(expr, env),
            Expr::FieldAccess { .. } => self.eval_field_access(expr, env),
            Expr::DotShorthand(_) => self.eval_dot_shorthand(expr, env),
            Expr::FnCall { .. } => self.eval_fn_call(expr, env),
            Expr::Pipeline { .. } => self.eval_pipeline(expr, env),
            Expr::If { .. } => self.eval_if(expr, env),
            Expr::Match { .. } => self.eval_match(expr, env),
            Expr::Block(stmts) => self.eval_block(stmts, env),
            Expr::StructLiteral { .. } => self.eval_struct_literal(expr, env),
            Expr::Ensure(pred) => self.eval_ensure(pred, env),
            Expr::ErrorPropagation(inner) => self.eval_error_propagation(inner, env),
            Expr::Is { .. } => self.eval_is(expr, env),
        }
    }

    // --- Placeholder methods (implemented in later tasks) ---
    fn eval_string(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Strings not yet implemented", None))
    }
    fn eval_binary_op(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Binary ops not yet implemented", None))
    }
    fn eval_unary_op(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Unary ops not yet implemented", None))
    }
    fn eval_field_access(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Field access not yet implemented", None))
    }
    fn eval_dot_shorthand(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Dot shorthand not yet implemented", None))
    }
    fn eval_fn_call(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Function calls not yet implemented", None))
    }
    fn eval_pipeline(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Pipeline not yet implemented", None))
    }
    fn eval_if(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("If not yet implemented", None))
    }
    fn eval_match(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Match not yet implemented", None))
    }
    fn eval_block(&mut self, _stmts: &[Statement], _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Block not yet implemented", None))
    }
    fn eval_struct_literal(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Struct literal not yet implemented", None))
    }
    fn eval_ensure(&mut self, _pred: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Ensure not yet implemented", None))
    }
    fn eval_error_propagation(&mut self, _inner: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Error propagation not yet implemented", None))
    }
    fn eval_is(&mut self, _expr: &Expr, _env: &mut Environment) -> Result<Value, RuntimeError> {
        Err(self.runtime_error("Is not yet implemented", None))
    }

    // --- Error helpers ---
    fn runtime_error(&self, message: &str, hint: Option<&str>) -> RuntimeError {
        RuntimeError {
            line: 0,
            column: 0,
            message: message.to_string(),
            hint: hint.map(|s| s.to_string()),
            source_line: String::new(),
        }
    }

    /// Create a ControlFlow::Return disguised as RuntimeError for propagation.
    /// The function body runner catches this and extracts the value.
    fn make_return(&self, value: Value) -> RuntimeError {
        RuntimeError {
            line: 0,
            column: 0,
            message: format!("__RETURN__:{}", serde_return_value(&value)),
            hint: None,
            source_line: String::new(),
        }
    }

    fn is_return_error(err: &RuntimeError) -> Option<Value> {
        // For v0.1, use a simple encoding for return values.
        // A proper ControlFlow enum would be cleaner but requires changing
        // the return type of eval_statement/eval_expr. For now, we tag
        // returns with a magic prefix in the message.
        None // Placeholder — proper implementation in Task 5
    }
}

fn serde_return_value(_value: &Value) -> String {
    // Placeholder
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn eval(input: &str) -> Value {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.interpret(&program).unwrap()
    }

    fn eval_fails(input: &str) -> RuntimeError {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.interpret(&program).unwrap_err()
    }

    #[test]
    fn eval_int_literal() {
        assert_eq!(eval("42"), Value::Int(42));
    }

    #[test]
    fn eval_float_literal() {
        assert_eq!(eval("3.14"), Value::Float(3.14));
    }

    #[test]
    fn eval_bool_literal() {
        assert_eq!(eval("true"), Value::Bool(true));
        assert_eq!(eval("false"), Value::Bool(false));
    }

    #[test]
    fn eval_nothing() {
        assert_eq!(eval("nothing"), Value::Nothing);
    }

    #[test]
    fn eval_let_binding() {
        assert_eq!(eval("let x: Int = 42\nx"), Value::Int(42));
    }

    #[test]
    fn eval_var_reassign() {
        assert_eq!(eval("var x: Int = 1\nx"), Value::Int(1));
    }

    #[test]
    fn eval_undefined_variable() {
        let err = eval_fails("x");
        assert!(err.message.contains("Undefined variable 'x'"));
    }
}
```

NOTE: The `return` handling via magic strings in RuntimeError is a temporary hack. The proper approach is to use a custom result type. The implementer should feel free to use a better approach — for example:

```rust
enum EvalResult {
    Value(Value),
    Return(Value),
    Error(RuntimeError),
}
```

Or keep `Result<Value, RuntimeError>` but use a special `RuntimeError` variant to signal returns. The tests above don't test `return` yet — that's Task 7.

- [ ] **Step 2: Run tests, commit**

Run: `cargo test`

```bash
git add src/interpreter/interpreter.rs
git commit -m "feat: add Interpreter scaffold with literal eval and let/var bindings"
```

---

### Task 3: Binary ops, unary ops, comparison, is

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Replace `eval_binary_op`, `eval_unary_op`, `eval_is` placeholders**

`eval_binary_op`: match on `BinaryOp` variant, eval left and right, apply operation. Handle Int+Int, Float+Float, Int+Float promotion, String concatenation (Add), Bool and/or. Comparison returns Bool.

`eval_unary_op`: `Neg` negates Int/Float, `Not` negates Bool.

`eval_is`: eval expr, check if it's a `Value::Variant` or `Value::Error` with matching name.

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_addition() { assert_eq!(eval("1 + 2"), Value::Int(3)); }

#[test]
fn eval_subtraction() { assert_eq!(eval("10 - 3"), Value::Int(7)); }

#[test]
fn eval_multiplication() { assert_eq!(eval("3 * 4"), Value::Int(12)); }

#[test]
fn eval_division() { assert_eq!(eval("10 / 3"), Value::Int(3)); }

#[test]
fn eval_float_arithmetic() { assert_eq!(eval("1.5 + 2.5"), Value::Float(4.0)); }

#[test]
fn eval_comparison_eq() { assert_eq!(eval("1 == 1"), Value::Bool(true)); }

#[test]
fn eval_comparison_neq() { assert_eq!(eval("1 != 2"), Value::Bool(true)); }

#[test]
fn eval_comparison_lt() { assert_eq!(eval("1 < 2"), Value::Bool(true)); }

#[test]
fn eval_and_or() { assert_eq!(eval("true and false"), Value::Bool(false)); }

#[test]
fn eval_not() { assert_eq!(eval("not true"), Value::Bool(false)); }

#[test]
fn eval_negation() { assert_eq!(eval("-5"), Value::Int(-5)); }

#[test]
fn eval_string_concat() { assert_eq!(eval(r#""hello" + " world""#), Value::String("hello world".to_string())); }

#[test]
fn eval_complex_arithmetic() { assert_eq!(eval("(1 + 2) * 3 - 4"), Value::Int(5)); }
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add binary ops, unary ops, comparison, is-expression evaluation"
```

---

### Task 4: Field access, dot shorthand, string interpolation

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Replace `eval_field_access`, `eval_dot_shorthand`, `eval_string` placeholders**

`eval_field_access`: eval object. If `Struct` → get field from HashMap. If `Effect` → get method from methods. Error with hint if field not found.

`eval_dot_shorthand`: lookup `_it` in env, then chain field access through the parts vec.

`eval_string`: If `Simple` → `Value::String`. If `Interpolated` → eval each part, concat. `StringPart::Literal` → string, `StringPart::Expr` → eval and convert to string via Display.

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_struct_field_access() {
    let input = "let user: User = User { name: \"Vitalii\", age: 30 }\nuser.name";
    // This requires struct literal eval — might need Task 6 first.
    // Alternative: test with a simpler setup
}

#[test]
fn eval_simple_string() {
    assert_eq!(eval(r#""hello""#), Value::String("hello".to_string()));
}

#[test]
fn eval_string_interpolation() {
    assert_eq!(eval(r#"let name: String = "world"
"hello {name}""#), Value::String("hello world".to_string()));
}

#[test]
fn eval_empty_string() {
    assert_eq!(eval(r#""""#), Value::String(String::new()));
}
```

Note: Field access on structs will be more thoroughly tested after struct literals are implemented (Task 6). For now, test string evaluation.

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add field access, dot shorthand, string interpolation evaluation"
```

---

### Task 5: Function declarations and calls

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Replace `eval_fn_call` placeholder and implement proper return handling**

This is the most complex task. Function calls need to:
1. Eval callee — get `Value::Function` or `Value::BuiltinFn`
2. Eval args
3. For `Function`: create new env (parent = global), bind params, eval body statements
4. For `BuiltinFn`: dispatch to `self.call_builtin(name, args)`
5. Handle `return` — the body may use `return` to exit early

For return handling, replace the magic-string hack with a proper approach. One clean option:

```rust
fn eval_function_body(&mut self, body: &[Statement], env: &mut Environment) -> Result<Value, RuntimeError> {
    let mut last = Value::Nothing;
    for stmt in body {
        match self.eval_statement(stmt, env) {
            Ok(val) => last = val,
            Err(err) => {
                // Check if this is a return, not a real error
                if err.message.starts_with("__RETURN__") {
                    // Extract the returned value — stored in a side channel
                    return Ok(self.take_return_value());
                }
                return Err(err);
            }
        }
    }
    Ok(last)
}
```

Or better: change `eval_statement` return to a custom enum. The implementer should choose the cleanest approach that works. The key contract: `return X` in a function body causes the function to immediately return `X`.

Also implement `eval_block` for `Expr::Block`: create child env, eval statements, return last value.

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_simple_function() {
    assert_eq!(eval("fn add(a: Int, b: Int) -> Int {\n  a + b\n}\nadd(1, 2)"), Value::Int(3));
}

#[test]
fn eval_function_multiple_statements() {
    assert_eq!(eval("fn double(x: Int) -> Int {\n  let result: Int = x * 2\n  result\n}\ndouble(5)"), Value::Int(10));
}

#[test]
fn eval_nested_function_call() {
    assert_eq!(eval("fn add(a: Int, b: Int) -> Int {\n  a + b\n}\nfn double(x: Int) -> Int {\n  add(x, x)\n}\ndouble(3)"), Value::Int(6));
}

#[test]
fn eval_recursive_function() {
    let input = "fn factorial(n: Int) -> Int {\n  if n <= 1 {\n    1\n  } else {\n    n * factorial(n - 1)\n  }\n}\nfactorial(5)";
    assert_eq!(eval(input), Value::Int(120));
}
```

Note: recursive function test requires `if` and `eval_if` — implement `eval_if` and `eval_match` here too since they're needed:

`eval_if`: eval condition, if truthy eval then_body (as block), else eval else_body if present.

`eval_match`: eval subject, iterate arms, check pattern match (Wildcard always matches, Identifier matches variant name or binds, Literal matches by equality), eval matching body.

```rust
#[test]
fn eval_if_true() {
    assert_eq!(eval("if true {\n  1\n} else {\n  2\n}"), Value::Int(1));
}

#[test]
fn eval_if_false() {
    assert_eq!(eval("if false {\n  1\n} else {\n  2\n}"), Value::Int(2));
}

#[test]
fn eval_match_wildcard() {
    assert_eq!(eval("match 42 {\n  _ => true,\n}"), Value::Bool(true));
}
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add function calls, if/else, match, block evaluation with return handling"
```

---

### Task 6: Struct literals, ensure, error propagation, return-if

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Replace `eval_struct_literal`, `eval_ensure`, `eval_error_propagation` placeholders**

`eval_struct_literal`: Eval each field. Handle `StructField::Named` (eval value) and `StructField::Spread` (eval, merge fields from spread struct). Construct `Value::Struct`.

`eval_ensure`: Eval predicate. If false → `RuntimeError` (this is an interpreter crash, not a business error). If true → `Value::Nothing`.

`eval_error_propagation` (`?`): Eval inner expression. If `Value::Ok(v)` → return `v`. If `Value::Error { .. }` → propagate as return (use the return mechanism from Task 5). Otherwise → RuntimeError.

Update `eval_statement` for `Return` with error wrapping: if `return NotFound` (where NotFound is a variant constructor), wrap in `Value::Error`. The caller decides based on their error_types whether the return value is Ok or Error.

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_struct_literal() {
    let input = r#"let user: User = User { name: "Vitalii", age: 30 }
user.name"#;
    assert_eq!(eval(input), Value::String("Vitalii".to_string()));
}

#[test]
fn eval_struct_with_spread() {
    let input = r#"let old: User = User { name: "A", age: 1 }
let new: User = User { ...old, name: "B" }
new.age"#;
    assert_eq!(eval(input), Value::Int(1));
}

#[test]
fn eval_ensure_passes() {
    assert_eq!(eval("ensure 1 > 0\n42"), Value::Int(42));
}

#[test]
fn eval_ensure_fails() {
    let err = eval_fails("ensure 1 < 0");
    assert!(err.message.contains("Ensure"));
}

#[test]
fn eval_error_propagation_ok() {
    // Simulate: function returns Ok, ? unwraps it
    // Need a function that returns Ok/Error first
}

#[test]
fn eval_anonymous_struct() {
    let input = r#"let info: Info = { status: "ok", count: 5 }
info.status"#;
    assert_eq!(eval(input), Value::String("ok".to_string()));
}
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat: add struct literals, ensure, error propagation, return-if evaluation"
```

---

### Task 7: Pipeline execution

**Files:**
- Modify: `src/interpreter/pipeline.rs`
- Modify: `src/interpreter/interpreter.rs` (replace `eval_pipeline` placeholder)

- [ ] **Step 1: Implement pipeline step execution**

In `src/interpreter/pipeline.rs`, implement a function:

```rust
pub fn execute_pipeline_step(
    interpreter: &mut Interpreter,
    step: &PipelineStep,
    current: Value,
    env: &mut Environment,
) -> Result<Value, RuntimeError> { ... }
```

For each step type:
- **Filter/Map/Sort/Each:** require `Value::List`, iterate with `_it` binding
- **Count/Sum/Flatten/Unique:** require `Value::List`, aggregate
- **Take/Skip/GroupBy:** require `Value::List`, slice/group
- **FindFirst/ExpectOne/ExpectAny:** require `Value::List`, return Ok/Error
- **OrDefault:** if Nothing → use default
- **Expr:** set `_it` = current value, eval expression

Non-List input for per-element steps → RuntimeError with hint.

In `interpreter.rs`, replace `eval_pipeline`:
```rust
fn eval_pipeline(&mut self, expr: &Expr, env: &mut Environment) -> Result<Value, RuntimeError> {
    if let Expr::Pipeline { source, steps } = expr {
        let mut current = self.eval_expr(source, env)?;
        for step in steps {
            current = pipeline::execute_pipeline_step(self, step, current, env)?;
        }
        Ok(current)
    } else {
        unreachable!()
    }
}
```

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_pipeline_count() {
    let input = "let items: List = [1, 2, 3]\nitems | count";
    // Note: list literals [...] are not in the parser yet.
    // Test via struct with list field, or add list literal support.
    // For now, test what we can:
}

#[test]
fn eval_pipeline_filter() {
    // Will need list literals or a way to construct lists
}

#[test]
fn eval_pipeline_map() {
    // Will need list literals
}
```

NOTE: PACT doesn't have list literal syntax `[1, 2, 3]` in the parser (lists come from db.query or function returns). The implementer should either:
a) Add a test helper that creates lists directly via the interpreter, OR
b) Test pipelines through function returns that produce lists, OR
c) Add a temporary `list(...)` builtin for testing

Option (c) is recommended — add a builtin `list(1, 2, 3)` that returns `Value::List`. This is useful for testing and doesn't change the language spec.

Better pipeline tests with `list()` builtin:

```rust
#[test]
fn eval_pipeline_count() {
    // Register list() builtin in interpreter setup
    assert_eq!(eval_with_builtins("list(1, 2, 3) | count"), Value::Int(3));
}

#[test]
fn eval_pipeline_sum() {
    assert_eq!(eval_with_builtins("list(1, 2, 3) | sum"), Value::Int(6));
}

#[test]
fn eval_pipeline_filter() {
    // filter where needs struct fields — test with simple comparison
    // list(1, 2, 3, 4) | filter where _it > 2
    // But DotShorthand doesn't work with raw values...
    // Need to test with structs
}

#[test]
fn eval_pipeline_multi_step() {
    assert_eq!(
        eval_with_builtins("list(1, 2, 3, 4, 5) | count"),
        Value::Int(5),
    );
}
```

The implementer should add a `list()` builtin and an `eval_with_builtins` test helper.

- [ ] **Step 3: Run tests, commit**

```bash
git add src/interpreter/pipeline.rs src/interpreter/interpreter.rs
git commit -m "feat: add pipeline execution with all step types"
```

---

### Task 8: Builtin functions and effect stubs

**Files:**
- Modify: `src/interpreter/builtins.rs`
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Implement builtin dispatch**

In `builtins.rs`, implement the dispatch function:

```rust
pub fn call_builtin(
    interpreter: &mut Interpreter,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    match name {
        // List constructor for testing
        "list" => Ok(Value::List(args)),

        // db methods
        "db.insert" => { /* insert into db_storage */ }
        "db.query" => { /* return all rows from table */ }
        "db.update" => { /* update matching rows */ }

        // time methods
        "time.now" => {
            Ok(Value::String(
                interpreter.fixed_time.clone().unwrap_or_else(|| "2026-04-02T12:00:00Z".to_string())
            ))
        }

        // rng methods
        "rng.uuid" => {
            interpreter.rng_counter += 1;
            let seed = interpreter.rng_seed.unwrap_or(0);
            Ok(Value::String(format!("uuid-{}-{}", seed, interpreter.rng_counter)))
        }

        _ => Err(RuntimeError {
            line: 0, column: 0,
            message: format!("Unknown builtin function '{}'", name),
            hint: None,
            source_line: String::new(),
        }),
    }
}
```

Wire `call_builtin` into `eval_fn_call` in interpreter.rs — when callee is `BuiltinFn { name }`, call `builtins::call_builtin(self, name, args)`.

Set up effect instances in the global environment. Add a method `setup_effects()` that registers db, time, rng as `Value::Effect` with their methods as `Value::BuiltinFn`.

- [ ] **Step 2: Tests**

```rust
#[test]
fn eval_list_builtin() {
    assert_eq!(eval("list(1, 2, 3)"), Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
}

#[test]
fn eval_time_now() {
    // Need to set up effects in the interpreter
    let input = "fn get_time() -> String needs time {\n  time.now()\n}\nget_time()";
    // This requires needs/effects wiring in function calls
}

#[test]
fn eval_rng_uuid() {
    // Similar — needs effects wiring
}
```

The implementer should wire effects into function calls: when `parse_fn_decl` specifies `needs db, time`, the function call should inject the effect values into the function's environment.

- [ ] **Step 3: Run tests, commit**

```bash
git add src/interpreter/builtins.rs src/interpreter/interpreter.rs
git commit -m "feat: add builtin functions and effect stubs (db, time, rng)"
```

---

### Task 9: Integration tests with real PACT code

**Files:**
- Modify: `src/interpreter/interpreter.rs` (tests section)

- [ ] **Step 1: Write integration tests**

```rust
#[test]
fn integration_simple_function() {
    let input = r#"fn add(a: Int, b: Int) -> Int {
  a + b
}
add(3, 4)"#;
    assert_eq!(eval(input), Value::Int(7));
}

#[test]
fn integration_function_with_if() {
    let input = r#"fn max(a: Int, b: Int) -> Int {
  if a > b {
    a
  } else {
    b
  }
}
max(3, 7)"#;
    assert_eq!(eval(input), Value::Int(7));
}

#[test]
fn integration_struct_creation_and_access() {
    let input = r#"let user: User = User { name: "Vitalii", age: 30, active: true }
user.active"#;
    assert_eq!(eval(input), Value::Bool(true));
}

#[test]
fn integration_match_expression() {
    let input = r#"fn describe(x: Int) -> String {
  match x {
    0 => "zero",
    1 => "one",
    _ => "many",
  }
}
describe(1)"#;
    assert_eq!(eval(input), Value::String("one".to_string()));
}

#[test]
fn integration_ensure_passes() {
    let input = r#"fn safe_div(a: Int, b: Int) -> Int {
  ensure b != 0
  a / b
}
safe_div(10, 2)"#;
    assert_eq!(eval(input), Value::Int(5));
}

#[test]
fn integration_pipeline_with_list() {
    let input = "list(10, 20, 30) | sum";
    assert_eq!(eval(input), Value::Int(60));
}

#[test]
fn integration_string_interpolation() {
    let input = r#"let name: String = "PACT"
let version: Int = 1
"Welcome to {name} v{version}""#;
    assert_eq!(eval(input), Value::String("Welcome to PACT v1".to_string()));
}

#[test]
fn integration_var_mutation() {
    let input = r#"var counter: Int = 0
counter = counter + 1
counter = counter + 1
counter"#;
    // Note: assignment syntax `counter = ...` may need parser support
    // If not in parser, skip this test
}
```

- [ ] **Step 2: Debug and fix any failures**

These integration tests will likely reveal edge cases. Fix them.

- [ ] **Step 3: Commit**

```bash
git commit -m "test: add integration tests for interpreter with real PACT code"
```

---

### Task 10: CLI `pact run` command + push

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update main.rs**

Add a `run` subcommand alongside existing behavior:

```rust
use pact::interpreter::Interpreter;

// In main():
if args.len() >= 3 && args[1] == "run" {
    let filename = &args[2];
    // Read, lex, parse, interpret
    let source = fs::read_to_string(filename)...;
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize()...;
    let mut parser = Parser::new(tokens, &source);
    let program = parser.parse()...;
    let mut interp = Interpreter::new(&source);
    match interp.interpret(&program) {
        Ok(value) => println!("{}", value),
        Err(err) => { eprintln!("{}", err); process::exit(1); }
    }
}
```

Usage:
- `pact <file.pact>` — tokens (existing)
- `pact <file.pact> --ast` — AST (existing)
- `pact run <file.pact>` — execute

- [ ] **Step 2: Test with sample file**

```bash
echo 'fn add(a: Int, b: Int) -> Int {
  a + b
}
add(21, 21)' > /tmp/test_run.pact

cargo run -- run /tmp/test_run.pact
```

Expected output: `42`

- [ ] **Step 3: Commit and push**

```bash
git add src/main.rs
git commit -m "feat: add pact run command for executing .pact files"
git push
```

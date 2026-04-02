use std::collections::HashMap;

use crate::parser::ast::{BinaryOp, Expr, MatchArm, Pattern, Program, Statement, StringExpr, StringPart, StructField, UnaryOp};
use super::environment::Environment;
use super::errors::RuntimeError;
use super::value::Value;

/// Result of evaluating a statement: either a normal value or an early return.
pub enum StmtResult {
    Value(Value),
    Return(Value),
}

pub struct Interpreter {
    pub global: Environment,
    source: String,
    db_storage: HashMap<String, Vec<Value>>,
    fixed_time: Option<String>,
    rng_seed: Option<u64>,
    rng_counter: u64,
    /// Storage for return value when propagating returns through expressions
    pending_return: Option<Value>,
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
            pending_return: None,
        }
    }

    pub fn interpret(&mut self, program: &Program) -> Result<Value, RuntimeError> {
        let mut result = Value::Nothing;
        // Clone statements to avoid borrow conflicts (self is &mut for eval)
        let stmts = program.statements.clone();
        for stmt in &stmts {
            match self.eval_statement(stmt, &mut self.global.clone())? {
                StmtResult::Value(val) => result = val,
                StmtResult::Return(val) => return Ok(val),
            }
        }
        Ok(result)
    }

    pub fn eval_statement(
        &mut self,
        stmt: &Statement,
        env: &mut Environment,
    ) -> Result<StmtResult, RuntimeError> {
        match stmt {
            Statement::Let {
                name,
                mutable,
                value,
                ..
            } => {
                let val = self.eval_expr(value, env)?;
                env.bind(name.clone(), val.clone(), *mutable);
                // Also bind in global if this is a top-level let
                self.global.bind(name.clone(), val.clone(), *mutable);
                Ok(StmtResult::Value(val))
            }
            Statement::FnDecl {
                name,
                params,
                body,
                ..
            } => {
                let func = Value::Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                env.bind(name.clone(), func.clone(), false);
                self.global.bind(name.clone(), func.clone(), false);
                Ok(StmtResult::Value(func))
            }
            Statement::TypeDecl(_) => Ok(StmtResult::Value(Value::Nothing)),
            Statement::Use { .. } => Ok(StmtResult::Value(Value::Nothing)),
            Statement::Return { value, condition } => {
                // If there's a condition, evaluate it — skip return if falsy
                if let Some(cond_expr) = condition {
                    let cond_val = self.eval_expr(cond_expr, env)?;
                    if !cond_val.is_truthy() {
                        return Ok(StmtResult::Value(Value::Nothing));
                    }
                }
                // Evaluate the return value (or Nothing if absent)
                let ret_val = if let Some(val_expr) = value {
                    match val_expr {
                        // `return NotFound` — uppercase identifier → Error variant
                        Expr::Identifier(name) if name.starts_with(char::is_uppercase) => {
                            Value::Error {
                                variant: name.clone(),
                                fields: None,
                            }
                        }
                        // `return BadRequest { message: "..." }` — struct literal with uppercase name → Error
                        Expr::StructLiteral { name: Some(sname), fields: sfields }
                            if sname.starts_with(char::is_uppercase) =>
                        {
                            let mut field_map = HashMap::new();
                            for f in sfields {
                                if let StructField::Named { name: fname, value: fval } = f {
                                    let v = self.eval_expr(fval, env)?;
                                    field_map.insert(fname.clone(), v);
                                }
                            }
                            Value::Error {
                                variant: sname.clone(),
                                fields: if field_map.is_empty() { None } else { Some(field_map) },
                            }
                        }
                        _ => self.eval_expr(val_expr, env)?,
                    }
                } else {
                    Value::Nothing
                };
                Ok(StmtResult::Return(ret_val))
            }
            Statement::Expression(expr) => {
                let val = self.eval_expr(expr, env)?;
                Ok(StmtResult::Value(val))
            }
        }
    }

    pub fn eval_expr(
        &mut self,
        expr: &Expr,
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        match expr {
            Expr::IntLiteral(n) => Ok(Value::Int(*n)),
            Expr::FloatLiteral(n) => Ok(Value::Float(*n)),
            Expr::BoolLiteral(b) => Ok(Value::Bool(*b)),
            Expr::Nothing => Ok(Value::Nothing),
            Expr::Identifier(name) => {
                if let Some(val) = env.lookup(name) {
                    Ok(val.clone())
                } else if let Some(val) = self.global.lookup(name) {
                    Ok(val.clone())
                } else {
                    Err(self.error(&format!("Undefined variable '{}'", name)))
                }
            }
            Expr::StringLiteral(string_expr) => match string_expr {
                StringExpr::Simple(s) => Ok(Value::String(s.clone())),
                StringExpr::Interpolated(parts) => {
                    let mut result = String::new();
                    for part in parts {
                        match part {
                            StringPart::Literal(s) => result.push_str(s),
                            StringPart::Expr(expr) => {
                                let val = self.eval_expr(expr, env)?;
                                result.push_str(&format!("{}", val));
                            }
                        }
                    }
                    Ok(Value::String(result))
                }
            },
            Expr::FieldAccess { object, field } => {
                let obj = self.eval_expr(object, env)?;
                match &obj {
                    Value::Struct { fields, .. } => {
                        fields.get(field).cloned().ok_or_else(|| {
                            self.error(&format!(
                                "Struct has no field '{}'",
                                field
                            ))
                        })
                    }
                    Value::Effect { methods, .. } => {
                        methods.get(field).cloned().ok_or_else(|| {
                            self.error(&format!(
                                "Effect has no method '{}'",
                                field
                            ))
                        })
                    }
                    _ => Err(self.error(&format!(
                        "Cannot access field on {} type",
                        obj.type_name()
                    ))),
                }
            }
            Expr::DotShorthand(parts) => {
                let mut val = env
                    .lookup("_it")
                    .or_else(|| self.global.lookup("_it"))
                    .cloned()
                    .ok_or_else(|| {
                        let mut err = self.error("Variable '_it' not found");
                        err.hint = Some(
                            "Dot shorthand (.field) can only be used inside pipeline steps"
                                .to_string(),
                        );
                        err
                    })?;
                for field in parts {
                    val = match &val {
                        Value::Struct { fields, .. } => {
                            fields.get(field).cloned().ok_or_else(|| {
                                self.error(&format!(
                                    "Struct has no field '{}'",
                                    field
                                ))
                            })?
                        }
                        _ => {
                            return Err(self.error(&format!(
                                "Cannot access field '{}' on {} type",
                                field,
                                val.type_name()
                            )));
                        }
                    };
                }
                Ok(val)
            }
            Expr::BinaryOp { left, op, right } => {
                let left_val = self.eval_expr(left, env)?;
                let right_val = self.eval_expr(right, env)?;
                match op {
                    BinaryOp::Add => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
                        (Value::String(a), Value::String(b)) => {
                            Ok(Value::String(format!("{}{}", a, b)))
                        }
                        _ => Err(self.error(&format!(
                            "Cannot add {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::Sub => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot subtract {} from {}",
                            right_val.type_name(),
                            left_val.type_name()
                        ))),
                    },
                    BinaryOp::Mul => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot multiply {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::Div => match (&left_val, &right_val) {
                        (Value::Int(_), Value::Int(0)) => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                        (Value::Float(_), Value::Float(b)) if *b == 0.0 => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                        (Value::Int(_), Value::Float(b)) if *b == 0.0 => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
                        (Value::Float(_), Value::Int(0)) => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot divide {} by {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::Eq => Ok(Value::Bool(left_val == right_val)),
                    BinaryOp::NotEq => Ok(Value::Bool(left_val != right_val)),
                    BinaryOp::Lt => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot compare {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::Gt => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool(*a as f64 > *b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot compare {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::LtEq => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool(*a as f64 <= *b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot compare {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::GtEq => match (&left_val, &right_val) {
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool(*a as f64 >= *b)),
                        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= *b as f64)),
                        _ => Err(self.error(&format!(
                            "Cannot compare {} and {}",
                            left_val.type_name(),
                            right_val.type_name()
                        ))),
                    },
                    BinaryOp::And => {
                        Ok(Value::Bool(left_val.is_truthy() && right_val.is_truthy()))
                    }
                    BinaryOp::Or => {
                        Ok(Value::Bool(left_val.is_truthy() || right_val.is_truthy()))
                    }
                }
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.eval_expr(operand, env)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(n) => Ok(Value::Float(-n)),
                        _ => Err(self.error(&format!(
                            "Cannot negate {} value",
                            val.type_name()
                        ))),
                    },
                    UnaryOp::Not => match val {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        _ => Err(self.error(&format!(
                            "Cannot apply 'not' to {} value",
                            val.type_name()
                        ))),
                    },
                }
            }
            Expr::ErrorPropagation(inner) => {
                self.eval_error_propagation(inner, env)
            }
            Expr::FnCall { callee, args } => {
                self.eval_fn_call(callee, args, env)
            }
            Expr::Pipeline { .. } => {
                Err(self.error("Pipelines are not yet implemented"))
            }
            Expr::If { condition, then_body, else_body } => {
                self.eval_if(condition, then_body, else_body, env)
            }
            Expr::Match { subject, arms } => {
                self.eval_match(subject, arms, env)
            }
            Expr::Block(stmts) => {
                self.eval_block(stmts, env)
            }
            Expr::StructLiteral { name, fields } => {
                self.eval_struct_literal(name, fields, env)
            }
            Expr::Ensure(predicate) => {
                self.eval_ensure(predicate, env)
            }
            Expr::Is { expr, type_name } => {
                let val = self.eval_expr(expr, env)?;
                let result = match &val {
                    Value::Variant { variant, .. } if variant == type_name => true,
                    Value::Error { variant, .. } if variant == type_name => true,
                    _ => val.type_name() == type_name,
                };
                Ok(Value::Bool(result))
            }
        }
    }

    // --- Function calls ---

    fn eval_fn_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let callee_val = self.eval_expr(callee, env)?;
        let mut arg_vals = Vec::new();
        for arg in args {
            arg_vals.push(self.eval_expr(arg, env)?);
        }

        match callee_val {
            Value::Function { params, body, .. } => {
                if params.len() != arg_vals.len() {
                    return Err(self.error(&format!(
                        "Expected {} arguments but got {}",
                        params.len(),
                        arg_vals.len()
                    )));
                }
                // Create new env with parent = global (not caller env)
                let mut fn_env = Environment::with_parent(self.global.clone());
                for (param, val) in params.iter().zip(arg_vals) {
                    fn_env.bind(param.name.clone(), val, false);
                }
                // Evaluate body statements
                let stmts = body.clone();
                let mut result = Value::Nothing;
                for stmt in &stmts {
                    match self.eval_statement(stmt, &mut fn_env) {
                        Ok(StmtResult::Value(val)) => result = val,
                        Ok(StmtResult::Return(val)) => return Ok(val),
                        Err(ref e) if Self::is_return_error(e) => {
                            // Return propagated through an expression (e.g., if block)
                            return Ok(self.take_return_value());
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(result)
            }
            Value::BuiltinFn { name } => {
                self.call_builtin(&name, arg_vals)
            }
            other => Err(self.error(&format!(
                "Cannot call {} as function",
                other.type_name()
            ))),
        }
    }

    fn call_builtin(&mut self, name: &str, _args: Vec<Value>) -> Result<Value, RuntimeError> {
        Err(self.error(&format!("Builtin function '{}' is not yet implemented", name)))
    }

    // --- If/else ---

    fn eval_if(
        &mut self,
        condition: &Expr,
        then_body: &[Statement],
        else_body: &Option<Vec<Statement>>,
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let cond_val = self.eval_expr(condition, env)?;
        if cond_val.is_truthy() {
            let mut child_env = Environment::with_parent(env.clone());
            self.eval_body(then_body, &mut child_env)
        } else if let Some(else_stmts) = else_body {
            let mut child_env = Environment::with_parent(env.clone());
            self.eval_body(else_stmts, &mut child_env)
        } else {
            Ok(Value::Nothing)
        }
    }

    /// Evaluate a list of statements, returning the last value.
    /// Propagates StmtResult::Return as a special RuntimeError so it
    /// can escape through expressions back to eval_fn_call.
    fn eval_body(
        &mut self,
        stmts: &[Statement],
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let stmts = stmts.to_vec();
        let mut result = Value::Nothing;
        for stmt in &stmts {
            match self.eval_statement(stmt, env)? {
                StmtResult::Value(val) => result = val,
                StmtResult::Return(val) => return Err(self.make_return_error(val)),
            }
        }
        Ok(result)
    }

    // --- Match ---

    fn eval_match(
        &mut self,
        subject: &Expr,
        arms: &[MatchArm],
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let subject_val = self.eval_expr(subject, env)?;

        for arm in arms {
            if self.pattern_matches(&arm.pattern, &subject_val, env)? {
                return self.eval_expr(&arm.body, env);
            }
        }

        Err(self.error("No matching pattern"))
    }

    fn pattern_matches(
        &mut self,
        pattern: &Pattern,
        subject: &Value,
        env: &mut Environment,
    ) -> Result<bool, RuntimeError> {
        match pattern {
            Pattern::Wildcard => Ok(true),
            Pattern::Literal(expr) => {
                let pat_val = self.eval_expr(expr, env)?;
                Ok(pat_val == *subject)
            }
            Pattern::Identifier(name) => {
                match subject {
                    Value::Variant { variant, .. } if variant == name => Ok(true),
                    Value::Error { variant, .. } if variant == name => Ok(true),
                    _ => {
                        // Fallback: treat as catch-all match (like wildcard)
                        Ok(true)
                    }
                }
            }
        }
    }

    // --- Struct literals ---

    fn eval_struct_literal(
        &mut self,
        name: &Option<String>,
        fields: &[StructField],
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let mut field_values = HashMap::new();
        for field in fields {
            match field {
                StructField::Named { name, value } => {
                    let val = self.eval_expr(value, env)?;
                    field_values.insert(name.clone(), val);
                }
                StructField::Spread(expr) => {
                    let val = self.eval_expr(expr, env)?;
                    if let Value::Struct { fields: src_fields, .. } = val {
                        for (k, v) in src_fields {
                            // Only insert if not already set by an explicit field
                            // (spread fields come first, explicit fields override)
                            field_values.entry(k).or_insert(v);
                        }
                    } else {
                        return Err(self.error(&format!(
                            "Cannot spread {} into struct literal",
                            val.type_name()
                        )));
                    }
                }
            }
        }
        let type_name = name.clone().unwrap_or_else(|| "anonymous".to_string());
        Ok(Value::Struct {
            type_name,
            fields: field_values,
        })
    }

    // --- Ensure ---

    fn eval_ensure(
        &mut self,
        predicate: &Expr,
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let val = self.eval_expr(predicate, env)?;
        if val.is_truthy() {
            Ok(Value::Nothing)
        } else {
            Err(self.error("Ensure failed: condition evaluated to false"))
        }
    }

    // --- Error propagation ---

    fn eval_error_propagation(
        &mut self,
        inner: &Expr,
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let val = self.eval_expr(inner, env)?;
        match val {
            Value::Ok(v) => Ok(*v),
            err @ Value::Error { .. } => {
                // Propagate the error as a return from the current function
                Err(self.make_return_error(err))
            }
            other => Err(self.error(&format!(
                "Cannot use '?' on {} value (expected Ok or Error)",
                other.type_name()
            ))),
        }
    }

    // --- Block evaluation ---

    fn eval_block(
        &mut self,
        stmts: &[Statement],
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        let mut child_env = Environment::with_parent(env.clone());
        self.eval_body(stmts, &mut child_env)
    }

    // --- Return error mechanism ---
    // We use a special RuntimeError to propagate returns through expressions.
    // eval_fn_call catches these and extracts the return value.

    fn make_return_error(&mut self, value: Value) -> RuntimeError {
        self.pending_return = Some(value);
        RuntimeError {
            line: 0,
            column: 0,
            message: "__RETURN__".to_string(),
            hint: None,
            source_line: String::new(),
        }
    }

    fn is_return_error(err: &RuntimeError) -> bool {
        err.message == "__RETURN__"
    }

    fn take_return_value(&mut self) -> Value {
        self.pending_return.take().unwrap_or(Value::Nothing)
    }

    /// Create a RuntimeError with the given message.
    /// Uses line 1, column 1, and the first source line as defaults
    /// since the AST doesn't carry position information.
    fn error(&self, message: &str) -> RuntimeError {
        let source_line = self
            .source
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        RuntimeError {
            line: 1,
            column: 1,
            message: message.to_string(),
            hint: None,
            source_line,
        }
    }
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
    fn eval_undefined_variable() {
        let err = eval_fails("x");
        assert!(err.message.contains("Undefined"));
    }

    // Task 3: Binary/unary ops, comparison, is

    #[test]
    fn eval_addition() {
        assert_eq!(eval("1 + 2"), Value::Int(3));
    }

    #[test]
    fn eval_subtraction() {
        assert_eq!(eval("10 - 3"), Value::Int(7));
    }

    #[test]
    fn eval_multiplication() {
        assert_eq!(eval("3 * 4"), Value::Int(12));
    }

    #[test]
    fn eval_division() {
        assert_eq!(eval("10 / 3"), Value::Int(3));
    }

    #[test]
    fn eval_float_arithmetic() {
        assert_eq!(eval("1.5 + 2.5"), Value::Float(4.0));
    }

    #[test]
    fn eval_mixed_int_float() {
        assert_eq!(eval("1 + 2.5"), Value::Float(3.5));
    }

    #[test]
    fn eval_string_concat() {
        assert_eq!(
            eval(r#""hello" + " world""#),
            Value::String("hello world".to_string())
        );
    }

    #[test]
    fn eval_comparison_eq() {
        assert_eq!(eval("1 == 1"), Value::Bool(true));
    }

    #[test]
    fn eval_comparison_neq() {
        assert_eq!(eval("1 != 2"), Value::Bool(true));
    }

    #[test]
    fn eval_comparison_lt() {
        assert_eq!(eval("1 < 2"), Value::Bool(true));
    }

    #[test]
    fn eval_comparison_gt() {
        assert_eq!(eval("2 > 1"), Value::Bool(true));
    }

    #[test]
    fn eval_and_or() {
        assert_eq!(eval("true and false or true"), Value::Bool(true));
    }

    #[test]
    fn eval_not() {
        assert_eq!(eval("not true"), Value::Bool(false));
    }

    #[test]
    fn eval_negation() {
        assert_eq!(eval("-5"), Value::Int(-5));
    }

    #[test]
    fn eval_complex_arithmetic() {
        assert_eq!(eval("(1 + 2) * 3 - 4"), Value::Int(5));
    }

    // Task 4: Field access, dot shorthand, string evaluation

    #[test]
    fn eval_simple_string() {
        assert_eq!(eval(r#""hello""#), Value::String("hello".to_string()));
    }

    #[test]
    fn eval_empty_string() {
        assert_eq!(eval(r#""""#), Value::String(String::new()));
    }

    #[test]
    fn eval_string_interpolation() {
        assert_eq!(
            eval("let name: String = \"world\"\n\"hello {name}\""),
            Value::String("hello world".to_string()),
        );
    }

    #[test]
    fn eval_raw_string() {
        assert_eq!(
            eval(r#"raw"no {interp}""#),
            Value::String("no {interp}".to_string())
        );
    }

    #[test]
    fn eval_string_interpolation_with_expr() {
        assert_eq!(
            eval("let x: Int = 5\n\"x is {x + 1}\""),
            Value::String("x is 6".to_string()),
        );
    }

    // Task 5: Function calls, if/else, match, blocks, return

    #[test]
    fn eval_simple_function() {
        assert_eq!(eval("fn add(a: Int, b: Int) -> Int {\n  a + b\n}\nadd(1, 2)"), Value::Int(3));
    }

    #[test]
    fn eval_function_multiple_stmts() {
        assert_eq!(eval("fn double(x: Int) -> Int {\n  let r: Int = x * 2\n  r\n}\ndouble(5)"), Value::Int(10));
    }

    #[test]
    fn eval_nested_calls() {
        assert_eq!(eval("fn add(a: Int, b: Int) -> Int {\n  a + b\n}\nfn double(x: Int) -> Int {\n  add(x, x)\n}\ndouble(3)"), Value::Int(6));
    }

    #[test]
    fn eval_recursive_function() {
        let input = "fn fact(n: Int) -> Int {\n  if n <= 1 {\n    1\n  } else {\n    n * fact(n - 1)\n  }\n}\nfact(5)";
        assert_eq!(eval(input), Value::Int(120));
    }

    #[test]
    fn eval_if_true() {
        assert_eq!(eval("if true {\n  1\n} else {\n  2\n}"), Value::Int(1));
    }

    #[test]
    fn eval_if_false() {
        assert_eq!(eval("if false {\n  1\n} else {\n  2\n}"), Value::Int(2));
    }

    #[test]
    fn eval_if_no_else() {
        assert_eq!(eval("if false {\n  1\n}"), Value::Nothing);
    }

    #[test]
    fn eval_match_wildcard() {
        assert_eq!(eval("match 42 {\n  _ => true,\n}"), Value::Bool(true));
    }

    #[test]
    fn eval_match_literal() {
        assert_eq!(eval("match 1 {\n  1 => \"one\",\n  _ => \"other\",\n}"), Value::String("one".to_string()));
    }

    #[test]
    fn eval_early_return() {
        let input = "fn verify(x: Int) -> Int {\n  if x > 0 {\n    return x\n  }\n  0\n}\nverify(5)";
        assert_eq!(eval(input), Value::Int(5));
    }

    // Task 6: Struct literals, ensure, error propagation

    #[test]
    fn eval_struct_literal() {
        assert_eq!(
            eval("let u: User = User { name: \"V\", age: 30 }\nu.name"),
            Value::String("V".to_string()),
        );
    }

    #[test]
    fn eval_struct_spread() {
        assert_eq!(
            eval("let old: X = X { a: 1, b: 2 }\nlet new: X = X { ...old, a: 10 }\nnew.b"),
            Value::Int(2),
        );
    }

    #[test]
    fn eval_struct_spread_override() {
        assert_eq!(
            eval("let old: X = X { a: 1, b: 2 }\nlet new: X = X { ...old, a: 10 }\nnew.a"),
            Value::Int(10),
        );
    }

    #[test]
    fn eval_anonymous_struct() {
        assert_eq!(
            eval("let info: Info = { status: \"ok\" }\ninfo.status"),
            Value::String("ok".to_string()),
        );
    }

    #[test]
    fn eval_ensure_pass() {
        assert_eq!(eval("ensure 1 > 0\n42"), Value::Int(42));
    }

    #[test]
    fn eval_ensure_fail() {
        let err = eval_fails("ensure 1 < 0");
        assert!(err.message.contains("Ensure") || err.message.contains("ensure"));
    }
}

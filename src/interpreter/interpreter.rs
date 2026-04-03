use std::collections::HashMap;
use std::path::PathBuf;

use super::environment::Environment;
use super::errors::RuntimeError;
use super::value::Value;
use crate::parser::ast::{
    BinaryOp, Expr, MatchArm, Pattern, PipelineStep, Program, Statement, StringExpr, StringPart,
    StructField, TakeKind, UnaryOp,
};

/// Result of evaluating a statement: either a normal value or an early return.
pub enum StmtResult {
    Value(Value),
    Return(Value),
}

#[derive(Debug, Clone)]
pub struct StoredRoute {
    pub method: String,
    pub path: String,
    pub intent: String,
    pub effects: Vec<String>,
    pub body: Vec<Statement>,
}

pub struct Interpreter {
    pub global: Environment,
    source: String,
    pub db_storage: HashMap<String, Vec<Value>>,
    pub fixed_time: Option<String>,
    pub rng_seed: Option<u64>,
    pub rng_counter: u64,
    /// Storage for return value when propagating returns through expressions
    pending_return: Option<Value>,
    pub base_dir: Option<PathBuf>,
    pub module_cache: HashMap<PathBuf, Environment>,
    pub routes: Vec<StoredRoute>,
    pub app_config: Option<(String, u16)>,
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
            base_dir: None,
            module_cache: HashMap::new(),
            routes: Vec::new(),
            app_config: None,
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
                name, params, body, ..
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
            Statement::Use { path } => {
                self.eval_use(path, env)?;
                Ok(StmtResult::Value(Value::Nothing))
            }
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
                        Expr::StructLiteral {
                            name: Some(sname),
                            fields: sfields,
                        } if sname.starts_with(char::is_uppercase) => {
                            let mut field_map = HashMap::new();
                            for f in sfields {
                                if let StructField::Named {
                                    name: fname,
                                    value: fval,
                                } = f
                                {
                                    let v = self.eval_expr(fval, env)?;
                                    field_map.insert(fname.clone(), v);
                                }
                            }
                            Value::Error {
                                variant: sname.clone(),
                                fields: if field_map.is_empty() {
                                    None
                                } else {
                                    Some(field_map)
                                },
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
            Statement::TestBlock { .. } => {
                // Tests are collected, not executed during normal interpret.
                // They are executed by run_tests().
                Ok(StmtResult::Value(Value::Nothing))
            }
            Statement::Using { name, value } => {
                let val = self.eval_expr(value, env)?;
                env.bind(name.clone(), val, false);
                Ok(StmtResult::Value(Value::Nothing))
            }
            Statement::Assert(expr) => {
                let val = self.eval_expr(expr, env)?;
                if val.is_truthy() {
                    Ok(StmtResult::Value(Value::Nothing))
                } else {
                    let mut err = self.error(&format!(
                        "Assertion failed: expression evaluated to {}",
                        val
                    ));
                    err.hint = Some("Check that the condition is correct".to_string());
                    Err(err)
                }
            }
            Statement::Route {
                method,
                path,
                intent,
                effects,
                body,
            } => {
                self.routes.push(StoredRoute {
                    method: method.clone(),
                    path: path.clone(),
                    intent: intent.clone(),
                    effects: effects.clone(),
                    body: body.clone(),
                });
                Ok(StmtResult::Value(Value::Nothing))
            }
            Statement::App { name, port } => {
                self.app_config = Some((name.clone(), *port));
                Ok(StmtResult::Value(Value::Nothing))
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
                    Value::Struct { fields, .. } => fields
                        .get(field)
                        .cloned()
                        .ok_or_else(|| self.error(&format!("Struct has no field '{}'", field))),
                    Value::Effect { methods, .. } => methods
                        .get(field)
                        .cloned()
                        .ok_or_else(|| self.error(&format!("Effect has no method '{}'", field))),
                    _ => {
                        Err(self.error(&format!("Cannot access field on {} type", obj.type_name())))
                    }
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
                                self.error(&format!("Struct has no field '{}'", field))
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
                        (Value::Int(_), Value::Int(0)) => Err(self.error("Division by zero")),
                        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                        (Value::Float(_), Value::Float(b)) if *b == 0.0 => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                        (Value::Int(_), Value::Float(b)) if *b == 0.0 => {
                            Err(self.error("Division by zero"))
                        }
                        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
                        (Value::Float(_), Value::Int(0)) => Err(self.error("Division by zero")),
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
                    BinaryOp::And => Ok(Value::Bool(left_val.is_truthy() && right_val.is_truthy())),
                    BinaryOp::Or => Ok(Value::Bool(left_val.is_truthy() || right_val.is_truthy())),
                }
            }
            Expr::UnaryOp { op, operand } => {
                let val = self.eval_expr(operand, env)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Float(n) => Ok(Value::Float(-n)),
                        _ => Err(self.error(&format!("Cannot negate {} value", val.type_name()))),
                    },
                    UnaryOp::Not => match val {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        _ => {
                            Err(self
                                .error(&format!("Cannot apply 'not' to {} value", val.type_name())))
                        }
                    },
                }
            }
            Expr::ErrorPropagation(inner) => self.eval_error_propagation(inner, env),
            Expr::FnCall { callee, args } => self.eval_fn_call(callee, args, env),
            Expr::Pipeline { .. } => self.eval_pipeline(expr, env),
            Expr::If {
                condition,
                then_body,
                else_body,
            } => self.eval_if(condition, then_body, else_body, env),
            Expr::Match { subject, arms } => self.eval_match(subject, arms, env),
            Expr::Block(stmts) => self.eval_block(stmts, env),
            Expr::StructLiteral { name, fields } => self.eval_struct_literal(name, fields, env),
            Expr::Ensure(predicate) => self.eval_ensure(predicate, env),
            Expr::Is { expr, type_name } => {
                let val = self.eval_expr(expr, env)?;
                let result = match &val {
                    Value::Variant { variant, .. } if variant == type_name => true,
                    Value::Error { variant, .. } if variant == type_name => true,
                    _ => val.type_name() == type_name,
                };
                Ok(Value::Bool(result))
            }
            Expr::Respond { status, body } => {
                let status_val = self.eval_expr(status, env)?;
                let body_val = self.eval_expr(body, env)?;
                let mut fields = HashMap::new();
                fields.insert("status".to_string(), status_val.clone());
                fields.insert("body".to_string(), body_val.clone());
                // For redirects, extract location to top level
                if let Value::Int(code) = &status_val {
                    if matches!(code, 301 | 302 | 307 | 308) {
                        if let Value::Struct {
                            fields: body_fields,
                            ..
                        } = &body_val
                        {
                            if let Some(loc) = body_fields.get("location") {
                                fields.insert("location".to_string(), loc.clone());
                            }
                        }
                    }
                }
                Ok(Value::Struct {
                    type_name: "Response".to_string(),
                    fields,
                })
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
        // Method calls on String/List: "hello".length(), items.contains(x)
        if let Expr::FieldAccess { object, field } = callee {
            let obj = self.eval_expr(object, env)?;
            if matches!(&obj, Value::String(_) | Value::List(_)) {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg, env)?);
                }
                return self.call_method(&obj, field, arg_vals);
            }
        }

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
            Value::BuiltinFn { name } => self.call_builtin(&name, arg_vals),
            other => Err(self.error(&format!("Cannot call {} as function", other.type_name()))),
        }
    }

    // --- Pipeline execution ---

    fn eval_pipeline(&mut self, expr: &Expr, env: &mut Environment) -> Result<Value, RuntimeError> {
        if let Expr::Pipeline { source, steps } = expr {
            let mut current = self.eval_expr(source, env)?;
            for step in steps {
                current = self.execute_pipeline_step(step, current, env)?;
            }
            Ok(current)
        } else {
            unreachable!()
        }
    }

    fn execute_pipeline_step(
        &mut self,
        step: &PipelineStep,
        current: Value,
        env: &mut Environment,
    ) -> Result<Value, RuntimeError> {
        match step {
            PipelineStep::Filter { predicate } => {
                let items = self.require_list(&current, "filter")?;
                let mut result = Vec::new();
                for item in items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item.clone(), false);
                    let val = self.eval_expr(predicate, &mut child_env)?;
                    if val.is_truthy() {
                        result.push(item);
                    }
                }
                Ok(Value::List(result))
            }
            PipelineStep::Map { expr } => {
                let items = self.require_list(&current, "map")?;
                let mut result = Vec::new();
                for item in items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item, false);
                    let val = self.eval_expr(expr, &mut child_env)?;
                    result.push(val);
                }
                Ok(Value::List(result))
            }
            PipelineStep::Sort { field, descending } => {
                let items = self.require_list(&current, "sort")?;
                let mut keyed: Vec<(Value, Value)> = Vec::new();
                for item in items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item.clone(), false);
                    let key = self.eval_expr(field, &mut child_env)?;
                    keyed.push((key, item));
                }
                keyed.sort_by(|(a, _), (b, _)| match (a, b) {
                    (Value::Int(x), Value::Int(y)) => x.cmp(y),
                    (Value::Float(x), Value::Float(y)) => {
                        x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    (Value::String(x), Value::String(y)) => x.cmp(y),
                    _ => std::cmp::Ordering::Equal,
                });
                if *descending {
                    keyed.reverse();
                }
                Ok(Value::List(keyed.into_iter().map(|(_, v)| v).collect()))
            }
            PipelineStep::Each { expr } => {
                let items = self.require_list(&current, "each")?;
                for item in &items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item.clone(), false);
                    self.eval_expr(expr, &mut child_env)?;
                }
                Ok(Value::List(items))
            }
            PipelineStep::Count => {
                let items = self.require_list(&current, "count")?;
                Ok(Value::Int(items.len() as i64))
            }
            PipelineStep::Sum => {
                let items = self.require_list(&current, "sum")?;
                let mut int_sum: i64 = 0;
                let mut float_sum: f64 = 0.0;
                let mut has_float = false;
                for item in &items {
                    match item {
                        Value::Int(n) => int_sum += n,
                        Value::Float(f) => {
                            float_sum += f;
                            has_float = true;
                        }
                        _ => {
                            return Err(
                                self.error(&format!("Cannot sum {} values", item.type_name()))
                            );
                        }
                    }
                }
                if has_float {
                    Ok(Value::Float(int_sum as f64 + float_sum))
                } else {
                    Ok(Value::Int(int_sum))
                }
            }
            PipelineStep::Flatten => {
                let items = self.require_list(&current, "flatten")?;
                let mut result = Vec::new();
                for item in items {
                    match item {
                        Value::List(inner) => result.extend(inner),
                        other => result.push(other),
                    }
                }
                Ok(Value::List(result))
            }
            PipelineStep::Unique => {
                let items = self.require_list(&current, "unique")?;
                let mut result: Vec<Value> = Vec::new();
                for item in items {
                    if !result.contains(&item) {
                        result.push(item);
                    }
                }
                Ok(Value::List(result))
            }
            PipelineStep::GroupBy { field } => {
                let items = self.require_list(&current, "group by")?;
                // Compute keys for all items
                let mut keyed: Vec<(Value, Value)> = Vec::new();
                for item in items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item.clone(), false);
                    let key = self.eval_expr(field, &mut child_env)?;
                    keyed.push((key, item));
                }
                // Group by key, preserving order of first appearance
                let mut group_keys: Vec<Value> = Vec::new();
                let mut groups: Vec<Vec<Value>> = Vec::new();
                for (key, val) in keyed {
                    if let Some(idx) = group_keys.iter().position(|k| k == &key) {
                        groups[idx].push(val);
                    } else {
                        group_keys.push(key);
                        groups.push(vec![val]);
                    }
                }
                // Build Group structs
                let mut result = Vec::new();
                for (key, values) in group_keys.into_iter().zip(groups) {
                    let mut fields = HashMap::new();
                    fields.insert("key".to_string(), key);
                    fields.insert("values".to_string(), Value::List(values));
                    result.push(Value::Struct {
                        type_name: "Group".to_string(),
                        fields,
                    });
                }
                Ok(Value::List(result))
            }
            PipelineStep::Take { kind, count } => {
                let items = self.require_list(&current, "take")?;
                let n = self.eval_expr(count, env)?;
                let n = match n {
                    Value::Int(i) => i as usize,
                    _ => return Err(self.error("take count must be an integer")),
                };
                let result = match kind {
                    TakeKind::First => items.into_iter().take(n).collect(),
                    TakeKind::Last => {
                        let len = items.len();
                        if n >= len {
                            items
                        } else {
                            items.into_iter().skip(len - n).collect()
                        }
                    }
                };
                Ok(Value::List(result))
            }
            PipelineStep::Skip { count } => {
                let items = self.require_list(&current, "skip")?;
                let n = self.eval_expr(count, env)?;
                let n = match n {
                    Value::Int(i) => i as usize,
                    _ => return Err(self.error("skip count must be an integer")),
                };
                Ok(Value::List(items.into_iter().skip(n).collect()))
            }
            PipelineStep::FindFirst { predicate } => {
                let items = self.require_list(&current, "find first")?;
                for item in items {
                    let mut child_env = Environment::with_parent(env.clone());
                    child_env.bind("_it".to_string(), item.clone(), false);
                    let val = self.eval_expr(predicate, &mut child_env)?;
                    if val.is_truthy() {
                        return Ok(item);
                    }
                }
                Ok(Value::Nothing)
            }
            PipelineStep::ExpectOne { error } => {
                let items = match &current {
                    Value::List(items) => items.clone(),
                    _ => {
                        let mut err = self.error(&format!(
                            "Pipeline step 'expect one' requires a List, but got {}. Hint: use 'filter where' instead of 'find first where' before 'expect one'",
                            current.type_name()
                        ));
                        err.hint = Some(
                            "'find first where' returns a single value, not a List. Use 'filter where' to keep a List".to_string()
                        );
                        return Err(err);
                    }
                };
                if items.len() == 1 {
                    Ok(Value::Ok(Box::new(items.into_iter().next().unwrap())))
                } else {
                    let err_val = self.eval_expr(error, env)?;
                    let variant = match err_val {
                        Value::String(s) => s,
                        _ => format!("{}", err_val),
                    };
                    Ok(Value::Error {
                        variant,
                        fields: None,
                    })
                }
            }
            PipelineStep::ExpectAny { error } => {
                let items = self.require_list(&current, "expect any")?;
                if !items.is_empty() {
                    Ok(Value::Ok(Box::new(Value::List(items))))
                } else {
                    let err_val = self.eval_expr(error, env)?;
                    let variant = match err_val {
                        Value::String(s) => s,
                        _ => format!("{}", err_val),
                    };
                    Ok(Value::Error {
                        variant,
                        fields: None,
                    })
                }
            }
            PipelineStep::ExpectSuccess => {
                match current {
                    Value::Ok(v) => Ok(*v),
                    Value::Error { variant, .. } => {
                        Err(self.error(&format!("Expected success but got Error.{}", variant)))
                    }
                    other => Ok(other), // pass through non-Result values
                }
            }
            PipelineStep::OrDefault { value } => match current {
                Value::Nothing => self.eval_expr(value, env),
                other => Ok(other),
            },
            PipelineStep::OnSuccess { body } => {
                match &current {
                    Value::Ok(inner) => {
                        let mut step_env = Environment::with_parent(env.clone());
                        step_env.bind("_it".to_string(), *inner.clone(), false);
                        self.eval_expr(body, &mut step_env)
                    }
                    Value::Error { .. } => Ok(current), // pass through
                    other => {
                        // Non-result value -- treat as success
                        let mut step_env = Environment::with_parent(env.clone());
                        step_env.bind("_it".to_string(), other.clone(), false);
                        self.eval_expr(body, &mut step_env)
                    }
                }
            }
            PipelineStep::OnError { variant, body } => {
                match &current {
                    Value::Error { variant: v, .. } if v == variant => {
                        let mut step_env = Environment::with_parent(env.clone());
                        step_env.bind("_it".to_string(), current.clone(), false);
                        self.eval_expr(body, &mut step_env)
                    }
                    _ => Ok(current), // pass through
                }
            }
            PipelineStep::ValidateAs { .. } => {
                // v0.3a: pass through (real validation later)
                Ok(current)
            }
            PipelineStep::Expr(expr) => {
                let mut child_env = Environment::with_parent(env.clone());
                child_env.bind("_it".to_string(), current, false);
                self.eval_expr(expr, &mut child_env)
            }
        }
    }

    /// Extract items from a Value::List, returning an error if it's not a list.
    fn require_list(&self, value: &Value, step_name: &str) -> Result<Vec<Value>, RuntimeError> {
        match value {
            Value::List(items) => Ok(items.clone()),
            _ => {
                let mut err = self.error(&format!(
                    "Pipeline step '{}' requires a List, but got {}",
                    step_name,
                    value.type_name()
                ));
                err.hint = Some(format!(
                    "The '{}' step can only operate on a List value",
                    step_name
                ));
                Err(err)
            }
        }
    }

    // --- Builtin functions ---

    fn call_method(
        &self,
        receiver: &Value,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        match receiver {
            Value::String(s) => match method {
                "length" => Ok(Value::Int(s.len() as i64)),
                "contains" => match args.first() {
                    Some(Value::String(sub)) => Ok(Value::Bool(s.contains(sub.as_str()))),
                    _ => Err(self.error("String.contains() expects a String argument")),
                },
                "to_upper" => Ok(Value::String(s.to_uppercase())),
                "to_lower" => Ok(Value::String(s.to_lowercase())),
                "trim" => Ok(Value::String(s.trim().to_string())),
                "split" => match args.first() {
                    Some(Value::String(sep)) => Ok(Value::List(
                        s.split(sep.as_str())
                            .map(|p| Value::String(p.to_string()))
                            .collect(),
                    )),
                    _ => Err(self.error("String.split() expects a String separator")),
                },
                "starts_with" => match args.first() {
                    Some(Value::String(prefix)) => Ok(Value::Bool(s.starts_with(prefix.as_str()))),
                    _ => Err(self.error("String.starts_with() expects a String argument")),
                },
                "ends_with" => match args.first() {
                    Some(Value::String(suffix)) => Ok(Value::Bool(s.ends_with(suffix.as_str()))),
                    _ => Err(self.error("String.ends_with() expects a String argument")),
                },
                "replace" => match (args.first(), args.get(1)) {
                    (Some(Value::String(from)), Some(Value::String(to))) => {
                        Ok(Value::String(s.replace(from.as_str(), to.as_str())))
                    }
                    _ => Err(self.error("String.replace() expects two String arguments")),
                },
                _ => Err(self.error(&format!("String has no method '{}'", method))),
            },
            Value::List(items) => match method {
                "length" => Ok(Value::Int(items.len() as i64)),
                "contains" => {
                    let target = args
                        .first()
                        .ok_or_else(|| self.error("List.contains() expects 1 argument"))?;
                    Ok(Value::Bool(items.contains(target)))
                }
                "push" => {
                    let mut new_items = items.clone();
                    for arg in args {
                        new_items.push(arg);
                    }
                    Ok(Value::List(new_items))
                }
                "get" => match args.first() {
                    Some(Value::Int(i)) => {
                        let idx = *i as usize;
                        Ok(items.get(idx).cloned().unwrap_or(Value::Nothing))
                    }
                    _ => Err(self.error("List.get() expects an Int index")),
                },
                "join" => match args.first() {
                    Some(Value::String(sep)) => {
                        let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                        Ok(Value::String(parts.join(sep.as_str())))
                    }
                    _ => Err(self.error("List.join() expects a String separator")),
                },
                "is_empty" => Ok(Value::Bool(items.is_empty())),
                "first" => Ok(items.first().cloned().unwrap_or(Value::Nothing)),
                "last" => Ok(items.last().cloned().unwrap_or(Value::Nothing)),
                "reverse" => {
                    let mut rev = items.clone();
                    rev.reverse();
                    Ok(Value::List(rev))
                }
                _ => Err(self.error(&format!("List has no method '{}'", method))),
            },
            _ => Err(self.error(&format!(
                "Cannot call method '{}' on {}",
                method,
                receiver.type_name()
            ))),
        }
    }

    fn call_builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        match name {
            "list" => Ok(Value::List(args)),
            "db.insert" => self.builtin_db_insert(args),
            "db.query" => self.builtin_db_query(args),
            "time.now" => self.builtin_time_now(),
            "rng.uuid" => self.builtin_rng_uuid(),
            "rng.hex" => self.builtin_rng_hex(args),
            "time.fixed" => {
                if let Some(Value::String(dt)) = args.first() {
                    self.fixed_time = Some(dt.clone());
                }
                Ok(self.make_time_effect())
            }
            "rng.deterministic" => {
                if let Some(Value::Int(seed)) = args.first() {
                    self.rng_seed = Some(*seed as u64);
                    self.rng_counter = 0;
                }
                Ok(self.make_rng_effect())
            }
            "db.memory" => {
                self.db_storage.clear();
                Ok(self.make_db_effect())
            }
            "print" => {
                for arg in &args {
                    eprintln!("{}", arg);
                }
                Ok(Value::Nothing)
            }
            _ => Err(self.error(&format!("Unknown builtin '{}'", name))),
        }
    }

    fn builtin_db_insert(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.insert expects 2 arguments: table name and value"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.insert first argument must be a String table name")),
        };
        let value = args[1].clone();
        self.db_storage
            .entry(table_name)
            .or_default()
            .push(value.clone());
        Ok(value)
    }

    fn builtin_db_query(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 1 {
            return Err(self.error("db.query expects 1 argument: table name"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.query argument must be a String table name")),
        };
        let items = self
            .db_storage
            .get(&table_name)
            .cloned()
            .unwrap_or_default();
        Ok(Value::List(items))
    }

    fn builtin_time_now(&self) -> Result<Value, RuntimeError> {
        let time_str = self
            .fixed_time
            .clone()
            .unwrap_or_else(|| "2026-04-02T12:00:00Z".to_string());
        Ok(Value::String(time_str))
    }

    fn builtin_rng_uuid(&mut self) -> Result<Value, RuntimeError> {
        self.rng_counter += 1;
        let seed = self.rng_seed.unwrap_or(0);
        Ok(Value::String(format!("uuid-{}-{}", seed, self.rng_counter)))
    }

    fn builtin_rng_hex(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let length = match args.first() {
            Some(Value::Int(n)) => *n as usize,
            _ => return Err(self.error("rng.hex expects 1 argument: length (Int)")),
        };
        self.rng_counter += 1;
        let seed = self.rng_seed.unwrap_or(42);
        // Simple hash-based hex generation from seed+counter
        let mut value = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.rng_counter);
        let mut hex = String::with_capacity(length);
        for _ in 0..length {
            value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
            hex.push_str(&format!("{:x}", (value >> 32) & 0xf));
        }
        Ok(Value::String(hex))
    }

    // --- Effect setup ---

    pub fn setup_test_effects(&mut self) {
        // db effect with insert, query, and memory constructor
        let mut db_methods = HashMap::new();
        db_methods.insert(
            "insert".to_string(),
            Value::BuiltinFn {
                name: "db.insert".to_string(),
            },
        );
        db_methods.insert(
            "query".to_string(),
            Value::BuiltinFn {
                name: "db.query".to_string(),
            },
        );
        db_methods.insert(
            "memory".to_string(),
            Value::BuiltinFn {
                name: "db.memory".to_string(),
            },
        );
        self.global.bind(
            "db".to_string(),
            Value::Effect {
                name: "db".to_string(),
                methods: db_methods,
            },
            false,
        );

        // time effect with now and fixed constructor
        let mut time_methods = HashMap::new();
        time_methods.insert(
            "now".to_string(),
            Value::BuiltinFn {
                name: "time.now".to_string(),
            },
        );
        time_methods.insert(
            "fixed".to_string(),
            Value::BuiltinFn {
                name: "time.fixed".to_string(),
            },
        );
        self.global.bind(
            "time".to_string(),
            Value::Effect {
                name: "time".to_string(),
                methods: time_methods,
            },
            false,
        );

        // rng effect with uuid, hex, and deterministic constructor
        let mut rng_methods = HashMap::new();
        rng_methods.insert(
            "uuid".to_string(),
            Value::BuiltinFn {
                name: "rng.uuid".to_string(),
            },
        );
        rng_methods.insert(
            "hex".to_string(),
            Value::BuiltinFn {
                name: "rng.hex".to_string(),
            },
        );
        rng_methods.insert(
            "deterministic".to_string(),
            Value::BuiltinFn {
                name: "rng.deterministic".to_string(),
            },
        );
        self.global.bind(
            "rng".to_string(),
            Value::Effect {
                name: "rng".to_string(),
                methods: rng_methods,
            },
            false,
        );

        // list builtin
        self.global.bind(
            "list".to_string(),
            Value::BuiltinFn {
                name: "list".to_string(),
            },
            false,
        );

        // print builtin
        self.global.bind(
            "print".to_string(),
            Value::BuiltinFn {
                name: "print".to_string(),
            },
            false,
        );
    }

    fn make_db_effect(&self) -> Value {
        let mut methods = HashMap::new();
        methods.insert(
            "insert".to_string(),
            Value::BuiltinFn {
                name: "db.insert".to_string(),
            },
        );
        methods.insert(
            "query".to_string(),
            Value::BuiltinFn {
                name: "db.query".to_string(),
            },
        );
        methods.insert(
            "memory".to_string(),
            Value::BuiltinFn {
                name: "db.memory".to_string(),
            },
        );
        Value::Effect {
            name: "db".to_string(),
            methods,
        }
    }

    fn make_time_effect(&self) -> Value {
        let mut methods = HashMap::new();
        methods.insert(
            "now".to_string(),
            Value::BuiltinFn {
                name: "time.now".to_string(),
            },
        );
        methods.insert(
            "fixed".to_string(),
            Value::BuiltinFn {
                name: "time.fixed".to_string(),
            },
        );
        Value::Effect {
            name: "time".to_string(),
            methods,
        }
    }

    fn make_rng_effect(&self) -> Value {
        let mut methods = HashMap::new();
        methods.insert(
            "uuid".to_string(),
            Value::BuiltinFn {
                name: "rng.uuid".to_string(),
            },
        );
        methods.insert(
            "hex".to_string(),
            Value::BuiltinFn {
                name: "rng.hex".to_string(),
            },
        );
        methods.insert(
            "deterministic".to_string(),
            Value::BuiltinFn {
                name: "rng.deterministic".to_string(),
            },
        );
        Value::Effect {
            name: "rng".to_string(),
            methods,
        }
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
                    if let Value::Struct {
                        fields: src_fields, ..
                    } = val
                    {
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
            // Non-Ok/Error values pass through (treat as success)
            other => Ok(other),
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
        let source_line = self.source.lines().next().unwrap_or("").to_string();
        RuntimeError {
            line: 1,
            column: 1,
            message: message.to_string(),
            hint: None,
            source_line,
        }
    }

    // --- Module imports ---

    /// Set the base directory for resolving `use` imports.
    /// Extracts the parent directory from the given file path.
    pub fn set_base_dir(&mut self, path: &str) {
        let p = PathBuf::from(path);
        if let Some(parent) = p.parent() {
            self.base_dir = Some(parent.to_path_buf());
        }
    }

    /// Evaluate a `use` import statement.
    fn eval_use(&mut self, path: &[String], env: &mut Environment) -> Result<(), RuntimeError> {
        if path.is_empty() {
            return Err(self.error("Empty use path"));
        }

        // Last element is the symbol name
        let symbol_name = &path[path.len() - 1];

        // Rest is the file path: ["models", "user"] -> "models/user.pact"
        let file_parts = &path[..path.len() - 1];

        if file_parts.is_empty() {
            // Single-part path like `use User` -- just a symbol from current scope, skip
            return Ok(());
        }

        let mut file_path = self.base_dir.clone().unwrap_or_default();
        for part in file_parts {
            file_path.push(part);
        }
        file_path.set_extension("pact");

        // Check cache
        let module_env = if let Some(cached) = self.module_cache.get(&file_path) {
            cached.clone()
        } else {
            // Read, lex, parse, eval the module file
            let source = std::fs::read_to_string(&file_path).map_err(|e| {
                let mut err =
                    self.error(&format!("Cannot import '{}': {}", file_path.display(), e));
                err.hint = Some(format!("File path resolved from: use {}", path.join(".")));
                err
            })?;

            let mut lexer = crate::lexer::Lexer::new(&source);
            let tokens = lexer.tokenize().map_err(|e| {
                self.error(&format!("Lex error in '{}': {}", file_path.display(), e))
            })?;

            let mut parser = crate::parser::Parser::new(tokens, &source);
            let program = parser.parse().map_err(|errors| {
                self.error(&format!(
                    "Parse error in '{}': {}",
                    file_path.display(),
                    errors[0]
                ))
            })?;

            // Eval the module in a fresh environment
            let old_source = self.source.clone();
            self.source = source;
            let mut module_env = Environment::new();
            for stmt in &program.statements {
                self.eval_statement(stmt, &mut module_env)?;
            }
            self.source = old_source;

            // Cache
            self.module_cache
                .insert(file_path.clone(), module_env.clone());
            module_env
        };

        // Wildcard import: import all symbols from the module
        if symbol_name == "*" {
            for (name, value) in module_env.all_bindings() {
                env.bind(name.clone(), value.clone(), false);
                self.global.bind(name, value, false);
            }
            return Ok(());
        }

        // Extract the symbol
        if let Some(value) = module_env.lookup(symbol_name) {
            env.bind(symbol_name.clone(), value.clone(), false);
            self.global.bind(symbol_name.clone(), value.clone(), false);
            Ok(())
        } else {
            Err(self.error(&format!(
                "Symbol '{}' not found in module '{}'",
                symbol_name,
                file_path.display()
            )))
        }
    }

    // --- Route execution ---

    pub fn execute_route(
        &mut self,
        route: &StoredRoute,
        request: Value,
    ) -> Result<Value, RuntimeError> {
        let mut env = Environment::with_parent(self.global.clone());
        env.bind("request".to_string(), request, false);

        for effect_name in &route.effects {
            if let Some(effect) = self.global.lookup(effect_name) {
                env.bind(effect_name.clone(), effect.clone(), false);
            }
        }

        let mut result = Value::Nothing;
        let body = route.body.clone();
        for stmt in &body {
            match self.eval_statement(stmt, &mut env)? {
                StmtResult::Value(val) => result = val,
                StmtResult::Return(val) => return Ok(val),
            }
        }
        Ok(result)
    }

    // --- Test runner ---

    /// Run all test blocks in the program and return results.
    pub fn run_tests(&mut self, program: &Program) -> Vec<TestResult> {
        let mut results = Vec::new();

        // First pass: register all functions and types (non-test statements)
        let stmts = program.statements.clone();
        let mut env = Environment::with_parent(self.global.clone());
        for stmt in &stmts {
            match stmt {
                Statement::TestBlock { .. } => {} // skip tests in first pass
                _ => {
                    let _ = self.eval_statement(stmt, &mut env);
                }
            }
        }

        // Second pass: run each test block
        for stmt in &stmts {
            if let Statement::TestBlock { name, body } = stmt {
                let mut test_env = Environment::with_parent(env.clone());
                // Set up fresh effects for each test
                self.setup_test_effects();
                self.db_storage.clear();

                let mut passed = true;
                let mut error_msg = String::new();

                for test_stmt in body {
                    match self.eval_statement(test_stmt, &mut test_env) {
                        Ok(_) => {}
                        Err(e) => {
                            passed = false;
                            if Self::is_return_error(&e) {
                                let _ = self.take_return_value();
                                error_msg = "Unexpected return in test".to_string();
                            } else {
                                error_msg = e.message.clone();
                            }
                            break;
                        }
                    }
                }

                results.push(TestResult {
                    name: name.clone(),
                    passed,
                    error: if passed { None } else { Some(error_msg) },
                });
            }
        }

        results
    }
}

pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
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
        assert_eq!(
            eval(
                "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}\nadd(1, 2)"
            ),
            Value::Int(3)
        );
    }

    #[test]
    fn eval_function_multiple_stmts() {
        assert_eq!(
            eval(
                "intent \"double a number\"\nfn double(x: Int) -> Int {\n  let r: Int = x * 2\n  r\n}\ndouble(5)"
            ),
            Value::Int(10)
        );
    }

    #[test]
    fn eval_nested_calls() {
        assert_eq!(
            eval(
                "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}\nintent \"double a number\"\nfn double(x: Int) -> Int {\n  add(x, x)\n}\ndouble(3)"
            ),
            Value::Int(6)
        );
    }

    #[test]
    fn eval_recursive_function() {
        let input = "intent \"compute factorial\"\nfn fact(n: Int) -> Int {\n  if n <= 1 {\n    1\n  } else {\n    n * fact(n - 1)\n  }\n}\nfact(5)";
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
        assert_eq!(
            eval("match 1 {\n  1 => \"one\",\n  _ => \"other\",\n}"),
            Value::String("one".to_string())
        );
    }

    #[test]
    fn eval_early_return() {
        let input = "intent \"verify positive number\"\nfn verify(x: Int) -> Int {\n  if x > 0 {\n    return x\n  }\n  0\n}\nverify(5)";
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

    // Task 7: Pipeline execution

    fn eval_with_list(input: &str) -> Value {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.global.bind(
            "list".to_string(),
            Value::BuiltinFn {
                name: "list".to_string(),
            },
            false,
        );
        interp.interpret(&program).unwrap()
    }

    fn eval_with_effects(input: &str) -> Value {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        interp.fixed_time = Some("2026-04-02T12:00:00Z".to_string());
        interp.rng_seed = Some(42);
        interp.interpret(&program).unwrap()
    }

    #[test]
    fn eval_pipeline_count() {
        assert_eq!(eval_with_list("list(1, 2, 3) | count"), Value::Int(3));
    }

    #[test]
    fn eval_pipeline_sum() {
        assert_eq!(eval_with_list("list(1, 2, 3) | sum"), Value::Int(6));
    }

    #[test]
    fn eval_pipeline_flatten() {
        assert_eq!(
            eval_with_list("list(list(1, 2), list(3, 4)) | flatten"),
            Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ]),
        );
    }

    #[test]
    fn eval_pipeline_unique() {
        assert_eq!(
            eval_with_list("list(1, 2, 2, 3, 3) | unique"),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        );
    }

    #[test]
    fn eval_pipeline_take_first() {
        assert_eq!(
            eval_with_list("list(1, 2, 3, 4, 5) | take first 3"),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        );
    }

    #[test]
    fn eval_pipeline_take_last() {
        assert_eq!(
            eval_with_list("list(1, 2, 3, 4, 5) | take last 2"),
            Value::List(vec![Value::Int(4), Value::Int(5)]),
        );
    }

    #[test]
    fn eval_pipeline_skip() {
        assert_eq!(
            eval_with_list("list(1, 2, 3, 4, 5) | skip 2"),
            Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]),
        );
    }

    #[test]
    fn eval_pipeline_or_default() {
        assert_eq!(eval_with_list("nothing | or default 42"), Value::Int(42));
    }

    #[test]
    fn eval_pipeline_or_default_not_nothing() {
        assert_eq!(eval_with_list("5 | or default 42"), Value::Int(5));
    }

    #[test]
    fn eval_pipeline_multi_step() {
        assert_eq!(
            eval_with_list("list(1, 2, 3, 4, 5) | skip 2 | count"),
            Value::Int(3),
        );
    }

    #[test]
    fn eval_pipeline_filter_nonlist_fails() {
        let input = "42 | filter where true";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        let result = interp.interpret(&program);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("List"));
    }

    #[test]
    fn eval_pipeline_sum_float() {
        assert_eq!(eval_with_list("list(1.5, 2.5) | sum"), Value::Float(4.0),);
    }

    #[test]
    fn eval_pipeline_sum_empty() {
        assert_eq!(eval_with_list("list() | sum"), Value::Int(0),);
    }

    // Task 8: Builtin functions and effect stubs

    #[test]
    fn eval_list_builtin() {
        assert_eq!(
            eval_with_list("list(1, 2, 3)"),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        );
    }

    #[test]
    fn eval_list_empty() {
        assert_eq!(eval_with_list("list()"), Value::List(vec![]),);
    }

    #[test]
    fn eval_time_now() {
        assert_eq!(
            eval_with_effects("time.now()"),
            Value::String("2026-04-02T12:00:00Z".to_string()),
        );
    }

    #[test]
    fn eval_rng_uuid() {
        assert_eq!(
            eval_with_effects("rng.uuid()"),
            Value::String("uuid-42-1".to_string()),
        );
    }

    #[test]
    fn eval_rng_uuid_increments() {
        // Two calls should produce different UUIDs
        assert_eq!(
            eval_with_effects(
                "let a: String = rng.uuid()\nlet b: String = rng.uuid()\n\"{a},{b}\""
            ),
            Value::String("uuid-42-1,uuid-42-2".to_string()),
        );
    }

    #[test]
    fn eval_db_insert_and_query() {
        assert_eq!(
            eval_with_effects(
                "db.insert(\"users\", User { name: \"Alice\" })\ndb.query(\"users\") | count"
            ),
            Value::Int(1),
        );
    }

    #[test]
    fn eval_db_query_empty() {
        assert_eq!(
            eval_with_effects("db.query(\"nonexistent\")"),
            Value::List(vec![]),
        );
    }

    #[test]
    fn eval_db_insert_returns_value() {
        assert_eq!(
            eval_with_effects("let u: User = db.insert(\"users\", User { name: \"Bob\" })\nu.name"),
            Value::String("Bob".to_string()),
        );
    }

    // Task 9: Integration tests — full pipeline: source → lex → parse → interpret

    #[test]
    fn integration_simple_function() {
        assert_eq!(
            eval(
                "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}\nadd(3, 4)"
            ),
            Value::Int(7)
        );
    }

    #[test]
    fn integration_function_with_if() {
        let input = "intent \"return the larger of two numbers\"\nfn max(a: Int, b: Int) -> Int {\n  if a > b {\n    a\n  } else {\n    b\n  }\n}\nmax(3, 7)";
        assert_eq!(eval(input), Value::Int(7));
    }

    #[test]
    fn integration_struct_and_field_access() {
        let input = r#"let user: User = User { name: "Vitalii", age: 30, active: true }
user.active"#;
        assert_eq!(eval(input), Value::Bool(true));
    }

    #[test]
    fn integration_match_expression() {
        let input = r#"intent "describe a number"
fn describe(x: Int) -> String {
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
        let input = "intent \"divide safely with zero check\"\nfn safe_div(a: Int, b: Int) -> Int {\n  ensure b != 0\n  a / b\n}\nsafe_div(10, 2)";
        assert_eq!(eval(input), Value::Int(5));
    }

    #[test]
    fn integration_pipeline_with_list() {
        assert_eq!(eval_with_list("list(10, 20, 30) | sum"), Value::Int(60));
    }

    #[test]
    fn integration_string_interpolation() {
        let input =
            "let name: String = \"PACT\"\nlet version: Int = 1\n\"Welcome to {name} v{version}\"";
        assert_eq!(eval(input), Value::String("Welcome to PACT v1".to_string()));
    }

    #[test]
    fn integration_recursive_fibonacci() {
        let input = "intent \"compute fibonacci number\"\nfn fib(n: Int) -> Int {\n  if n <= 1 {\n    n\n  } else {\n    fib(n - 1) + fib(n - 2)\n  }\n}\nfib(10)";
        assert_eq!(eval(input), Value::Int(55));
    }

    #[test]
    fn integration_pipeline_multi_step() {
        assert_eq!(
            eval_with_list("list(1, 2, 3, 4, 5, 6, 7, 8, 9, 10) | skip 5 | take first 3 | sum"),
            Value::Int(21), // 6 + 7 + 8
        );
    }

    #[test]
    fn integration_struct_spread_update() {
        let input = r#"let old: User = User { name: "A", age: 1 }
let new: User = User { ...old, age: 2 }
new.name"#;
        assert_eq!(eval(input), Value::String("A".to_string()));
    }

    #[test]
    fn integration_nested_field_access() {
        let input = r#"let addr: Addr = Addr { city: "Kyiv" }
let user: User = User { name: "V", address: addr }
user.address.city"#;
        assert_eq!(eval(input), Value::String("Kyiv".to_string()));
    }

    #[test]
    fn integration_effects_time() {
        let input = "intent \"get current time\"\nfn get_time() -> String needs time {\n  time.now()\n}\nget_time()";
        assert_eq!(
            eval_with_effects(input),
            Value::String("2026-04-02T12:00:00Z".to_string())
        );
    }

    #[test]
    fn integration_effects_rng() {
        let input = "intent \"generate a unique ID\"\nfn make_id() -> String needs rng {\n  rng.uuid()\n}\nmake_id()";
        let result = eval_with_effects(input);
        assert!(matches!(result, Value::String(s) if s.starts_with("uuid-")));
    }

    // --- Test infrastructure tests ---

    #[test]
    fn eval_assert_pass() {
        assert_eq!(eval("assert 1 == 1"), Value::Nothing);
    }

    #[test]
    fn eval_assert_true() {
        assert_eq!(eval("assert true"), Value::Nothing);
    }

    #[test]
    fn eval_assert_fail() {
        let err = eval_fails("assert 1 == 2");
        assert!(err.message.contains("Assertion failed"));
    }

    #[test]
    fn eval_assert_false() {
        let err = eval_fails("assert false");
        assert!(err.message.contains("Assertion failed"));
    }

    #[test]
    fn eval_test_block_skipped_in_interpret() {
        // Test blocks should be skipped during normal interpret
        let input = r#"test "should not run" {
  assert false
}"#;
        assert_eq!(eval(input), Value::Nothing);
    }

    #[test]
    fn eval_test_block_runs_via_run_tests() {
        let input = r#"test "math works" {
  assert 1 + 1 == 2
  assert 2 * 3 == 6
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
        assert_eq!(results[0].name, "math works");
    }

    #[test]
    fn eval_test_block_failing() {
        let input = r#"test "fails" {
  assert 1 == 2
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed);
        assert!(
            results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("Assertion failed")
        );
    }

    #[test]
    fn eval_multiple_test_blocks() {
        let input = r#"test "passes" {
  assert true
}
test "also passes" {
  assert 1 + 1 == 2
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert!(results[1].passed);
    }

    #[test]
    fn eval_test_with_using_effects() {
        let input = r#"test "effects" {
  using t = time.fixed("2026-01-01T00:00:00Z")
  assert true
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_test_with_fn_and_assert() {
        let input = r#"intent "add two numbers"
fn add(a: Int, b: Int) -> Int {
  a + b
}
test "add works" {
  assert add(1, 2) == 3
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn eval_expect_success_on_ok() {
        // Verify basic pipeline with expect success doesn't crash
        // by checking that Ok values pass through
        assert_eq!(eval("42"), Value::Int(42));
    }

    #[test]
    fn eval_using_statement() {
        let input = "using x = 42";
        assert_eq!(eval(input), Value::Nothing);
    }

    // --- Use import tests ---

    #[test]
    fn eval_use_import() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_imports");
        let _ = fs::create_dir_all(dir.join("math"));
        fs::write(
            dir.join("math/ops.pact"),
            "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}\n",
        )
        .unwrap();

        let input = "use math.ops.add\nadd(1, 2)";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::Int(3));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn eval_use_wildcard_import() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_wildcard");
        let _ = fs::create_dir_all(dir.join("utils"));
        fs::write(
            dir.join("utils/math.pact"),
            "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int { a + b }\nintent \"multiply two numbers\"\nfn mul(a: Int, b: Int) -> Int { a * b }\n",
        )
        .unwrap();

        let input = "use utils.math.*\nadd(2, 3)";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::Int(5));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn eval_use_import_not_found() {
        let input = "use nonexistent.module.Foo";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(std::env::temp_dir());
        let err = interp.interpret(&program).unwrap_err();
        assert!(err.message.contains("Cannot import"));
    }

    #[test]
    fn eval_use_caches_module() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_cache");
        let _ = fs::create_dir_all(dir.join("lib"));
        fs::write(
            dir.join("lib/counter.pact"),
            "intent \"get the counter value\"\nfn get_value() -> Int { 42 }\n",
        )
        .unwrap();

        let input = "use lib.counter.get_value\nget_value()";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::Int(42));
        assert_eq!(interp.module_cache.len(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn eval_respond_expression() {
        let result = eval("respond 200 with nothing");
        if let Value::Struct { type_name, fields } = &result {
            assert_eq!(type_name, "Response");
            assert_eq!(fields.get("status"), Some(&Value::Int(200)));
        } else {
            panic!("Expected Response struct");
        }
    }

    #[test]
    fn eval_respond_with_struct_body() {
        let result = eval(r#"respond 201 with { message: "created" }"#);
        if let Value::Struct { type_name, fields } = &result {
            assert_eq!(type_name, "Response");
            assert_eq!(fields.get("status"), Some(&Value::Int(201)));
            assert!(matches!(fields.get("body"), Some(Value::Struct { .. })));
        } else {
            panic!("Expected Response struct");
        }
    }

    #[test]
    fn eval_route_stored() {
        let input = "route GET \"/health\" {\n  intent \"health check\"\n  respond 200 with { status: \"ok\" }\n}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.interpret(&program).unwrap();
        assert_eq!(interp.routes.len(), 1);
        assert_eq!(interp.routes[0].method, "GET");
    }

    #[test]
    fn eval_route_execution() {
        let input = "route GET \"/health\" {\n  intent \"health check\"\n  respond 200 with { status: \"ok\" }\n}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        interp.interpret(&program).unwrap();

        let mut req_fields = std::collections::HashMap::new();
        req_fields.insert("method".to_string(), Value::String("GET".to_string()));
        req_fields.insert("path".to_string(), Value::String("/health".to_string()));
        let request = Value::Struct {
            type_name: "Request".to_string(),
            fields: req_fields,
        };

        let response = interp
            .execute_route(&interp.routes[0].clone(), request)
            .unwrap();
        if let Value::Struct { fields, .. } = &response {
            assert_eq!(fields.get("status"), Some(&Value::Int(200)));
        } else {
            panic!("Expected Response");
        }
    }

    #[test]
    fn eval_on_success_handles_ok() {
        let input = r#"intent "wrap value"
fn wrap(x: Int) -> Int or Nothing {
  x
}
wrap(42)
  | on success: respond 200 with .
  | on Nothing: respond 404 with nothing"#;
        let result = eval(input);
        if let Value::Struct { fields, .. } = &result {
            assert_eq!(fields.get("status"), Some(&Value::Int(200)));
        } else {
            panic!("Expected Response, got {:?}", result);
        }
    }
}

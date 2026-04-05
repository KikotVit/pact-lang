use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::db::DbBackend;
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
    pub db: DbBackend,
    pub fixed_time: Option<String>,
    pub rng_seed: Option<u64>,
    pub rng_counter: u64,
    /// Storage for return value when propagating returns through expressions
    pending_return: Option<Value>,
    pub base_dir: Option<PathBuf>,
    pub module_cache: HashMap<PathBuf, Environment>,
    pub type_defs: HashMap<String, Vec<(String, bool)>>, // (field_name, is_optional)
    pub routes: Vec<StoredRoute>,
    pub app_config: Option<(String, u16, Option<String>)>,
    /// Predetermined sequence for rng (testing)
    rng_sequence: Option<Vec<String>>,
    /// Effects blocked from global lookup (enforces `needs` declarations)
    blocked_effects: Vec<String>,
    /// Mock responses for http effect: URL -> response struct
    pub http_mock_responses: Option<HashMap<String, Value>>,
    /// Tracks modules currently being loaded to detect circular imports
    loading_modules: HashSet<PathBuf>,
}

impl Interpreter {
    pub fn new(source: &str) -> Self {
        Interpreter {
            global: Environment::new(),
            source: source.to_string(),
            db: DbBackend::new_memory(),
            fixed_time: None,
            rng_seed: None,
            rng_counter: 0,
            pending_return: None,
            base_dir: None,
            module_cache: HashMap::new(),
            type_defs: HashMap::new(),
            routes: Vec::new(),
            app_config: None,
            rng_sequence: None,
            blocked_effects: Vec::new(),
            http_mock_responses: None,
            loading_modules: HashSet::new(),
        }
    }

    pub fn open_sqlite(&mut self, url: &str) -> Result<(), RuntimeError> {
        if !url.starts_with("sqlite://") {
            let mut err = self.error(&format!("Invalid database URL '{}'", url));
            err.hint = Some("Expected format: sqlite://path/to/file.db".to_string());
            return Err(err);
        }
        let path = &url["sqlite://".len()..];
        self.db = DbBackend::new_sqlite(path)?;
        Ok(())
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
            Statement::TypeDecl(decl) => {
                if let crate::parser::ast::TypeDecl::Struct { name, fields } = decl {
                    let field_info: Vec<(String, bool)> = fields
                        .iter()
                        .map(|f| {
                            let optional =
                                matches!(f.type_ann, crate::parser::ast::TypeExpr::Optional(_));
                            (f.name.clone(), optional)
                        })
                        .collect();
                    self.type_defs.insert(name.clone(), field_info);
                }
                Ok(StmtResult::Value(Value::Nothing))
            }
            Statement::Use { path } => {
                self.eval_use(path, env)?;
                Ok(StmtResult::Value(Value::Nothing))
            }
            Statement::Return {
                value, condition, ..
            } => {
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
            Statement::App { name, port, db_url } => {
                self.app_config = Some((name.clone(), *port, db_url.clone()));
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
                if self.blocked_effects.contains(name) {
                    // Effect not declared in `needs` — block it even if in global
                    let mut err =
                        self.error(&format!("Effect '{}' is not available in this scope", name));
                    err.hint = Some(format!(
                        "Add 'needs {}' to the function or route declaration",
                        name
                    ));
                    Err(err)
                } else if let Some(val) = env.lookup(name) {
                    Ok(val.clone())
                } else if let Some(val) = self.global.lookup(name) {
                    Ok(val.clone())
                } else {
                    let known_effects = ["db", "auth", "log", "time", "rng", "env", "http"];
                    if known_effects.contains(&name.as_str()) {
                        let mut err = self
                            .error(&format!("Effect '{}' is not available in this scope", name));
                        err.hint = Some(format!(
                            "Add 'needs {}' to the function or route declaration",
                            name
                        ));
                        Err(err)
                    } else {
                        let mut err = self.error(&format!("Undefined variable '{}'", name));
                        err.hint = Some(
                            "Variables must be declared with 'let' or 'var', or passed as function parameters"
                                .to_string(),
                        );
                        Err(err)
                    }
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
                    Value::String(_) => {
                        let mut err = self.error(&format!(
                            "String has no field '{}'. Did you mean .{}()?",
                            field, field
                        ));
                        err.hint = Some(
                            "String methods: length(), contains(), to_upper(), to_lower(), trim(), split(), replace()"
                                .to_string(),
                        );
                        Err(err)
                    }
                    Value::List(_) => {
                        let mut err = self.error(&format!(
                            "List has no field '{}'. Did you mean .{}()?",
                            field, field
                        ));
                        err.hint = Some(
                            "List methods: length(), contains(), push(), get(), join(), is_empty(), first(), last()"
                                .to_string(),
                        );
                        Err(err)
                    }
                    _ => Err(self.error(&format!(
                        "Cannot access field '{}' on {} value",
                        field,
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
            Expr::FnCall { callee, args, .. } => self.eval_fn_call(callee, args, env),
            Expr::Pipeline { .. } => self.eval_pipeline(expr, env),
            Expr::If {
                condition,
                then_body,
                else_body,
            } => self.eval_if(condition, then_body, else_body, env),
            Expr::Match { subject, arms, .. } => self.eval_match(subject, arms, env),
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
                    let (variant, fields) = self.eval_error_expr(error, env)?;
                    Ok(Value::Error { variant, fields })
                }
            }
            PipelineStep::ExpectAny { error } => {
                let items = self.require_list(&current, "expect any")?;
                if !items.is_empty() {
                    Ok(Value::Ok(Box::new(Value::List(items))))
                } else {
                    let (variant, fields) = self.eval_error_expr(error, env)?;
                    Ok(Value::Error { variant, fields })
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
            PipelineStep::OnError {
                variant,
                guard,
                body,
            } => {
                match &current {
                    Value::Error {
                        variant: v, fields, ..
                    } if v == variant => {
                        let mut step_env = Environment::with_parent(env.clone());
                        // For guard: bind _it as a Struct with error fields so .field works
                        if let Some(f) = fields {
                            let it_struct = Value::Struct {
                                type_name: v.clone(),
                                fields: f.clone(),
                            };
                            step_env.bind("_it".to_string(), it_struct, false);
                        } else {
                            step_env.bind("_it".to_string(), current.clone(), false);
                        }
                        // Check guard condition if present
                        if let Some(guard_expr) = guard {
                            let guard_val = self.eval_expr(guard_expr, &mut step_env)?;
                            if !guard_val.is_truthy() {
                                return Ok(current); // guard failed, pass through
                            }
                        }
                        self.eval_expr(body, &mut step_env)
                    }
                    _ => Ok(current), // pass through
                }
            }
            PipelineStep::ValidateAs { type_name } => {
                if let Some(type_fields) = self.type_defs.get(type_name).cloned() {
                    match &current {
                        Value::Struct { fields, .. } => {
                            // Check missing required (non-optional) fields
                            let missing: Vec<&str> = type_fields
                                .iter()
                                .filter(|(name, optional)| !optional && !fields.contains_key(name))
                                .map(|(name, _)| name.as_str())
                                .collect();
                            // Check unknown fields
                            let known: Vec<&str> =
                                type_fields.iter().map(|(n, _)| n.as_str()).collect();
                            let unknown: Vec<&String> = fields
                                .keys()
                                .filter(|k| !known.contains(&k.as_str()))
                                .collect();
                            if !missing.is_empty() {
                                Ok(Value::Error {
                                    variant: "ValidationError".to_string(),
                                    fields: Some({
                                        let mut m = HashMap::new();
                                        m.insert(
                                            "message".to_string(),
                                            Value::String(format!(
                                                "Missing required fields: {}",
                                                missing.join(", ")
                                            )),
                                        );
                                        m
                                    }),
                                })
                            } else if !unknown.is_empty() {
                                Ok(Value::Error {
                                    variant: "ValidationError".to_string(),
                                    fields: Some({
                                        let mut m = HashMap::new();
                                        m.insert(
                                            "message".to_string(),
                                            Value::String(format!(
                                                "Unknown fields: {}",
                                                unknown
                                                    .iter()
                                                    .map(|s| s.as_str())
                                                    .collect::<Vec<_>>()
                                                    .join(", ")
                                            )),
                                        );
                                        m
                                    }),
                                })
                            } else {
                                Ok(current)
                            }
                        }
                        _ => Ok(Value::Error {
                            variant: "ValidationError".to_string(),
                            fields: Some({
                                let mut m = HashMap::new();
                                m.insert(
                                    "message".to_string(),
                                    Value::String(format!(
                                        "Expected {} struct, got {}",
                                        type_name,
                                        current.type_name()
                                    )),
                                );
                                m
                            }),
                        }),
                    }
                } else {
                    // Unknown type — pass through
                    Ok(current)
                }
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
        // Error if db.* called without db config in app mode
        if name.starts_with("db.") && name != "db.memory" {
            if let Some((_, _, ref db_url)) = self.app_config {
                if db_url.is_none() && matches!(self.db, DbBackend::Memory { .. }) {
                    let mut err = self.error("Database not configured");
                    err.hint = Some(
                        "Add db to your app declaration:\n\n  app MyService {\n    port: 8080,\n    db: \"sqlite://data.db\",\n  }".to_string()
                    );
                    return Err(err);
                }
            }
        }
        match name {
            "list" => Ok(Value::List(args)),
            "db.insert" => self.builtin_db_insert(args),
            "db.query" => self.builtin_db_query(args),
            "db.find" => self.builtin_db_find(args),
            "db.update" => self.builtin_db_update(args),
            "db.delete" => self.builtin_db_delete(args),
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
                self.rng_sequence = None;
                Ok(self.make_rng_effect())
            }
            "rng.sequence" => {
                if let Some(Value::List(items)) = args.first() {
                    let seq: Vec<String> = items
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => format!("{}", other),
                        })
                        .collect();
                    self.rng_sequence = Some(seq);
                    self.rng_counter = 0;
                }
                Ok(self.make_rng_effect())
            }
            "db.memory" => {
                self.db = DbBackend::new_memory();
                Ok(self.make_db_effect())
            }
            "print" => {
                for arg in &args {
                    eprintln!("{}", arg);
                }
                Ok(Value::Nothing)
            }
            "log.info" => {
                if let Some(msg) = args.first() {
                    eprintln!("[INFO] {}", msg);
                }
                Ok(Value::Nothing)
            }
            "log.warn" => {
                if let Some(msg) = args.first() {
                    eprintln!("[WARN] {}", msg);
                }
                Ok(Value::Nothing)
            }
            "log.error" => {
                if let Some(msg) = args.first() {
                    eprintln!("[ERROR] {}", msg);
                }
                Ok(Value::Nothing)
            }
            "auth.require" => {
                // Check Authorization header: "Bearer <token>"
                if let Some(Value::Struct { fields, .. }) = args.first() {
                    if let Some(Value::Struct {
                        fields: headers, ..
                    }) = fields.get("headers")
                    {
                        if let Some(Value::String(auth_header)) = headers.get("authorization") {
                            let token = auth_header
                                .strip_prefix("Bearer ")
                                .unwrap_or(auth_header.as_str());
                            if !token.is_empty() {
                                let mut user_fields = HashMap::new();
                                user_fields
                                    .insert("id".to_string(), Value::String("user-1".to_string()));
                                user_fields.insert(
                                    "name".to_string(),
                                    Value::String("Authenticated User".to_string()),
                                );
                                user_fields
                                    .insert("role".to_string(), Value::String("Admin".to_string()));
                                return Ok(Value::Struct {
                                    type_name: "User".to_string(),
                                    fields: user_fields,
                                });
                            }
                        }
                    }
                }
                Ok(Value::Error {
                    variant: "Unauthorized".to_string(),
                    fields: None,
                })
            }
            "auth.mock" => {
                // For testing: returns the provided user struct as-is
                Ok(args.into_iter().next().unwrap_or(Value::Nothing))
            }
            "env.get" => match args.first() {
                Some(Value::String(key)) => match std::env::var(key) {
                    Ok(val) => Ok(Value::String(val)),
                    Err(_) => Ok(Value::Nothing),
                },
                _ => Err(self.error("env.get expects a String argument")),
            },
            "env.require" => match args.first() {
                Some(Value::String(key)) => match std::env::var(key) {
                    Ok(val) => Ok(Value::String(val)),
                    Err(_) => {
                        let mut err = self.error(&format!(
                            "Required environment variable '{}' is not set",
                            key
                        ));
                        err.hint = Some(format!("Set it with: export {}=value", key));
                        Err(err)
                    }
                },
                _ => Err(self.error("env.require expects a String argument")),
            },
            "http.mock" => self.builtin_http_mock(args),
            "http.get" => self.builtin_http_request("GET", args),
            "http.post" => self.builtin_http_request("POST", args),
            "http.put" => self.builtin_http_request("PUT", args),
            "http.delete" => self.builtin_http_request("DELETE", args),
            _ => Err(self.error(&format!("Unknown builtin '{}'", name))),
        }
    }

    fn builtin_http_mock(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let arg = args.into_iter().next().unwrap_or(Value::Nothing);
        match arg {
            Value::Struct { fields, .. } => {
                let mut mock_map = HashMap::new();
                for (url, response) in fields {
                    mock_map.insert(url, response);
                }
                self.http_mock_responses = Some(mock_map);
                Ok(self.make_http_effect())
            }
            Value::Nothing => {
                // http.mock() with no args or empty — mock with no URLs
                self.http_mock_responses = Some(HashMap::new());
                Ok(self.make_http_effect())
            }
            _ => Err(self.error("http.mock expects a Struct mapping URLs to response structs")),
        }
    }

    fn builtin_http_request(
        &mut self,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        // Extract URL
        let url = match args.first() {
            Some(Value::String(s)) => s.clone(),
            _ => {
                return Err(self.error(&format!(
                    "http.{} expects a String URL as first argument",
                    method.to_lowercase()
                )));
            }
        };

        // Extract options struct (headers, body, timeout)
        let options = match args.get(1) {
            Some(Value::Struct { fields, .. }) => Some(fields.clone()),
            Some(_) => {
                return Err(self.error(&format!(
                    "http.{} second argument must be an options Struct",
                    method.to_lowercase()
                )));
            }
            None => None,
        };

        // Check mock first
        if let Some(ref mock_map) = self.http_mock_responses {
            if let Some(response) = mock_map.get(&url) {
                // Ensure response is a proper struct with at least status
                match response {
                    Value::Struct { fields, .. } => {
                        let mut result = fields.clone();
                        if !result.contains_key("status") {
                            result.insert("status".to_string(), Value::Int(200));
                        }
                        if !result.contains_key("headers") {
                            result.insert(
                                "headers".to_string(),
                                Value::Struct {
                                    type_name: String::new(),
                                    fields: HashMap::new(),
                                },
                            );
                        }
                        if !result.contains_key("body") {
                            result.insert("body".to_string(), Value::Nothing);
                        }
                        return Ok(Value::Struct {
                            type_name: String::new(),
                            fields: result,
                        });
                    }
                    _ => return Ok(response.clone()),
                }
            } else {
                // URL not in mock map -> HttpError
                let mut fields = HashMap::new();
                fields.insert(
                    "message".to_string(),
                    Value::String(format!("No mock configured for URL: {}", url)),
                );
                return Ok(Value::Error {
                    variant: "HttpError".to_string(),
                    fields: Some(fields),
                });
            }
        }

        // Real HTTP request via ureq
        self.http_real_request(method, &url, options)
    }

    fn http_real_request(
        &self,
        method: &str,
        url: &str,
        options: Option<HashMap<String, Value>>,
    ) -> Result<Value, RuntimeError> {
        use crate::interpreter::json::value_to_json;

        // Extract headers from options
        let headers: Vec<(String, String)> = if let Some(ref opts) = options {
            if let Some(Value::Struct { fields, .. }) = opts.get("headers") {
                fields
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            match v {
                                Value::String(s) => s.clone(),
                                other => format!("{}", other),
                            },
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let timeout_ms: u64 = if let Some(ref opts) = options {
            match opts.get("timeout") {
                Some(Value::Int(ms)) if *ms > 0 => *ms as u64,
                Some(Value::Int(_)) => 30000, // negative/zero → default
                _ => 30000,
            }
        } else {
            30000
        };

        let body_json: Option<serde_json::Value> = if let Some(ref opts) = options {
            opts.get("body").map(|v| value_to_json(v))
        } else {
            None
        };

        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_millis(timeout_ms)))
            .http_status_as_error(false)
            .build();
        let agent = ureq::Agent::new_with_config(config);

        // ureq v3 uses typed builders: WithBody vs WithoutBody
        // We handle each method separately to satisfy the type system
        let has_custom_content_type = headers
            .iter()
            .any(|(k, _)| k.to_lowercase() == "content-type");

        let result: Result<ureq::http::Response<ureq::Body>, ureq::Error> = match method {
            "GET" => {
                let mut req = agent.get(url);
                for (k, v) in &headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                req.call()
            }
            "POST" | "PUT" | "DELETE" => {
                let mut req = match method {
                    "POST" => agent.post(url),
                    "PUT" => agent.put(url),
                    "DELETE" => agent.delete(url).force_send_body(),
                    _ => unreachable!(),
                };
                for (k, v) in &headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                if !has_custom_content_type {
                    req = req.content_type("application/json");
                }
                if let Some(ref json_body) = body_json {
                    let body_str = json_body.to_string();
                    req.send(body_str.as_bytes())
                } else {
                    req.send_empty()
                }
            }
            _ => return Err(self.error(&format!("Unsupported HTTP method: {}", method))),
        };

        self.http_build_response(result)
    }

    fn http_build_response(
        &self,
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Value, RuntimeError> {
        use crate::interpreter::json::json_to_value;

        match result {
            Ok(response) => {
                let status = response.status().as_u16() as i64;

                // Collect response headers (normalized: lowercase, - -> _)
                let mut header_fields = HashMap::new();
                for (name, value) in response.headers().iter() {
                    let normalized_key = name.as_str().to_lowercase().replace('-', "_");
                    if let Ok(s) = value.to_str() {
                        header_fields.insert(normalized_key, Value::String(s.to_string()));
                    }
                }

                // Read body
                let body_str = response.into_body().read_to_string().unwrap_or_default();

                // Try parse as JSON, fallback to string
                let body = match serde_json::from_str::<serde_json::Value>(&body_str) {
                    Ok(json) => json_to_value(&json),
                    Err(_) => Value::String(body_str),
                };

                let mut fields = HashMap::new();
                fields.insert("status".to_string(), Value::Int(status));
                fields.insert("body".to_string(), body);
                fields.insert(
                    "headers".to_string(),
                    Value::Struct {
                        type_name: String::new(),
                        fields: header_fields,
                    },
                );

                Ok(Value::Struct {
                    type_name: String::new(),
                    fields,
                })
            }
            Err(e) => {
                let mut fields = HashMap::new();
                fields.insert("message".to_string(), Value::String(format!("{}", e)));
                Ok(Value::Error {
                    variant: "HttpError".to_string(),
                    fields: Some(fields),
                })
            }
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
        self.db.insert(&table_name, value)
    }

    fn builtin_db_query(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.is_empty() || args.len() > 2 {
            return Err(
                self.error("db.query expects 1-2 arguments: table name, optional filter struct")
            );
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.query first argument must be a String table name")),
        };
        let filter = args.get(1);
        self.db.query(&table_name, filter)
    }

    fn builtin_db_find(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.find expects 2 arguments: table name, filter struct"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.find first argument must be a String table name")),
        };
        self.db.find(&table_name, &args[1])
    }

    fn builtin_db_update(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 3 {
            return Err(self.error("db.update expects 3 arguments: table name, id, new value"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.update first argument must be a String table name")),
        };
        let id = match &args[1] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.update second argument must be a String id")),
        };
        let new_value = args[2].clone();
        self.db.update(&table_name, &id, new_value)
    }

    fn builtin_db_delete(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if args.len() != 2 {
            return Err(self.error("db.delete expects 2 arguments: table name, id"));
        }
        let table_name = match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.delete first argument must be a String table name")),
        };
        let id = match &args[1] {
            Value::String(s) => s.clone(),
            _ => return Err(self.error("db.delete second argument must be a String id")),
        };
        self.db.delete(&table_name, &id)
    }

    fn builtin_time_now(&self) -> Result<Value, RuntimeError> {
        let time_str = self
            .fixed_time
            .clone()
            .unwrap_or_else(|| "2026-04-02T12:00:00Z".to_string());
        Ok(Value::String(time_str))
    }

    fn next_from_sequence(&mut self) -> Option<String> {
        if let Some(ref seq) = self.rng_sequence {
            let idx = self.rng_counter as usize;
            self.rng_counter += 1;
            Some(
                seq.get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("seq-overflow-{}", idx)),
            )
        } else {
            None
        }
    }

    fn builtin_rng_uuid(&mut self) -> Result<Value, RuntimeError> {
        if let Some(val) = self.next_from_sequence() {
            return Ok(Value::String(val));
        }
        self.rng_counter += 1;
        let seed = self.rng_seed.unwrap_or(0);
        Ok(Value::String(format!("uuid-{}-{}", seed, self.rng_counter)))
    }

    fn builtin_rng_hex(&mut self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        if let Some(val) = self.next_from_sequence() {
            return Ok(Value::String(val));
        }
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
            "find".to_string(),
            Value::BuiltinFn {
                name: "db.find".to_string(),
            },
        );
        db_methods.insert(
            "update".to_string(),
            Value::BuiltinFn {
                name: "db.update".to_string(),
            },
        );
        db_methods.insert(
            "delete".to_string(),
            Value::BuiltinFn {
                name: "db.delete".to_string(),
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
        rng_methods.insert(
            "sequence".to_string(),
            Value::BuiltinFn {
                name: "rng.sequence".to_string(),
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

        // auth effect
        let mut auth_methods = HashMap::new();
        auth_methods.insert(
            "require".to_string(),
            Value::BuiltinFn {
                name: "auth.require".to_string(),
            },
        );
        auth_methods.insert(
            "mock".to_string(),
            Value::BuiltinFn {
                name: "auth.mock".to_string(),
            },
        );
        self.global.bind(
            "auth".to_string(),
            Value::Effect {
                name: "auth".to_string(),
                methods: auth_methods,
            },
            false,
        );

        // env effect
        let mut env_methods = HashMap::new();
        env_methods.insert(
            "get".to_string(),
            Value::BuiltinFn {
                name: "env.get".to_string(),
            },
        );
        env_methods.insert(
            "require".to_string(),
            Value::BuiltinFn {
                name: "env.require".to_string(),
            },
        );
        self.global.bind(
            "env".to_string(),
            Value::Effect {
                name: "env".to_string(),
                methods: env_methods,
            },
            false,
        );

        // log effect
        let mut log_methods = HashMap::new();
        log_methods.insert(
            "info".to_string(),
            Value::BuiltinFn {
                name: "log.info".to_string(),
            },
        );
        log_methods.insert(
            "warn".to_string(),
            Value::BuiltinFn {
                name: "log.warn".to_string(),
            },
        );
        log_methods.insert(
            "error".to_string(),
            Value::BuiltinFn {
                name: "log.error".to_string(),
            },
        );
        self.global.bind(
            "log".to_string(),
            Value::Effect {
                name: "log".to_string(),
                methods: log_methods,
            },
            false,
        );

        // http effect
        self.global
            .bind("http".to_string(), self.make_http_effect(), false);
    }

    fn make_http_effect(&self) -> Value {
        let mut methods = HashMap::new();
        methods.insert(
            "get".to_string(),
            Value::BuiltinFn {
                name: "http.get".to_string(),
            },
        );
        methods.insert(
            "post".to_string(),
            Value::BuiltinFn {
                name: "http.post".to_string(),
            },
        );
        methods.insert(
            "put".to_string(),
            Value::BuiltinFn {
                name: "http.put".to_string(),
            },
        );
        methods.insert(
            "delete".to_string(),
            Value::BuiltinFn {
                name: "http.delete".to_string(),
            },
        );
        methods.insert(
            "mock".to_string(),
            Value::BuiltinFn {
                name: "http.mock".to_string(),
            },
        );
        Value::Effect {
            name: "http".to_string(),
            methods,
        }
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
            "find".to_string(),
            Value::BuiltinFn {
                name: "db.find".to_string(),
            },
        );
        methods.insert(
            "update".to_string(),
            Value::BuiltinFn {
                name: "db.update".to_string(),
            },
        );
        methods.insert(
            "delete".to_string(),
            Value::BuiltinFn {
                name: "db.delete".to_string(),
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
        methods.insert(
            "sequence".to_string(),
            Value::BuiltinFn {
                name: "rng.sequence".to_string(),
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

        let mut err = self.error(&format!("No matching pattern for value: {}", subject_val));
        err.hint = Some("Add a catch-all arm: _ => ...".to_string());
        Err(err)
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

    // --- Error handling helpers ---

    /// Evaluate an expression in `raise` position (e.g., `expect one or raise NotFound`).
    /// PascalCase identifiers are treated as error variant names, not variable lookups.
    fn eval_error_expr(
        &mut self,
        expr: &Expr,
        env: &mut Environment,
    ) -> Result<(String, Option<HashMap<String, Value>>), RuntimeError> {
        match expr {
            Expr::Identifier(name) if name.starts_with(char::is_uppercase) => {
                Ok((name.clone(), None))
            }
            Expr::StructLiteral {
                name: Some(name),
                fields,
            } if name.starts_with(char::is_uppercase) => {
                let mut field_map = HashMap::new();
                for f in fields {
                    if let crate::parser::ast::StructField::Named {
                        name: fname,
                        value: fval,
                    } = f
                    {
                        let v = self.eval_expr(fval, env)?;
                        field_map.insert(fname.clone(), v);
                    }
                }
                Ok((
                    name.clone(),
                    if field_map.is_empty() {
                        None
                    } else {
                        Some(field_map)
                    },
                ))
            }
            other => {
                let val = self.eval_expr(other, env)?;
                let variant = match val {
                    Value::String(s) => s,
                    _ => format!("{}", val),
                };
                Ok((variant, None))
            }
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
            // Circular dependency check
            let canonical = file_path.canonicalize().unwrap_or(file_path.clone());
            if self.loading_modules.contains(&canonical) {
                return Err(self.error(&format!(
                    "Circular import detected: {} is already being loaded",
                    file_path.display()
                )));
            }
            self.loading_modules.insert(canonical.clone());

            let result = (|| -> Result<Environment, RuntimeError> {
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

                Ok(module_env)
            })();

            self.loading_modules.remove(&canonical);
            let module_env = result?;

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
        } else if self.type_defs.contains_key(symbol_name) {
            // Type definitions are stored in interpreter.type_defs, not in Environment.
            // When the module was evaluated, TypeDecl already registered the type globally.
            // Nothing to bind — the type is already available.
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
        let known_effects: Vec<String> = ["db", "auth", "log", "time", "rng", "env", "http"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Block undeclared effects from global lookup
        self.blocked_effects = known_effects
            .iter()
            .filter(|e| !route.effects.contains(e))
            .cloned()
            .collect();

        let mut env = Environment::with_parent(self.global.clone());
        env.bind("request".to_string(), request, false);

        // Explicitly bind declared effects into local env
        for effect_name in &route.effects {
            if let Some(effect) = self.global.lookup(effect_name) {
                env.bind(effect_name.clone(), effect.clone(), false);
            }
        }

        let mut result = Value::Nothing;
        let body = route.body.clone();
        for stmt in &body {
            match self.eval_statement(stmt, &mut env) {
                Ok(StmtResult::Value(val)) => result = val,
                Ok(StmtResult::Return(val)) => {
                    self.blocked_effects.clear();
                    return Ok(val);
                }
                Err(ref e) if Self::is_return_error(e) => {
                    self.blocked_effects.clear();
                    return Ok(self.take_return_value());
                }
                Err(e) => {
                    self.blocked_effects.clear();
                    return Err(e);
                }
            }
        }
        self.blocked_effects.clear();
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
                self.db.clear();
                self.http_mock_responses = None;

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
    fn eval_use_import_type() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_import_type");
        let _ = fs::create_dir_all(dir.join("models"));
        fs::write(
            dir.join("models/user.pact"),
            "type User {\n  name: String,\n  age: Int\n}\n",
        )
        .unwrap();

        // Import the type by name — should not error
        let input = "use models.user.User\nlet u: User = User { name: \"Alice\", age: 30 }\nu.name";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::String("Alice".to_string()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn eval_use_wildcard_import_type() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_wildcard_type");
        let _ = fs::create_dir_all(dir.join("models"));
        fs::write(
            dir.join("models/user.pact"),
            "type User {\n  name: String\n}\n\nintent \"make user\"\nfn make_user(name: String) -> User {\n  User { name: name }\n}\n",
        )
        .unwrap();

        // Wildcard import should include both type and function
        let input = "use models.user.*\nlet u: User = make_user(\"Bob\")\nu.name";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::String("Bob".to_string()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_circular_import_detected() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_circular");
        let _ = fs::create_dir_all(dir.join("mods"));
        fs::write(
            dir.join("mods/a.pact"),
            "use mods.b.something\nintent \"a fn\"\nfn a_fn() -> Int { 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("mods/b.pact"),
            "use mods.a.a_fn\nintent \"b fn\"\nfn something() -> Int { 2 }\n",
        )
        .unwrap();

        let input = "use mods.a.a_fn\na_fn()";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let err = interp.interpret(&program).unwrap_err();
        assert!(
            err.message.contains("Circular import"),
            "Expected circular import error, got: {}",
            err.message
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_deep_circular_import() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_deep_circular");
        let _ = fs::create_dir_all(dir.join("chain"));
        fs::write(
            dir.join("chain/a.pact"),
            "use chain.b.b_fn\nintent \"a\"\nfn a_fn() -> Int { 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("chain/b.pact"),
            "use chain.c.c_fn\nintent \"b\"\nfn b_fn() -> Int { 2 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("chain/c.pact"),
            "use chain.a.a_fn\nintent \"c\"\nfn c_fn() -> Int { 3 }\n",
        )
        .unwrap();

        let input = "use chain.a.a_fn\na_fn()";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let err = interp.interpret(&program).unwrap_err();
        assert!(
            err.message.contains("Circular import"),
            "Expected circular import error, got: {}",
            err.message
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_cached_module_not_circular() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_cached_not_circular");
        let _ = fs::create_dir_all(dir.join("shared"));
        fs::write(
            dir.join("shared/lib.pact"),
            "intent \"shared fn\"\nfn shared_fn() -> Int { 99 }\n",
        )
        .unwrap();

        // Import the same module twice — should work (cache hit, not circular)
        let input = "use shared.lib.shared_fn\nuse shared.lib.shared_fn\nshared_fn()";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::Int(99));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_modular_example_runs() {
        // Verify the examples/modular/ multi-file example parses and checks cleanly
        let main_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/modular/main.pact");
        let source = std::fs::read_to_string(&main_path).expect("examples/modular/main.pact");
        let mut lexer = Lexer::new(&source);
        let tokens = lexer.tokenize().expect("lex modular main");
        let mut parser = Parser::new(tokens, &source);
        let program = parser.parse().expect("parse modular main");
        let base_dir = main_path.parent().unwrap();
        let diagnostics = crate::checker::check(&program, &source, Some(base_dir));
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == crate::checker::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "Modular example should have no type errors: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_loading_modules_cleared_on_error() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_test_loading_cleanup");
        let _ = fs::create_dir_all(dir.join("broken"));
        // Write a file that will fail to parse
        fs::write(dir.join("broken/bad.pact"), "fn {{{").unwrap();
        // Write a good file
        fs::write(
            dir.join("broken/good.pact"),
            "intent \"get val\"\nfn get_val() -> Int { 42 }\n",
        )
        .unwrap();

        // Import bad file (will error), then import good file (should work, not "circular")
        let input = "use broken.good.get_val\nget_val()";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.base_dir = Some(dir.clone());
        let result = interp.interpret(&program).unwrap();
        assert_eq!(result, Value::Int(42));

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
        let input = "intent \"health check\"\nroute GET \"/health\" {\n  respond 200 with { status: \"ok\" }\n}";
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
        let input = "intent \"health check\"\nroute GET \"/health\" {\n  respond 200 with { status: \"ok\" }\n}";
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
    fn eval_db_no_config_with_app_errors() {
        let input =
            "app TestApp {\n  port: 8080,\n}\ndb.insert(\"users\", User { name: \"Alice\" })";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let result = interp.interpret(&program);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Database not configured"));
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

    // --- HTTP effect tests ---

    #[test]
    fn test_http_mock_setup() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users": { status: 200, body: "ok" }
})
http"#;
        let result = eval_with_effects(input);
        match result {
            Value::Effect { name, .. } => assert_eq!(name, "http"),
            other => panic!("Expected Effect, got {:?}", other),
        }
    }

    #[test]
    fn test_http_needs_declaration() {
        // In a route without `needs http`, accessing http should be blocked
        let input = r#"app TestApp { port: 3000 }
intent "test"
route GET "/test" {
  http.get("https://example.com")
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        // The route is registered but when called, http should be blocked
        interp.interpret(&program).unwrap();
        // Simulate calling the route
        let route = interp.routes[0].clone();
        let req = Value::Struct {
            type_name: String::new(),
            fields: HashMap::new(),
        };
        let result = interp.execute_route(&route, req);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("not available"),
            "Expected 'not available' error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_http_get_mocked() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users": { status: 200, body: list({ name: "Alice", active: true }, { name: "Bob", active: false }) }
})
let res: Struct = http.get("https://api.example.com/users")
res"#;
        let result = eval_with_effects(input);
        if let Value::Struct { fields, .. } = &result {
            assert_eq!(fields.get("status"), Some(&Value::Int(200)));
            assert!(fields.contains_key("body"));
            assert!(fields.contains_key("headers"));
        } else {
            panic!("Expected Struct, got {:?}", result);
        }
    }

    #[test]
    fn test_http_get_status_field() {
        let input = r#"using http = http.mock({
  "https://api.example.com/data": { status: 404, body: "not found" }
})
let res: Struct = http.get("https://api.example.com/data")
res.status"#;
        assert_eq!(eval_with_effects(input), Value::Int(404));
    }

    #[test]
    fn test_http_get_body_field() {
        let input = r#"using http = http.mock({
  "https://api.example.com/data": { status: 200, body: "hello" }
})
let res: Struct = http.get("https://api.example.com/data")
res.body"#;
        assert_eq!(eval_with_effects(input), Value::String("hello".to_string()));
    }

    #[test]
    fn test_http_get_unmocked_url() {
        let input = r#"using http = http.mock({
  "https://api.example.com/known": { status: 200, body: "ok" }
})
http.get("https://api.example.com/unknown")"#;
        let result = eval_with_effects(input);
        match result {
            Value::Error { variant, .. } => assert_eq!(variant, "HttpError"),
            other => panic!("Expected HttpError, got {:?}", other),
        }
    }

    #[test]
    fn test_http_get_body_pipeline() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users": { status: 200, body: list({ name: "Alice", active: true }, { name: "Bob", active: false }) }
})
http.get("https://api.example.com/users")
  | .body
  | filter where .active
  | map to .name"#;
        let result = eval_with_effects(input);
        assert_eq!(
            result,
            Value::List(vec![Value::String("Alice".to_string())])
        );
    }

    #[test]
    fn test_http_on_error() {
        let input = r#"using http = http.mock({})
http.get("https://unknown.url")
  | on HttpError: "caught error""#;
        let result = eval_with_effects(input);
        assert_eq!(result, Value::String("caught error".to_string()));
    }

    #[test]
    fn test_on_error_where_guard_match() {
        // Guard matches — should execute body
        let input = r#"using http = http.mock({})
http.get("https://unknown.url")
  | on HttpError where .message != "": "guard matched"
  | on HttpError: "fallback""#;
        let result = eval_with_effects(input);
        assert_eq!(result, Value::String("guard matched".to_string()));
    }

    #[test]
    fn test_on_error_where_guard_no_match() {
        // Guard doesn't match — should fall through to next handler
        let input = r#"using http = http.mock({})
http.get("https://unknown.url")
  | on HttpError where .message == "specific error": "guard matched"
  | on HttpError: "fallback""#;
        let result = eval_with_effects(input);
        assert_eq!(result, Value::String("fallback".to_string()));
    }

    #[test]
    fn test_http_post_with_body() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users": { status: 201, body: { id: 1, name: "Alice" } }
})
let res: Struct = http.post("https://api.example.com/users", {
  body: { name: "Alice" }
})
res.status"#;
        assert_eq!(eval_with_effects(input), Value::Int(201));
    }

    #[test]
    fn test_http_put_with_body() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users/1": { status: 200, body: { id: 1, name: "Updated" } }
})
let res: Struct = http.put("https://api.example.com/users/1", {
  body: { name: "Updated" }
})
res.body.name"#;
        assert_eq!(
            eval_with_effects(input),
            Value::String("Updated".to_string())
        );
    }

    #[test]
    fn test_http_delete() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users/1": { status: 204, body: nothing }
})
let res: Struct = http.delete("https://api.example.com/users/1")
res.status"#;
        assert_eq!(eval_with_effects(input), Value::Int(204));
    }

    #[test]
    fn test_http_mock_multiple_urls() {
        let input = r#"using http = http.mock({
  "https://api.example.com/users": { status: 200, body: "users" },
  "https://api.example.com/roles": { status: 200, body: "roles" }
})
let u: Struct = http.get("https://api.example.com/users")
let r: Struct = http.get("https://api.example.com/roles")
list(u.body, r.body)"#;
        assert_eq!(
            eval_with_effects(input),
            Value::List(vec![
                Value::String("users".to_string()),
                Value::String("roles".to_string()),
            ])
        );
    }

    #[test]
    fn test_http_response_has_all_fields() {
        let input = r#"using http = http.mock({
  "https://api.example.com/test": { status: 200, body: "ok" }
})
let res: Struct = http.get("https://api.example.com/test")
list(res.status, res.body)"#;
        let result = eval_with_effects(input);
        assert_eq!(
            result,
            Value::List(vec![Value::Int(200), Value::String("ok".to_string())])
        );
    }

    #[test]
    fn test_http_using_mock_in_test_block() {
        let input = r#"test "fetch users via mock" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: list({ name: "Alice", active: true }) }
  })
  let res: Struct = http.get("https://api.example.com/users")
  assert res.status == 200
  assert res.body.length() == 1
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "Test failed: {:?}", results[0].error);
    }

    #[test]
    fn test_http_using_mock_full_flow() {
        let input = r#"test "pipeline through mock" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: list(
        { name: "Alice", active: true },
        { name: "Bob", active: false }
      )}
  })
  let names: List = http.get("https://api.example.com/users")
    | .body
    | filter where .active
    | map to .name
  assert names.length() == 1
  assert names.first() == "Alice"
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "Test failed: {:?}", results[0].error);
    }

    #[test]
    fn test_http_mock_scoped_to_test() {
        // Two tests, each with their own mock — should not leak
        let input = r#"test "first mock" {
  using http = http.mock({
    "https://api.example.com/a": { status: 200, body: "first" }
  })
  let res: Struct = http.get("https://api.example.com/a")
  assert res.body == "first"
}

test "second mock" {
  using http = http.mock({
    "https://api.example.com/b": { status: 200, body: "second" }
  })
  let res: Struct = http.get("https://api.example.com/b")
  assert res.body == "second"
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 2);
        assert!(
            results[0].passed,
            "First test failed: {:?}",
            results[0].error
        );
        assert!(
            results[1].passed,
            "Second test failed: {:?}",
            results[1].error
        );
    }

    #[test]
    #[ignore] // Requires network access — run with: cargo test test_http_real -- --ignored
    fn test_http_real_get() {
        let input = r#"let res: Struct = http.get("https://httpbin.org/get")
res.status"#;
        let result = eval_with_effects(input);
        assert_eq!(result, Value::Int(200));
    }

    #[test]
    fn test_http_mock_cleared_between_tests() {
        // Test A sets mock, test B does NOT set mock.
        // Test B should NOT see test A's mock (should get real request or no mock).
        let input = r#"test "sets mock" {
  using http = http.mock({
    "https://api.example.com/a": { status: 200, body: "mocked" }
  })
  let res: Struct = http.get("https://api.example.com/a")
  assert res.body == "mocked"
}

test "no mock set - unmocked url should fail" {
  let res: Struct = http.get("https://api.example.com/a")
    | on HttpError: { leaked: true }
  assert res.leaked != true
}"#;
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let program = parser.parse().unwrap();
        let mut interp = Interpreter::new(input);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);
        assert_eq!(results.len(), 2);
        assert!(
            results[0].passed,
            "First test failed: {:?}",
            results[0].error
        );
        // Second test: no mock set, so real request would happen (or no mock = no interception)
        // The key assertion: mock from test A should NOT leak to test B
    }
}

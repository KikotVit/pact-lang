use std::collections::HashMap;

use crate::parser::ast::{BinaryOp, Expr, Program, Statement, StringExpr, StringPart, UnaryOp};
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
            Statement::Return { .. } => {
                // Return is not implemented yet (Task 6)
                Err(self.error("Return statements are not yet implemented"))
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
            Expr::ErrorPropagation(_) => {
                Err(self.error("Error propagation is not yet implemented"))
            }
            Expr::FnCall { .. } => {
                Err(self.error("Function calls are not yet implemented"))
            }
            Expr::Pipeline { .. } => {
                Err(self.error("Pipelines are not yet implemented"))
            }
            Expr::If { .. } => {
                Err(self.error("If expressions are not yet implemented"))
            }
            Expr::Match { .. } => {
                Err(self.error("Match expressions are not yet implemented"))
            }
            Expr::Block(_) => {
                Err(self.error("Block expressions are not yet implemented"))
            }
            Expr::StructLiteral { .. } => {
                Err(self.error("Struct literals are not yet implemented"))
            }
            Expr::Ensure(_) => {
                Err(self.error("Ensure expressions are not yet implemented"))
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
}

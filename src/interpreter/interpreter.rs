use std::collections::HashMap;

use crate::parser::ast::{Expr, Program, Statement};
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
            Expr::StringLiteral(_) => {
                Err(self.error("String literals are not yet implemented"))
            }
            Expr::FieldAccess { .. } => {
                Err(self.error("Field access is not yet implemented"))
            }
            Expr::DotShorthand(_) => {
                Err(self.error("Dot shorthand is not yet implemented"))
            }
            Expr::BinaryOp { .. } => {
                Err(self.error("Binary operations are not yet implemented"))
            }
            Expr::UnaryOp { .. } => {
                Err(self.error("Unary operations are not yet implemented"))
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
            Expr::Is { .. } => {
                Err(self.error("Is expressions are not yet implemented"))
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
}

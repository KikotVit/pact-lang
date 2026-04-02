use crate::lexer::{Token, TokenKind, Lexer};
use crate::parser::ast::*;
use crate::parser::errors::ParseError;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: &str) -> Self {
        Parser {
            tokens,
            pos: 0,
            source: source.to_string(),
        }
    }

    pub fn parse(&mut self) -> Result<Program, Vec<ParseError>> {
        // Placeholder -- finalized in Task 14
        Ok(Program { statements: vec![] })
    }

    // --- Token navigation ---

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn current_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek(&self) -> &TokenKind {
        if self.pos + 1 < self.tokens.len() {
            &self.tokens[self.pos + 1].kind
        } else {
            &TokenKind::Eof
        }
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.current_kind() == kind
    }

    fn at_eof(&self) -> bool {
        self.at(&TokenKind::Eof)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<&Token, ParseError> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(&format!(
                "Expected {:?}, found {:?}",
                kind, self.current_kind()
            ), None))
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.current_kind().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(
                &format!("Expected identifier, found {:?}", self.current_kind()),
                None,
            )),
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(&TokenKind::Newline) {
            self.advance();
        }
    }

    // --- Error creation ---

    fn error(&self, message: &str, hint: Option<&str>) -> ParseError {
        let token = self.current();
        let source_line = self.source
            .lines()
            .nth(token.span.line - 1)
            .unwrap_or("")
            .to_string();
        ParseError {
            line: token.span.line,
            column: token.span.column,
            message: message.to_string(),
            hint: hint.map(|s| s.to_string()),
            source_line,
        }
    }

    fn fail<T>(&self, message: &str, hint: Option<&str>) -> Result<T, ParseError> {
        Err(self.error(message, hint))
    }

    // --- Expression parsing ---

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        self.parse_and()
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        self.parse_not()
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        self.parse_addition()
    }

    fn parse_addition(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.at(&TokenKind::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp { op: UnaryOp::Neg, operand: Box::new(operand) });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.current_kind() {
                TokenKind::Dot => {
                    self.advance(); // consume '.'
                    let field = self.expect_identifier()?;
                    expr = Expr::FieldAccess {
                        object: Box::new(expr),
                        field,
                    };
                }
                TokenKind::LParen => {
                    self.advance(); // consume '('
                    let args = self.parse_args_list()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Expr::FnCall {
                        callee: Box::new(expr),
                        args,
                    };
                }
                TokenKind::Question => {
                    self.advance(); // consume '?'
                    expr = Expr::ErrorPropagation(Box::new(expr));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.at(&TokenKind::RParen) {
            return Ok(args);
        }
        args.push(self.parse_expression()?);
        while self.eat(&TokenKind::Comma) {
            // Handle trailing comma
            if self.at(&TokenKind::RParen) {
                break;
            }
            args.push(self.parse_expression()?);
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.current_kind().clone() {
            TokenKind::IntLiteral(n) => {
                self.advance();
                Ok(Expr::IntLiteral(n))
            }
            TokenKind::FloatLiteral(n) => {
                self.advance();
                Ok(Expr::FloatLiteral(n))
            }
            TokenKind::BoolLiteral(b) => {
                self.advance();
                Ok(Expr::BoolLiteral(b))
            }
            TokenKind::Nothing => {
                self.advance();
                Ok(Expr::Nothing)
            }
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(Expr::Identifier(name))
            }
            TokenKind::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Dot => {
                self.parse_dot_shorthand()
            }
            TokenKind::StringStart | TokenKind::RawStringLiteral(_) => {
                self.fail("Strings not yet implemented", None)
            }
            TokenKind::If => {
                self.fail("If not yet implemented", None)
            }
            TokenKind::Match => {
                self.fail("Match not yet implemented", None)
            }
            TokenKind::Ensure => {
                self.advance(); // consume 'ensure'
                let expr = self.parse_expression()?;
                Ok(Expr::Ensure(Box::new(expr)))
            }
            _ => {
                self.fail("Expected expression", None)
            }
        }
    }

    fn parse_dot_shorthand(&mut self) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::Dot)?; // consume initial '.'
        let first = self.expect_identifier()?;
        let mut parts = vec![first];
        while self.eat(&TokenKind::Dot) {
            let name = self.expect_identifier()?;
            parts.push(name);
        }
        Ok(Expr::DotShorthand(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_expr(input: &str) -> Expr {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        parser.parse_expression().unwrap()
    }

    fn parse_program(input: &str) -> Program {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        parser.parse().unwrap()
    }

    #[test]
    fn parser_creates_empty_program() {
        let prog = parse_program("");
        assert_eq!(prog.statements.len(), 0);
    }

    #[test]
    fn parse_int_literal() {
        assert_eq!(parse_expr("42"), Expr::IntLiteral(42));
    }

    #[test]
    fn parse_float_literal() {
        assert_eq!(parse_expr("3.14"), Expr::FloatLiteral(3.14));
    }

    #[test]
    fn parse_bool_literal() {
        assert_eq!(parse_expr("true"), Expr::BoolLiteral(true));
        assert_eq!(parse_expr("false"), Expr::BoolLiteral(false));
    }

    #[test]
    fn parse_nothing() {
        assert_eq!(parse_expr("nothing"), Expr::Nothing);
    }

    #[test]
    fn parse_identifier() {
        assert_eq!(parse_expr("foo"), Expr::Identifier("foo".to_string()));
    }

    #[test]
    fn parse_grouped_expression() {
        assert_eq!(parse_expr("(42)"), Expr::IntLiteral(42));
    }

    #[test]
    fn parse_dot_shorthand_simple() {
        assert_eq!(
            parse_expr(".active"),
            Expr::DotShorthand(vec!["active".to_string()]),
        );
    }

    #[test]
    fn parse_dot_shorthand_nested() {
        assert_eq!(
            parse_expr(".values.length"),
            Expr::DotShorthand(vec!["values".to_string(), "length".to_string()]),
        );
    }

    #[test]
    fn parse_field_access() {
        assert_eq!(
            parse_expr("user.name"),
            Expr::FieldAccess {
                object: Box::new(Expr::Identifier("user".to_string())),
                field: "name".to_string(),
            },
        );
    }

    #[test]
    fn parse_chained_field_access() {
        assert_eq!(
            parse_expr("user.address.city"),
            Expr::FieldAccess {
                object: Box::new(Expr::FieldAccess {
                    object: Box::new(Expr::Identifier("user".to_string())),
                    field: "address".to_string(),
                }),
                field: "city".to_string(),
            },
        );
    }

    #[test]
    fn parse_fn_call_no_args() {
        assert_eq!(
            parse_expr("foo()"),
            Expr::FnCall {
                callee: Box::new(Expr::Identifier("foo".to_string())),
                args: vec![],
            },
        );
    }

    #[test]
    fn parse_fn_call_with_args() {
        assert_eq!(
            parse_expr("add(1, 2)"),
            Expr::FnCall {
                callee: Box::new(Expr::Identifier("add".to_string())),
                args: vec![Expr::IntLiteral(1), Expr::IntLiteral(2)],
            },
        );
    }

    #[test]
    fn parse_method_call() {
        let expr = parse_expr("db.query(x)");
        assert!(matches!(
            expr,
            Expr::FnCall { ref callee, ref args } if matches!(**callee, Expr::FieldAccess { .. }) && args.len() == 1
        ));
    }

    #[test]
    fn parse_error_propagation() {
        assert_eq!(
            parse_expr("foo()?"),
            Expr::ErrorPropagation(Box::new(
                Expr::FnCall {
                    callee: Box::new(Expr::Identifier("foo".to_string())),
                    args: vec![],
                }
            )),
        );
    }

    #[test]
    fn parse_postfix_chain() {
        // find_user(id)?.name → FieldAccess { ErrorPropagation(FnCall), "name" }
        let expr = parse_expr("find_user(id)?.name");
        assert!(matches!(expr, Expr::FieldAccess { .. }));
        if let Expr::FieldAccess { object, field } = expr {
            assert_eq!(field, "name");
            assert!(matches!(*object, Expr::ErrorPropagation(_)));
        }
    }

    #[test]
    fn parse_addition() {
        assert_eq!(parse_expr("1 + 2"), Expr::BinaryOp {
            left: Box::new(Expr::IntLiteral(1)), op: BinaryOp::Add, right: Box::new(Expr::IntLiteral(2)),
        });
    }

    #[test]
    fn parse_subtraction() {
        assert_eq!(parse_expr("a - b"), Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::Sub, right: Box::new(Expr::Identifier("b".to_string())),
        });
    }

    #[test]
    fn parse_multiplication_precedence() {
        // 1 + 2 * 3 → Add(1, Mul(2, 3))
        assert_eq!(parse_expr("1 + 2 * 3"), Expr::BinaryOp {
            left: Box::new(Expr::IntLiteral(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::IntLiteral(2)), op: BinaryOp::Mul, right: Box::new(Expr::IntLiteral(3)),
            }),
        });
    }

    #[test]
    fn parse_unary_negation() {
        assert_eq!(parse_expr("-42"), Expr::UnaryOp { op: UnaryOp::Neg, operand: Box::new(Expr::IntLiteral(42)) });
    }

    #[test]
    fn parse_division() {
        assert_eq!(parse_expr("a / b"), Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::Div, right: Box::new(Expr::Identifier("b".to_string())),
        });
    }
}

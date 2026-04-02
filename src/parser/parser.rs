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
        let source = self.parse_or()?;
        if !self.at(&TokenKind::Pipe) {
            return Ok(source);
        }
        let mut steps = Vec::new();
        while self.eat(&TokenKind::Pipe) {
            self.skip_newlines();
            let step = self.parse_pipeline_step()?;
            steps.push(step);
        }
        Ok(Expr::Pipeline { source: Box::new(source), steps })
    }

    fn parse_pipeline_step(&mut self) -> Result<PipelineStep, ParseError> {
        // Handle `or default <value>` — `or` is a keyword token, not an identifier
        if self.at(&TokenKind::Or) {
            self.advance(); // consume `or`
            self.expect_contextual("default")?;
            let value = self.parse_or()?;
            return Ok(PipelineStep::OrDefault { value });
        }

        if let TokenKind::Identifier(ref name) = self.current_kind().clone() {
            match name.as_str() {
                "filter" => {
                    self.advance();
                    self.expect_contextual("where")?;
                    let predicate = self.parse_or()?;
                    Ok(PipelineStep::Filter { predicate })
                }
                "map" => {
                    self.advance();
                    self.expect_contextual("to")?;
                    let expr = self.parse_or()?;
                    Ok(PipelineStep::Map { expr })
                }
                "sort" => {
                    self.advance();
                    self.expect_contextual("by")?;
                    let field = self.parse_or()?;
                    let descending = if self.eat_contextual("ascending") {
                        false
                    } else if self.eat_contextual("descending") {
                        true
                    } else {
                        false
                    };
                    Ok(PipelineStep::Sort { field, descending })
                }
                "group" => {
                    self.advance();
                    self.expect_contextual("by")?;
                    let field = self.parse_or()?;
                    Ok(PipelineStep::GroupBy { field })
                }
                "take" => {
                    self.advance();
                    let kind = if self.eat_contextual("first") {
                        TakeKind::First
                    } else if self.eat_contextual("last") {
                        TakeKind::Last
                    } else {
                        return self.fail(
                            &format!("Expected 'first' or 'last' after 'take', found {:?}", self.current_kind()),
                            None,
                        );
                    };
                    let count = self.parse_or()?;
                    Ok(PipelineStep::Take { kind, count })
                }
                "skip" => {
                    self.advance();
                    let count = self.parse_or()?;
                    Ok(PipelineStep::Skip { count })
                }
                "each" => {
                    self.advance();
                    let expr = self.parse_or()?;
                    Ok(PipelineStep::Each { expr })
                }
                "find" => {
                    self.advance();
                    self.expect_contextual("first")?;
                    self.expect_contextual("where")?;
                    let predicate = self.parse_or()?;
                    Ok(PipelineStep::FindFirst { predicate })
                }
                "expect" => {
                    self.advance();
                    if self.eat_contextual("one") {
                        self.expect(&TokenKind::Or)?;
                        self.expect_contextual("raise")?;
                        let error = self.parse_or()?;
                        Ok(PipelineStep::ExpectOne { error })
                    } else if self.eat_contextual("any") {
                        self.expect(&TokenKind::Or)?;
                        self.expect_contextual("raise")?;
                        let error = self.parse_or()?;
                        Ok(PipelineStep::ExpectAny { error })
                    } else {
                        self.fail(
                            &format!("Expected 'one' or 'any' after 'expect', found {:?}", self.current_kind()),
                            None,
                        )
                    }
                }
                "flatten" => {
                    self.advance();
                    Ok(PipelineStep::Flatten)
                }
                "unique" => {
                    self.advance();
                    Ok(PipelineStep::Unique)
                }
                "count" => {
                    self.advance();
                    Ok(PipelineStep::Count)
                }
                "sum" => {
                    self.advance();
                    Ok(PipelineStep::Sum)
                }
                _ => {
                    let expr = self.parse_or()?;
                    Ok(PipelineStep::Expr(expr))
                }
            }
        } else {
            let expr = self.parse_or()?;
            Ok(PipelineStep::Expr(expr))
        }
    }

    fn expect_contextual(&mut self, word: &str) -> Result<(), ParseError> {
        if let TokenKind::Identifier(ref w) = self.current_kind().clone() {
            if w == word {
                self.advance();
                return Ok(());
            }
        }
        Err(self.error(&format!("Expected '{}', found {:?}", word, self.current_kind()), None))
    }

    fn eat_contextual(&mut self, word: &str) -> bool {
        if let TokenKind::Identifier(ref w) = self.current_kind().clone() {
            if w == word {
                self.advance();
                return true;
            }
        }
        false
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.at(&TokenKind::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp { left: Box::new(left), op: BinaryOp::Or, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.at(&TokenKind::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinaryOp { left: Box::new(left), op: BinaryOp::And, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.at(&TokenKind::Not) {
            self.advance();
            let operand = self.parse_not()?;
            return Ok(Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(operand) });
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_addition()?;

        // Check for `is` (contextual keyword)
        if let TokenKind::Identifier(ref word) = self.current_kind().clone() {
            if word == "is" {
                self.advance();
                let type_name = self.expect_identifier()?;
                return Ok(Expr::Is { expr: Box::new(left), type_name });
            }
        }

        let op = match self.current_kind() {
            TokenKind::Eq => BinaryOp::Eq,
            TokenKind::NotEq => BinaryOp::NotEq,
            TokenKind::LAngle => BinaryOp::Lt,
            TokenKind::RAngle => BinaryOp::Gt,
            TokenKind::LessEq => BinaryOp::LtEq,
            TokenKind::GreaterEq => BinaryOp::GtEq,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_addition()?;
        Ok(Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) })
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
                self.parse_string_expr()
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

    fn parse_string_expr(&mut self) -> Result<Expr, ParseError> {
        match self.current_kind().clone() {
            TokenKind::RawStringLiteral(content) => {
                self.advance();
                Ok(Expr::StringLiteral(StringExpr::Simple(content)))
            }
            TokenKind::StringStart => {
                self.advance(); // consume StringStart

                // Empty string: StringStart immediately followed by StringEnd
                if self.at(&TokenKind::StringEnd) {
                    self.advance();
                    return Ok(Expr::StringLiteral(StringExpr::Simple(String::new())));
                }

                let parts = self.collect_string_parts()?;
                self.expect(&TokenKind::StringEnd)?;

                // If only one literal part and no interpolation, treat as simple
                if parts.len() == 1 {
                    if let StringPart::Literal(ref text) = parts[0] {
                        return Ok(Expr::StringLiteral(StringExpr::Simple(text.clone())));
                    }
                }

                Ok(Expr::StringLiteral(StringExpr::Interpolated(parts)))
            }
            _ => self.fail("Expected string", None),
        }
    }

    fn collect_string_parts(&mut self) -> Result<Vec<StringPart>, ParseError> {
        let mut parts = Vec::new();
        loop {
            match self.current_kind().clone() {
                TokenKind::StringFragment(text) => {
                    self.advance();
                    parts.push(StringPart::Literal(text));
                }
                TokenKind::InterpolationStart => {
                    self.advance(); // consume InterpolationStart
                    let expr = self.parse_expression()?;
                    self.expect(&TokenKind::InterpolationEnd)?;
                    parts.push(StringPart::Expr(expr));
                }
                TokenKind::StringEnd => break,
                _ => return self.fail("Unexpected token in string", None),
            }
        }
        Ok(parts)
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

    #[test]
    fn parse_comparison_eq() {
        assert_eq!(parse_expr("a == b"), Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::Eq, right: Box::new(Expr::Identifier("b".to_string())),
        });
    }

    #[test]
    fn parse_comparison_not_eq() {
        assert_eq!(parse_expr("a != b"), Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::NotEq, right: Box::new(Expr::Identifier("b".to_string())),
        });
    }

    #[test]
    fn parse_less_than() {
        assert_eq!(parse_expr("a < b"), Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::Lt, right: Box::new(Expr::Identifier("b".to_string())),
        });
    }

    #[test]
    fn parse_and_or() {
        // a and b or c → Or(And(a, b), c)  (and binds tighter than or)
        assert_eq!(parse_expr("a and b or c"), Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())), op: BinaryOp::And, right: Box::new(Expr::Identifier("b".to_string())),
            }),
            op: BinaryOp::Or,
            right: Box::new(Expr::Identifier("c".to_string())),
        });
    }

    #[test]
    fn parse_not() {
        assert_eq!(parse_expr("not x"), Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(Expr::Identifier("x".to_string())) });
    }

    #[test]
    fn parse_is_expr() {
        assert_eq!(parse_expr("result is NotFound"), Expr::Is {
            expr: Box::new(Expr::Identifier("result".to_string())), type_name: "NotFound".to_string(),
        });
    }

    #[test]
    fn parse_precedence_comparison_vs_arithmetic() {
        // a + 1 == b → Eq(Add(a, 1), b)
        assert!(matches!(parse_expr("a + 1 == b"), Expr::BinaryOp { op: BinaryOp::Eq, .. }));
    }

    #[test]
    fn parse_simple_string() {
        assert_eq!(parse_expr(r#""hello""#), Expr::StringLiteral(StringExpr::Simple("hello".to_string())));
    }

    #[test]
    fn parse_interpolated_string() {
        let expr = parse_expr(r#""hello {name}""#);
        assert!(matches!(expr, Expr::StringLiteral(StringExpr::Interpolated(_))));
        if let Expr::StringLiteral(StringExpr::Interpolated(parts)) = expr {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], StringPart::Literal("hello ".to_string()));
            assert!(matches!(&parts[1], StringPart::Expr(Expr::Identifier(n)) if n == "name"));
        }
    }

    #[test]
    fn parse_raw_string() {
        assert_eq!(parse_expr(r#"raw"no {interp}""#), Expr::StringLiteral(StringExpr::Simple("no {interp}".to_string())));
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(parse_expr(r#""""#), Expr::StringLiteral(StringExpr::Simple(String::new())));
    }

    // --- Pipeline tests ---

    #[test]
    fn parse_simple_pipeline() {
        let expr = parse_expr("users | count");
        assert!(matches!(expr, Expr::Pipeline { .. }));
        if let Expr::Pipeline { source, steps } = expr {
            assert!(matches!(*source, Expr::Identifier(ref n) if n == "users"));
            assert_eq!(steps.len(), 1);
            assert!(matches!(steps[0], PipelineStep::Count));
        }
    }

    #[test]
    fn parse_filter_where() {
        let expr = parse_expr("users | filter where .active");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Filter { .. }));
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_map_to() {
        let expr = parse_expr("users | map to .name");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Map { .. }));
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_sort_by_ascending() {
        let expr = parse_expr("users | sort by .name ascending");
        if let Expr::Pipeline { steps, .. } = expr {
            if let PipelineStep::Sort { descending, .. } = &steps[0] {
                assert!(!descending);
            } else { panic!("Expected Sort"); }
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_sort_by_descending() {
        let expr = parse_expr("users | sort by .name descending");
        if let Expr::Pipeline { steps, .. } = expr {
            if let PipelineStep::Sort { descending, .. } = &steps[0] {
                assert!(descending);
            } else { panic!("Expected Sort"); }
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_multi_step_pipeline() {
        let expr = parse_expr("users\n  | filter where .active\n  | map to .name\n  | sort by .name");
        if let Expr::Pipeline { steps, .. } = expr {
            assert_eq!(steps.len(), 3);
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_pipeline_expr_fallback() {
        let expr = parse_expr("request | api_pipeline");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Expr(_)));
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_or_default_pipeline() {
        let expr = parse_expr("x | or default 1");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::OrDefault { .. }));
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_take_first() {
        let expr = parse_expr("users | take first 10");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Take { kind: TakeKind::First, .. }));
        } else { panic!("Expected Pipeline"); }
    }

    #[test]
    fn parse_pipeline_flatten_unique() {
        let expr = parse_expr("items | flatten | unique");
        if let Expr::Pipeline { steps, .. } = expr {
            assert_eq!(steps.len(), 2);
            assert!(matches!(steps[0], PipelineStep::Flatten));
            assert!(matches!(steps[1], PipelineStep::Unique));
        } else { panic!("Expected Pipeline"); }
    }
}

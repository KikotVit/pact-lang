use crate::lexer::{Token, TokenKind};
use crate::parser::ast::*;
use crate::parser::errors::ParseError;

struct BlockContext {
    kind: &'static str,
    line: usize,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
    block_stack: Vec<BlockContext>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: &str) -> Self {
        Parser {
            tokens,
            pos: 0,
            source: source.to_string(),
            block_stack: Vec::new(),
        }
    }

    pub fn parse(&mut self) -> Result<Program, Vec<ParseError>> {
        self.skip_newlines();
        let mut statements = Vec::new();
        while !self.at_eof() {
            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(e) => return Err(vec![e]),
            }
            self.skip_newlines();
        }
        Ok(Program { statements })
    }

    // --- Token navigation ---

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn current_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            &self.tokens[idx].kind
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

    fn at_string(&self) -> bool {
        matches!(
            self.current_kind(),
            TokenKind::RawStringLiteral(_) | TokenKind::StringStart
        )
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
            let hint = match kind {
                TokenKind::LBrace => Some("Blocks and struct literals start with '{'"),
                TokenKind::RBrace => Some("Did you forget a closing '}'?"),
                TokenKind::LParen => Some("Function arguments start with '('"),
                TokenKind::RParen => Some("Did you forget a closing ')'?"),
                TokenKind::Colon => Some("Types use 'name: Type' syntax"),
                TokenKind::Assign => Some("Assignment uses '='"),
                TokenKind::FatArrow => Some("Match arms use '=>' syntax: pattern => body"),
                _ => None,
            };
            Err(self.error(
                &format!("Expected {}, found {}", kind, self.current_kind()),
                hint,
            ))
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.current_kind().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(
                &format!("Expected identifier, found {}", self.current_kind()),
                Some("Identifiers are names like: x, myVar, user_name"),
            )),
        }
    }

    fn expect_int(&mut self) -> Result<i64, ParseError> {
        match self.current_kind().clone() {
            TokenKind::IntLiteral(n) => {
                self.advance();
                Ok(n)
            }
            _ => Err(self.error(
                &format!("Expected integer, found {}", self.current_kind()),
                None,
            )),
        }
    }

    fn expect_string_literal(&mut self) -> Result<String, ParseError> {
        match self.current_kind().clone() {
            TokenKind::RawStringLiteral(s) => {
                self.advance();
                Ok(s)
            }
            _ => Err(self.error(
                &format!("Expected string, found {}", self.current_kind()),
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
        let source_line = self
            .source
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

    fn push_block(&mut self, kind: &'static str) {
        self.block_stack.push(BlockContext {
            kind,
            line: self.current().span.line,
        });
    }

    fn expect_closing_brace(&mut self) -> Result<(), ParseError> {
        if self.eat(&TokenKind::RBrace) {
            self.block_stack.pop();
            Ok(())
        } else {
            let context = self.block_stack.last();
            let msg = if let Some(ctx) = context {
                format!(
                    "Unclosed '{}' block (opened at line {}). Expected '}}', found {}",
                    ctx.kind,
                    ctx.line,
                    self.current_kind()
                )
            } else {
                format!("Expected '}}', found {}", self.current_kind())
            };
            Err(self.error(&msg, None))
        }
    }

    // --- Expression parsing ---

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Result<Expr, ParseError> {
        let source = self.parse_or()?;
        // Check for pipe, including across newlines
        if !self.at(&TokenKind::Pipe) && !self.is_pipe_after_newlines() {
            return Ok(source);
        }
        let mut steps = Vec::new();
        loop {
            // Skip newlines before checking for pipe continuation
            if self.at(&TokenKind::Newline) && self.is_pipe_after_newlines() {
                self.skip_newlines();
            }
            if !self.eat(&TokenKind::Pipe) {
                break;
            }
            self.skip_newlines();
            let step = self.parse_pipeline_step()?;
            steps.push(step);
        }
        Ok(Expr::Pipeline {
            source: Box::new(source),
            steps,
        })
    }

    /// Check if there's a Pipe token after skipping newlines (lookahead without consuming).
    fn is_pipe_after_newlines(&self) -> bool {
        let mut offset = 0;
        while *self.peek_at(offset) == TokenKind::Newline {
            offset += 1;
        }
        // Check current + offset for newlines starting from current position
        let mut pos = self.pos;
        while pos < self.tokens.len() && self.tokens[pos].kind == TokenKind::Newline {
            pos += 1;
        }
        pos < self.tokens.len() && self.tokens[pos].kind == TokenKind::Pipe
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
                    let descending =
                        !self.eat_contextual("ascending") && self.eat_contextual("descending");
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
                            &format!(
                                "Expected 'first' or 'last' after 'take', found {}",
                                self.current_kind()
                            ),
                            Some("Syntax: | take first N  or  | take last N"),
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
                    if self.eat_contextual("success") {
                        Ok(PipelineStep::ExpectSuccess)
                    } else if self.eat_contextual("one") {
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
                            &format!(
                                "Expected 'success', 'one', or 'any' after 'expect', found {}",
                                self.current_kind()
                            ),
                            Some("Syntax: | expect one or raise Error  |  | expect any or raise Error  |  | expect success"),
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
                "chars" => {
                    self.advance();
                    Ok(PipelineStep::Chars)
                }
                "on" => {
                    self.advance();
                    if self.eat_contextual("success") {
                        self.expect(&TokenKind::Colon)?;
                        let body = self.parse_or()?;
                        Ok(PipelineStep::OnSuccess { body })
                    } else {
                        let variant = self.expect_identifier()?;
                        let guard = if self.eat_contextual("where") {
                            Some(self.parse_or()?)
                        } else {
                            None
                        };
                        self.expect(&TokenKind::Colon)?;
                        let body = self.parse_or()?;
                        Ok(PipelineStep::OnError {
                            variant,
                            guard,
                            body,
                        })
                    }
                }
                "validate" => {
                    self.advance();
                    self.expect(&TokenKind::As)?;
                    let type_name = self.expect_identifier()?;
                    Ok(PipelineStep::ValidateAs { type_name })
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
        Err(self.error(
            &format!("Expected '{}', found {}", word, self.current_kind()),
            None,
        ))
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
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.at(&TokenKind::And) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.at(&TokenKind::Not) {
            self.advance();
            let operand = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
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
                return Ok(Expr::Is {
                    expr: Box::new(left),
                    type_name,
                });
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
        Ok(Expr::BinaryOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        })
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
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
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
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.at(&TokenKind::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            });
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
                    let call_span = self.current().span.clone();
                    self.advance(); // consume '('
                    let args = self.parse_args_list()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Expr::FnCall {
                        callee: Box::new(expr),
                        args,
                        span: Some(call_span),
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
            TokenKind::Identifier(ref name) if name == "respond" => {
                self.advance();
                let status = self.parse_or()?;
                self.expect_contextual("with")?;
                let body = self.parse_or()?;
                let content_type = if self.eat(&TokenKind::As) {
                    match self.current().kind {
                        TokenKind::RawStringLiteral(ref s) => {
                            let ct = s.clone();
                            self.advance();
                            Some(ct)
                        }
                        TokenKind::StringStart => {
                            self.advance(); // consume StringStart
                            if let TokenKind::StringFragment(text) = self.current_kind().clone() {
                                self.advance();
                                self.expect(&TokenKind::StringEnd)?;
                                Some(text)
                            } else if self.at(&TokenKind::StringEnd) {
                                self.advance();
                                Some(String::new())
                            } else {
                                return Err(self.error(
                                    "Expected string after 'as' in respond",
                                    Some("Example: respond 200 with body as \"text/html\""),
                                ));
                            }
                        }
                        _ => {
                            return Err(self.error(
                                "Expected string after 'as' in respond",
                                Some("Example: respond 200 with body as \"text/html\""),
                            ))
                        }
                    }
                } else {
                    None
                };
                Ok(Expr::Respond {
                    status: Box::new(status),
                    body: Box::new(body),
                    content_type,
                })
            }
            TokenKind::Identifier(ref name) if name == "send" => {
                self.advance();
                let body = self.parse_or()?;
                Ok(Expr::Send {
                    body: Box::new(body),
                })
            }
            TokenKind::Identifier(name) => {
                self.advance();
                // Struct literal: PascalCase followed by {
                if self.at(&TokenKind::LBrace)
                    && name.chars().next().is_some_and(|c| c.is_uppercase())
                {
                    return self.parse_struct_literal(Some(name));
                }
                Ok(Expr::Identifier(name))
            }
            TokenKind::LBrace => {
                if self.is_struct_literal_start() {
                    self.parse_struct_literal(None)
                } else {
                    self.push_block("block");
                    self.advance(); // consume {
                    let body = self.parse_block_body()?;
                    self.expect_closing_brace()?;
                    Ok(Expr::Block(body))
                }
            }
            TokenKind::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Dot => self.parse_dot_shorthand(),
            TokenKind::StringStart | TokenKind::RawStringLiteral(_) => self.parse_string_expr(),
            TokenKind::If => self.parse_if_expr(),
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::Ensure => {
                self.advance(); // consume 'ensure'
                let expr = self.parse_expression()?;
                Ok(Expr::Ensure(Box::new(expr)))
            }
            _ => self.fail(
                &format!("Expected expression, found {}", self.current_kind()),
                Some("Expressions: literal (42, \"text\", true), identifier, function call, if/match, { block }"),
            ),
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        self.advance(); // consume `if`
        let condition = self.parse_expression()?;
        self.push_block("if");
        self.expect(&TokenKind::LBrace)?;
        let then_body = self.parse_block_body()?;
        self.expect_closing_brace()?;
        let else_body = if self.eat(&TokenKind::Else) {
            self.push_block("else");
            self.expect(&TokenKind::LBrace)?;
            let body = self.parse_block_body()?;
            self.expect_closing_brace()?;
            Some(body)
        } else {
            None
        };
        Ok(Expr::If {
            condition: Box::new(condition),
            then_body,
            else_body,
        })
    }

    fn parse_match_expr(&mut self) -> Result<Expr, ParseError> {
        let span = self.current().span.clone();
        self.advance(); // consume `match`
        let subject = self.parse_expression()?;
        self.push_block("match");
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow)?;
            let body = self.parse_expression()?;
            arms.push(MatchArm { pattern, body });
            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }
        self.expect_closing_brace()?;
        Ok(Expr::Match {
            subject: Box::new(subject),
            arms,
            span: Some(span),
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.current_kind().clone() {
            TokenKind::Underscore => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(Pattern::Identifier(name))
            }
            TokenKind::IntLiteral(n) => {
                self.advance();
                Ok(Pattern::Literal(Expr::IntLiteral(n)))
            }
            TokenKind::BoolLiteral(b) => {
                self.advance();
                Ok(Pattern::Literal(Expr::BoolLiteral(b)))
            }
            TokenKind::StringStart | TokenKind::RawStringLiteral(_) => {
                let expr = self.parse_string_expr()?;
                Ok(Pattern::Literal(expr))
            }
            _ => self.fail(
                &format!("Expected match pattern, found {}", self.current_kind()),
                Some("Valid patterns: identifier (Admin), _ (wildcard), or literal (42, true, \"text\")"),
            ),
        }
    }

    fn parse_block_body(&mut self) -> Result<Vec<Statement>, ParseError> {
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    // --- Type expression parsing ---

    pub fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        if !matches!(self.current_kind(), TokenKind::Identifier(_)) {
            return Err(self.error(
                &format!("Expected type name, found {}", self.current_kind()),
                Some("Built-in types: Int, Float, String, Bool, List, Struct"),
            ));
        }
        let name = self.expect_identifier()?;

        let base = if name == "Optional" && self.at(&TokenKind::LAngle) {
            // Optional<T> → TypeExpr::Optional
            self.advance(); // consume <
            let inner = self.parse_type_expr()?;
            self.expect(&TokenKind::RAngle)?;
            TypeExpr::Optional(Box::new(inner))
        } else if self.at(&TokenKind::LAngle) {
            // Generic<A, B, ...>
            self.advance(); // consume <
            let mut args = Vec::new();
            args.push(self.parse_type_expr()?);
            while self.eat(&TokenKind::Comma) {
                args.push(self.parse_type_expr()?);
            }
            self.expect(&TokenKind::RAngle)?;
            TypeExpr::Generic { name, args }
        } else {
            TypeExpr::Named(name)
        };

        // Check for `or ErrorName` result type
        if self.at(&TokenKind::Or) {
            let mut errors = Vec::new();
            while self.eat(&TokenKind::Or) {
                let error_name = self.expect_identifier()?;
                errors.push(error_name);
            }
            Ok(TypeExpr::Result {
                ok: Box::new(base),
                errors,
            })
        } else {
            Ok(base)
        }
    }

    fn is_constraint_keyword(&self) -> bool {
        if let TokenKind::Identifier(s) = self.peek_at(1) {
            matches!(
                s.as_str(),
                "min" | "max" | "minlen" | "maxlen" | "format" | "pattern"
            )
        } else {
            false
        }
    }

    fn parse_field_constraints(&mut self) -> Result<Vec<Constraint>, ParseError> {
        let mut constraints = Vec::new();
        while self.at(&TokenKind::Pipe) && self.is_constraint_keyword() {
            self.advance(); // consume |
            let kw = self.expect_identifier()?;
            match kw.as_str() {
                "min" => {
                    let n = self.expect_int()?;
                    constraints.push(Constraint::Min(n));
                }
                "max" => {
                    let n = self.expect_int()?;
                    constraints.push(Constraint::Max(n));
                }
                "minlen" => {
                    let n = self.expect_int()?;
                    constraints.push(Constraint::MinLen(n as usize));
                }
                "maxlen" => {
                    let n = self.expect_int()?;
                    constraints.push(Constraint::MaxLen(n as usize));
                }
                "format" => {
                    let f = self.expect_identifier()?;
                    constraints.push(Constraint::Format(f));
                }
                "pattern" => {
                    let s = self.expect_string_literal()?;
                    constraints.push(Constraint::Pattern(s));
                }
                _ => {
                    return Err(self.error(
                        &format!("Unknown constraint '{}'", kw),
                        Some("Valid constraints: min, max, minlen, maxlen, format, pattern"),
                    ));
                }
            }
        }
        Ok(constraints)
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let stmt = match self.current_kind() {
            TokenKind::Let => self.parse_let_or_var(false)?,
            TokenKind::Var => self.parse_let_or_var(true)?,
            TokenKind::Return => self.parse_return()?,
            TokenKind::Use => self.parse_use()?,
            TokenKind::Fn => {
                return Err(self.error(
                    "Missing 'intent' block before function declaration",
                    Some("Write: intent \"description\" on the line before fn"),
                ));
            }
            TokenKind::Intent => {
                self.advance(); // consume intent
                let intent = self.parse_intent_string()?;
                self.skip_newlines();
                if self.at(&TokenKind::Route) {
                    self.parse_route_with_intent(intent)?
                } else if self.at(&TokenKind::Stream) {
                    self.parse_stream_with_intent(intent)?
                } else {
                    self.parse_fn_decl(Some(intent))?
                }
            }
            TokenKind::Route => {
                return Err(self.error(
                    "Missing 'intent' block before route declaration",
                    Some("Write: intent \"description\" on the line before route"),
                ));
            }
            TokenKind::Stream => {
                return Err(self.error(
                    "Missing 'intent' block before stream declaration",
                    Some("Write: intent \"description\" on the line before stream"),
                ));
            }
            TokenKind::App => self.parse_app()?,
            TokenKind::Type => self.parse_type_decl_stmt()?,
            TokenKind::Test => self.parse_test_block()?,
            TokenKind::Identifier(word) if word == "using" => {
                self.advance(); // consume "using"
                let name = self.expect_identifier()?;
                self.expect(&TokenKind::Assign)?;
                let value = self.parse_expression()?;
                Statement::Using { name, value }
            }
            TokenKind::Identifier(word) if word == "assert" => {
                self.advance(); // consume "assert"
                let expr = self.parse_expression()?;
                Statement::Assert(expr)
            }
            _ => {
                let expr = self.parse_expression()?;
                // Detect `x = value` or `x.field = value` — PACT doesn't have standalone assignment
                if self.at(&TokenKind::Assign) {
                    if let Expr::Identifier(name) = &expr {
                        return Err(self.error(
                            &format!("Unexpected '=' after '{}'. PACT variables are immutable by default", name),
                            Some("Declare variables with: let name: Type = value"),
                        ));
                    }
                    if matches!(&expr, Expr::FieldAccess { .. }) {
                        return Err(self.error(
                            "PACT structs are immutable. Field assignment is not supported",
                            Some("Create a new struct with the updated field: { ...old, field: new_value }"),
                        ));
                    }
                }
                Statement::Expression(expr)
            }
        };
        self.skip_newlines();
        Ok(stmt)
    }

    fn parse_let_or_var(&mut self, mutable: bool) -> Result<Statement, ParseError> {
        let keyword = if mutable { "var" } else { "let" };
        let span = self.current().span.clone();
        self.advance(); // consume let/var
        let name = self.expect_identifier()?;
        if self.at(&TokenKind::Assign) {
            return Err(self.error(
                &format!(
                    "Missing type annotation. PACT requires: {} {}: Type = value",
                    keyword, name
                ),
                Some(&format!(
                    "Example: {} {}: Int = 42  or  {} {}: String = \"hello\"",
                    keyword, name, keyword, name
                )),
            ));
        }
        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_expr()?;
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_expression()?;
        Ok(Statement::Let {
            name,
            mutable,
            type_ann,
            value,
            span: Some(span),
        })
    }

    fn parse_return(&mut self) -> Result<Statement, ParseError> {
        let span = self.current().span.clone();
        self.advance(); // consume return
        // If next is newline/eof/rbrace → return with no value
        if self.at(&TokenKind::Newline) || self.at_eof() || self.at(&TokenKind::RBrace) {
            return Ok(Statement::Return {
                value: None,
                condition: None,
                span: Some(span),
            });
        }
        let value = self.parse_expression()?;
        // Check for trailing `if` condition
        let condition = if self.eat(&TokenKind::If) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        Ok(Statement::Return {
            value: Some(value),
            condition,
            span: Some(span),
        })
    }

    fn parse_use(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume use
        let mut path = Vec::new();
        path.push(self.expect_identifier()?);
        while self.eat(&TokenKind::Dot) {
            // Allow `*` as the last path component for wildcard imports
            if self.eat(&TokenKind::Star) {
                path.push("*".to_string());
                break;
            }
            path.push(self.expect_identifier()?);
        }
        Ok(Statement::Use { path })
    }

    fn parse_test_block(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume `test`
        let name = self.parse_intent_string()?; // reuse intent string parser for test name
        self.push_block("test");
        self.expect(&TokenKind::LBrace)?;
        let body = self.parse_block_body()?;
        self.expect_closing_brace()?;
        Ok(Statement::TestBlock { name, body })
    }

    fn parse_fn_decl(&mut self, intent: Option<String>) -> Result<Statement, ParseError> {
        let span = self.current().span.clone();
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_identifier()?;

        // Parameters
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;

        // Optional return type: -> TypeExpr
        let mut return_type = None;
        let mut error_types = Vec::new();
        if self.eat(&TokenKind::Arrow) {
            let ty = self.parse_type_expr()?;
            // If the type is a Result, extract ok type and errors
            match ty {
                TypeExpr::Result { ok, errors } => {
                    return_type = Some(*ok);
                    error_types = errors;
                }
                other => {
                    return_type = Some(other);
                }
            }
        }

        // Optional needs effect1, effect2
        self.skip_newlines();
        let mut effects = Vec::new();
        if self.eat(&TokenKind::Needs) {
            // Parse comma-separated identifiers until we hit `{` (start of function body)
            if !self.at(&TokenKind::LBrace) {
                effects.push(self.expect_identifier()?);
                while self.eat(&TokenKind::Comma) {
                    if self.at(&TokenKind::LBrace) {
                        break;
                    }
                    effects.push(self.expect_identifier()?);
                }
            }
        }

        // Body
        self.skip_newlines();
        self.push_block("function");
        if !self.at(&TokenKind::LBrace) {
            return self.fail(
                "Function body must start with '{'",
                Some(&format!(
                    "Found {} instead of opening brace",
                    self.current_kind()
                )),
            );
        }
        self.advance(); // consume `{`
        let body = self.parse_block_body()?;
        self.expect_closing_brace()?;

        Ok(Statement::FnDecl {
            name,
            intent,
            params,
            return_type,
            error_types,
            effects,
            body,
            span: Some(span),
        })
    }

    fn parse_intent_string(&mut self) -> Result<String, ParseError> {
        match self.current_kind().clone() {
            TokenKind::RawStringLiteral(content) => {
                self.advance();
                Ok(content)
            }
            TokenKind::StringStart => {
                self.advance(); // consume StringStart
                // Expect a single string fragment (intent strings are not interpolated)
                if let TokenKind::StringFragment(text) = self.current_kind().clone() {
                    self.advance();
                    self.expect(&TokenKind::StringEnd)?;
                    Ok(text)
                } else if self.at(&TokenKind::StringEnd) {
                    self.advance();
                    Ok(String::new())
                } else {
                    self.fail(
                        "Expected string content for intent",
                        Some("Syntax: intent \"description of what this does\""),
                    )
                }
            }
            _ => self.fail(
                &format!(
                    "Expected string after 'intent', found {}",
                    self.current_kind()
                ),
                Some("Syntax: intent \"description of what this function does\""),
            ),
        }
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.at(&TokenKind::RParen) {
            return Ok(params);
        }
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_expr()?;
        params.push(Param { name, type_ann });
        while self.eat(&TokenKind::Comma) {
            if self.at(&TokenKind::RParen) {
                break;
            } // trailing comma
            let name = self.expect_identifier()?;
            self.expect(&TokenKind::Colon)?;
            let type_ann = self.parse_type_expr()?;
            params.push(Param { name, type_ann });
        }
        Ok(params)
    }

    fn parse_type_decl_stmt(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume `type`
        let name = self.expect_identifier()?;
        self.skip_newlines(); // allow multi-line type declarations

        if self.at(&TokenKind::LBrace) {
            // Struct: type Name { field: Type, ... }
            self.push_block("type");
            self.advance(); // consume {
            self.skip_newlines();
            let mut fields = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let type_ann = self.parse_type_expr()?;
                let constraints = self.parse_field_constraints()?;
                fields.push(Field {
                    name: field_name,
                    type_ann,
                    constraints,
                });
                self.eat(&TokenKind::Comma);
                self.skip_newlines();
            }
            self.expect_closing_brace()?;
            Ok(Statement::TypeDecl(TypeDecl::Struct { name, fields }))
        } else if self.eat(&TokenKind::Assign) {
            // Union: type Name = Variant1 | Variant2 { field: Type }
            self.skip_newlines();
            let mut variants = Vec::new();
            variants.push(self.parse_union_variant()?);
            while self.eat(&TokenKind::Pipe) {
                self.skip_newlines();
                variants.push(self.parse_union_variant()?);
            }
            Ok(Statement::TypeDecl(TypeDecl::Union { name, variants }))
        } else {
            self.fail(
                &format!(
                    "Expected '{{' or '=' after type name, found {}",
                    self.current_kind()
                ),
                Some("Struct: type Name { field: Type }  |  Union: type Name = A | B | C"),
            )
        }
    }

    fn parse_union_variant(&mut self) -> Result<UnionVariant, ParseError> {
        let name = self.expect_identifier()?;
        let fields = if self.at(&TokenKind::LBrace) {
            self.push_block("type");
            self.advance(); // consume {
            self.skip_newlines();
            let mut fs = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let type_ann = self.parse_type_expr()?;
                let constraints = self.parse_field_constraints()?;
                fs.push(Field {
                    name: field_name,
                    type_ann,
                    constraints,
                });
                self.eat(&TokenKind::Comma);
                self.skip_newlines();
            }
            self.expect_closing_brace()?;
            Some(fs)
        } else {
            None
        };
        Ok(UnionVariant { name, fields })
    }

    fn is_struct_literal_start(&self) -> bool {
        // We're looking at `{`. Check if the content looks like struct fields:
        // { identifier : ... or { ... (spread)
        if !self.at(&TokenKind::LBrace) {
            return false;
        }
        // peek past { (and any newlines)
        let mut offset = 1;
        while *self.peek_at(offset) == TokenKind::Newline {
            offset += 1;
        }
        // Check for spread
        if *self.peek_at(offset) == TokenKind::Spread {
            return true;
        }
        // Check for identifier followed by colon
        if let TokenKind::Identifier(_) = self.peek_at(offset) {
            if *self.peek_at(offset + 1) == TokenKind::Colon {
                return true;
            }
        }
        // Check for string key followed by colon (e.g. { "url": value })
        match self.peek_at(offset) {
            TokenKind::RawStringLiteral(_) => {
                if *self.peek_at(offset + 1) == TokenKind::Colon {
                    return true;
                }
            }
            TokenKind::StringStart => {
                // Skip past StringStart, StringFragment(s), StringEnd to find colon
                let mut off = offset + 1;
                while !matches!(self.peek_at(off), TokenKind::StringEnd | TokenKind::Eof) {
                    off += 1;
                }
                // off is at StringEnd, check off+1 for colon
                if *self.peek_at(off) == TokenKind::StringEnd
                    && *self.peek_at(off + 1) == TokenKind::Colon
                {
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn parse_struct_literal(&mut self, name: Option<String>) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.at(&TokenKind::Spread) {
                self.advance(); // consume ...
                let expr = self.parse_expression()?;
                fields.push(StructField::Spread(expr));
            } else if self.at_string() {
                // String key in struct literal: "key": value
                let key_expr = self.parse_string_expr()?;
                let field_name = match key_expr {
                    Expr::StringLiteral(StringExpr::Simple(s)) => s,
                    _ => {
                        return self.fail(
                            "Expected simple string key in struct literal",
                            Some("Struct fields use: { name: value, age: 30 }"),
                        );
                    }
                };
                self.expect(&TokenKind::Colon)?;
                let value = self.parse_expression()?;
                fields.push(StructField::Named {
                    name: field_name,
                    value,
                });
            } else {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let value = self.parse_expression()?;
                fields.push(StructField::Named {
                    name: field_name,
                    value,
                });
            }
            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Expr::StructLiteral { name, fields })
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
            _ => self.fail(
                "Expected string",
                Some("Strings use double quotes: \"text\""),
            ),
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
                _ => {
                    return self.fail(
                        "Unexpected token in string",
                        Some("String interpolation uses: \"Hello {name}\""),
                    );
                }
            }
        }
        Ok(parts)
    }

    fn parse_route_path(&mut self) -> Result<String, ParseError> {
        match self.current_kind().clone() {
            TokenKind::RawStringLiteral(content) => {
                self.advance();
                Ok(content)
            }
            TokenKind::StringStart => {
                self.advance(); // consume StringStart
                let mut path = String::new();
                loop {
                    match self.current_kind().clone() {
                        TokenKind::StringFragment(text) => {
                            self.advance();
                            path.push_str(&text);
                        }
                        TokenKind::InterpolationStart => {
                            self.advance(); // consume ${
                            // Collect the identifier as a path parameter template
                            let param = self.expect_identifier()?;
                            path.push('{');
                            path.push_str(&param);
                            path.push('}');
                            self.expect(&TokenKind::InterpolationEnd)?;
                        }
                        TokenKind::StringEnd => {
                            self.advance();
                            break;
                        }
                        _ => {
                            return self.fail(
                                &format!("Unexpected token in route path: {}", self.current_kind()),
                                Some("Route path must be a string like \"/users/{id}\""),
                            );
                        }
                    }
                }
                Ok(path)
            }
            _ => self.fail(
                &format!(
                    "Expected string for route path, found {}",
                    self.current_kind()
                ),
                Some("Syntax: route GET \"/path\" { ... }"),
            ),
        }
    }

    fn parse_route_with_intent(&mut self, intent: String) -> Result<Statement, ParseError> {
        self.advance(); // consume `route`
        let method = self.expect_identifier()?;
        let path = self.parse_route_path()?;
        self.push_block("route");
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        // Optional: needs
        let mut effects = Vec::new();
        if self.eat(&TokenKind::Needs)
            && !self.at(&TokenKind::LBrace)
            && !self.at(&TokenKind::Newline)
        {
            effects.push(self.expect_identifier()?);
            while self.eat(&TokenKind::Comma) {
                if self.at(&TokenKind::LBrace) || self.at(&TokenKind::Newline) {
                    break;
                }
                effects.push(self.expect_identifier()?);
            }
        }
        self.skip_newlines();

        // Body
        let body = self.parse_block_body()?;
        self.expect_closing_brace()?;

        Ok(Statement::Route {
            method,
            path,
            intent,
            effects,
            body,
        })
    }

    fn parse_stream_with_intent(&mut self, intent: String) -> Result<Statement, ParseError> {
        self.advance(); // consume `stream`
        let method = self.expect_identifier()?;
        let path = self.parse_route_path()?;
        self.push_block("stream");
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        // Optional: needs
        let mut effects = Vec::new();
        if self.eat(&TokenKind::Needs)
            && !self.at(&TokenKind::LBrace)
            && !self.at(&TokenKind::Newline)
        {
            effects.push(self.expect_identifier()?);
            while self.eat(&TokenKind::Comma) {
                if self.at(&TokenKind::LBrace) || self.at(&TokenKind::Newline) {
                    break;
                }
                effects.push(self.expect_identifier()?);
            }
        }
        self.skip_newlines();

        // Body
        let body = self.parse_block_body()?;
        self.expect_closing_brace()?;

        Ok(Statement::Stream {
            method,
            path,
            intent,
            effects,
            body,
        })
    }

    fn parse_app(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume `app`
        if self.at(&TokenKind::LBrace) {
            return Err(self.error(
                "app requires a name, e.g.: app MyService { port: 8080 }",
                None,
            ));
        }
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut port: Option<u16> = None;
        let mut db_url: Option<String> = None;

        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let key = self.expect_identifier()?;
            self.expect(&TokenKind::Colon)?;

            match key.as_str() {
                "port" => {
                    port = Some(match self.current_kind().clone() {
                        TokenKind::IntLiteral(n) => {
                            self.advance();
                            n as u16
                        }
                        _ => {
                            return self.fail(
                                &format!(
                                    "Expected integer for port, found {}",
                                    self.current_kind()
                                ),
                                Some("Syntax: app Name { port: 8080 }"),
                            );
                        }
                    });
                }
                "db" => {
                    let expr = self.parse_string_expr().map_err(|_| {
                        self.error(
                            &format!("Expected string for db, found {}", self.current_kind()),
                            Some("Syntax: app Name { port: 8080, db: \"sqlite://data.db\" }"),
                        )
                    })?;
                    match expr {
                        Expr::StringLiteral(StringExpr::Simple(s)) => {
                            db_url = Some(s);
                        }
                        _ => {
                            return self.fail(
                                "db value must be a plain string (no interpolation)",
                                Some("Syntax: app Name { port: 8080, db: \"sqlite://data.db\" }"),
                            );
                        }
                    }
                }
                other => {
                    return self.fail(
                        &format!("Unknown app property '{}'", other),
                        Some("Known properties: port, db"),
                    );
                }
            }

            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }

        let port = port.ok_or_else(|| {
            self.error(
                "app declaration requires 'port'",
                Some("Syntax: app Name { port: 8080 }"),
            )
        })?;

        self.expect(&TokenKind::RBrace)?;

        Ok(Statement::App { name, port, db_url })
    }

    fn parse_dot_shorthand(&mut self) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::Dot)?; // consume initial '.'
        // Bare `.` (not followed by identifier) means "the current value" (_it)
        if let TokenKind::Identifier(_) = self.current_kind() {
            let first = self.expect_identifier()?;
            let mut parts = vec![first];
            while self.eat(&TokenKind::Dot) {
                let name = self.expect_identifier()?;
                parts.push(name);
            }
            Ok(Expr::DotShorthand(parts))
        } else {
            Ok(Expr::DotShorthand(vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

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
        let expr = parse_expr("foo()");
        if let Expr::FnCall { callee, args, .. } = &expr {
            assert_eq!(**callee, Expr::Identifier("foo".to_string()));
            assert!(args.is_empty());
        } else {
            panic!("Expected FnCall");
        }
    }

    #[test]
    fn parse_fn_call_with_args() {
        let expr = parse_expr("add(1, 2)");
        if let Expr::FnCall { callee, args, .. } = &expr {
            assert_eq!(**callee, Expr::Identifier("add".to_string()));
            assert_eq!(args, &[Expr::IntLiteral(1), Expr::IntLiteral(2)]);
        } else {
            panic!("Expected FnCall");
        }
    }

    #[test]
    fn parse_method_call() {
        let expr = parse_expr("db.query(x)");
        assert!(matches!(
            expr,
            Expr::FnCall { ref callee, ref args, .. } if matches!(**callee, Expr::FieldAccess { .. }) && args.len() == 1
        ));
    }

    #[test]
    fn parse_error_propagation() {
        let expr = parse_expr("foo()?");
        if let Expr::ErrorPropagation(inner) = &expr {
            assert!(matches!(&**inner, Expr::FnCall { callee, args, .. }
                if matches!(&**callee, Expr::Identifier(n) if n == "foo") && args.is_empty()
            ));
        } else {
            panic!("Expected ErrorPropagation");
        }
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
        assert_eq!(
            parse_expr("1 + 2"),
            Expr::BinaryOp {
                left: Box::new(Expr::IntLiteral(1)),
                op: BinaryOp::Add,
                right: Box::new(Expr::IntLiteral(2)),
            }
        );
    }

    #[test]
    fn parse_subtraction() {
        assert_eq!(
            parse_expr("a - b"),
            Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::Sub,
                right: Box::new(Expr::Identifier("b".to_string())),
            }
        );
    }

    #[test]
    fn parse_multiplication_precedence() {
        // 1 + 2 * 3 → Add(1, Mul(2, 3))
        assert_eq!(
            parse_expr("1 + 2 * 3"),
            Expr::BinaryOp {
                left: Box::new(Expr::IntLiteral(1)),
                op: BinaryOp::Add,
                right: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::IntLiteral(2)),
                    op: BinaryOp::Mul,
                    right: Box::new(Expr::IntLiteral(3)),
                }),
            }
        );
    }

    #[test]
    fn parse_unary_negation() {
        assert_eq!(
            parse_expr("-42"),
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(Expr::IntLiteral(42))
            }
        );
    }

    #[test]
    fn parse_division() {
        assert_eq!(
            parse_expr("a / b"),
            Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::Div,
                right: Box::new(Expr::Identifier("b".to_string())),
            }
        );
    }

    #[test]
    fn parse_comparison_eq() {
        assert_eq!(
            parse_expr("a == b"),
            Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::Identifier("b".to_string())),
            }
        );
    }

    #[test]
    fn parse_comparison_not_eq() {
        assert_eq!(
            parse_expr("a != b"),
            Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::NotEq,
                right: Box::new(Expr::Identifier("b".to_string())),
            }
        );
    }

    #[test]
    fn parse_less_than() {
        assert_eq!(
            parse_expr("a < b"),
            Expr::BinaryOp {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::Lt,
                right: Box::new(Expr::Identifier("b".to_string())),
            }
        );
    }

    #[test]
    fn parse_and_or() {
        // a and b or c → Or(And(a, b), c)  (and binds tighter than or)
        assert_eq!(
            parse_expr("a and b or c"),
            Expr::BinaryOp {
                left: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::Identifier("a".to_string())),
                    op: BinaryOp::And,
                    right: Box::new(Expr::Identifier("b".to_string())),
                }),
                op: BinaryOp::Or,
                right: Box::new(Expr::Identifier("c".to_string())),
            }
        );
    }

    #[test]
    fn parse_not() {
        assert_eq!(
            parse_expr("not x"),
            Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::Identifier("x".to_string()))
            }
        );
    }

    #[test]
    fn parse_is_expr() {
        assert_eq!(
            parse_expr("result is NotFound"),
            Expr::Is {
                expr: Box::new(Expr::Identifier("result".to_string())),
                type_name: "NotFound".to_string(),
            }
        );
    }

    #[test]
    fn parse_precedence_comparison_vs_arithmetic() {
        // a + 1 == b → Eq(Add(a, 1), b)
        assert!(matches!(
            parse_expr("a + 1 == b"),
            Expr::BinaryOp {
                op: BinaryOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn parse_simple_string() {
        assert_eq!(
            parse_expr(r#""hello""#),
            Expr::StringLiteral(StringExpr::Simple("hello".to_string()))
        );
    }

    #[test]
    fn parse_interpolated_string() {
        let expr = parse_expr(r#""hello {name}""#);
        assert!(matches!(
            expr,
            Expr::StringLiteral(StringExpr::Interpolated(_))
        ));
        if let Expr::StringLiteral(StringExpr::Interpolated(parts)) = expr {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], StringPart::Literal("hello ".to_string()));
            assert!(matches!(&parts[1], StringPart::Expr(Expr::Identifier(n)) if n == "name"));
        }
    }

    #[test]
    fn parse_raw_string() {
        assert_eq!(
            parse_expr(r#"raw"no {interp}""#),
            Expr::StringLiteral(StringExpr::Simple("no {interp}".to_string()))
        );
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(
            parse_expr(r#""""#),
            Expr::StringLiteral(StringExpr::Simple(String::new()))
        );
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
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_map_to() {
        let expr = parse_expr("users | map to .name");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Map { .. }));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_sort_by_ascending() {
        let expr = parse_expr("users | sort by .name ascending");
        if let Expr::Pipeline { steps, .. } = expr {
            if let PipelineStep::Sort { descending, .. } = &steps[0] {
                assert!(!descending);
            } else {
                panic!("Expected Sort");
            }
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_sort_by_descending() {
        let expr = parse_expr("users | sort by .name descending");
        if let Expr::Pipeline { steps, .. } = expr {
            if let PipelineStep::Sort { descending, .. } = &steps[0] {
                assert!(descending);
            } else {
                panic!("Expected Sort");
            }
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_multi_step_pipeline() {
        let expr =
            parse_expr("users\n  | filter where .active\n  | map to .name\n  | sort by .name");
        if let Expr::Pipeline { steps, .. } = expr {
            assert_eq!(steps.len(), 3);
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_pipeline_expr_fallback() {
        let expr = parse_expr("request | api_pipeline");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::Expr(_)));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_or_default_pipeline() {
        let expr = parse_expr("x | or default 1");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(&steps[0], PipelineStep::OrDefault { .. }));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_take_first() {
        let expr = parse_expr("users | take first 10");
        if let Expr::Pipeline { steps, .. } = expr {
            assert!(matches!(
                &steps[0],
                PipelineStep::Take {
                    kind: TakeKind::First,
                    ..
                }
            ));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_pipeline_flatten_unique() {
        let expr = parse_expr("items | flatten | unique");
        if let Expr::Pipeline { steps, .. } = expr {
            assert_eq!(steps.len(), 2);
            assert!(matches!(steps[0], PipelineStep::Flatten));
            assert!(matches!(steps[1], PipelineStep::Unique));
        } else {
            panic!("Expected Pipeline");
        }
    }

    // --- Control flow tests ---

    #[test]
    fn parse_if_else() {
        let expr = parse_expr("if age >= 18 {\n  true\n} else {\n  false\n}");
        assert!(matches!(expr, Expr::If { .. }));
        if let Expr::If { else_body, .. } = &expr {
            assert!(else_body.is_some());
        }
    }

    #[test]
    fn parse_if_without_else() {
        let expr = parse_expr("if x {\n  1\n}");
        assert!(matches!(expr, Expr::If { .. }));
        if let Expr::If { else_body, .. } = &expr {
            assert!(else_body.is_none());
        }
    }

    #[test]
    fn parse_match_expression() {
        let input = "match role {\n  Admin => true,\n  _ => false,\n}";
        let expr = parse_expr(input);
        assert!(matches!(expr, Expr::Match { .. }));
        if let Expr::Match { arms, .. } = &expr {
            assert_eq!(arms.len(), 2);
            assert!(matches!(&arms[0].pattern, Pattern::Identifier(n) if n == "Admin"));
            assert!(matches!(&arms[1].pattern, Pattern::Wildcard));
        }
    }

    #[test]
    fn parse_match_with_literals() {
        let input = "match x {\n  42 => true,\n  _ => false,\n}";
        let expr = parse_expr(input);
        if let Expr::Match { arms, .. } = &expr {
            assert!(matches!(
                &arms[0].pattern,
                Pattern::Literal(Expr::IntLiteral(42))
            ));
        } else {
            panic!("Expected Match");
        }
    }

    // --- Type expression tests ---

    #[test]
    fn parse_simple_type() {
        let mut lexer = Lexer::new("Int");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, "Int");
        assert_eq!(
            parser.parse_type_expr().unwrap(),
            TypeExpr::Named("Int".to_string())
        );
    }

    #[test]
    fn parse_generic_type() {
        let mut lexer = Lexer::new("List<User>");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, "List<User>");
        assert_eq!(
            parser.parse_type_expr().unwrap(),
            TypeExpr::Generic {
                name: "List".to_string(),
                args: vec![TypeExpr::Named("User".to_string())],
            }
        );
    }

    #[test]
    fn parse_optional_type() {
        let mut lexer = Lexer::new("Optional<String>");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, "Optional<String>");
        assert_eq!(
            parser.parse_type_expr().unwrap(),
            TypeExpr::Optional(Box::new(TypeExpr::Named("String".to_string())))
        );
    }

    #[test]
    fn parse_result_type() {
        let mut lexer = Lexer::new("User or NotFound or DbError");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, "User or NotFound or DbError");
        assert_eq!(
            parser.parse_type_expr().unwrap(),
            TypeExpr::Result {
                ok: Box::new(TypeExpr::Named("User".to_string())),
                errors: vec!["NotFound".to_string(), "DbError".to_string()],
            }
        );
    }

    #[test]
    fn parse_nested_generic_type() {
        let mut lexer = Lexer::new("Map<String, List<Int>>");
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, "Map<String, List<Int>>");
        let ty = parser.parse_type_expr().unwrap();
        assert!(
            matches!(ty, TypeExpr::Generic { ref name, ref args } if name == "Map" && args.len() == 2)
        );
    }

    // --- Statement tests ---

    #[test]
    fn parse_let_statement() {
        let prog = parse_program(r#"let name: String = "Vitalii""#);
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(
            &prog.statements[0],
            Statement::Let { mutable: false, .. }
        ));
    }

    #[test]
    fn parse_var_statement() {
        let prog = parse_program("var counter: Int = 0");
        assert!(matches!(
            &prog.statements[0],
            Statement::Let { mutable: true, .. }
        ));
    }

    #[test]
    fn parse_return_statement() {
        let prog = parse_program("return 42");
        assert!(matches!(
            &prog.statements[0],
            Statement::Return {
                value: Some(_),
                condition: None,
                ..
            }
        ));
    }

    #[test]
    fn parse_return_if() {
        let prog = parse_program("return NotFound if not user.active");
        assert!(matches!(
            &prog.statements[0],
            Statement::Return {
                value: Some(_),
                condition: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn parse_use_statement() {
        let prog = parse_program("use models.user.User");
        if let Statement::Use { path } = &prog.statements[0] {
            assert_eq!(path, &["models", "user", "User"]);
        } else {
            panic!("Expected Use");
        }
    }

    #[test]
    fn parse_ensure_as_expression_statement() {
        let prog = parse_program("ensure amount > 0");
        assert!(matches!(
            &prog.statements[0],
            Statement::Expression(Expr::Ensure(_))
        ));
    }

    // --- Function declaration tests ---

    #[test]
    fn parse_simple_fn() {
        let prog = parse_program(
            "intent \"add two numbers\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}",
        );
        if let Statement::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = &prog.statements[0]
        {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert!(return_type.is_some());
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_with_intent() {
        let prog =
            parse_program("intent \"find user by ID\"\nfn find_user(id: ID) -> User {\n  id\n}");
        if let Statement::FnDecl { intent, .. } = &prog.statements[0] {
            assert_eq!(intent.as_deref(), Some("find user by ID"));
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_without_intent_is_error() {
        let input = "fn foo() -> Int {\n  1\n}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        assert!(err[0].message.contains("Missing 'intent'"));
    }

    #[test]
    fn parse_fn_with_needs() {
        let prog = parse_program(
            "intent \"save user to database\"\nfn save(user: User) -> User needs db {\n  user\n}",
        );
        if let Statement::FnDecl { effects, .. } = &prog.statements[0] {
            assert_eq!(effects, &["db"]);
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_with_error_types() {
        let prog = parse_program(
            "intent \"find user by ID\"\nfn find(id: ID) -> User or NotFound needs db {\n  id\n}",
        );
        if let Statement::FnDecl {
            error_types,
            return_type,
            ..
        } = &prog.statements[0]
        {
            assert!(return_type.is_some());
            assert_eq!(error_types, &["NotFound"]);
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_no_params_no_return() {
        let prog = parse_program("intent \"greet the user\"\nfn greet() {\n  nothing\n}");
        if let Statement::FnDecl {
            params,
            return_type,
            ..
        } = &prog.statements[0]
        {
            assert_eq!(params.len(), 0);
            assert!(return_type.is_none());
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_needs_multiple_effects() {
        let prog = parse_program(
            "intent \"save user with multiple effects\"\nfn save(user: User) -> User needs db, time, rng {\n  user\n}",
        );
        if let Statement::FnDecl { effects, .. } = &prog.statements[0] {
            assert_eq!(effects, &["db", "time", "rng"]);
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn parse_fn_needs_single_effect() {
        let prog = parse_program(
            "intent \"save user to db\"\nfn save(user: User) -> User needs db {\n  user\n}",
        );
        if let Statement::FnDecl { effects, .. } = &prog.statements[0] {
            assert_eq!(effects, &["db"]);
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn error_message_includes_block_context() {
        // Intentionally malformed: missing closing brace for function
        let input = "intent \"do something\"\nfn foo() {\n  42\n";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        let msg = &err[0].message;
        assert!(
            msg.contains("function"),
            "Error should mention 'function' context, got: {}",
            msg
        );
    }

    // --- Type declaration tests ---

    #[test]
    fn parse_struct_type() {
        let prog = parse_program("type User {\n  id: ID,\n  name: String,\n}");
        if let Statement::TypeDecl(TypeDecl::Struct { name, fields }) = &prog.statements[0] {
            assert_eq!(name, "User");
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected Struct TypeDecl");
        }
    }

    #[test]
    fn parse_union_type() {
        let prog = parse_program("type Role = Admin | Editor | Viewer");
        if let Statement::TypeDecl(TypeDecl::Union { name, variants }) = &prog.statements[0] {
            assert_eq!(name, "Role");
            assert_eq!(variants.len(), 3);
            assert!(variants[0].fields.is_none());
        } else {
            panic!("Expected Union TypeDecl");
        }
    }

    #[test]
    fn parse_union_with_fields() {
        let prog = parse_program("type ApiError = NotFound | BadRequest { message: String }");
        if let Statement::TypeDecl(TypeDecl::Union { variants, .. }) = &prog.statements[0] {
            assert_eq!(variants.len(), 2);
            assert!(variants[0].fields.is_none());
            assert!(variants[1].fields.is_some());
        } else {
            panic!("Expected Union TypeDecl");
        }
    }

    // --- Struct literal tests ---

    #[test]
    fn parse_struct_literal() {
        let expr = parse_expr("User { name: x, age: 30 }");
        if let Expr::StructLiteral { name, fields } = &expr {
            assert_eq!(name.as_deref(), Some("User"));
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected StructLiteral");
        }
    }

    #[test]
    fn parse_struct_literal_with_spread() {
        let expr = parse_expr("User { ...old, name: x }");
        if let Expr::StructLiteral { fields, .. } = &expr {
            assert!(matches!(&fields[0], StructField::Spread(_)));
            assert!(matches!(&fields[1], StructField::Named { .. }));
        } else {
            panic!("Expected StructLiteral");
        }
    }

    #[test]
    fn parse_anonymous_struct() {
        let expr = parse_expr("{ status: ok }");
        if let Expr::StructLiteral { name, .. } = &expr {
            assert!(name.is_none());
        } else {
            panic!("Expected anonymous StructLiteral, got {:?}", expr);
        }
    }

    #[test]
    fn parse_multi_statement_program() {
        let prog = parse_program("use models.user.User\n\nlet x: Int = 1\nlet y: Int = 2");
        assert_eq!(prog.statements.len(), 3);
    }

    // --- Integration tests with real PACT code ---

    #[test]
    fn integration_pact_function_with_pipeline() {
        let input = r#"intent "get names of active admins"
fn active_admins(users: List<User>) -> List<String> {
  users
    | filter where .active
    | filter where .role == Admin
    | sort by .name
    | map to .name
}"#;
        let prog = parse_program(input);
        assert_eq!(prog.statements.len(), 1);
        if let Statement::FnDecl { name, body, .. } = &prog.statements[0] {
            assert_eq!(name, "active_admins");
            assert_eq!(body.len(), 1);
            if let Statement::Expression(Expr::Pipeline { steps, .. }) = &body[0] {
                assert_eq!(steps.len(), 4);
            } else {
                panic!("Expected pipeline in body");
            }
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn integration_type_and_function() {
        let input = r#"type Role = Admin | Editor | Viewer

intent "check if role is admin"
fn is_admin(role: Role) -> Bool {
  match role {
    Admin => true,
    _ => false,
  }
}"#;
        let prog = parse_program(input);
        assert_eq!(prog.statements.len(), 2);
        assert!(matches!(
            &prog.statements[0],
            Statement::TypeDecl(TypeDecl::Union { .. })
        ));
        assert!(matches!(&prog.statements[1], Statement::FnDecl { .. }));
    }

    #[test]
    fn integration_let_with_error_propagation() {
        let input = r#"let user: User = find_user(id)?"#;
        let prog = parse_program(input);
        if let Statement::Let { value, .. } = &prog.statements[0] {
            assert!(matches!(value, Expr::ErrorPropagation(_)));
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn integration_fn_with_ensure_and_return_if() {
        let input = r#"intent "withdraw amount from account"
fn withdraw(account: Account, amount: Int) -> Account or InsufficientFunds {
  ensure amount > 0
  return InsufficientFunds if account.balance < amount
  Account { ...account, balance: account.balance - amount }
}"#;
        let prog = parse_program(input);
        if let Statement::FnDecl {
            body, error_types, ..
        } = &prog.statements[0]
        {
            assert_eq!(error_types, &["InsufficientFunds"]);
            assert!(body.len() >= 3);
            assert!(matches!(&body[0], Statement::Expression(Expr::Ensure(_))));
            assert!(matches!(
                &body[1],
                Statement::Return {
                    condition: Some(_),
                    ..
                }
            ));
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn integration_use_statements() {
        let input = "use models.user.User\nuse models.order.Order";
        let prog = parse_program(input);
        assert_eq!(prog.statements.len(), 2);
        assert!(matches!(&prog.statements[0], Statement::Use { path } if path.len() == 3));
    }

    #[test]
    fn integration_intent_fn_with_needs() {
        let input = r#"intent "create a new user"
fn create_user(data: NewUser) -> User needs db, time, rng {
  let id: ID = rng.uuid()
  User {
    id: id,
    name: data.name,
    active: true,
  }
}"#;
        let prog = parse_program(input);
        if let Statement::FnDecl {
            intent,
            effects,
            body,
            ..
        } = &prog.statements[0]
        {
            assert_eq!(intent.as_deref(), Some("create a new user"));
            assert_eq!(effects, &["db", "time", "rng"]);
            assert!(body.len() >= 2); // let + struct literal
        } else {
            panic!("Expected FnDecl");
        }
    }

    #[test]
    fn integration_struct_type_declaration() {
        let input = r#"type User {
  id: ID,
  name: String,
  email: String,
  age: Int,
  role: Role,
  active: Bool,
}"#;
        let prog = parse_program(input);
        if let Statement::TypeDecl(TypeDecl::Struct { name, fields }) = &prog.statements[0] {
            assert_eq!(name, "User");
            assert_eq!(fields.len(), 6);
            assert_eq!(fields[0].name, "id");
            assert_eq!(fields[5].name, "active");
        } else {
            panic!("Expected Struct TypeDecl");
        }
    }

    #[test]
    fn integration_union_with_fields() {
        let input = "type AppError\n  = NotFound { resource: String }\n  | Forbidden { reason: String }\n  | BadRequest { message: String }";
        let prog = parse_program(input);
        if let Statement::TypeDecl(TypeDecl::Union { name, variants }) = &prog.statements[0] {
            assert_eq!(name, "AppError");
            assert_eq!(variants.len(), 3);
            assert!(variants[0].fields.is_some());
            assert_eq!(variants[0].fields.as_ref().unwrap()[0].name, "resource");
        } else {
            panic!("Expected Union TypeDecl");
        }
    }

    // --- Test block, using, assert parsing ---

    #[test]
    fn parse_test_block() {
        let prog = parse_program("test \"basic\" {\n  assert true\n}");
        assert!(matches!(&prog.statements[0], Statement::TestBlock { .. }));
        if let Statement::TestBlock { name, body } = &prog.statements[0] {
            assert_eq!(name, "basic");
            assert_eq!(body.len(), 1);
            assert!(matches!(&body[0], Statement::Assert(_)));
        }
    }

    #[test]
    fn parse_test_block_with_multiple_asserts() {
        let input = r#"test "math" {
  assert 1 + 1 == 2
  assert 2 * 3 == 6
}"#;
        let prog = parse_program(input);
        if let Statement::TestBlock { name, body } = &prog.statements[0] {
            assert_eq!(name, "math");
            assert_eq!(body.len(), 2);
        } else {
            panic!("Expected TestBlock");
        }
    }

    #[test]
    fn parse_using_statement() {
        let prog = parse_program("using db = db.memory()");
        assert!(matches!(&prog.statements[0], Statement::Using { .. }));
        if let Statement::Using { name, .. } = &prog.statements[0] {
            assert_eq!(name, "db");
        }
    }

    #[test]
    fn parse_assert_statement() {
        let prog = parse_program("assert 1 == 1");
        assert!(matches!(&prog.statements[0], Statement::Assert(_)));
    }

    #[test]
    fn parse_expect_success_pipeline() {
        let input = r#"42 | expect success"#;
        let expr = parse_expr(input);
        if let Expr::Pipeline { steps, .. } = &expr {
            assert!(matches!(&steps[0], PipelineStep::ExpectSuccess));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_test_block_with_using() {
        let input = r#"test "with effects" {
  using db = db.memory()
  assert true
}"#;
        let prog = parse_program(input);
        if let Statement::TestBlock { name, body } = &prog.statements[0] {
            assert_eq!(name, "with effects");
            assert_eq!(body.len(), 2);
            assert!(matches!(&body[0], Statement::Using { .. }));
            assert!(matches!(&body[1], Statement::Assert(_)));
        } else {
            panic!("Expected TestBlock");
        }
    }

    #[test]
    fn parse_simple_route() {
        let input =
            "intent \"health check\"\nroute GET \"/health\" {\n  respond 200 with nothing\n}";
        let prog = parse_program(input);
        assert!(matches!(&prog.statements[0], Statement::Route { method, .. } if method == "GET"));
    }

    #[test]
    fn parse_route_with_needs() {
        let input = "intent \"list users\"\nroute GET \"/users\" {\n  needs db, auth\n  respond 200 with nothing\n}";
        let prog = parse_program(input);
        if let Statement::Route { effects, .. } = &prog.statements[0] {
            assert_eq!(effects, &["db", "auth"]);
        } else {
            panic!("Expected Route");
        }
    }

    #[test]
    fn parse_route_without_intent_fails() {
        let input = "route GET \"/health\" {\n  respond 200 with nothing\n}";
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        assert!(err[0].message.contains("intent"));
    }

    #[test]
    fn parse_respond_expression() {
        let expr = parse_expr("respond 200 with nothing");
        assert!(matches!(expr, Expr::Respond { .. }));
    }

    #[test]
    fn parse_respond_with_content_type() {
        let expr = parse_expr("respond 200 with html as \"text/html\"");
        match expr {
            Expr::Respond {
                status,
                body,
                content_type,
            } => {
                assert!(matches!(*status, Expr::IntLiteral(200)));
                assert!(matches!(*body, Expr::Identifier(ref n) if n == "html"));
                assert_eq!(content_type, Some("text/html".to_string()));
            }
            other => panic!("Expected Respond, got: {:?}", other),
        }
    }

    #[test]
    fn parse_respond_without_content_type() {
        let expr = parse_expr("respond 200 with data");
        match expr {
            Expr::Respond { content_type, .. } => {
                assert_eq!(content_type, None);
            }
            other => panic!("Expected Respond, got: {:?}", other),
        }
    }

    #[test]
    fn parse_on_success_pipeline() {
        let expr = parse_expr("x | on success: respond 200 with .");
        if let Expr::Pipeline { steps, .. } = &expr {
            assert!(matches!(&steps[0], PipelineStep::OnSuccess { .. }));
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_on_error_pipeline() {
        let expr = parse_expr("x | on NotFound: respond 404 with nothing");
        if let Expr::Pipeline { steps, .. } = &expr {
            assert!(
                matches!(&steps[0], PipelineStep::OnError { variant, .. } if variant == "NotFound")
            );
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_validate_as_pipeline() {
        let expr = parse_expr("x | validate as NewUser");
        if let Expr::Pipeline { steps, .. } = &expr {
            assert!(
                matches!(&steps[0], PipelineStep::ValidateAs { type_name } if type_name == "NewUser")
            );
        } else {
            panic!("Expected Pipeline");
        }
    }

    #[test]
    fn parse_route_with_pipeline() {
        let input = r#"intent "get user by ID"
route GET "/users/{id}" {
  needs db
  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "not found" }
}"#;
        let prog = parse_program(input);
        if let Statement::Route { body, .. } = &prog.statements[0] {
            assert!(!body.is_empty());
        } else {
            panic!("Expected Route");
        }
    }

    #[test]
    fn parse_app() {
        let prog = parse_program("app UserService {\n  port: 8080,\n}");
        assert!(
            matches!(&prog.statements[0], Statement::App { name, port, db_url } if name == "UserService" && *port == 8080 && db_url.is_none())
        );
    }

    #[test]
    fn parse_app_with_db() {
        let input = "app UserService {\n  port: 8080,\n  db: \"sqlite://data.db\",\n}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let prog = parser.parse().unwrap();
        assert!(
            matches!(&prog.statements[0], Statement::App { name, port, db_url }
                if name == "UserService" && *port == 8080 && db_url.as_deref() == Some("sqlite://data.db"))
        );
    }

    #[test]
    fn parse_struct_with_string_keys() {
        let expr = parse_expr(r#"{ "https://example.com": { status: 200 } }"#);
        if let Expr::StructLiteral { name, fields } = &expr {
            assert!(name.is_none());
            assert_eq!(fields.len(), 1);
            if let StructField::Named { name, .. } = &fields[0] {
                assert_eq!(name, "https://example.com");
            } else {
                panic!("Expected Named field");
            }
        } else {
            panic!("Expected StructLiteral, got {:?}", expr);
        }
    }

    #[test]
    fn parse_struct_with_mixed_keys() {
        let expr = parse_expr(r#"{ "url": { status: 200 }, normal_key: 42 }"#);
        if let Expr::StructLiteral { fields, .. } = &expr {
            assert_eq!(fields.len(), 2);
            if let StructField::Named { name, .. } = &fields[0] {
                assert_eq!(name, "url");
            } else {
                panic!("Expected Named field");
            }
        } else {
            panic!("Expected StructLiteral, got {:?}", expr);
        }
    }

    #[test]
    fn parse_struct_with_multiple_string_keys() {
        let expr =
            parse_expr(r#"{ "https://a.com": { status: 200 }, "https://b.com": { status: 404 } }"#);
        if let Expr::StructLiteral { fields, .. } = &expr {
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected StructLiteral, got {:?}", expr);
        }
    }

    #[test]
    fn parse_error_missing_brace_has_hint() {
        let input = "intent \"test\"\nfn add(a: Int) -> Int";
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        // Should mention the missing '{'
        assert!(
            err[0].hint.is_some(),
            "Error should have a hint: {:?}",
            err[0]
        );
    }

    #[test]
    fn parse_error_missing_paren_has_hint() {
        let input = "intent \"test\"\nfn add(a: Int -> Int { a }";
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        assert!(
            err[0].hint.is_some(),
            "Error should have a hint: {:?}",
            err[0]
        );
    }

    #[test]
    fn parse_stream_route() {
        let input = r#"
intent "stream data"
stream GET "/events" {
  needs db
  send db.watch("items")
}
"#;
        let program = parse_program(input);
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::Stream {
                method,
                path,
                intent,
                effects,
                ..
            } => {
                assert_eq!(method, "GET");
                assert_eq!(path, "/events");
                assert_eq!(intent, "stream data");
                assert_eq!(effects, &["db"]);
            }
            other => panic!("Expected Stream, got: {:?}", other),
        }
    }

    #[test]
    fn parse_send_expression() {
        let input = r#"
intent "stream"
stream GET "/live" {
  send 42
}
"#;
        let program = parse_program(input);
        match &program.statements[0] {
            Statement::Stream { body, .. } => {
                // body should contain an expression statement with Send
                assert!(!body.is_empty());
            }
            other => panic!("Expected Stream, got: {:?}", other),
        }
    }

    #[test]
    fn bare_stream_error_has_hint() {
        let input = r#"stream GET "/events" { }"#;
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        let err = parser.parse().unwrap_err();
        assert!(err[0].message.contains("intent"));
        assert!(err[0].hint.is_some());
    }

    #[test]
    fn parse_field_constraints() {
        let input = r#"type NewUser {
  name: String | minlen 1 | maxlen 100,
  email: String | format email,
  age: Int | min 0 | max 150,
}
"#;
        let program = parse_program(input);
        if let Statement::TypeDecl(TypeDecl::Struct { fields, .. }) = &program.statements[0] {
            assert_eq!(fields[0].name, "name");
            assert_eq!(fields[0].constraints.len(), 2);
            assert_eq!(fields[0].constraints[0], Constraint::MinLen(1));
            assert_eq!(fields[0].constraints[1], Constraint::MaxLen(100));

            assert_eq!(fields[1].name, "email");
            assert_eq!(fields[1].constraints.len(), 1);
            assert_eq!(
                fields[1].constraints[0],
                Constraint::Format("email".to_string())
            );

            assert_eq!(fields[2].name, "age");
            assert_eq!(fields[2].constraints.len(), 2);
            assert_eq!(fields[2].constraints[0], Constraint::Min(0));
            assert_eq!(fields[2].constraints[1], Constraint::Max(150));
        } else {
            panic!("Expected TypeDecl::Struct");
        }
    }

    #[test]
    fn parse_field_no_constraints() {
        let input = "type User { id: String, name: String }\n";
        let program = parse_program(input);
        if let Statement::TypeDecl(TypeDecl::Struct { fields, .. }) = &program.statements[0] {
            assert!(fields[0].constraints.is_empty());
            assert!(fields[1].constraints.is_empty());
        } else {
            panic!("Expected TypeDecl::Struct");
        }
    }
}

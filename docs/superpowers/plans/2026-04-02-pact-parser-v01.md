# PACT Parser v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a recursive descent parser that converts PACT token streams into an AST, covering all v0.1 constructs (let/var, fn, type, if/match, pipeline, use, intent, ensure, needs).

**Architecture:** Bottom-up parser construction — AST types first, then parsing from lowest-level (primary expressions) up to top-level (program). Each parsing method delegates to the next-lower precedence level. Fail-fast error handling with `Result<Program, Vec<ParseError>>`.

**Tech Stack:** Rust 1.94, no external dependencies. Consumes `Vec<Token>` from existing lexer.

**Design spec:** `docs/superpowers/specs/2026-04-02-pact-parser-v01-design.md`

**IMPORTANT:** Before any `cargo` command, run `source "$HOME/.cargo/env"`. Working directory: `/Users/kikotvit/Documents/REPOS/KikotVit/pact-lang`

---

## File Structure

```
src/
  lib.rs               — add: pub mod parser
  main.rs              — update: add --ast flag to print AST
  parser/
    mod.rs             — pub mod, re-exports
    ast.rs             — all AST types (Program, Statement, Expr, TypeExpr, etc.)
    errors.rs          — ParseError with Display impl
    parser.rs          — struct Parser, all parsing logic + tests
```

---

### Task 1: AST types and ParseError

**Files:**
- Create: `src/parser/ast.rs`
- Create: `src/parser/errors.rs`
- Create: `src/parser/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/parser/ast.rs` with all AST types**

```rust
/// PACT Abstract Syntax Tree types

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        mutable: bool,
        type_ann: TypeExpr,
        value: Expr,
    },
    FnDecl {
        name: String,
        intent: Option<String>,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        error_types: Vec<String>,
        effects: Vec<String>,
        body: Vec<Statement>,
    },
    TypeDecl(TypeDecl),
    Use {
        path: Vec<String>,
    },
    Return {
        value: Option<Expr>,
        condition: Option<Expr>,
    },
    Expression(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_ann: TypeExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDecl {
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Union {
        name: String,
        variants: Vec<UnionVariant>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub type_ann: TypeExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant {
    pub name: String,
    pub fields: Option<Vec<Field>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(StringExpr),
    BoolLiteral(bool),
    Nothing,

    Identifier(String),
    FieldAccess { object: Box<Expr>, field: String },
    DotShorthand(Vec<String>),

    BinaryOp { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    ErrorPropagation(Box<Expr>),

    FnCall { callee: Box<Expr>, args: Vec<Expr> },

    Pipeline { source: Box<Expr>, steps: Vec<PipelineStep> },

    If {
        condition: Box<Expr>,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Block(Vec<Statement>),

    StructLiteral {
        name: Option<String>,
        fields: Vec<StructField>,
    },

    Ensure(Box<Expr>),
    Is { expr: Box<Expr>, type_name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructField {
    Named { name: String, value: Expr },
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringExpr {
    Simple(String),
    Interpolated(Vec<StringPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(String),
    Wildcard,
    Literal(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStep {
    Filter { predicate: Expr },
    Map { expr: Expr },
    Sort { field: Expr, descending: bool },
    GroupBy { field: Expr },
    Take { kind: TakeKind, count: Expr },
    Skip { count: Expr },
    Each { expr: Expr },
    FindFirst { predicate: Expr },
    ExpectOne { error: Expr },
    ExpectAny { error: Expr },
    OrDefault { value: Expr },
    Flatten,
    Unique,
    Count,
    Sum,
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TakeKind {
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Generic { name: String, args: Vec<TypeExpr> },
    Optional(Box<TypeExpr>),
    Result { ok: Box<TypeExpr>, errors: Vec<String> },
}
```

- [ ] **Step 2: Create `src/parser/errors.rs`**

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Parse error at line {}, col {}:", self.line, self.column)?;
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
    fn parse_error_display() {
        let error = ParseError {
            line: 5,
            column: 10,
            message: "Expected '{' after function parameters".to_string(),
            hint: Some("Function bodies must be wrapped in { }".to_string()),
            source_line: "fn add(a: Int, b: Int) -> Int".to_string(),
        };
        let output = format!("{}", error);
        assert!(output.contains("Parse error at line 5, col 10"));
        assert!(output.contains("fn add(a: Int, b: Int) -> Int"));
        assert!(output.contains("Expected '{' after function parameters"));
        assert!(output.contains("Hint:"));
    }
}
```

- [ ] **Step 3: Create `src/parser/mod.rs` and update `src/lib.rs`**

`src/parser/mod.rs`:
```rust
pub mod ast;
pub mod errors;
pub mod parser;

pub use ast::*;
pub use errors::ParseError;
pub use parser::Parser;
```

Add to `src/lib.rs`:
```rust
pub mod lexer;
pub mod parser;
```

- [ ] **Step 4: Run tests to verify compilation**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: all existing 56 tests pass + 1 new ParseError test.

- [ ] **Step 5: Commit**

```bash
git add src/parser/ src/lib.rs
git commit -m "feat: add AST types and ParseError for PACT parser v0.1"
```

---

### Task 2: Parser scaffold and helper methods

**Files:**
- Create: `src/parser/parser.rs`
- Modify: `src/parser/mod.rs` (already exports parser mod)

- [ ] **Step 1: Write tests for parser helpers**

Add to `src/parser/parser.rs`:

```rust
use crate::lexer::{Token, TokenKind, Span, Lexer};
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
        // Placeholder — implemented in Task 12
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
                kind,
                self.current_kind()
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
}

// --- Test helpers ---

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

    fn parse_fails(input: &str) -> Vec<ParseError> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens, input);
        parser.parse().unwrap_err()
    }

    #[test]
    fn parser_creates_empty_program() {
        let prog = parse_program("");
        assert_eq!(prog.statements.len(), 0);
    }
}
```

Note: `parse_expression` doesn't exist yet — the `parse_expr` helper will be used in later tasks. For now only `parser_creates_empty_program` runs.

- [ ] **Step 2: Run tests**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 58 tests pass (56 lexer + 1 ParseError + 1 parser).

- [ ] **Step 3: Commit**

```bash
git add src/parser/parser.rs
git commit -m "feat: add Parser scaffold with token navigation and error helpers"
```

---

### Task 3: parse_primary — literals and identifiers

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests for primary expressions**

Add to the `tests` module in `src/parser/parser.rs`:

```rust
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
    assert_eq!(
        parse_expr("(42)"),
        Expr::IntLiteral(42),
    );
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
```

- [ ] **Step 2: Implement `parse_expression` and `parse_primary`**

Add these methods to the `impl Parser` block (before the `// --- Token navigation ---` section):

```rust
    // --- Expression parsing ---

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down for now, pipeline added in Task 8
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 6
        self.parse_and()
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 6
        self.parse_not()
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 6
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 6
        self.parse_addition()
    }

    fn parse_addition(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 5
        self.parse_multiplication()
    }

    fn parse_multiplication(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 5
        self.parse_unary()
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 5
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        // Placeholder — delegates down, implemented in Task 4
        self.parse_primary()
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
                self.advance(); // consume (
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
                self.parse_if_expr()
            }
            TokenKind::Match => {
                self.parse_match_expr()
            }
            TokenKind::Ensure => {
                self.advance(); // consume ensure
                let expr = self.parse_expression()?;
                Ok(Expr::Ensure(Box::new(expr)))
            }
            _ => {
                self.fail(
                    &format!("Expected expression, found {:?}", self.current_kind()),
                    None,
                )
            }
        }
    }

    fn parse_dot_shorthand(&mut self) -> Result<Expr, ParseError> {
        self.advance(); // consume initial .
        let mut parts = vec![self.expect_identifier()?];
        while self.eat(&TokenKind::Dot) {
            parts.push(self.expect_identifier()?);
        }
        Ok(Expr::DotShorthand(parts))
    }

    // Placeholder methods for constructs implemented in later tasks:

    fn parse_string_expr(&mut self) -> Result<Expr, ParseError> {
        self.fail("Strings not yet implemented in parser", None)
    }

    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        self.fail("If not yet implemented in parser", None)
    }

    fn parse_match_expr(&mut self) -> Result<Expr, ParseError> {
        self.fail("Match not yet implemented in parser", None)
    }
```

- [ ] **Step 3: Run tests**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: all pass including 8 new parse_primary tests.

- [ ] **Step 4: Commit**

```bash
git add src/parser/parser.rs
git commit -m "feat: add parse_primary with literals, identifiers, grouped exprs, dot shorthand"
```

---

### Task 4: parse_postfix — field access, function calls, error propagation

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
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
    // db.query("users") → FnCall { callee: FieldAccess { db, "query" }, args: [StringLiteral] }
    assert!(matches!(
        parse_expr(r#"db.query("users")"#),
        Expr::FnCall { callee, args } if matches!(*callee, Expr::FieldAccess { .. }) && args.len() == 1
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
}
```

- [ ] **Step 2: Implement `parse_postfix`**

Replace the placeholder `parse_postfix` in `src/parser/parser.rs`:

```rust
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.current_kind() {
                TokenKind::Dot => {
                    self.advance(); // consume .
                    let field = self.expect_identifier()?;
                    expr = Expr::FieldAccess {
                        object: Box::new(expr),
                        field,
                    };
                }
                TokenKind::LParen => {
                    self.advance(); // consume (
                    let args = self.parse_args_list()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Expr::FnCall {
                        callee: Box::new(expr),
                        args,
                    };
                }
                TokenKind::Question => {
                    self.advance(); // consume ?
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
            self.skip_newlines();
            if self.at(&TokenKind::RParen) {
                break; // trailing comma
            }
            args.push(self.parse_expression()?);
        }
        Ok(args)
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add parse_postfix with field access, function calls, error propagation"
```

---

### Task 5: Arithmetic expressions — unary, multiplication, addition

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_addition() {
    assert_eq!(
        parse_expr("1 + 2"),
        Expr::BinaryOp {
            left: Box::new(Expr::IntLiteral(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::IntLiteral(2)),
        },
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
        },
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
        },
    );
}

#[test]
fn parse_unary_negation() {
    assert_eq!(
        parse_expr("-42"),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::IntLiteral(42)),
        },
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
        },
    );
}
```

- [ ] **Step 2: Implement arithmetic parsing**

Replace the `parse_addition`, `parse_multiplication`, and `parse_unary` placeholders:

```rust
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
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add arithmetic expression parsing (add, sub, mul, div, unary neg)"
```

---

### Task 6: Comparison, boolean, and `is` expressions

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_comparison_eq() {
    assert_eq!(
        parse_expr("a == b"),
        Expr::BinaryOp {
            left: Box::new(Expr::Identifier("a".to_string())),
            op: BinaryOp::Eq,
            right: Box::new(Expr::Identifier("b".to_string())),
        },
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
        },
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
        },
    );
}

#[test]
fn parse_and_or() {
    // a and b or c → Or(And(a, b), c)
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
        },
    );
}

#[test]
fn parse_not() {
    assert_eq!(
        parse_expr("not x"),
        Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Identifier("x".to_string())),
        },
    );
}

#[test]
fn parse_is_expr() {
    assert_eq!(
        parse_expr("result is NotFound"),
        Expr::Is {
            expr: Box::new(Expr::Identifier("result".to_string())),
            type_name: "NotFound".to_string(),
        },
    );
}

#[test]
fn parse_precedence_comparison_vs_arithmetic() {
    // a + 1 == b → Eq(Add(a, 1), b)
    assert!(matches!(
        parse_expr("a + 1 == b"),
        Expr::BinaryOp { op: BinaryOp::Eq, .. }
    ));
}
```

- [ ] **Step 2: Implement comparison and boolean parsing**

Replace the `parse_or`, `parse_and`, `parse_not`, and `parse_comparison` placeholders:

```rust
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
        let mut left = self.parse_addition()?;

        // Check for `is` — special case: expr is TypeName
        if self.at(&TokenKind::Identifier(String::new())) {
            if let TokenKind::Identifier(ref word) = self.current_kind().clone() {
                if word == "is" {
                    self.advance(); // consume `is`
                    let type_name = self.expect_identifier()?;
                    return Ok(Expr::Is {
                        expr: Box::new(left),
                        type_name,
                    });
                }
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
```

Note: The `is` check uses a workaround to match on contextual identifier "is". Since `is` is not a keyword (it's contextual), we need to check the identifier string.

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add comparison, boolean (and/or/not), and is-expression parsing"
```

---

### Task 7: String expression parsing

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_simple_string() {
    assert_eq!(
        parse_expr(r#""hello""#),
        Expr::StringLiteral(StringExpr::Simple("hello".to_string())),
    );
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
    assert_eq!(
        parse_expr(r#"raw"no {interp}""#),
        Expr::StringLiteral(StringExpr::Simple("no {interp}".to_string())),
    );
}

#[test]
fn parse_empty_string() {
    assert_eq!(
        parse_expr(r#""""#),
        Expr::StringLiteral(StringExpr::Simple(String::new())),
    );
}
```

- [ ] **Step 2: Implement `parse_string_expr`**

Replace the `parse_string_expr` placeholder:

```rust
    fn parse_string_expr(&mut self) -> Result<Expr, ParseError> {
        // Raw strings
        if let TokenKind::RawStringLiteral(content) = self.current_kind().clone() {
            self.advance();
            return Ok(Expr::StringLiteral(StringExpr::Simple(content)));
        }

        // Regular and interpolated strings: StringStart [fragments/interpolations] StringEnd
        self.expect(&TokenKind::StringStart)?;

        // Empty string
        if self.at(&TokenKind::StringEnd) {
            self.advance();
            return Ok(Expr::StringLiteral(StringExpr::Simple(String::new())));
        }

        // Check if it's a simple string (just one fragment, no interpolation)
        if let TokenKind::StringFragment(ref text) = self.current_kind().clone() {
            let text = text.clone();
            self.advance();
            if self.at(&TokenKind::StringEnd) {
                self.advance();
                return Ok(Expr::StringLiteral(StringExpr::Simple(text)));
            }
            // Has more parts — it's interpolated. Start building parts list.
            let mut parts = vec![StringPart::Literal(text)];
            self.collect_string_parts(&mut parts)?;
            self.expect(&TokenKind::StringEnd)?;
            return Ok(Expr::StringLiteral(StringExpr::Interpolated(parts)));
        }

        // Starts with interpolation directly
        let mut parts = Vec::new();
        self.collect_string_parts(&mut parts)?;
        self.expect(&TokenKind::StringEnd)?;
        Ok(Expr::StringLiteral(StringExpr::Interpolated(parts)))
    }

    fn collect_string_parts(&mut self, parts: &mut Vec<StringPart>) -> Result<(), ParseError> {
        loop {
            match self.current_kind().clone() {
                TokenKind::StringEnd => break,
                TokenKind::StringFragment(text) => {
                    self.advance();
                    parts.push(StringPart::Literal(text));
                }
                TokenKind::InterpolationStart => {
                    self.advance();
                    let expr = self.parse_expression()?;
                    self.expect(&TokenKind::InterpolationEnd)?;
                    parts.push(StringPart::Expr(expr));
                }
                _ => {
                    return Err(self.error(
                        &format!("Unexpected token in string: {:?}", self.current_kind()),
                        None,
                    ));
                }
            }
        }
        Ok(())
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add string expression parsing (simple, interpolated, raw)"
```

---

### Task 8: Pipeline parsing

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
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
fn parse_sort_by() {
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
fn parse_multi_step_pipeline() {
    let expr = parse_expr("users\n  | filter where .active\n  | map to .name\n  | sort by .name");
    if let Expr::Pipeline { steps, .. } = expr {
        assert_eq!(steps.len(), 3);
    } else {
        panic!("Expected Pipeline");
    }
}

#[test]
fn parse_pipeline_expr_fallback() {
    // api_pipeline? is an arbitrary expression after |, should be PipelineStep::Expr
    let expr = parse_expr("request | api_pipeline");
    if let Expr::Pipeline { steps, .. } = expr {
        assert!(matches!(&steps[0], PipelineStep::Expr(_)));
    } else {
        panic!("Expected Pipeline");
    }
}

#[test]
fn parse_or_default() {
    let expr = parse_expr("x | or default 1");
    if let Expr::Pipeline { steps, .. } = expr {
        assert!(matches!(&steps[0], PipelineStep::OrDefault { .. }));
    } else {
        panic!("Expected Pipeline");
    }
}
```

- [ ] **Step 2: Implement `parse_pipeline` and `parse_pipeline_step`**

Replace the `parse_pipeline` placeholder:

```rust
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

        Ok(Expr::Pipeline {
            source: Box::new(source),
            steps,
        })
    }

    fn parse_pipeline_step(&mut self) -> Result<PipelineStep, ParseError> {
        // Check for contextual keywords
        if let TokenKind::Identifier(ref word) = self.current_kind().clone() {
            match word.as_str() {
                "filter" => {
                    self.advance();
                    self.expect_contextual("where")?;
                    let predicate = self.parse_or()?;
                    return Ok(PipelineStep::Filter { predicate });
                }
                "map" => {
                    self.advance();
                    self.expect_contextual("to")?;
                    let expr = self.parse_or()?;
                    return Ok(PipelineStep::Map { expr });
                }
                "sort" => {
                    self.advance();
                    self.expect_contextual("by")?;
                    let field = self.parse_or()?;
                    let descending = if self.eat_contextual("descending") {
                        true
                    } else {
                        self.eat_contextual("ascending");
                        false
                    };
                    return Ok(PipelineStep::Sort { field, descending });
                }
                "group" => {
                    self.advance();
                    self.expect_contextual("by")?;
                    let field = self.parse_or()?;
                    return Ok(PipelineStep::GroupBy { field });
                }
                "take" => {
                    self.advance();
                    let kind = if self.eat_contextual("last") {
                        TakeKind::Last
                    } else {
                        self.expect_contextual("first")?;
                        TakeKind::First
                    };
                    let count = self.parse_or()?;
                    return Ok(PipelineStep::Take { kind, count });
                }
                "skip" => {
                    self.advance();
                    let count = self.parse_or()?;
                    return Ok(PipelineStep::Skip { count });
                }
                "each" => {
                    self.advance();
                    let expr = self.parse_or()?;
                    return Ok(PipelineStep::Each { expr });
                }
                "find" => {
                    self.advance();
                    self.expect_contextual("first")?;
                    self.expect_contextual("where")?;
                    let predicate = self.parse_or()?;
                    return Ok(PipelineStep::FindFirst { predicate });
                }
                "expect" => {
                    self.advance();
                    if self.eat_contextual("one") {
                        self.expect(&TokenKind::Or)?;
                        self.expect_contextual("raise")?;
                        let error = self.parse_or()?;
                        return Ok(PipelineStep::ExpectOne { error });
                    } else if self.eat_contextual("any") {
                        self.expect(&TokenKind::Or)?;
                        self.expect_contextual("raise")?;
                        let error = self.parse_or()?;
                        return Ok(PipelineStep::ExpectAny { error });
                    } else {
                        return Err(self.error("Expected 'one' or 'any' after 'expect'", None));
                    }
                }
                "flatten" => { self.advance(); return Ok(PipelineStep::Flatten); }
                "unique" => { self.advance(); return Ok(PipelineStep::Unique); }
                "count" => { self.advance(); return Ok(PipelineStep::Count); }
                "sum" => { self.advance(); return Ok(PipelineStep::Sum); }
                _ => {}
            }
        }

        // `or default <expr>` — `or` is a keyword token
        if self.at(&TokenKind::Or) {
            self.advance();
            self.expect_contextual("default")?;
            let value = self.parse_or()?;
            return Ok(PipelineStep::OrDefault { value });
        }

        // Fallback: arbitrary expression
        let expr = self.parse_or()?;
        Ok(PipelineStep::Expr(expr))
    }

    // --- Contextual keyword helpers ---

    fn expect_contextual(&mut self, word: &str) -> Result<(), ParseError> {
        if let TokenKind::Identifier(ref w) = self.current_kind().clone() {
            if w == word {
                self.advance();
                return Ok(());
            }
        }
        Err(self.error(
            &format!("Expected '{}', found {:?}", word, self.current_kind()),
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
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add pipeline parsing with all step types and expr fallback"
```

---

### Task 9: Control flow — if/else and match

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
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
fn parse_match_expr() {
    let input = "match role {\n  Admin => true,\n  _ => false,\n}";
    let expr = parse_expr(input);
    assert!(matches!(expr, Expr::Match { .. }));
    if let Expr::Match { arms, .. } = &expr {
        assert_eq!(arms.len(), 2);
        assert!(matches!(&arms[1].pattern, Pattern::Wildcard));
    }
}
```

- [ ] **Step 2: Implement `parse_if_expr` and `parse_match_expr`**

Replace the `parse_if_expr` and `parse_match_expr` placeholders:

```rust
    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        self.advance(); // consume `if`
        let condition = self.parse_expression()?;
        self.expect(&TokenKind::LBrace)?;
        let then_body = self.parse_block_body()?;
        self.expect(&TokenKind::RBrace)?;

        let else_body = if self.eat(&TokenKind::Else) {
            self.expect(&TokenKind::LBrace)?;
            let body = self.parse_block_body()?;
            self.expect(&TokenKind::RBrace)?;
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
        self.advance(); // consume `match`
        let subject = self.parse_expression()?;
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::FatArrow)?;
            let body = self.parse_expression()?;
            arms.push(MatchArm { pattern, body });
            // Consume comma and/or newlines between arms
            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Expr::Match {
            subject: Box::new(subject),
            arms,
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
                &format!("Expected pattern, found {:?}", self.current_kind()),
                Some("Patterns can be identifiers (Admin), _ (wildcard), or literals (42, true)"),
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

    // parse_statement placeholder — implemented in Task 12
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        // For now, parse an expression as a statement
        let expr = self.parse_expression()?;
        self.skip_newlines();
        Ok(Statement::Expression(expr))
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add if/else and match expression parsing"
```

---

### Task 10: Type expression parsing

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_simple_type() {
    let mut lexer = Lexer::new("Int");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, "Int");
    let ty = parser.parse_type_expr().unwrap();
    assert_eq!(ty, TypeExpr::Named("Int".to_string()));
}

#[test]
fn parse_generic_type() {
    let mut lexer = Lexer::new("List<User>");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, "List<User>");
    let ty = parser.parse_type_expr().unwrap();
    assert_eq!(
        ty,
        TypeExpr::Generic {
            name: "List".to_string(),
            args: vec![TypeExpr::Named("User".to_string())],
        },
    );
}

#[test]
fn parse_optional_type() {
    let mut lexer = Lexer::new("Optional<String>");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, "Optional<String>");
    let ty = parser.parse_type_expr().unwrap();
    assert_eq!(
        ty,
        TypeExpr::Optional(Box::new(TypeExpr::Named("String".to_string()))),
    );
}

#[test]
fn parse_result_type() {
    // User or NotFound or DbError
    let mut lexer = Lexer::new("User or NotFound or DbError");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, "User or NotFound or DbError");
    let ty = parser.parse_type_expr().unwrap();
    assert_eq!(
        ty,
        TypeExpr::Result {
            ok: Box::new(TypeExpr::Named("User".to_string())),
            errors: vec!["NotFound".to_string(), "DbError".to_string()],
        },
    );
}

#[test]
fn parse_nested_generic_type() {
    let mut lexer = Lexer::new("Map<String, List<Int>>");
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, "Map<String, List<Int>>");
    let ty = parser.parse_type_expr().unwrap();
    assert!(matches!(ty, TypeExpr::Generic { ref name, ref args } if name == "Map" && args.len() == 2));
}
```

- [ ] **Step 2: Implement `parse_type_expr`**

```rust
    pub fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let name = self.expect_identifier()?;

        // Optional<T> is sugar
        if name == "Optional" {
            self.expect(&TokenKind::LAngle)?;
            let inner = self.parse_type_expr()?;
            self.expect(&TokenKind::RAngle)?;
            let ty = TypeExpr::Optional(Box::new(inner));
            return self.maybe_result_type(ty);
        }

        // Check for generic: Name<...>
        let base = if self.at(&TokenKind::LAngle) {
            self.advance();
            let mut args = vec![self.parse_type_expr()?];
            while self.eat(&TokenKind::Comma) {
                args.push(self.parse_type_expr()?);
            }
            self.expect(&TokenKind::RAngle)?;
            TypeExpr::Generic { name, args }
        } else {
            TypeExpr::Named(name)
        };

        self.maybe_result_type(base)
    }

    fn maybe_result_type(&mut self, ok_type: TypeExpr) -> Result<TypeExpr, ParseError> {
        if !self.at(&TokenKind::Or) {
            return Ok(ok_type);
        }

        // Peek: is this `or ErrorName` (type context) or `or` (expression context)?
        // In type context, the thing after `or` is always a PascalCase identifier.
        // We check: if next token after `or` is an Identifier, treat as result type.
        if let Some(TokenKind::Identifier(_)) = {
            if self.pos + 1 < self.tokens.len() {
                Some(self.tokens[self.pos + 1].kind.clone())
            } else {
                None
            }
        } {
            let mut errors = Vec::new();
            while self.eat(&TokenKind::Or) {
                let error_name = self.expect_identifier()?;
                errors.push(error_name);
            }
            Ok(TypeExpr::Result {
                ok: Box::new(ok_type),
                errors,
            })
        } else {
            Ok(ok_type)
        }
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add type expression parsing (named, generic, optional, result)"
```

---

### Task 11: Statement parsing — let/var, return, use, ensure

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_let_statement() {
    let prog = parse_program(r#"let name: String = "Vitalii""#);
    assert_eq!(prog.statements.len(), 1);
    assert!(matches!(&prog.statements[0], Statement::Let { mutable: false, .. }));
}

#[test]
fn parse_var_statement() {
    let prog = parse_program("var counter: Int = 0");
    assert_eq!(prog.statements.len(), 1);
    assert!(matches!(&prog.statements[0], Statement::Let { mutable: true, .. }));
}

#[test]
fn parse_return_statement() {
    let prog = parse_program("return 42");
    assert!(matches!(&prog.statements[0], Statement::Return { value: Some(_), condition: None }));
}

#[test]
fn parse_return_if() {
    let prog = parse_program("return NotFound if not user.active");
    assert!(matches!(&prog.statements[0], Statement::Return { value: Some(_), condition: Some(_) }));
}

#[test]
fn parse_use_statement() {
    let prog = parse_program("use models.user.User");
    assert!(matches!(&prog.statements[0], Statement::Use { ref path } if path == &["models", "user", "User"]));
}

#[test]
fn parse_ensure_statement() {
    let prog = parse_program("ensure amount > 0");
    assert!(matches!(&prog.statements[0], Statement::Expression(Expr::Ensure(_))));
}
```

- [ ] **Step 2: Implement statement parsing methods**

Replace the `parse_statement` method and add `parse_let_or_var`, `parse_return`, `parse_use`:

```rust
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        self.skip_newlines();
        match self.current_kind().clone() {
            TokenKind::Let => self.parse_let_or_var(false),
            TokenKind::Var => self.parse_let_or_var(true),
            TokenKind::Return => self.parse_return(),
            TokenKind::Use => self.parse_use(),
            // intent/fn/type handled in Task 12-13
            _ => {
                let expr = self.parse_expression()?;
                Ok(Statement::Expression(expr))
            }
        }
    }

    fn parse_let_or_var(&mut self, mutable: bool) -> Result<Statement, ParseError> {
        self.advance(); // consume let/var
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_expr()?;
        self.expect(&TokenKind::Assign)?;
        let value = self.parse_expression()?;
        Ok(Statement::Let {
            name,
            mutable,
            type_ann,
            value,
        })
    }

    fn parse_return(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume return

        // return without value (just `return`)
        if self.at(&TokenKind::Newline) || self.at_eof() || self.at(&TokenKind::RBrace) {
            return Ok(Statement::Return {
                value: None,
                condition: None,
            });
        }

        let value = self.parse_expression()?;

        // Check for conditional: return X if Y
        let condition = if self.at(&TokenKind::If) {
            self.advance(); // consume if
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Statement::Return {
            value: Some(value),
            condition,
        })
    }

    fn parse_use(&mut self) -> Result<Statement, ParseError> {
        self.advance(); // consume use
        let mut path = vec![self.expect_identifier()?];
        while self.eat(&TokenKind::Dot) {
            path.push(self.expect_identifier()?);
        }
        Ok(Statement::Use { path })
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add let/var, return, use, ensure statement parsing"
```

---

### Task 12: Function declarations with intent, needs

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_simple_fn() {
    let prog = parse_program("fn add(a: Int, b: Int) -> Int {\n  a + b\n}");
    assert_eq!(prog.statements.len(), 1);
    if let Statement::FnDecl { name, params, return_type, body, .. } = &prog.statements[0] {
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
    let prog = parse_program("intent \"find user by ID\"\nfn find_user(id: ID) -> User {\n  id\n}");
    if let Statement::FnDecl { intent, .. } = &prog.statements[0] {
        assert_eq!(intent.as_deref(), Some("find user by ID"));
    } else {
        panic!("Expected FnDecl");
    }
}

#[test]
fn parse_fn_with_needs() {
    let prog = parse_program("fn save(user: User) -> User needs { db } {\n  user\n}");
    if let Statement::FnDecl { effects, .. } = &prog.statements[0] {
        assert_eq!(effects, &["db"]);
    } else {
        panic!("Expected FnDecl");
    }
}

#[test]
fn parse_fn_with_error_types() {
    let prog = parse_program("fn find(id: ID) -> User or NotFound needs { db } {\n  id\n}");
    if let Statement::FnDecl { error_types, return_type, .. } = &prog.statements[0] {
        assert!(return_type.is_some());
        assert_eq!(error_types, &["NotFound"]);
    } else {
        panic!("Expected FnDecl");
    }
}
```

- [ ] **Step 2: Implement `parse_fn_decl` and update `parse_statement`**

Add `parse_fn_decl` and update the statement dispatch:

```rust
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        self.skip_newlines();
        match self.current_kind().clone() {
            TokenKind::Let => self.parse_let_or_var(false),
            TokenKind::Var => self.parse_let_or_var(true),
            TokenKind::Return => self.parse_return(),
            TokenKind::Use => self.parse_use(),
            TokenKind::Fn => self.parse_fn_decl(None),
            TokenKind::Intent => {
                self.advance(); // consume intent
                let intent = self.parse_intent_string()?;
                self.skip_newlines();
                self.parse_fn_decl(Some(intent))
            }
            TokenKind::Type => {
                let td = self.parse_type_decl()?;
                Ok(Statement::TypeDecl(td))
            }
            _ => {
                let expr = self.parse_expression()?;
                Ok(Statement::Expression(expr))
            }
        }
    }

    fn parse_intent_string(&mut self) -> Result<String, ParseError> {
        // intent is followed by a string literal
        match self.current_kind().clone() {
            TokenKind::StringStart => {
                self.advance(); // consume StringStart
                if let TokenKind::StringFragment(text) = self.current_kind().clone() {
                    self.advance();
                    self.expect(&TokenKind::StringEnd)?;
                    Ok(text)
                } else if self.at(&TokenKind::StringEnd) {
                    self.advance();
                    Ok(String::new())
                } else {
                    self.fail("Expected string after 'intent'", None)
                }
            }
            TokenKind::RawStringLiteral(text) => {
                self.advance();
                Ok(text)
            }
            _ => self.fail(
                &format!("Expected string after 'intent', found {:?}", self.current_kind()),
                Some("Usage: intent \"description of function purpose\""),
            ),
        }
    }

    fn parse_fn_decl(&mut self, intent: Option<String>) -> Result<Statement, ParseError> {
        self.expect(&TokenKind::Fn)?;
        let name = self.expect_identifier()?;

        // Parameters
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;

        // Return type: -> Type or Type1 or Type2
        let mut return_type = None;
        let mut error_types = Vec::new();
        if self.eat(&TokenKind::Arrow) {
            let ty = self.parse_type_expr()?;
            // Check if parse_type_expr consumed `or ErrorType` into a Result type
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

        // Effects: needs { db, time }
        let mut effects = Vec::new();
        if self.at(&TokenKind::Needs) {
            self.advance();
            self.expect(&TokenKind::LBrace)?;
            if !self.at(&TokenKind::RBrace) {
                effects.push(self.expect_identifier()?);
                while self.eat(&TokenKind::Comma) {
                    if self.at(&TokenKind::RBrace) {
                        break; // trailing comma
                    }
                    effects.push(self.expect_identifier()?);
                }
            }
            self.expect(&TokenKind::RBrace)?;
        }

        // Body
        self.expect(&TokenKind::LBrace)?;
        let body = self.parse_block_body()?;
        self.expect(&TokenKind::RBrace)?;

        Ok(Statement::FnDecl {
            name,
            intent,
            params,
            return_type,
            error_types,
            effects,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.at(&TokenKind::RParen) {
            return Ok(params);
        }
        params.push(self.parse_param()?);
        while self.eat(&TokenKind::Comma) {
            if self.at(&TokenKind::RParen) {
                break; // trailing comma
            }
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param, ParseError> {
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::Colon)?;
        let type_ann = self.parse_type_expr()?;
        Ok(Param { name, type_ann })
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add function declaration parsing with intent, needs, error types"
```

---

### Task 13: Type declarations — struct and union

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn parse_struct_type() {
    let prog = parse_program("type User {\n  id: ID,\n  name: String,\n}");
    if let Statement::TypeDecl(TypeDecl::Struct { name, fields }) = &prog.statements[0] {
        assert_eq!(name, "User");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "id");
        assert_eq!(fields[1].name, "name");
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
        assert_eq!(variants[0].name, "Admin");
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
        assert_eq!(variants[1].fields.as_ref().unwrap().len(), 1);
    } else {
        panic!("Expected Union TypeDecl");
    }
}
```

- [ ] **Step 2: Implement `parse_type_decl`**

Add the `parse_type_decl` method (it's called from `parse_statement` which already dispatches on `TokenKind::Type`):

```rust
    fn parse_type_decl(&mut self) -> Result<TypeDecl, ParseError> {
        self.advance(); // consume `type`
        let name = self.expect_identifier()?;

        if self.eat(&TokenKind::LBrace) {
            // Struct: type Name { fields }
            self.skip_newlines();
            let mut fields = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let type_ann = self.parse_type_expr()?;
                fields.push(Field { name: field_name, type_ann });
                self.eat(&TokenKind::Comma);
                self.skip_newlines();
            }
            self.expect(&TokenKind::RBrace)?;
            Ok(TypeDecl::Struct { name, fields })
        } else if self.eat(&TokenKind::Assign) {
            // Union: type Name = Variant1 | Variant2 { fields }
            let mut variants = Vec::new();
            variants.push(self.parse_union_variant()?);
            while self.eat(&TokenKind::Pipe) {
                variants.push(self.parse_union_variant()?);
            }
            Ok(TypeDecl::Union { name, variants })
        } else {
            self.fail(
                &format!("Expected '{{' or '=' after type name '{}', found {:?}", name, self.current_kind()),
                Some("Use 'type Name { fields }' for struct or 'type Name = A | B' for union"),
            )
        }
    }

    fn parse_union_variant(&mut self) -> Result<UnionVariant, ParseError> {
        let name = self.expect_identifier()?;
        let fields = if self.at(&TokenKind::LBrace) {
            self.advance();
            self.skip_newlines();
            let mut fields = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at_eof() {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let type_ann = self.parse_type_expr()?;
                fields.push(Field { name: field_name, type_ann });
                self.eat(&TokenKind::Comma);
                self.skip_newlines();
            }
            self.expect(&TokenKind::RBrace)?;
            Some(fields)
        } else {
            None
        };
        Ok(UnionVariant { name, fields })
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add type declaration parsing (struct and union with variant fields)"
```

---

### Task 14: Struct literals and parse_program

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write tests**

```rust
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
        panic!("Expected anonymous StructLiteral");
    }
}

#[test]
fn parse_multi_statement_program() {
    let prog = parse_program("use models.user.User\n\nlet x: Int = 1\nlet y: Int = 2");
    assert_eq!(prog.statements.len(), 3);
}

#[test]
fn parse_program_with_eof() {
    let prog = parse_program("");
    assert_eq!(prog.statements.len(), 0);
}
```

- [ ] **Step 2: Implement struct literal parsing and finalize parse_program**

For struct literals, there's a parsing challenge: when we see `Identifier` followed by `{`, is it a struct literal `User { ... }` or an identifier followed by a block? The rule: if the identifier starts with uppercase (PascalCase), it's a struct literal. Otherwise it's not.

Update `parse_primary` to handle struct literals. After the `Identifier` case:

```rust
            TokenKind::Identifier(name) => {
                self.advance();
                // Check for struct literal: PascalCase identifier followed by {
                if self.at(&TokenKind::LBrace) && name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    return self.parse_struct_literal(Some(name));
                }
                Ok(Expr::Identifier(name))
            }
```

Also add handling for anonymous structs `{ ... }` in parse_primary. Add before the `_` catch-all:

```rust
            TokenKind::LBrace => {
                // Could be anonymous struct literal { field: value } or block
                // Peek ahead: if we see `Identifier Colon` or `Spread`, it's a struct literal
                // Otherwise it's a block expression
                if self.is_struct_literal_start() {
                    self.parse_struct_literal(None)
                } else {
                    self.advance(); // consume {
                    let body = self.parse_block_body()?;
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr::Block(body))
                }
            }
```

Add the struct literal methods:

```rust
    fn is_struct_literal_start(&self) -> bool {
        // Look at token after {: if it's `identifier :` or `...`, it's a struct literal
        if self.pos + 1 >= self.tokens.len() {
            return false;
        }
        let after_brace = &self.tokens[self.pos + 1].kind;
        match after_brace {
            TokenKind::Spread => true,
            TokenKind::Identifier(_) => {
                // Check if identifier is followed by `:`
                if self.pos + 2 < self.tokens.len() {
                    matches!(self.tokens[self.pos + 2].kind, TokenKind::Colon)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn parse_struct_literal(&mut self, name: Option<String>) -> Result<Expr, ParseError> {
        if name.is_none() || self.at(&TokenKind::LBrace) {
            self.expect(&TokenKind::LBrace)?;
        }
        self.skip_newlines();
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at_eof() {
            if self.at(&TokenKind::Spread) {
                self.advance(); // consume ...
                let expr = self.parse_expression()?;
                fields.push(StructField::Spread(expr));
            } else {
                let field_name = self.expect_identifier()?;
                self.expect(&TokenKind::Colon)?;
                let value = self.parse_expression()?;
                fields.push(StructField::Named { name: field_name, value });
            }
            self.eat(&TokenKind::Comma);
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Expr::StructLiteral { name, fields })
    }
```

Finalize `parse_program` (replace the placeholder in `parse`):

```rust
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
```

- [ ] **Step 3: Run tests and commit**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add struct literal parsing and finalize parse_program"
```

---

### Task 15: Integration tests with real PACT code

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Write integration tests**

```rust
#[test]
fn integration_pact_function_with_pipeline() {
    let input = r#"fn active_admins(users: List<User>) -> List<String> {
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

fn is_admin(role: Role) -> Bool {
  match role {
    Admin => true,
    _ => false,
  }
}"#;
    let prog = parse_program(input);
    assert_eq!(prog.statements.len(), 2);
    assert!(matches!(&prog.statements[0], Statement::TypeDecl(TypeDecl::Union { .. })));
    assert!(matches!(&prog.statements[1], Statement::FnDecl { .. }));
}

#[test]
fn integration_let_with_fn_call_pipeline() {
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
    let input = r#"fn withdraw(account: Account, amount: Int) -> Account or InsufficientFunds {
  ensure amount > 0
  return InsufficientFunds if account.balance < amount
  Account { ...account, balance: account.balance - amount }
}"#;
    let prog = parse_program(input);
    if let Statement::FnDecl { body, error_types, .. } = &prog.statements[0] {
        assert_eq!(error_types, &["InsufficientFunds"]);
        assert!(body.len() >= 3);
        assert!(matches!(&body[0], Statement::Expression(Expr::Ensure(_))));
        assert!(matches!(&body[1], Statement::Return { condition: Some(_), .. }));
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
fn create_user(data: NewUser) -> User needs { db, time, rng } {
  let id: ID = rng.uuid()
  User {
    id: id,
    name: data.name,
    active: true,
  }
}"#;
    let prog = parse_program(input);
    if let Statement::FnDecl { intent, effects, .. } = &prog.statements[0] {
        assert_eq!(intent.as_deref(), Some("create a new user"));
        assert_eq!(effects, &["db", "time", "rng"]);
    } else {
        panic!("Expected FnDecl");
    }
}
```

- [ ] **Step 2: Run tests — fix any issues that emerge**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`

If tests fail, debug and fix. These integration tests exercise the full parser pipeline and may reveal edge cases in how statements and expressions interact.

- [ ] **Step 3: Commit**

```bash
git add src/parser/parser.rs
git commit -m "test: add integration tests with real PACT code (functions, pipelines, types)"
```

---

### Task 16: Update CLI to support AST output

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update main.rs to add --ast flag**

```rust
use std::env;
use std::fs;
use std::process;

use pact::lexer::{Lexer, TokenKind};
use pact::parser::Parser;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: pact <file.pact> [--ast]");
        eprintln!("  Tokenizes a .pact file and prints the token stream.");
        eprintln!("  --ast  Parse and print the AST instead of tokens.");
        process::exit(1);
    }

    let filename = &args[1];
    let show_ast = args.iter().any(|a| a == "--ast");

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    if show_ast {
        let mut parser = Parser::new(tokens, &source);
        match parser.parse() {
            Ok(program) => {
                println!("{:#?}", program);
            }
            Err(errors) => {
                for e in &errors {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }
    } else {
        for token in &tokens {
            match &token.kind {
                TokenKind::Eof => {}
                TokenKind::Newline => {
                    println!("  {:>3}:{:<3}  Newline", token.span.line, token.span.column);
                }
                _ => {
                    println!("  {:>3}:{:<3}  {:?}", token.span.line, token.span.column, token.kind);
                }
            }
        }
        let meaningful = tokens.iter().filter(|t| !matches!(t.kind, TokenKind::Eof | TokenKind::Newline)).count();
        println!("\n{} tokens", meaningful);
    }
}
```

- [ ] **Step 2: Build and test with a real .pact file**

```bash
source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang

# Create a test file
cat > /tmp/test_parser.pact << 'PACT'
type Role = Admin | Editor | Viewer

fn add(a: Int, b: Int) -> Int {
  a + b
}

let x: Int = add(1, 2)
PACT

cargo run -- /tmp/test_parser.pact --ast
```

Expected: AST printed with `Program { statements: [...] }`.

- [ ] **Step 3: Commit and push**

```bash
git add src/main.rs
git commit -m "feat: update CLI with --ast flag to print parsed AST"
git push
```

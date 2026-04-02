# PACT Lexer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a hand-written lexer for the PACT programming language that tokenizes `.pact` source files with rich error messages.

**Architecture:** Single-pass character-by-character lexer with a delimiter depth stack for newline suppression and a mode stack for string interpolation. Every token carries a `Span` with line/column/offset/length. Errors include source line, column pointer, and hint.

**Tech Stack:** Rust 1.94, no external dependencies. `cargo test` for testing.

**Design spec:** `docs/superpowers/specs/2026-04-02-pact-lexer-design.md`

---

## File Structure

```
src/
  main.rs              — CLI entry point: reads .pact file, runs lexer, prints tokens
  lib.rs               — pub mod lexer
  lexer/
    mod.rs             — pub mod re-exports (Lexer, Token, TokenKind, Span, LexerError)
    token.rs           — Token struct, TokenKind enum, Span struct
    lexer.rs           — struct Lexer with all tokenization logic
    errors.rs          — LexerError struct with line/col/message/hint/source_line
```

---

### Task 1: Token and Span types

**Files:**
- Create: `src/lexer/token.rs`
- Create: `src/lexer/mod.rs`
- Create: `src/lib.rs`

- [ ] **Step 1: Write the test for TokenKind and Span**

In `src/lexer/token.rs`, add a `#[cfg(test)]` module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_display() {
        let token = Token {
            kind: TokenKind::Identifier("hello".to_string()),
            span: Span {
                line: 1,
                column: 1,
                offset: 0,
                length: 5,
            },
        };
        assert_eq!(token.span.line, 1);
        assert_eq!(token.span.column, 1);
        assert_eq!(token.span.length, 5);
        assert!(matches!(token.kind, TokenKind::Identifier(ref s) if s == "hello"));
    }

    #[test]
    fn keyword_from_str() {
        assert_eq!(
            TokenKind::keyword_from_str("fn"),
            Some(TokenKind::Fn)
        );
        assert_eq!(
            TokenKind::keyword_from_str("let"),
            Some(TokenKind::Let)
        );
        assert_eq!(
            TokenKind::keyword_from_str("where"),
            None // contextual, not a keyword
        );
        assert_eq!(
            TokenKind::keyword_from_str("hello"),
            None
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: compilation error — `token.rs` doesn't exist yet.

- [ ] **Step 3: Write Token, TokenKind, and Span**

Create `src/lexer/token.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    RawStringLiteral(String),

    // String interpolation
    StringStart,
    StringEnd,
    StringFragment(String),
    InterpolationStart,
    InterpolationEnd,

    // Keywords (23 reserved)
    Fn,
    Let,
    Var,
    Type,
    If,
    Else,
    Match,
    Return,
    Use,
    Intent,
    Ensure,
    Needs,
    Route,
    Test,
    App,
    Check,
    True,
    False,
    Nothing,
    And,
    Or,
    Not,
    As,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Assign,
    Eq,
    NotEq,
    LAngle,
    RAngle,
    LessEq,
    GreaterEq,
    Pipe,
    Question,
    Dot,
    Spread,
    Arrow,
    FatArrow,
    Underscore,

    // Delimiters
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Comma,

    // Other
    Identifier(String),
    Newline,
    Eof,
}

impl TokenKind {
    pub fn keyword_from_str(s: &str) -> Option<TokenKind> {
        match s {
            "fn" => Some(TokenKind::Fn),
            "let" => Some(TokenKind::Let),
            "var" => Some(TokenKind::Var),
            "type" => Some(TokenKind::Type),
            "if" => Some(TokenKind::If),
            "else" => Some(TokenKind::Else),
            "match" => Some(TokenKind::Match),
            "return" => Some(TokenKind::Return),
            "use" => Some(TokenKind::Use),
            "intent" => Some(TokenKind::Intent),
            "ensure" => Some(TokenKind::Ensure),
            "needs" => Some(TokenKind::Needs),
            "route" => Some(TokenKind::Route),
            "test" => Some(TokenKind::Test),
            "app" => Some(TokenKind::App),
            "check" => Some(TokenKind::Check),
            "true" => Some(TokenKind::True),
            "false" => Some(TokenKind::False),
            "nothing" => Some(TokenKind::Nothing),
            "and" => Some(TokenKind::And),
            "or" => Some(TokenKind::Or),
            "not" => Some(TokenKind::Not),
            "as" => Some(TokenKind::As),
            _ => None,
        }
    }
}
```

Create `src/lexer/mod.rs`:

```rust
pub mod token;

pub use token::{Token, TokenKind, Span};
```

Create `src/lib.rs`:

```rust
pub mod lexer;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/token.rs src/lexer/mod.rs src/lib.rs
git commit -m "feat: add Token, TokenKind, and Span types for PACT lexer"
```

---

### Task 2: LexerError with rich context

**Files:**
- Create: `src/lexer/errors.rs`
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write the test for LexerError**

In `src/lexer/errors.rs`, add a `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_with_hint() {
        let error = LexerError {
            line: 12,
            column: 8,
            length: 6,
            message: "Expected ':' after field name".to_string(),
            hint: Some("type fields use 'name: Type' syntax".to_string()),
            source_line: "  name String check { min 1 }".to_string(),
        };

        let output = format!("{}", error);
        assert!(output.contains("line 12, col 8"));
        assert!(output.contains("name String check { min 1 }"));
        assert!(output.contains("^^^^^^"));
        assert!(output.contains("Expected ':' after field name"));
        assert!(output.contains("Hint: type fields use 'name: Type' syntax"));
    }

    #[test]
    fn error_display_without_hint() {
        let error = LexerError {
            line: 1,
            column: 1,
            length: 1,
            message: "Unexpected character '@'".to_string(),
            hint: None,
            source_line: "@hello".to_string(),
        };

        let output = format!("{}", error);
        assert!(output.contains("line 1, col 1"));
        assert!(output.contains("@hello"));
        assert!(output.contains("^"));
        assert!(!output.contains("Hint:"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: compilation error — `errors.rs` doesn't exist yet.

- [ ] **Step 3: Write LexerError**

Create `src/lexer/errors.rs`:

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct LexerError {
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Error at line {}, col {}:", self.line, self.column)?;
        writeln!(f, "  {}", self.source_line)?;
        let padding = self.column - 1 + 2; // +2 for the "  " prefix
        let carets = "^".repeat(self.length);
        writeln!(f, "{:>width$}{}", "", carets, width = padding)?;
        write!(f, "  {}", self.message)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}
```

Update `src/lexer/mod.rs` to add:

```rust
pub mod token;
pub mod errors;

pub use token::{Token, TokenKind, Span};
pub use errors::LexerError;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 4 tests pass (2 from task 1, 2 from task 2).

- [ ] **Step 5: Commit**

```bash
git add src/lexer/errors.rs src/lexer/mod.rs
git commit -m "feat: add LexerError with rich context (source line, caret pointer, hint)"
```

---

### Task 3: Lexer scaffold — struct, `new()`, and single-character operators

**Files:**
- Create: `src/lexer/lexer.rs`
- Modify: `src/lexer/mod.rs`

- [ ] **Step 1: Write tests for basic operators and EOF**

In `src/lexer/lexer.rs`, add a `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::TokenKind;

    fn tokenize(input: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty_input() {
        assert_eq!(tokenize(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn single_char_operators() {
        assert_eq!(
            tokenize("+ - * / = : , . ?"),
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Assign,
                TokenKind::Colon,
                TokenKind::Comma,
                TokenKind::Dot,
                TokenKind::Question,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn delimiters() {
        assert_eq!(
            tokenize("( ) { } [ ]"),
            vec![
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::LBracket,
                TokenKind::RBracket,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn pipe_and_angle_brackets() {
        assert_eq!(
            tokenize("| < >"),
            vec![
                TokenKind::Pipe,
                TokenKind::LAngle,
                TokenKind::RAngle,
                TokenKind::Eof,
            ]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: compilation error — `Lexer` struct doesn't exist.

- [ ] **Step 3: Write Lexer struct with single-char tokenization**

Create `src/lexer/lexer.rs`:

```rust
use crate::lexer::errors::LexerError;
use crate::lexer::token::{Token, TokenKind, Span};

pub struct Lexer {
    source: Vec<char>,
    source_str: String,
    pos: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    delimiter_depth: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            source: input.chars().collect(),
            source_str: input.to_string(),
            pos: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            delimiter_depth: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        while !self.is_at_end() {
            self.skip_whitespace_except_newline();
            if self.is_at_end() {
                break;
            }
            let token = self.next_token()?;
            self.tokens.push(token);
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: self.current_span(0),
        });
        Ok(self.tokens.clone())
    }

    fn next_token(&mut self) -> Result<Token, LexerError> {
        let ch = self.current();
        let start_line = self.line;
        let start_col = self.column;
        let start_offset = self.pos;

        match ch {
            '+' => { self.advance(); Ok(self.make_token(TokenKind::Plus, start_line, start_col, start_offset, 1)) }
            '*' => { self.advance(); Ok(self.make_token(TokenKind::Star, start_line, start_col, start_offset, 1)) }
            '/' => {
                if self.peek() == Some('/') {
                    self.skip_comment();
                    if self.is_at_end() {
                        return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
                    }
                    self.next_token()
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::Slash, start_line, start_col, start_offset, 1))
                }
            }
            '|' => { self.advance(); Ok(self.make_token(TokenKind::Pipe, start_line, start_col, start_offset, 1)) }
            '?' => { self.advance(); Ok(self.make_token(TokenKind::Question, start_line, start_col, start_offset, 1)) }
            ':' => { self.advance(); Ok(self.make_token(TokenKind::Colon, start_line, start_col, start_offset, 1)) }
            ',' => { self.advance(); Ok(self.make_token(TokenKind::Comma, start_line, start_col, start_offset, 1)) }
            '(' => { self.delimiter_depth += 1; self.advance(); Ok(self.make_token(TokenKind::LParen, start_line, start_col, start_offset, 1)) }
            ')' => { if self.delimiter_depth > 0 { self.delimiter_depth -= 1; } self.advance(); Ok(self.make_token(TokenKind::RParen, start_line, start_col, start_offset, 1)) }
            '{' => { self.delimiter_depth += 1; self.advance(); Ok(self.make_token(TokenKind::LBrace, start_line, start_col, start_offset, 1)) }
            '}' => { if self.delimiter_depth > 0 { self.delimiter_depth -= 1; } self.advance(); Ok(self.make_token(TokenKind::RBrace, start_line, start_col, start_offset, 1)) }
            '[' => { self.delimiter_depth += 1; self.advance(); Ok(self.make_token(TokenKind::LBracket, start_line, start_col, start_offset, 1)) }
            ']' => { if self.delimiter_depth > 0 { self.delimiter_depth -= 1; } self.advance(); Ok(self.make_token(TokenKind::RBracket, start_line, start_col, start_offset, 1)) }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::LessEq, start_line, start_col, start_offset, 2))
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::LAngle, start_line, start_col, start_offset, 1))
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::GreaterEq, start_line, start_col, start_offset, 2))
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::RAngle, start_line, start_col, start_offset, 1))
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::Eq, start_line, start_col, start_offset, 2))
                } else if self.peek() == Some('>') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::FatArrow, start_line, start_col, start_offset, 2))
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::Assign, start_line, start_col, start_offset, 1))
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::NotEq, start_line, start_col, start_offset, 2))
                } else {
                    Err(self.error(1, "Unexpected character '!'", Some("PACT uses 'not' keyword instead of '!'")))
                }
            }
            '-' => {
                if self.peek() == Some('>') {
                    self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::Arrow, start_line, start_col, start_offset, 2))
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::Minus, start_line, start_col, start_offset, 1))
                }
            }
            '.' => {
                if self.peek() == Some('.') && self.peek_at(2) == Some('.') {
                    self.advance(); self.advance(); self.advance();
                    Ok(self.make_token(TokenKind::Spread, start_line, start_col, start_offset, 3))
                } else {
                    self.advance();
                    Ok(self.make_token(TokenKind::Dot, start_line, start_col, start_offset, 1))
                }
            }
            '_' => {
                if self.peek().map_or(true, |c| !c.is_alphanumeric() && c != '_') {
                    self.advance();
                    Ok(self.make_token(TokenKind::Underscore, start_line, start_col, start_offset, 1))
                } else {
                    self.read_identifier_or_keyword(start_line, start_col, start_offset)
                }
            }
            '\n' => {
                self.handle_newline(start_line, start_col, start_offset)
            }
            c if c.is_ascii_digit() => {
                self.read_number(start_line, start_col, start_offset)
            }
            c if c.is_alphabetic() || c == '_' => {
                self.read_identifier_or_keyword(start_line, start_col, start_offset)
            }
            '"' => {
                self.read_string(start_line, start_col, start_offset)
            }
            c => {
                Err(self.error(1, &format!("Unexpected character '{}'", c), None))
            }
        }
    }

    // --- Helper methods ---

    fn current(&self) -> char {
        self.source[self.pos]
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> char {
        let ch = self.source[self.pos];
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        ch
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn skip_whitespace_except_newline(&mut self) {
        while !self.is_at_end() && self.current() != '\n' && self.current().is_ascii_whitespace() {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        // Skip // and everything until end of line
        while !self.is_at_end() && self.current() != '\n' {
            self.advance();
        }
    }

    fn make_token(&self, kind: TokenKind, line: usize, column: usize, offset: usize, length: usize) -> Token {
        Token {
            kind,
            span: Span { line, column, offset, length },
        }
    }

    fn current_span(&self, length: usize) -> Span {
        Span {
            line: self.line,
            column: self.column,
            offset: self.pos,
            length,
        }
    }

    fn get_source_line(&self, line: usize) -> String {
        self.source_str.lines().nth(line - 1).unwrap_or("").to_string()
    }

    fn error(&self, length: usize, message: &str, hint: Option<&str>) -> LexerError {
        LexerError {
            line: self.line,
            column: self.column,
            length,
            message: message.to_string(),
            hint: hint.map(|s| s.to_string()),
            source_line: self.get_source_line(self.line),
        }
    }

    // --- Placeholder methods for later tasks ---

    fn handle_newline(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
        self.advance(); // consume '\n'

        if self.delimiter_depth > 0 {
            // Inside balanced delimiters — suppress newline
            if self.is_at_end() {
                return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
            }
            return self.next_token();
        }

        // Peek ahead: skip whitespace, check if next non-whitespace is `|`
        let mut peek_pos = self.pos;
        while peek_pos < self.source.len() && self.source[peek_pos] != '\n' && self.source[peek_pos].is_ascii_whitespace() {
            peek_pos += 1;
        }
        if peek_pos < self.source.len() && self.source[peek_pos] == '|' {
            // Next meaningful char is `|` — suppress newline
            if self.is_at_end() {
                return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
            }
            return self.next_token();
        }

        // Consume consecutive newlines as a single Newline token
        while !self.is_at_end() && self.current() == '\n' {
            self.advance();
            self.skip_whitespace_except_newline();
        }

        Ok(self.make_token(TokenKind::Newline, start_line, start_col, start_offset, 1))
    }

    fn read_number(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
        // Placeholder — implemented in Task 4
        Err(self.error(1, "Numbers not yet implemented", None))
    }

    fn read_identifier_or_keyword(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
        // Placeholder — implemented in Task 5
        Err(self.error(1, "Identifiers not yet implemented", None))
    }

    fn read_string(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
        // Placeholder — implemented in Task 6
        Err(self.error(1, "Strings not yet implemented", None))
    }
}
```

Update `src/lexer/mod.rs`:

```rust
pub mod token;
pub mod errors;
pub mod lexer;

pub use token::{Token, TokenKind, Span};
pub use errors::LexerError;
pub use lexer::Lexer;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 6 tests pass (2 from task 1, 2 from task 2, 4 new).

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs src/lexer/mod.rs
git commit -m "feat: add Lexer scaffold with single-char operators, delimiters, multi-char operators"
```

---

### Task 4: Number literals (Int and Float)

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests for numbers**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn integer_literals() {
    assert_eq!(
        tokenize("0 42 1000"),
        vec![
            TokenKind::IntLiteral(0),
            TokenKind::IntLiteral(42),
            TokenKind::IntLiteral(1000),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn float_literals() {
    assert_eq!(
        tokenize("3.14 0.5 100.0"),
        vec![
            TokenKind::FloatLiteral(3.14),
            TokenKind::FloatLiteral(0.5),
            TokenKind::FloatLiteral(100.0),
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: FAIL — `read_number` returns error placeholder.

- [ ] **Step 3: Implement `read_number`**

Replace the `read_number` placeholder in `src/lexer/lexer.rs`:

```rust
fn read_number(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    let mut num_str = String::new();
    let mut is_float = false;

    while !self.is_at_end() && (self.current().is_ascii_digit() || self.current() == '.') {
        if self.current() == '.' {
            // Check if this is a spread `...` or field access `.foo`
            if self.peek() == Some('.') {
                break; // Part of `..` or `...`
            }
            if is_float {
                break; // Second dot — not part of this number
            }
            if self.peek().map_or(true, |c| !c.is_ascii_digit()) {
                break; // Dot not followed by digit — field access
            }
            is_float = true;
        }
        num_str.push(self.advance());
    }

    let length = num_str.len();
    if is_float {
        let value: f64 = num_str.parse().map_err(|_| {
            LexerError {
                line: start_line,
                column: start_col,
                length,
                message: format!("Invalid float literal '{}'", num_str),
                hint: None,
                source_line: self.get_source_line(start_line),
            }
        })?;
        Ok(self.make_token(TokenKind::FloatLiteral(value), start_line, start_col, start_offset, length))
    } else {
        let value: i64 = num_str.parse().map_err(|_| {
            LexerError {
                line: start_line,
                column: start_col,
                length,
                message: format!("Invalid integer literal '{}'", num_str),
                hint: Some("Integer values must fit in 64-bit signed range".to_string()),
                source_line: self.get_source_line(start_line),
            }
        })?;
        Ok(self.make_token(TokenKind::IntLiteral(value), start_line, start_col, start_offset, length))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "feat: add integer and float literal lexing"
```

---

### Task 5: Identifiers and keywords

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests for identifiers and keywords**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn identifiers() {
    assert_eq!(
        tokenize("hello world foo_bar _private"),
        vec![
            TokenKind::Identifier("hello".to_string()),
            TokenKind::Identifier("world".to_string()),
            TokenKind::Identifier("foo_bar".to_string()),
            TokenKind::Identifier("_private".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn keywords() {
    assert_eq!(
        tokenize("fn let var type if else match return"),
        vec![
            TokenKind::Fn,
            TokenKind::Let,
            TokenKind::Var,
            TokenKind::Type,
            TokenKind::If,
            TokenKind::Else,
            TokenKind::Match,
            TokenKind::Return,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn contextual_words_are_identifiers() {
    assert_eq!(
        tokenize("where by to first last ascending descending"),
        vec![
            TokenKind::Identifier("where".to_string()),
            TokenKind::Identifier("by".to_string()),
            TokenKind::Identifier("to".to_string()),
            TokenKind::Identifier("first".to_string()),
            TokenKind::Identifier("last".to_string()),
            TokenKind::Identifier("ascending".to_string()),
            TokenKind::Identifier("descending".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn bool_literals_from_keywords() {
    assert_eq!(
        tokenize("true false"),
        vec![
            TokenKind::BoolLiteral(true),
            TokenKind::BoolLiteral(false),
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: FAIL — `read_identifier_or_keyword` returns error placeholder.

- [ ] **Step 3: Implement `read_identifier_or_keyword`**

Replace the placeholder in `src/lexer/lexer.rs`:

```rust
fn read_identifier_or_keyword(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    let mut ident = String::new();

    while !self.is_at_end() && (self.current().is_alphanumeric() || self.current() == '_') {
        ident.push(self.advance());
    }

    let length = ident.len();

    // Check for true/false → BoolLiteral
    let kind = match ident.as_str() {
        "true" => TokenKind::BoolLiteral(true),
        "false" => TokenKind::BoolLiteral(false),
        _ => {
            // Check reserved keywords
            TokenKind::keyword_from_str(&ident)
                .unwrap_or(TokenKind::Identifier(ident))
        }
    };

    Ok(self.make_token(kind, start_line, start_col, start_offset, length))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 12 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "feat: add identifier and keyword lexing with bool literals"
```

---

### Task 6: String literals with interpolation

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests for strings**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn simple_string() {
    assert_eq!(
        tokenize(r#""hello world""#),
        vec![
            TokenKind::StringStart,
            TokenKind::StringFragment("hello world".to_string()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn string_with_interpolation() {
    assert_eq!(
        tokenize(r#""hello {name}""#),
        vec![
            TokenKind::StringStart,
            TokenKind::StringFragment("hello ".to_string()),
            TokenKind::InterpolationStart,
            TokenKind::Identifier("name".to_string()),
            TokenKind::InterpolationEnd,
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn string_with_escaped_braces() {
    assert_eq!(
        tokenize(r#""JSON: {{key: value}}""#),
        vec![
            TokenKind::StringStart,
            TokenKind::StringFragment("JSON: {key: value}".to_string()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn string_with_dotted_interpolation() {
    assert_eq!(
        tokenize(r#""Hello {user.name}""#),
        vec![
            TokenKind::StringStart,
            TokenKind::StringFragment("Hello ".to_string()),
            TokenKind::InterpolationStart,
            TokenKind::Identifier("user".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("name".to_string()),
            TokenKind::InterpolationEnd,
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn empty_string() {
    assert_eq!(
        tokenize(r#""""#),
        vec![
            TokenKind::StringStart,
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: FAIL — `read_string` returns error placeholder.

- [ ] **Step 3: Implement `read_string`**

Replace the `read_string` placeholder in `src/lexer/lexer.rs`. Also add `read_string_content` and `read_interpolation` helpers:

```rust
fn read_string(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    self.advance(); // consume opening "

    // Check for multiline """ or empty string ""
    if !self.is_at_end() && self.current() == '"' {
        if self.peek() == Some('"') {
            // Multiline string """..."""
            self.advance(); // second "
            self.advance(); // third "
            return self.read_multiline_string(start_line, start_col, start_offset);
        } else {
            // Empty string ""
            self.advance(); // closing "
            let mut result = vec![
                self.make_token(TokenKind::StringStart, start_line, start_col, start_offset, 1),
                self.make_token(TokenKind::StringEnd, self.line, self.column - 1, self.pos - 1, 1),
            ];
            // Push all but last, return last
            let last = result.pop().unwrap();
            for t in result {
                self.tokens.push(t);
            }
            return Ok(last);
        }
    }

    // Emit StringStart
    self.tokens.push(self.make_token(TokenKind::StringStart, start_line, start_col, start_offset, 1));

    // Read string content with interpolation
    self.read_string_content()
}

fn read_string_content(&mut self) -> Result<Token, LexerError> {
    let mut fragment = String::new();
    let frag_start_line = self.line;
    let frag_start_col = self.column;
    let frag_start_offset = self.pos;

    loop {
        if self.is_at_end() {
            return Err(LexerError {
                line: frag_start_line,
                column: frag_start_col,
                length: 1,
                message: "Unterminated string literal".to_string(),
                hint: Some("Add a closing '\"' to end the string".to_string()),
                source_line: self.get_source_line(frag_start_line),
            });
        }

        match self.current() {
            '"' => {
                // End of string
                if !fragment.is_empty() {
                    self.tokens.push(self.make_token(
                        TokenKind::StringFragment(fragment),
                        frag_start_line, frag_start_col, frag_start_offset,
                        self.pos - frag_start_offset,
                    ));
                }
                let end_line = self.line;
                let end_col = self.column;
                let end_offset = self.pos;
                self.advance(); // consume closing "
                return Ok(self.make_token(TokenKind::StringEnd, end_line, end_col, end_offset, 1));
            }
            '{' => {
                if self.peek() == Some('{') {
                    // Escaped brace {{ -> {
                    self.advance();
                    self.advance();
                    fragment.push('{');
                } else {
                    // Interpolation start
                    if !fragment.is_empty() {
                        self.tokens.push(self.make_token(
                            TokenKind::StringFragment(fragment.clone()),
                            frag_start_line, frag_start_col, frag_start_offset,
                            self.pos - frag_start_offset,
                        ));
                        fragment.clear();
                    }
                    let interp_line = self.line;
                    let interp_col = self.column;
                    let interp_offset = self.pos;
                    self.advance(); // consume {
                    self.tokens.push(self.make_token(TokenKind::InterpolationStart, interp_line, interp_col, interp_offset, 1));

                    // Lex tokens inside interpolation until matching }
                    self.read_interpolation()?;

                    // Reset fragment tracking for text after interpolation
                    // fragment is already cleared above
                    // Update fragment start position
                    // We'll continue the loop which will set positions correctly
                    // Actually we need to update frag_start for next fragment piece
                    // Can't reassign frag_start_line etc because they're not mut
                    // Instead, just return to read_string_content recursively
                    return self.read_string_content();
                }
            }
            '}' => {
                if self.peek() == Some('}') {
                    // Escaped brace }} -> }
                    self.advance();
                    self.advance();
                    fragment.push('}');
                } else {
                    return Err(LexerError {
                        line: self.line,
                        column: self.column,
                        length: 1,
                        message: "Unexpected '}' in string".to_string(),
                        hint: Some("Use '}}' for a literal '}' in strings".to_string()),
                        source_line: self.get_source_line(self.line),
                    });
                }
            }
            '\\' => {
                self.advance(); // consume backslash
                if self.is_at_end() {
                    return Err(self.error(1, "Unterminated escape sequence", None));
                }
                match self.current() {
                    'n' => { self.advance(); fragment.push('\n'); }
                    't' => { self.advance(); fragment.push('\t'); }
                    'r' => { self.advance(); fragment.push('\r'); }
                    '\\' => { self.advance(); fragment.push('\\'); }
                    '"' => { self.advance(); fragment.push('"'); }
                    c => {
                        return Err(self.error(1, &format!("Unknown escape sequence '\\{}'", c), Some("Valid escapes: \\n, \\t, \\r, \\\\, \\\"")));
                    }
                }
            }
            c => {
                self.advance();
                fragment.push(c);
            }
        }
    }
}

fn read_interpolation(&mut self) -> Result<(), LexerError> {
    // Lex tokens until we hit an unmatched }
    let mut brace_depth = 0;

    loop {
        self.skip_whitespace_except_newline();

        if self.is_at_end() {
            return Err(self.error(1, "Unterminated string interpolation", Some("Add a closing '}' to end the interpolation")));
        }

        if self.current() == '}' && brace_depth == 0 {
            let end_line = self.line;
            let end_col = self.column;
            let end_offset = self.pos;
            self.advance();
            self.tokens.push(self.make_token(TokenKind::InterpolationEnd, end_line, end_col, end_offset, 1));
            return Ok(());
        }

        if self.current() == '{' {
            brace_depth += 1;
        }
        if self.current() == '}' {
            brace_depth -= 1;
        }

        let token = self.next_token()?;
        self.tokens.push(token);
    }
}

fn read_multiline_string(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    // Emit StringStart for the opening """
    self.tokens.push(self.make_token(TokenKind::StringStart, start_line, start_col, start_offset, 3));

    let mut fragment = String::new();
    let frag_start_line = self.line;
    let frag_start_col = self.column;
    let frag_start_offset = self.pos;

    loop {
        if self.is_at_end() {
            return Err(LexerError {
                line: start_line,
                column: start_col,
                length: 3,
                message: "Unterminated multiline string".to_string(),
                hint: Some(r#"Add closing '"""' to end the multiline string"#.to_string()),
                source_line: self.get_source_line(start_line),
            });
        }

        if self.current() == '"' && self.peek() == Some('"') && self.peek_at(2) == Some('"') {
            // End of multiline string
            if !fragment.is_empty() {
                self.tokens.push(self.make_token(
                    TokenKind::StringFragment(fragment),
                    frag_start_line, frag_start_col, frag_start_offset,
                    self.pos - frag_start_offset,
                ));
            }
            let end_line = self.line;
            let end_col = self.column;
            let end_offset = self.pos;
            self.advance(); self.advance(); self.advance(); // consume """
            return Ok(self.make_token(TokenKind::StringEnd, end_line, end_col, end_offset, 3));
        }

        if self.current() == '{' {
            if self.peek() == Some('{') {
                self.advance(); self.advance();
                fragment.push('{');
            } else {
                // Interpolation in multiline string
                if !fragment.is_empty() {
                    self.tokens.push(self.make_token(
                        TokenKind::StringFragment(fragment.clone()),
                        frag_start_line, frag_start_col, frag_start_offset,
                        self.pos - frag_start_offset,
                    ));
                    fragment.clear();
                }
                let interp_line = self.line;
                let interp_col = self.column;
                let interp_offset = self.pos;
                self.advance();
                self.tokens.push(self.make_token(TokenKind::InterpolationStart, interp_line, interp_col, interp_offset, 1));
                self.read_interpolation()?;

                // Continue reading the rest of the multiline string via recursion would be complex,
                // so just continue the loop and update fragment start
                // (fragment is cleared, will start fresh)
                continue;
            }
        } else if self.current() == '}' && self.peek() == Some('}') {
            self.advance(); self.advance();
            fragment.push('}');
        } else {
            fragment.push(self.advance());
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 17 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "feat: add string lexing with interpolation, multiline, and escape sequences"
```

---

### Task 7: Raw strings and `intent` string literals

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests for raw strings and intent**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn raw_string() {
    assert_eq!(
        tokenize(r#"raw"no {interpolation} here""#),
        vec![
            TokenKind::RawStringLiteral("no {interpolation} here".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn intent_with_string() {
    assert_eq!(
        tokenize(r#"intent "do something""#),
        vec![
            TokenKind::Intent,
            TokenKind::StringStart,
            TokenKind::StringFragment("do something".to_string()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: FAIL — `raw"..."` is lexed as identifier `raw` followed by a string, not as `RawStringLiteral`.

- [ ] **Step 3: Handle `raw` prefix in identifier lexing**

In `read_identifier_or_keyword`, after reading the identifier string, add a check: if the identifier is `"raw"` and the next character is `"`, consume the raw string:

```rust
fn read_identifier_or_keyword(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    let mut ident = String::new();

    while !self.is_at_end() && (self.current().is_alphanumeric() || self.current() == '_') {
        ident.push(self.advance());
    }

    // Check for raw string: raw"..."
    if ident == "raw" && !self.is_at_end() && self.current() == '"' {
        return self.read_raw_string(start_line, start_col, start_offset);
    }

    let length = ident.len();

    let kind = match ident.as_str() {
        "true" => TokenKind::BoolLiteral(true),
        "false" => TokenKind::BoolLiteral(false),
        _ => {
            TokenKind::keyword_from_str(&ident)
                .unwrap_or(TokenKind::Identifier(ident))
        }
    };

    Ok(self.make_token(kind, start_line, start_col, start_offset, length))
}

fn read_raw_string(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    self.advance(); // consume opening "
    let mut content = String::new();

    loop {
        if self.is_at_end() {
            return Err(LexerError {
                line: start_line,
                column: start_col,
                length: 4, // raw"
                message: "Unterminated raw string literal".to_string(),
                hint: Some("Add a closing '\"' to end the raw string".to_string()),
                source_line: self.get_source_line(start_line),
            });
        }
        if self.current() == '"' {
            self.advance(); // consume closing "
            let length = self.pos - start_offset;
            return Ok(self.make_token(TokenKind::RawStringLiteral(content), start_line, start_col, start_offset, length));
        }
        content.push(self.advance());
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 19 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "feat: add raw string literals and intent keyword with string"
```

---

### Task 8: Newline handling (continuation rules)

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests for newline behavior**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn newline_as_statement_separator() {
    assert_eq!(
        tokenize("let x\nlet y"),
        vec![
            TokenKind::Let,
            TokenKind::Identifier("x".to_string()),
            TokenKind::Newline,
            TokenKind::Let,
            TokenKind::Identifier("y".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_inside_braces() {
    assert_eq!(
        tokenize("{\na\n}"),
        vec![
            TokenKind::LBrace,
            TokenKind::Identifier("a".to_string()),
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_inside_parens() {
    assert_eq!(
        tokenize("(\na\n)"),
        vec![
            TokenKind::LParen,
            TokenKind::Identifier("a".to_string()),
            TokenKind::RParen,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_before_pipe() {
    assert_eq!(
        tokenize("users\n  | filter"),
        vec![
            TokenKind::Identifier("users".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("filter".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_after_pipe() {
    // After `|`, the next line continues the pipeline
    assert_eq!(
        tokenize("users |\nfilter"),
        vec![
            TokenKind::Identifier("users".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("filter".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_after_comma() {
    assert_eq!(
        tokenize("a,\nb"),
        vec![
            TokenKind::Identifier("a".to_string()),
            TokenKind::Comma,
            TokenKind::Identifier("b".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn newline_suppressed_after_arrow() {
    assert_eq!(
        tokenize("fn foo() ->\nInt"),
        vec![
            TokenKind::Fn,
            TokenKind::Identifier("foo".to_string()),
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::Identifier("Int".to_string()),
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify which fail**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: Some tests pass (braces/parens already handled by `delimiter_depth`), others fail (pipe continuation, comma continuation).

- [ ] **Step 3: Add continuation token tracking to `handle_newline`**

The `handle_newline` method already handles `delimiter_depth > 0` and peek-ahead for `|`. Add tracking for the "after continuation token" rule. Add a field `last_token_kind: Option<TokenKind>` to `Lexer` struct and update it after each token. Then check it in `handle_newline`:

Add to `Lexer` struct:

```rust
pub struct Lexer {
    source: Vec<char>,
    source_str: String,
    pos: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    delimiter_depth: usize,
    last_token_kind: Option<TokenKind>,
}
```

Initialize `last_token_kind: None` in `new()`.

In `tokenize()`, after pushing each token, update `last_token_kind`:

```rust
pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
    while !self.is_at_end() {
        self.skip_whitespace_except_newline();
        if self.is_at_end() {
            break;
        }
        let token = self.next_token()?;
        self.last_token_kind = Some(token.kind.clone());
        self.tokens.push(token);
    }
    self.tokens.push(Token {
        kind: TokenKind::Eof,
        span: self.current_span(0),
    });
    Ok(self.tokens.clone())
}
```

Update `handle_newline` to also check `last_token_kind`:

```rust
fn handle_newline(&mut self, start_line: usize, start_col: usize, start_offset: usize) -> Result<Token, LexerError> {
    self.advance(); // consume '\n'

    // Rule 1: Inside balanced delimiters — suppress
    if self.delimiter_depth > 0 {
        if self.is_at_end() {
            return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
        }
        return self.next_token();
    }

    // Rule 2: After continuation tokens — suppress
    let is_continuation = matches!(
        self.last_token_kind,
        Some(TokenKind::Pipe)
        | Some(TokenKind::Arrow)
        | Some(TokenKind::FatArrow)
        | Some(TokenKind::Comma)
        | Some(TokenKind::Assign)
        | Some(TokenKind::Eq)
        | Some(TokenKind::NotEq)
        | Some(TokenKind::LessEq)
        | Some(TokenKind::GreaterEq)
        | Some(TokenKind::Plus)
        | Some(TokenKind::Minus)
        | Some(TokenKind::Star)
        | Some(TokenKind::Slash)
        | Some(TokenKind::And)
        | Some(TokenKind::Or)
    );
    if is_continuation {
        if self.is_at_end() {
            return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
        }
        return self.next_token();
    }

    // Rule 3: Next non-whitespace is `|` — suppress
    let mut peek_pos = self.pos;
    while peek_pos < self.source.len() && self.source[peek_pos] != '\n' && self.source[peek_pos].is_ascii_whitespace() {
        peek_pos += 1;
    }
    if peek_pos < self.source.len() && self.source[peek_pos] == '|' {
        if self.is_at_end() {
            return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
        }
        return self.next_token();
    }

    // Rule 4: Emit Newline, collapse consecutive newlines
    while !self.is_at_end() && self.current() == '\n' {
        self.advance();
        self.skip_whitespace_except_newline();
    }

    // Skip trailing whitespace-only "newlines" at end of input
    if self.is_at_end() {
        return Ok(self.make_token(TokenKind::Eof, self.line, self.column, self.pos, 0));
    }

    // Check AGAIN if next non-whitespace is `|` after collapsing
    let mut peek_pos = self.pos;
    while peek_pos < self.source.len() && self.source[peek_pos] != '\n' && self.source[peek_pos].is_ascii_whitespace() {
        peek_pos += 1;
    }
    if peek_pos < self.source.len() && self.source[peek_pos] == '|' {
        return self.next_token();
    }

    Ok(self.make_token(TokenKind::Newline, start_line, start_col, start_offset, 1))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: 26 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "feat: add newline continuation rules (delimiters, pipe, comma, arrow)"
```

---

### Task 9: Multi-char operators and comment skipping

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write tests**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn multi_char_operators() {
    assert_eq!(
        tokenize("== != <= >= -> => ..."),
        vec![
            TokenKind::Eq,
            TokenKind::NotEq,
            TokenKind::LessEq,
            TokenKind::GreaterEq,
            TokenKind::Arrow,
            TokenKind::FatArrow,
            TokenKind::Spread,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn comments_are_skipped() {
    assert_eq!(
        tokenize("let x // this is a comment\nlet y"),
        vec![
            TokenKind::Let,
            TokenKind::Identifier("x".to_string()),
            TokenKind::Newline,
            TokenKind::Let,
            TokenKind::Identifier("y".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn comment_at_end_of_file() {
    assert_eq!(
        tokenize("let x // trailing comment"),
        vec![
            TokenKind::Let,
            TokenKind::Identifier("x".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn underscore_wildcard() {
    assert_eq!(
        tokenize("_ => x"),
        vec![
            TokenKind::Underscore,
            TokenKind::FatArrow,
            TokenKind::Identifier("x".to_string()),
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run tests**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: All pass — these operators are already implemented in Task 3. This task verifies and locks them in.

- [ ] **Step 3: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "test: add tests for multi-char operators, comments, and underscore wildcard"
```

---

### Task 10: Integration test — real PACT code

**Files:**
- Modify: `src/lexer/lexer.rs`

- [ ] **Step 1: Write integration test with real PACT snippet**

Add to the `#[cfg(test)]` module in `src/lexer/lexer.rs`:

```rust
#[test]
fn real_pact_function() {
    let input = r#"fn add(a: Int, b: Int) -> Int {
  a + b
}"#;
    let kinds = tokenize(input);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Fn,
            TokenKind::Identifier("add".to_string()),
            TokenKind::LParen,
            TokenKind::Identifier("a".to_string()),
            TokenKind::Colon,
            TokenKind::Identifier("Int".to_string()),
            TokenKind::Comma,
            TokenKind::Identifier("b".to_string()),
            TokenKind::Colon,
            TokenKind::Identifier("Int".to_string()),
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::Identifier("Int".to_string()),
            TokenKind::LBrace,
            TokenKind::Identifier("a".to_string()),
            TokenKind::Plus,
            TokenKind::Identifier("b".to_string()),
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn real_pact_pipeline() {
    let input = r#"users
  | filter where .active
  | map to .name
  | sort by .name"#;
    let kinds = tokenize(input);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier("users".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("filter".to_string()),
            TokenKind::Identifier("where".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("active".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("map".to_string()),
            TokenKind::Identifier("to".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("name".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("sort".to_string()),
            TokenKind::Identifier("by".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("name".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn real_pact_type_with_union() {
    let input = "type Role = Admin | Editor | Viewer";
    let kinds = tokenize(input);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Type,
            TokenKind::Identifier("Role".to_string()),
            TokenKind::Assign,
            TokenKind::Identifier("Admin".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("Editor".to_string()),
            TokenKind::Pipe,
            TokenKind::Identifier("Viewer".to_string()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn real_pact_route() {
    let input = r#"route GET "/users/{id}" {
  find_user(request.params.id)
    | on success: respond 200 with .
}"#;
    let kinds = tokenize(input);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Route,
            TokenKind::Identifier("GET".to_string()),
            TokenKind::StringStart,
            TokenKind::StringFragment("/users/".to_string()),
            TokenKind::InterpolationStart,
            TokenKind::Identifier("id".to_string()),
            TokenKind::InterpolationEnd,
            TokenKind::StringEnd,
            TokenKind::LBrace,
            TokenKind::Identifier("find_user".to_string()),
            TokenKind::LParen,
            TokenKind::Identifier("request".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("params".to_string()),
            TokenKind::Dot,
            TokenKind::Identifier("id".to_string()),
            TokenKind::RParen,
            TokenKind::Pipe,
            TokenKind::Identifier("on".to_string()),
            TokenKind::Identifier("success".to_string()),
            TokenKind::Colon,
            TokenKind::Identifier("respond".to_string()),
            TokenKind::IntLiteral(200),
            TokenKind::Identifier("with".to_string()),
            TokenKind::Dot,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn real_pact_check_syntax() {
    let input = "name: String check { min 1, max 100 }";
    let kinds = tokenize(input);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Identifier("name".to_string()),
            TokenKind::Colon,
            TokenKind::Identifier("String".to_string()),
            TokenKind::Check,
            TokenKind::LBrace,
            TokenKind::Identifier("min".to_string()),
            TokenKind::IntLiteral(1),
            TokenKind::Comma,
            TokenKind::Identifier("max".to_string()),
            TokenKind::IntLiteral(100),
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run tests**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo test`
Expected: All pass. If any fail, fix the lexer logic.

- [ ] **Step 3: Commit**

```bash
git add src/lexer/lexer.rs
git commit -m "test: add integration tests with real PACT code snippets"
```

---

### Task 11: CLI entry point

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write `main.rs`**

```rust
use std::env;
use std::fs;
use std::process;

use pact::lexer::{Lexer, TokenKind};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: pact <file.pact>");
        eprintln!("  Tokenizes a .pact file and prints the token stream.");
        process::exit(1);
    }

    let filename = &args[1];
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let mut lexer = Lexer::new(&source);
    match lexer.tokenize() {
        Ok(tokens) => {
            for token in &tokens {
                match &token.kind {
                    TokenKind::Eof => {}
                    TokenKind::Newline => {
                        println!(
                            "  {:>3}:{:<3}  Newline",
                            token.span.line, token.span.column
                        );
                    }
                    _ => {
                        println!(
                            "  {:>3}:{:<3}  {:?}",
                            token.span.line, token.span.column, token.kind
                        );
                    }
                }
            }
            // Count tokens excluding Eof and Newline
            let meaningful = tokens.iter().filter(|t| !matches!(t.kind, TokenKind::Eof | TokenKind::Newline)).count();
            println!("\n{} tokens", meaningful);
        }
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}
```

- [ ] **Step 2: Build and test with a sample file**

Run: `source "$HOME/.cargo/env" && cd /Users/kikotvit/Documents/REPOS/KikotVit/pact-lang && cargo build 2>&1`
Expected: compiles successfully.

Create a test file and run:

```bash
echo 'fn add(a: Int, b: Int) -> Int {
  a + b
}' > /tmp/test.pact
cargo run -- /tmp/test.pact
```

Expected output: token listing with line:col positions.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add CLI entry point for tokenizing .pact files"
```

---

### Task 12: Copy spec files into repo and initial push

**Files:**
- Create: `docs/spec/PACT_SPEC_v0.1.md`
- Create: `docs/spec/PACT_Examples_Backend.pact`

- [ ] **Step 1: Copy spec files**

```bash
mkdir -p docs/spec
cp "/Users/kikotvit/Documents/PACT lang/PACT_SPEC_v0.1.md" docs/spec/
cp "/Users/kikotvit/Documents/PACT lang/PACT_Examples_Backend.pact" docs/spec/
```

- [ ] **Step 2: Commit everything and push**

```bash
git add -A
git commit -m "feat: PACT lexer v0.1 — hand-written lexer with full token set, string interpolation, newline rules, rich errors"
git push -u origin main
```

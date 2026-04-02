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
        loop {
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            self.tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(self.tokens.clone())
    }

    fn next_token(&mut self) -> Result<Token, LexerError> {
        self.skip_whitespace_except_newline();

        if self.is_at_end() {
            return Ok(self.make_token(TokenKind::Eof, 0));
        }

        let ch = self.current();

        match ch {
            // Newline handling
            '\n' => {
                let token = self.make_token(TokenKind::Newline, 1);
                self.advance();

                // Inside balanced delimiters: suppress newline
                if self.delimiter_depth > 0 {
                    return self.next_token();
                }

                // Peek ahead: if next non-whitespace is `|`, suppress newline
                let mut peek_pos = self.pos;
                while peek_pos < self.source.len() {
                    let c = self.source[peek_pos];
                    if c == '|' {
                        return self.next_token();
                    } else if c == ' ' || c == '\t' || c == '\r' {
                        peek_pos += 1;
                    } else {
                        break;
                    }
                }

                // Collapse consecutive newlines
                while !self.is_at_end() && self.current() == '\n' {
                    self.advance();
                    // Also skip whitespace between newlines
                    self.skip_whitespace_except_newline();
                }

                Ok(token)
            }

            // Delimiters
            '(' => {
                self.delimiter_depth += 1;
                let token = self.make_token(TokenKind::LParen, 1);
                self.advance();
                Ok(token)
            }
            ')' => {
                if self.delimiter_depth > 0 {
                    self.delimiter_depth -= 1;
                }
                let token = self.make_token(TokenKind::RParen, 1);
                self.advance();
                Ok(token)
            }
            '{' => {
                self.delimiter_depth += 1;
                let token = self.make_token(TokenKind::LBrace, 1);
                self.advance();
                Ok(token)
            }
            '}' => {
                if self.delimiter_depth > 0 {
                    self.delimiter_depth -= 1;
                }
                let token = self.make_token(TokenKind::RBrace, 1);
                self.advance();
                Ok(token)
            }
            '[' => {
                self.delimiter_depth += 1;
                let token = self.make_token(TokenKind::LBracket, 1);
                self.advance();
                Ok(token)
            }
            ']' => {
                if self.delimiter_depth > 0 {
                    self.delimiter_depth -= 1;
                }
                let token = self.make_token(TokenKind::RBracket, 1);
                self.advance();
                Ok(token)
            }

            // Single-char operators
            '+' => {
                let token = self.make_token(TokenKind::Plus, 1);
                self.advance();
                Ok(token)
            }
            '*' => {
                let token = self.make_token(TokenKind::Star, 1);
                self.advance();
                Ok(token)
            }
            '|' => {
                let token = self.make_token(TokenKind::Pipe, 1);
                self.advance();
                Ok(token)
            }
            '?' => {
                let token = self.make_token(TokenKind::Question, 1);
                self.advance();
                Ok(token)
            }
            ':' => {
                let token = self.make_token(TokenKind::Colon, 1);
                self.advance();
                Ok(token)
            }
            ',' => {
                let token = self.make_token(TokenKind::Comma, 1);
                self.advance();
                Ok(token)
            }

            // Multi-char or ambiguous operators
            '=' => {
                if self.peek() == Some('=') {
                    let token = self.make_token(TokenKind::Eq, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else if self.peek() == Some('>') {
                    let token = self.make_token(TokenKind::FatArrow, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    let token = self.make_token(TokenKind::Assign, 1);
                    self.advance();
                    Ok(token)
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    let token = self.make_token(TokenKind::NotEq, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    Err(self.error(
                        1,
                        "Unexpected character '!'",
                        Some("PACT uses 'not' keyword instead of '!'. Did you mean '!='?"),
                    ))
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    let token = self.make_token(TokenKind::LessEq, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    let token = self.make_token(TokenKind::LAngle, 1);
                    self.advance();
                    Ok(token)
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    let token = self.make_token(TokenKind::GreaterEq, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    let token = self.make_token(TokenKind::RAngle, 1);
                    self.advance();
                    Ok(token)
                }
            }
            '-' => {
                if self.peek() == Some('>') {
                    let token = self.make_token(TokenKind::Arrow, 2);
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    let token = self.make_token(TokenKind::Minus, 1);
                    self.advance();
                    Ok(token)
                }
            }
            '.' => {
                if self.peek() == Some('.') && self.peek_at(2) == Some('.') {
                    let token = self.make_token(TokenKind::Spread, 3);
                    self.advance();
                    self.advance();
                    self.advance();
                    Ok(token)
                } else {
                    let token = self.make_token(TokenKind::Dot, 1);
                    self.advance();
                    Ok(token)
                }
            }
            '/' => {
                if self.peek() == Some('/') {
                    self.skip_comment();
                    self.next_token()
                } else {
                    let token = self.make_token(TokenKind::Slash, 1);
                    self.advance();
                    Ok(token)
                }
            }

            // Underscore: standalone = Underscore token, _foo = identifier
            '_' => {
                if self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    self.read_identifier_or_keyword()
                } else {
                    let token = self.make_token(TokenKind::Underscore, 1);
                    self.advance();
                    Ok(token)
                }
            }

            // Numbers
            '0'..='9' => self.read_number(),

            // Strings
            '"' => self.read_string(),

            // Identifiers and keywords
            c if c.is_alphabetic() => self.read_identifier_or_keyword(),

            // Unknown character
            _ => Err(self.error(
                1,
                &format!("Unexpected character '{}'", ch),
                None,
            )),
        }
    }

    // --- Placeholder methods (to be implemented in Tasks 4, 5, 6) ---

    fn read_number(&mut self) -> Result<Token, LexerError> {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.column;
        let mut num_str = String::new();
        let mut is_float = false;

        // Read integer digits
        while !self.is_at_end() && self.current().is_ascii_digit() {
            num_str.push(self.current());
            self.advance();
        }

        // Check for decimal point: must be followed by a digit (not another '.' for spread, not a non-digit for field access)
        if !self.is_at_end() && self.current() == '.' {
            // Peek at what follows the dot
            let after_dot = self.peek_at(1);
            if after_dot.is_some_and(|c| c.is_ascii_digit()) {
                // It's a float: consume the dot and fractional digits
                is_float = true;
                num_str.push(self.current()); // the '.'
                self.advance();
                while !self.is_at_end() && self.current().is_ascii_digit() {
                    num_str.push(self.current());
                    self.advance();
                }
            }
            // Otherwise, leave the dot for the next token
        }

        let length = self.pos - start_pos;
        let span = Span {
            line: start_line,
            column: start_col,
            offset: start_pos,
            length,
        };

        if is_float {
            match num_str.parse::<f64>() {
                Ok(val) => Ok(Token { kind: TokenKind::FloatLiteral(val), span }),
                Err(e) => Err(self.error(length, &format!("Invalid float literal '{}': {}", num_str, e), None)),
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(val) => Ok(Token { kind: TokenKind::IntLiteral(val), span }),
                Err(e) => Err(self.error(length, &format!("Invalid integer literal '{}': {}", num_str, e), None)),
            }
        }
    }

    fn read_identifier_or_keyword(&mut self) -> Result<Token, LexerError> {
        Err(self.error(1, "Not yet implemented", None))
    }

    fn read_string(&mut self) -> Result<Token, LexerError> {
        Err(self.error(1, "Not yet implemented", None))
    }

    // --- Helper methods ---

    fn current(&self) -> char {
        self.source[self.pos]
    }

    fn peek(&self) -> Option<char> {
        self.peek_at(1)
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        let idx = self.pos + offset;
        if idx < self.source.len() {
            Some(self.source[idx])
        } else {
            None
        }
    }

    fn advance(&mut self) {
        if self.pos < self.source.len() {
            if self.source[self.pos] == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn skip_whitespace_except_newline(&mut self) {
        while !self.is_at_end() {
            let ch = self.current();
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        // Skip the '//' and everything until end of line
        while !self.is_at_end() && self.current() != '\n' {
            self.advance();
        }
    }

    fn make_token(&self, kind: TokenKind, length: usize) -> Token {
        Token {
            kind,
            span: self.current_span(length),
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

    fn get_source_line(&self) -> String {
        let lines: Vec<&str> = self.source_str.lines().collect();
        if self.line > 0 && self.line <= lines.len() {
            lines[self.line - 1].to_string()
        } else {
            String::new()
        }
    }

    fn error(&self, length: usize, message: &str, hint: Option<&str>) -> LexerError {
        LexerError {
            line: self.line,
            column: self.column,
            length,
            message: message.to_string(),
            hint: hint.map(|s| s.to_string()),
            source_line: self.get_source_line(),
        }
    }
}

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
                TokenKind::Plus, TokenKind::Minus, TokenKind::Star,
                TokenKind::Slash, TokenKind::Assign, TokenKind::Colon,
                TokenKind::Comma, TokenKind::Dot, TokenKind::Question,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn delimiters() {
        assert_eq!(
            tokenize("( ) { } [ ]"),
            vec![
                TokenKind::LParen, TokenKind::RParen,
                TokenKind::LBrace, TokenKind::RBrace,
                TokenKind::LBracket, TokenKind::RBracket,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn pipe_and_angle_brackets() {
        assert_eq!(
            tokenize("| < >"),
            vec![TokenKind::Pipe, TokenKind::LAngle, TokenKind::RAngle, TokenKind::Eof]
        );
    }

    #[test]
    fn multi_char_operators() {
        assert_eq!(
            tokenize("== != <= >= -> => ..."),
            vec![
                TokenKind::Eq, TokenKind::NotEq, TokenKind::LessEq,
                TokenKind::GreaterEq, TokenKind::Arrow, TokenKind::FatArrow,
                TokenKind::Spread, TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn comment_skipped() {
        assert_eq!(
            tokenize("+ // comment\n- // another"),
            vec![
                TokenKind::Plus, TokenKind::Newline,
                TokenKind::Minus, TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn underscore_wildcard() {
        // Standalone _ is Underscore, _foo would be identifier (but that requires Task 5)
        assert_eq!(
            tokenize("_ + _"),
            vec![TokenKind::Underscore, TokenKind::Plus, TokenKind::Underscore, TokenKind::Eof]
        );
    }

    #[test]
    fn bang_without_eq_is_error() {
        let mut lexer = Lexer::new("!");
        let result = lexer.tokenize();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Unexpected character '!'"));
        assert!(err.hint.is_some());
    }

    #[test]
    fn newline_suppressed_inside_braces() {
        assert_eq!(
            tokenize("{\n+\n}"),
            vec![TokenKind::LBrace, TokenKind::Plus, TokenKind::RBrace, TokenKind::Eof]
        );
    }

    #[test]
    fn newline_emitted_at_top_level() {
        assert_eq!(
            tokenize("+\n-"),
            vec![TokenKind::Plus, TokenKind::Newline, TokenKind::Minus, TokenKind::Eof]
        );
    }

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

    #[test]
    fn number_followed_by_dot_field() {
        // 42.foo should be IntLiteral(42), Dot, Identifier("foo")
        // But identifier lexing isn't done yet, so just test 42. stops at the dot
        assert_eq!(
            tokenize("42."),
            vec![
                TokenKind::IntLiteral(42),
                TokenKind::Dot,
                TokenKind::Eof,
            ]
        );
    }
}

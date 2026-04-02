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

    // --- Expression parsing (stub, replaced in Task 3+) ---

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.fail("Expression parsing not yet implemented", None)
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
}

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
}

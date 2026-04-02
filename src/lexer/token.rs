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
        assert_eq!(TokenKind::keyword_from_str("fn"), Some(TokenKind::Fn));
        assert_eq!(TokenKind::keyword_from_str("let"), Some(TokenKind::Let));
        assert_eq!(TokenKind::keyword_from_str("where"), None);
        assert_eq!(TokenKind::keyword_from_str("hello"), None);
    }
}

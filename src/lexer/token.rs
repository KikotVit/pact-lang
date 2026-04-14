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

    // Keywords (25 reserved)
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
    Stream,
    Test,
    App,
    Schedule,
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
    Percent,
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

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenKind::IntLiteral(n) => write!(f, "integer {}", n),
            TokenKind::FloatLiteral(n) => write!(f, "float {}", n),
            TokenKind::BoolLiteral(b) => write!(f, "{}", b),
            TokenKind::RawStringLiteral(s) => write!(f, "string \"{}\"", s),
            TokenKind::StringStart => write!(f, "string start"),
            TokenKind::StringEnd => write!(f, "string end"),
            TokenKind::StringFragment(s) => write!(f, "string \"{}\"", s),
            TokenKind::InterpolationStart => write!(f, "interpolation"),
            TokenKind::InterpolationEnd => write!(f, "interpolation end"),
            TokenKind::Fn => write!(f, "'fn'"),
            TokenKind::Let => write!(f, "'let'"),
            TokenKind::Var => write!(f, "'var'"),
            TokenKind::Type => write!(f, "'type'"),
            TokenKind::If => write!(f, "'if'"),
            TokenKind::Else => write!(f, "'else'"),
            TokenKind::Match => write!(f, "'match'"),
            TokenKind::Return => write!(f, "'return'"),
            TokenKind::Use => write!(f, "'use'"),
            TokenKind::Intent => write!(f, "'intent'"),
            TokenKind::Ensure => write!(f, "'ensure'"),
            TokenKind::Needs => write!(f, "'needs'"),
            TokenKind::Route => write!(f, "'route'"),
            TokenKind::Stream => write!(f, "'stream'"),
            TokenKind::Test => write!(f, "'test'"),
            TokenKind::App => write!(f, "'app'"),
            TokenKind::Schedule => write!(f, "'schedule'"),
            TokenKind::Check => write!(f, "'check'"),
            TokenKind::True => write!(f, "'true'"),
            TokenKind::False => write!(f, "'false'"),
            TokenKind::Nothing => write!(f, "'nothing'"),
            TokenKind::And => write!(f, "'and'"),
            TokenKind::Or => write!(f, "'or'"),
            TokenKind::Not => write!(f, "'not'"),
            TokenKind::As => write!(f, "'as'"),
            TokenKind::Plus => write!(f, "'+'"),
            TokenKind::Minus => write!(f, "'-'"),
            TokenKind::Star => write!(f, "'*'"),
            TokenKind::Slash => write!(f, "'/'"),
            TokenKind::Percent => write!(f, "'%'"),
            TokenKind::Assign => write!(f, "'='"),
            TokenKind::Eq => write!(f, "'=='"),
            TokenKind::NotEq => write!(f, "'!='"),
            TokenKind::LAngle => write!(f, "'<'"),
            TokenKind::RAngle => write!(f, "'>'"),
            TokenKind::LessEq => write!(f, "'<='"),
            TokenKind::GreaterEq => write!(f, "'>='"),
            TokenKind::Pipe => write!(f, "'|'"),
            TokenKind::Question => write!(f, "'?'"),
            TokenKind::Dot => write!(f, "'.'"),
            TokenKind::Spread => write!(f, "'...'"),
            TokenKind::Arrow => write!(f, "'->'"),
            TokenKind::FatArrow => write!(f, "'=>'"),
            TokenKind::Underscore => write!(f, "'_'"),
            TokenKind::LBrace => write!(f, "'{{'"),
            TokenKind::RBrace => write!(f, "'}}'"),
            TokenKind::LParen => write!(f, "'('"),
            TokenKind::RParen => write!(f, "')'"),
            TokenKind::LBracket => write!(f, "'['"),
            TokenKind::RBracket => write!(f, "']'"),
            TokenKind::Colon => write!(f, "':'"),
            TokenKind::Comma => write!(f, "','"),
            TokenKind::Identifier(name) => write!(f, "'{}'", name),
            TokenKind::Newline => write!(f, "end of line"),
            TokenKind::Eof => write!(f, "end of file"),
        }
    }
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
            "stream" => Some(TokenKind::Stream),
            "test" => Some(TokenKind::Test),
            "app" => Some(TokenKind::App),
            "schedule" => Some(TokenKind::Schedule),
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

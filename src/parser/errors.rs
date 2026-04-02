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

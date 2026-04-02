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

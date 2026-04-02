use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

impl std::error::Error for RuntimeError {}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Runtime error at line {}, col {}:",
            self.line, self.column
        )?;
        writeln!(f, "  {}", self.source_line)?;
        let padding = self.column - 1 + 2; // +2 for the "  " prefix
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
    fn runtime_error_display() {
        let error = RuntimeError {
            line: 3,
            column: 5,
            message: "Undefined variable 'x'".to_string(),
            hint: Some("Did you mean 'y'?".to_string()),
            source_line: "let z = x + 1".to_string(),
        };

        let output = format!("{}", error);
        assert!(output.contains("Runtime error at line 3, col 5"));
        assert!(output.contains("let z = x + 1"));
        assert!(output.contains("^"));
        assert!(output.contains("Undefined variable 'x'"));
        assert!(output.contains("Hint: Did you mean 'y'?"));
    }

    #[test]
    fn runtime_error_display_without_hint() {
        let error = RuntimeError {
            line: 1,
            column: 1,
            message: "Division by zero".to_string(),
            hint: None,
            source_line: "10 / 0".to_string(),
        };

        let output = format!("{}", error);
        assert!(output.contains("Runtime error at line 1, col 1"));
        assert!(output.contains("10 / 0"));
        assert!(output.contains("Division by zero"));
        assert!(!output.contains("Hint:"));
    }
}

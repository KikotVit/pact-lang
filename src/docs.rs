pub fn get_doc(topic: &str) -> Option<&'static str> {
    match topic {
        "quickstart" => Some(include_str!("docs/quickstart.md")),
        "pipeline" => Some(include_str!("docs/pipeline.md")),
        "route" => Some(include_str!("docs/route.md")),
        "fn" => Some(include_str!("docs/fn.md")),
        "type" => Some(include_str!("docs/type.md")),
        "db" => Some(include_str!("docs/db.md")),
        "test" => Some(include_str!("docs/test.md")),
        "effects" => Some(include_str!("docs/effects.md")),
        "match" => Some(include_str!("docs/match.md")),
        "error" => Some(include_str!("docs/error.md")),
        "string" => Some(include_str!("docs/string.md")),
        "list" => Some(include_str!("docs/list.md")),
        "app" => Some(include_str!("docs/app.md")),
        _ => None,
    }
}

pub fn list_topics() -> Vec<(&'static str, &'static str)> {
    vec![
        ("quickstart", "Build your first PACT API in minutes"),
        (
            "pipeline",
            "All 21 pipeline steps: filter, map, sort, take, skip, and more",
        ),
        ("route", "HTTP routes with path parameters and respond"),
        ("fn", "Functions, intent, needs, error types"),
        ("type", "Struct types, union types, optional fields"),
        (
            "db",
            "Database operations: insert, query, find, update, delete",
        ),
        ("test", "Test blocks, using (mock effects), assert"),
        (
            "effects",
            "All 6 built-in effects: db, time, rng, auth, log, env",
        ),
        ("match", "Match expressions, patterns, wildcard"),
        (
            "error",
            "Error handling: ?, on ErrorType:, error propagation",
        ),
        ("string", "String interpolation, methods, raw strings"),
        ("list", "list(), methods, pipeline operations on lists"),
        ("app", "App declaration, port, db config"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn test_all_topics_exist() {
        let topics = [
            "quickstart",
            "pipeline",
            "route",
            "fn",
            "type",
            "db",
            "test",
            "effects",
            "match",
            "error",
            "string",
            "list",
            "app",
        ];
        for topic in &topics {
            let content = get_doc(topic);
            assert!(content.is_some(), "Topic '{}' should exist", topic);
            assert!(
                !content.unwrap().is_empty(),
                "Topic '{}' should be non-empty",
                topic
            );
        }
    }

    #[test]
    fn test_list_topics_count() {
        let topics = list_topics();
        assert_eq!(topics.len(), 13, "Expected 13 topics, got {}", topics.len());
    }

    #[test]
    fn test_unknown_topic() {
        assert!(get_doc("nonexistent").is_none());
    }

    #[test]
    fn test_quickstart_contains_app() {
        let content = get_doc("quickstart").expect("quickstart topic should exist");
        assert!(content.contains("app"), "quickstart should contain 'app'");
    }

    #[test]
    fn test_code_examples_parse() {
        let topics = list_topics();
        for (topic, _) in &topics {
            let content = get_doc(topic).expect(&format!("Topic '{}' should exist", topic));

            // Extract ```pact blocks
            let mut in_pact_block = false;
            let mut current_block = String::new();
            let mut blocks: Vec<(String, usize)> = Vec::new(); // (code, start_line)
            let mut line_num = 0;

            for line in content.lines() {
                line_num += 1;
                if line.trim() == "```pact" {
                    in_pact_block = true;
                    current_block.clear();
                    continue;
                }
                if line.trim() == "```" && in_pact_block {
                    in_pact_block = false;
                    blocks.push((current_block.clone(), line_num));
                    continue;
                }
                if in_pact_block {
                    current_block.push_str(line);
                    current_block.push('\n');
                }
            }

            for (code, start_line) in &blocks {
                let mut lexer = Lexer::new(code);
                let tokens = match lexer.tokenize() {
                    Ok(t) => t,
                    Err(e) => panic!(
                        "Lexer error in docs topic '{}' at doc line ~{}: {}",
                        topic, start_line, e.message
                    ),
                };

                let mut parser = Parser::new(tokens, code);
                if let Err(errors) = parser.parse() {
                    let msgs: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
                    panic!(
                        "Parser error in docs topic '{}' at doc line ~{}:\nCode:\n{}\nErrors: {}",
                        topic,
                        start_line,
                        code,
                        msgs.join(", ")
                    );
                }
            }
        }
    }
}

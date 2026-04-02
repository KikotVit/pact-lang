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
            let meaningful = tokens.iter().filter(|t| !matches!(t.kind, TokenKind::Eof | TokenKind::Newline)).count();
            println!("\n{} tokens", meaningful);
        }
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}

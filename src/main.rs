use std::env;
use std::fs;
use std::process;

use pact::lexer::{Lexer, TokenKind};
use pact::parser::Parser;
use pact::interpreter::{Interpreter, Value};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("pact {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if args.len() < 2 {
        eprintln!("Usage: pact <file.pact> [--ast]");
        eprintln!("       pact run <file.pact>    Execute a .pact file");
        eprintln!("       pact test <file.pact>   Run test blocks in a .pact file");
        eprintln!("  Tokenizes a .pact file and prints the token stream.");
        eprintln!("  --ast  Parse and print the AST instead of tokens.");
        process::exit(1);
    }

    // pact test <file>
    if args.len() >= 3 && args[1] == "test" {
        let filename = &args[2];
        let source = match fs::read_to_string(filename) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", filename, e);
                process::exit(1);
            }
        };

        let mut lexer = Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => { eprintln!("{}", e); process::exit(1); }
        };

        let mut parser = Parser::new(tokens, &source);
        let program = match parser.parse() {
            Ok(p) => p,
            Err(errors) => {
                for e in &errors { eprintln!("{}", e); }
                process::exit(1);
            }
        };

        let mut interp = Interpreter::new(&source);
        interp.set_base_dir(filename);
        interp.setup_test_effects();
        let results = interp.run_tests(&program);

        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;

        for result in &results {
            if result.passed {
                println!("\u{2713} {}", result.name);
            } else {
                println!("\u{2717} {}", result.name);
                if let Some(ref err) = result.error {
                    println!("  {}", err);
                }
            }
        }

        println!("\n{} tests, {} passed, {} failed", total, passed, failed);

        if failed > 0 {
            process::exit(1);
        }
        return;
    }

    // pact run <file>
    if args.len() >= 3 && args[1] == "run" {
        let filename = &args[2];
        let source = match fs::read_to_string(filename) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", filename, e);
                process::exit(1);
            }
        };

        let mut lexer = Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => { eprintln!("{}", e); process::exit(1); }
        };

        let mut parser = Parser::new(tokens, &source);
        let program = match parser.parse() {
            Ok(p) => p,
            Err(errors) => {
                for e in &errors { eprintln!("{}", e); }
                process::exit(1);
            }
        };

        let mut interp = Interpreter::new(&source);
        interp.set_base_dir(filename);
        interp.setup_test_effects();  // provide effects for now
        match interp.interpret(&program) {
            Ok(value) => {
                if let Some((name, port)) = interp.app_config.clone() {
                    pact::interpreter::server::start_server(&mut interp, &name, port);
                } else {
                    match value {
                        Value::Nothing => {} // don't print nothing
                        _ => println!("{}", value),
                    }
                }
            }
            Err(e) => { eprintln!("{}", e); process::exit(1); }
        }
        return;
    }

    let filename = &args[1];
    let show_ast = args.iter().any(|a| a == "--ast");

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    if show_ast {
        let mut parser = Parser::new(tokens, &source);
        match parser.parse() {
            Ok(program) => {
                println!("{:#?}", program);
            }
            Err(errors) => {
                for e in &errors {
                    eprintln!("{}", e);
                }
                process::exit(1);
            }
        }
    } else {
        for token in &tokens {
            match &token.kind {
                TokenKind::Eof => {}
                TokenKind::Newline => {
                    println!("  {:>3}:{:<3}  Newline", token.span.line, token.span.column);
                }
                _ => {
                    println!("  {:>3}:{:<3}  {:?}", token.span.line, token.span.column, token.kind);
                }
            }
        }
        let meaningful = tokens.iter().filter(|t| !matches!(t.kind, TokenKind::Eof | TokenKind::Newline)).count();
        println!("\n{} tokens", meaningful);
    }
}

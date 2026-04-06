# PACT Language Compiler

## Environment
- `cargo` is already in PATH. Do NOT prefix commands with `source "$HOME/.cargo/env"`.
- Just use `cargo build`, `cargo test`, `cargo run` directly.
- Always run `cargo fmt` before committing.

## Architecture
- Tree-walking interpreter, NOT a transpiler. Executes .pact files directly.
- `intent` always goes BEFORE `fn`, `route`, and `stream` declarations — one pattern for everything.
- Errors are values (`Value::Error`), not exceptions. `RuntimeError` is only for interpreter crashes.

## Testing
- `cargo test` runs all Rust tests (~449).
- `pact test file.pact` runs PACT test blocks.
- Tests use `db.memory()` (HashMap), not SQLite.

## Key files
- `src/interpreter/interpreter.rs` — main interpreter logic
- `src/interpreter/db.rs` — DbBackend (Memory/SQLite)
- `src/parser/parser.rs` — parser
- `src/parser/ast.rs` — AST types
- `src/interpreter/server.rs` — HTTP server (tiny_http)
- `src/formatter.rs` — code formatter (AST pretty-printer)
- `src/lsp.rs` — LSP server (stdio JSON-RPC)
- `src/mcp.rs` — MCP server (5 tools)
- `docs/spec/PACT_SPEC_v0.1.md` — language spec (target to implement towards, not just docs)

# PACT Lexer Design

## Overview

Hand-written lexer for the PACT programming language, implemented in Rust. No external lexer generators — full control over error messages and string interpolation handling.

## Key Design Decisions

### Pipe token (`|`)
Single `Pipe` token for both contexts (pipeline operator and union type separator). Parser disambiguates.

Validation constraints use `check { ... }` keyword instead of `|` (spec change: avoids triple-overloading `|`).

### String interpolation
Lexer breaks interpolated strings into parts:
- `StringStart` — opening `"`
- `StringFragment(String)` — text between interpolations, escapes processed (`{{` -> `{`)
- `InterpolationStart` — `{` inside string
- (normal tokens for the expression)
- `InterpolationEnd` — `}` closing interpolation
- `StringEnd` — closing `"`
- Multiline strings (`"""..."""`) use the same token types
- `RawStringLiteral(String)` — `raw"..."` emitted whole, no interpolation

### Angle brackets (`<`, `>`)
Emitted as `LAngle`/`RAngle`. Parser resolves whether it's comparison or generic. No attempt to disambiguate at lexer level.

### Spread operator
`...` (three dots) is the spread operator. `..` (two dots) is not introduced to avoid range ambiguity.

### Keywords vs contextual identifiers

**Reserved keywords (23):**
`fn`, `let`, `var`, `type`, `if`, `else`, `match`, `return`, `use`, `intent`, `ensure`, `needs`, `route`, `test`, `app`, `check`, `true`, `false`, `nothing`, `and`, `or`, `not`, `as`

**Contextual (emitted as Identifier, parser disambiguates):**
`where`, `by`, `to`, `first`, `last`, `ascending`, `descending`, `with`, `success`, `default`, `using`, `assert`, `filter`, `map`, `sort`, `group`, `take`, `each`, `count`, `sum`, `flatten`, `unique`, `find`, `expect`, `on`, `respond`, `validate`, `skip`, `raise`, `format`, `min`, `max`

### Newline handling
PACT uses newlines as statement separators (no semicolons). Rules for when newlines are significant:

1. **Inside balanced delimiters** `()`, `[]`, `{}` — newline suppressed
2. **After continuation tokens** (`|`, `->`, `=>`, `,`, `=`) — newline suppressed
3. **Before `|`** on next line — preceding newline suppressed (lexer peeks ahead)
4. **All other cases** — emit `Newline` token

### Comments
`//` single-line comments are skipped by the lexer (not emitted as tokens). Future: optional mode for tooling/LSP.

### Error messages
`LexerError` includes:
- `line: usize` — 1-based line number
- `column: usize` — 1-based column
- `message: String` — what went wrong
- `hint: Option<String>` — suggestion for fixing
- `source_line: String` — the actual source line for display

Error output format:
```
Error at line 12, col 8:
  name String check { min 1 }
       ^^^^^^
  Expected ':' after field name
  Hint: type fields use 'name: Type' syntax
```

## Token Types

### Literals
- `IntLiteral(i64)`
- `FloatLiteral(f64)`
- `BoolLiteral(bool)` — from `true`/`false` keywords
- `RawStringLiteral(String)`

### String interpolation
- `StringStart`, `StringEnd`
- `StringFragment(String)`
- `InterpolationStart`, `InterpolationEnd`

### Keywords (23)
Each maps to its own `TokenKind` variant.

### Operators
`Plus`, `Minus`, `Star`, `Slash`, `Assign`, `Eq`, `NotEq`, `LAngle`, `RAngle`, `LessEq`, `GreaterEq`, `Pipe`, `Question`, `Dot`, `Spread`, `Arrow`, `FatArrow`

### Delimiters
`LBrace`, `RBrace`, `LParen`, `RParen`, `LBracket`, `RBracket`, `Colon`, `Comma`

### Other
- `Identifier(String)`
- `Newline`
- `Eof`

### Span
Every token wrapped in `Span { line: usize, column: usize, offset: usize, length: usize }`.

## Module Structure

```
src/
  main.rs          — entry point, reads .pact file, runs lexer, prints tokens
  lib.rs           — module re-exports
  lexer/
    mod.rs         — pub mod, re-exports
    token.rs       — Token, TokenKind, Span
    lexer.rs       — struct Lexer, core logic
    errors.rs      — LexerError with rich context
```

## Source Spec
Language spec: `PACT_SPEC_v0.1.md`
Backend examples: `PACT_Examples_Backend.pact`
Spec change: validation constraints use `check { ... }` instead of `| min N | max N`

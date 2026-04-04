# PACT Type Linter — Design Spec

## Goal

Add a type linter to PACT: a static analysis pass that catches type mismatches before runtime. Not a full type system — a targeted set of checks that catch 80% of mistakes agents make. Runs as part of `pact check` and MCP `pact_check`.

## Why

Agents write PACT code, run `pact check`, get syntax errors. But `pact check` only validates syntax — `let x: Int = "hello"` passes. Agent runs code, hits runtime error, debugs, fixes, reruns. Each round-trip costs tokens and time. A type linter catches these obvious mismatches at check time: one iteration instead of three.

## Prerequisite: Spans in AST

AST nodes (`Statement`, `Expr`) currently have **no position information**. `Span` (line, column, offset, length) exists only on `Token` in the lexer. The parser uses `token.span` for parse errors but does not store spans in AST nodes.

The checker needs line/column to produce useful diagnostics. Without spans, it can only say "type mismatch" but not where.

**Solution:** Add `span: Span` field to `Statement` and `Expr` variants that the checker needs:
- `Statement::Let` — for Check 1 (let binding)
- `Statement::FnDecl` — for Check 3 (return type)
- `Statement::Return` — for Check 3 (explicit return)
- `Expr::FnCall` — for Check 2 (argument types)
- `Expr::Match` — for Check 4 (exhaustiveness)

The parser already has the span at parse time (`self.current().span`). It needs to save it into the AST node. This is a focused change in `parser.rs` — add span capture at the parse site for these 5 node types.

Wrap span in a new `Spanned<T>` or add an `Option<Span>` field. `Option<Span>` is simpler — existing code that creates AST nodes (tests, etc.) can pass `None`.

## Architecture

New module `src/checker.rs` — a separate phase between parser and interpreter.

```
source → Lexer → tokens → Parser → AST → Checker → diagnostics
                                                       ↓
                                              errors[] + warnings[]
```

Pipeline becomes: lex → parse → **type lint**. If lex or parse fails, checker doesn't run. Checker never blocks on its own errors — it always returns all diagnostics it found.

### Public API (`src/checker.rs`)

```rust
pub enum Severity {
    Error,
    Warning,
}

pub struct Diagnostic {
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

/// Analyze a parsed program for type errors. Returns all diagnostics found.
/// Empty vec = no issues.
pub fn check(program: &Program, source: &str) -> Vec<Diagnostic>
```

Same shape as `ParseError` / `RuntimeError` — line, column, message, hint, source_line. Callers already know how to format this.

### Display format

`Diagnostic` implements `fmt::Display` with the same format as `ParseError`:

```
Type error at line 3, col 5:
  let name: Int = "hello"
      ^
  Type mismatch: expected Int, got String
  Hint: The variable 'name' is declared as Int but the value is a String
```

### Internal state

The checker walks the AST and maintains three lookup tables:

```rust
struct Checker<'a> {
    source: &'a str,
    scopes: Vec<HashMap<String, ResolvedType>>,  // variable types, nested scopes
    fn_sigs: HashMap<String, FnSig>,              // function signatures
    type_defs: HashMap<String, TypeDef>,           // struct/union definitions
    diagnostics: Vec<Diagnostic>,                  // collected errors + warnings
    current_fn_return: Option<ResolvedType>,       // return type of function being checked
}
```

### ResolvedType

```rust
enum ResolvedType {
    Int,
    Float,
    String,
    Bool,
    Nothing,
    List,                        // no generic param in v1
    Map,                         // no generic param in v1
    Struct(String),              // type name, e.g. "User", "Todo"
    Optional(Box<ResolvedType>),
    Result { ok: Box<ResolvedType>, errors: Vec<String> },
    Unknown,                     // db.find(), builtins, complex exprs — skip checking
}
```

`Unknown` is the escape hatch. Any comparison involving `Unknown` succeeds silently. This prevents false positives from builtins (`db.*`, `rng.*`, `time.*`) and complex expressions.

### TypeDef and FnSig

```rust
enum TypeDef {
    Struct { fields: Vec<(String, ResolvedType)> },
    Union { variants: Vec<String> },
}

struct FnSig {
    params: Vec<(String, ResolvedType)>,
    return_type: ResolvedType,
}
```

### Type resolution: TypeExpr → ResolvedType

`TypeExpr` (AST) maps to `ResolvedType` (checker):

| TypeExpr | ResolvedType |
|----------|-------------|
| `Named("Int")` | `Int` |
| `Named("Float")` | `Float` |
| `Named("String")` | `String` |
| `Named("Bool")` | `Bool` |
| `Named("Nothing")` | `Nothing` |
| `Named("List")` | `List` |
| `Named("Map")` | `Map` |
| `Named("User")` (known struct) | `Struct("User")` |
| `Named("Foo")` (unknown) | `Unknown` |
| `Generic { name: "List", .. }` | `List` |
| `Generic { name: "Map", .. }` | `Map` |
| `Optional(inner)` | `Optional(resolve(inner))` |
| `Result { ok, errors }` | `Result { ok: resolve(ok), errors }` |

### Expression type inference

The checker infers types from expressions bottom-up:

| Expression | Inferred type |
|-----------|--------------|
| `IntLiteral` | `Int` |
| `FloatLiteral` | `Float` |
| `StringLiteral` | `String` |
| `BoolLiteral` | `Bool` |
| `Nothing` | `Nothing` |
| `Identifier(x)` | lookup in scopes, or `Unknown` |
| `FnCall(name, args)` | lookup in fn_sigs → return type, or `Unknown` |
| `StructLiteral { name: Some(n) }` | `Struct(n)` |
| `StructLiteral { name: None }` | `Unknown` (anonymous struct) |
| `BinaryOp(+, Int, Int)` | `Int` |
| `BinaryOp(+, Float, _)` | `Float` |
| `BinaryOp(+, String, _)` | `String` |
| `BinaryOp(==, _, _)` | `Bool` |
| `BinaryOp(and/or, _, _)` | `Bool` |
| `UnaryOp(Neg, Int)` | `Int` |
| `UnaryOp(Not, _)` | `Bool` |
| `ErrorPropagation(Result { ok, .. })` | `ok` type |
| `ErrorPropagation(other)` | `Unknown` |
| `If { .. }` | `Unknown` (branch types may differ) |
| `Match { .. }` | `Unknown` (arm types may differ) |
| `FieldAccess { .. }` | `Unknown` (needs struct field tracking — v2) |
| `Pipeline { .. }` | `Unknown` (pipeline transforms are complex) |
| `Respond { .. }` | `Nothing` |
| Anything else | `Unknown` |

The pattern: literals and simple expressions → known types. Complex expressions → `Unknown`. No false positives.

## Four checks

### Check 1: Let binding type mismatch (Error)

```pact
let x: Int = "hello"    // Error: expected Int, got String
let y: Bool = 42        // Error: expected Bool, got Int
let z: Int = 1 + 2      // OK: Int = Int
let w: String = name     // OK or skip: depends on whether 'name' type is known
```

**Logic:** For each `Statement::Let { type_ann, value, .. }`:
1. Resolve `type_ann` → `ResolvedType` (expected)
2. Infer type of `value` → `ResolvedType` (actual)
3. If both are known and don't match → emit Error

### Check 2: Function argument type mismatch (Error)

```pact
fn greet(name: String) -> String { "Hello, ${name}" }
greet(42)    // Error: argument 'name' expects String, got Int
```

**Logic:** For each `Expr::FnCall { callee, args }`:
1. Look up callee in `fn_sigs`
2. If found and arg count matches, compare each arg type to param type
3. If both are known and don't match → emit Error

### Check 3: Return type mismatch (Error)

```pact
fn get_age() -> Int {
  "twenty"    // Error: function 'get_age' should return Int, got String
}
```

**Logic:** While checking a `FnDecl` body:
1. Set `current_fn_return` to the declared return type
2. The last expression in the body is the implicit return value
3. For explicit `Return { value }` statements, check value type
4. If return type is known and actual type is known and they don't match → emit Error

Note: PACT uses implicit returns (last expression value). Explicit `return` also exists.

### Check 4: Match exhaustiveness (Warning)

```pact
type Status = Active | Inactive | Banned

match status {
  Active => "ok"
  Inactive => "paused"
}
// Warning: match on Status does not cover: Banned
```

**Logic:** For each `Expr::Match { subject, arms }`:
1. Infer type of subject
2. If it's `Struct(name)` and `name` is a known union type in `type_defs`
3. Collect all `Pattern::Identifier` names from arms
4. If there's a `Pattern::Wildcard` → exhaustive, skip
5. Compare arm names to union variants, find missing
6. If any missing → emit Warning with list of missing variants

### Type compatibility rules

Two `ResolvedType` values are compatible if:
- Either is `Unknown` → always compatible (escape hatch)
- Both are the same primitive (`Int == Int`, `String == String`)
- Both are `Struct(name)` with the same name
- Both are `List` (no inner type in v1)
- Both are `Map` (no inner type in v1)
- `Optional(T)` is compatible with `T` (assigning non-optional to optional is ok)
- `Nothing` is compatible with `Optional(_)` (nothing = absent value)

Everything else is a mismatch.

## Two-pass approach

The checker walks the AST in two passes:

**Pass 1 — Collect declarations.** Walk all top-level statements:
- `TypeDecl::Struct` → register in `type_defs`
- `TypeDecl::Union` → register in `type_defs`
- `FnDecl` → register signature in `fn_sigs`

This allows forward references: a function can call another function declared later in the file.

**Pass 2 — Check bodies.** Walk all statements again:
- `Let` → Check 1
- `FnDecl` body → push scope with params, check body, Check 3
- `Expression(FnCall)` → Check 2
- `Expression(Match)` → Check 4
- `Route` body → push scope with `request` as Unknown, check body
- Nested expressions → recurse, applying Check 2 wherever FnCall appears

## Integration

### `pact check` (CLI in main.rs)

Currently: lex → parse → print result.

After: lex → parse → `checker::check(&program, &source)` → print result.

```
$ pact check file.pact
Type error at line 3, col 5:
  let name: Int = "hello"
      ^
  Type mismatch: expected Int, got String
```

- If only type diagnostics (no parse errors): print diagnostics, exit 1 for errors, exit 0 for warnings-only
- Parse errors still printed first, type checking skipped if parse fails

**Note:** `pact check` currently doesn't exist as a CLI command — only as MCP tool `pact_check`. Add `pact check <file>` CLI command alongside the type linter.

### `pact_check` (MCP in mcp.rs)

Currently returns:
```json
{"valid": true, "statements": 5}
{"errors": [{"phase": "parser", ...}]}
```

After — successful parse with type issues:
```json
{
  "valid": false,
  "statements": 5,
  "errors": [{"phase": "checker", "line": 3, "column": 5, "message": "...", "hint": "...", "source_line": "..."}],
  "warnings": [{"phase": "checker", "line": 10, "column": 3, "message": "...", "hint": "...", "source_line": "..."}]
}
```

Rules:
- Type errors → `"valid": false`, `isError: true`
- Only warnings, no errors → `"valid": true`, `isError: false`, include `"warnings"` array
- No diagnostics → `"valid": true` (current behavior)
- `"phase": "checker"` distinguishes from `"parser"` and `"lexer"` errors

### `pact run` — NOT changed

`pact run` does not run the type linter. It executes code directly. Agent workflow: write code → `pact check` → fix type errors → `pact run`.

## Testing

### Unit tests in `src/checker.rs`

**Check 1 — Let binding:**
- `test_let_int_gets_string` — `let x: Int = "hello"` → Error
- `test_let_string_gets_int` — `let x: String = 42` → Error
- `test_let_bool_gets_int` — `let x: Bool = 1` → Error
- `test_let_int_gets_int` — `let x: Int = 42` → no errors
- `test_let_unknown_rhs` — `let x: Int = some_var` (unknown) → no errors (Unknown escape)
- `test_let_struct_type` — `let u: User = User { name: "Alice" }` → no errors

**Check 2 — Function args:**
- `test_fn_call_wrong_arg_type` — `fn foo(x: Int) ...` called with `foo("hello")` → Error
- `test_fn_call_correct_types` — `fn foo(x: Int) ...` called with `foo(42)` → no errors
- `test_fn_call_unknown_function` — call to unknown fn → no errors (not registered)
- `test_fn_call_multiple_args` — check each arg independently

**Check 3 — Return type:**
- `test_fn_returns_wrong_type` — `fn foo() -> Int { "hello" }` → Error
- `test_fn_returns_correct_type` — `fn foo() -> Int { 42 }` → no errors
- `test_fn_no_return_type` — `fn foo() { ... }` (no annotation) → no checking
- `test_fn_returns_unknown` — `fn foo() -> Int { some_call() }` → no errors (Unknown)

**Check 4 — Match exhaustiveness:**
- `test_match_missing_variant` — union with 3 variants, match covers 2 → Warning
- `test_match_all_variants` — all covered → no warnings
- `test_match_with_wildcard` — wildcard covers rest → no warnings
- `test_match_non_union` — match on non-union type → no warning (can't check)

**Integration:**
- `test_empty_program` — no diagnostics
- `test_multiple_errors` — program with several type errors → all caught
- `test_forward_reference` — function calls function declared later → works (two-pass)
- `test_nested_scope` — variable in inner scope doesn't leak to outer

### MCP tests in `src/mcp.rs`

- `test_pact_check_type_error` — code with type mismatch → `isError: true`, `phase: "checker"`
- `test_pact_check_warning_only` — code with only match warning → `valid: true`, has `"warnings"`
- `test_pact_check_clean` — correct code → `valid: true`, no errors, no warnings

### CLI test

Add `pact check <file>` CLI command. Manual verification:
```bash
pact check examples/todo.pact           # should pass (no type errors)
pact check test_type_error.pact         # should show type error
```

## File structure

### New files
- `src/checker.rs` — `check()`, `Checker` struct, `Diagnostic`, `Severity`, `ResolvedType`, all four checks, tests

### Modified files
- `src/parser/ast.rs` — add `span: Option<Span>` to Let, FnDecl, Return, FnCall, Match nodes
- `src/parser/parser.rs` — capture and store span at parse site for those 5 nodes
- `src/lib.rs` — add `pub mod checker;`
- `src/main.rs` — add `pact check <file>` CLI command, call `checker::check()` after parse
- `src/mcp.rs` — call `checker::check()` in `execute_pact_check()`, include diagnostics in response

## Out of scope

- Generic type parameter checking (`List<Int>` vs `List<String>`)
- Field access type resolution (`.name` on a struct)
- Pipeline type tracking
- Effect verification beyond current runtime checks
- Type inference (all types are explicitly annotated in PACT)
- Cross-file type checking
- `pact run` integration

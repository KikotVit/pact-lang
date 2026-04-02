# PACT Parser v0.1 Design

## Overview

Hand-written recursive descent parser for PACT. Takes token stream from lexer, produces AST. Fail-fast error handling with `Result<Program, Vec<ParseError>>` API (single error now, ready for recovery later).

## Scope — v0.1

Everything needed to parse `user_service.pact` — functions with contracts, pipelines, types:

`let`/`var`, `fn`, `type` (struct + union), `if`/`else`, `match`, `return`, `use`, expressions (arithmetic, comparison, field access, function calls, pipeline `|`), literals (int, float, string, bool, nothing), `intent`, `ensure`, `needs`

**Not in v0.1:** `route`, `app`, `test`, `check`, `on success`/`on error`, `using` — those are v0.2 (HTTP layer and tests).

## AST Types

### Program

```rust
struct Program {
    statements: Vec<Statement>,
}
```

### Statements

```rust
enum Statement {
    Let {
        name: String,
        mutable: bool,         // let vs var
        type_ann: TypeExpr,
        value: Expr,
    },
    FnDecl {
        name: String,
        intent: Option<String>,         // intent "description" before fn
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        error_types: Vec<String>,       // fn foo() -> T or E1 or E2
        effects: Vec<String>,           // needs { db, time }
        body: Vec<Statement>,
    },
    TypeDecl(TypeDecl),
    Use {
        path: Vec<String>,             // use models.user.User → ["models", "user", "User"]
    },
    Return {
        value: Option<Expr>,
        condition: Option<Expr>,        // return X if Y
    },
    Expression(Expr),
}

struct Param {
    name: String,
    type_ann: TypeExpr,
}
```

### Type declarations

```rust
enum TypeDecl {
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Union {
        name: String,
        variants: Vec<UnionVariant>,
    },
}

struct Field {
    name: String,
    type_ann: TypeExpr,
}

struct UnionVariant {
    name: String,
    fields: Option<Vec<Field>>,  // None for simple (Admin), Some for complex (BadRequest { message: String })
}
```

### Expressions

```rust
enum Expr {
    // Literals
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(StringExpr),
    BoolLiteral(bool),
    Nothing,

    // Identifiers and access
    Identifier(String),
    FieldAccess { object: Box<Expr>, field: String },
    DotShorthand(Vec<String>),          // .active, .values.length — implicit pipeline arg

    // Operations
    BinaryOp { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    ErrorPropagation(Box<Expr>),        // expr?

    // Calls — method calls like db.query(...) are modeled as
    // FnCall { callee: FieldAccess { object: "db", field: "query" }, args }
    // No separate MethodCall node; postfix parsing handles it naturally.
    FnCall { callee: Box<Expr>, args: Vec<Expr> },

    // Pipeline
    Pipeline { source: Box<Expr>, steps: Vec<PipelineStep> },

    // Control flow
    If { condition: Box<Expr>, then_body: Vec<Statement>, else_body: Option<Vec<Statement>> },
    Match { subject: Box<Expr>, arms: Vec<MatchArm> },
    Block(Vec<Statement>),

    // Struct construction
    StructLiteral { name: Option<String>, fields: Vec<StructField> },

    // Contracts
    Ensure(Box<Expr>),

    // Type checking
    Is { expr: Box<Expr>, type_name: String },      // result is NotFound
}

enum StructField {
    Named { name: String, value: Expr },
    Spread(Expr),                       // ...user
}

enum StringExpr {
    Simple(String),
    Interpolated(Vec<StringPart>),
}

enum StringPart {
    Literal(String),
    Expr(Expr),
}

struct MatchArm {
    pattern: Pattern,
    body: Expr,
}

enum Pattern {
    Identifier(String),     // Admin, NotFound
    Wildcard,               // _
    Literal(Expr),          // 42, "hello", true
}

enum BinaryOp {
    Add, Sub, Mul, Div,
    Eq, NotEq, Lt, Gt, LtEq, GtEq,
    And, Or,
}

enum UnaryOp {
    Neg,    // -x
    Not,    // not x
}
```

### Pipeline steps

```rust
enum PipelineStep {
    Filter { predicate: Expr },
    Map { expr: Expr },
    Sort { field: Expr, descending: bool },
    GroupBy { field: Expr },
    Take { kind: TakeKind, count: Expr },
    Skip { count: Expr },
    Each { expr: Expr },
    FindFirst { predicate: Expr },
    ExpectOne { error: Expr },
    ExpectAny { error: Expr },
    OrDefault { value: Expr },
    Flatten,
    Unique,
    Count,
    Sum,
    Expr(Expr),             // fallback for arbitrary expressions after |
}

enum TakeKind {
    First,
    Last,
}
```

### Type expressions

```rust
enum TypeExpr {
    Named(String),                                      // Int, String, User
    Generic { name: String, args: Vec<TypeExpr> },      // List<User>, Map<String, Int>
    Optional(Box<TypeExpr>),                             // Optional<T>
    Result { ok: Box<TypeExpr>, errors: Vec<String> },   // User or NotFound or DbError
}
```

### Errors

```rust
struct ParseError {
    line: usize,
    column: usize,
    message: String,
    hint: Option<String>,
    source_line: String,
}
```

Parser returns `Result<Program, Vec<ParseError>>`. Fail-fast for now (always one element), but API is ready for error recovery later.

## Expression Precedence (lowest to highest)

1. **Pipeline** `|`
2. **Or** (logical and result type `or`)
3. **And**
4. **Not** (unary prefix)
5. **Comparison** `==`, `!=`, `<`, `>`, `<=`, `>=`, `is`
6. **Addition** `+`, `-`
7. **Multiplication** `*`, `/`
8. **Unary** `-` (negation)
9. **Postfix** `.field`, `(args)`, `?` — left-to-right, same level

`find_user(id)?.name` parses as: FnCall → ErrorPropagation → FieldAccess. Sequential, no ambiguity.

## Module Structure

```
src/
  parser/
    mod.rs         — pub mod, re-exports (Parser, Program, all AST types, ParseError)
    ast.rs         — all AST types (Statement, Expr, TypeExpr, PipelineStep, etc.)
    parser.rs      — struct Parser, all parsing logic
    errors.rs      — ParseError with Display impl (same format as LexerError)
```

## Parser Structure

```rust
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,   // original source for error messages
}

impl Parser {
    fn new(tokens: Vec<Token>, source: &str) -> Self;
    fn parse(&mut self) -> Result<Program, Vec<ParseError>>;
}
```

### Key parsing methods

```
parse_program()          → Program (loop parse_statement until Eof)
parse_statement()        → Statement (dispatch on current token)
  parse_let_or_var()     → Statement::Let
  parse_fn_decl()        → Statement::FnDecl (with optional intent before)
  parse_type_decl()      → TypeDecl (branch on { vs = after name)
  parse_use()            → Statement::Use
  parse_return()         → Statement::Return (with optional `if` condition)
parse_expression()       → Expr (entry point, delegates to parse_pipeline)
  parse_pipeline()       → Pipeline if `|` follows, otherwise delegates
  parse_or()             → BinaryOp(Or) or delegates
  parse_and()            → BinaryOp(And) or delegates
  parse_not()            → UnaryOp(Not) or delegates
  parse_comparison()     → BinaryOp(Eq/NotEq/Lt/Gt/etc) or delegates
  parse_addition()       → BinaryOp(Add/Sub) or delegates
  parse_multiplication() → BinaryOp(Mul/Div) or delegates
  parse_unary()          → UnaryOp(Neg) or delegates
  parse_postfix()        → FieldAccess, FnCall, ErrorPropagation (loop)
  parse_primary()        → literals, identifiers, grouped (), if, match, block, dot-shorthand
parse_type_expr()        → TypeExpr
parse_pipeline_step()    → PipelineStep (match on contextual keyword after |)
parse_block()            → Vec<Statement> (between { })
```

### Pipeline step parsing

After `|`, look at the next identifier:
- `filter` → expect `where`, parse predicate
- `map` → expect `to`, parse expression
- `sort` → expect `by`, parse field, optional `ascending`/`descending`
- `group` → expect `by`, parse field
- `take` → expect `first`/`last`, parse count
- `skip` → parse count expression
- `each` → parse expression
- `find` → expect `first`, expect `where`, parse predicate
- `expect` → expect `one`/`any`, expect `or`, expect `raise`, parse error
- `or` → expect `default`, parse value
- `flatten`, `unique`, `count`, `sum` → no args
- anything else → `PipelineStep::Expr(parse_expression())`

### Newline significance

Newlines are statement separators. The lexer already handles continuation rules (suppressed inside delimiters, after operators, before `|`). The parser receives `Newline` tokens and uses them to separate statements. Multiple consecutive newlines are already collapsed by the lexer.

## Testing Strategy

- Unit tests per parsing method (parse_let, parse_fn, parse_type, etc.)
- Expression precedence tests
- Pipeline parsing tests
- Integration tests with real PACT code from spec
- Error message tests (wrong token → helpful message)

## Design Decisions

### Method calls via postfix chaining

`db.query(...)` is NOT a separate AST node. It parses as:
`Identifier("db")` → `FieldAccess { object: db, field: "query" }` → `FnCall { callee: FieldAccess, args }`

The interpreter sees `FnCall` where `callee` is `FieldAccess` and treats it as a method call on an effect/object. This avoids a separate `MethodCall` node — postfix parsing handles it naturally.

### `where` only in pipeline context

The original spec had `db.query("users", where .id == id)` — `where` as a function argument. This creates ambiguity (is `where` a named argument? a DSL keyword? something else?).

Decision: `where` only appears after `| filter` in a pipeline. Database queries use pipeline filtering:

```pact
// correct PACT
db.query("users") | filter where .id == id

// NOT valid — where inside function call args
db.query("users", where .id == id)
```

This keeps `where` unambiguous — it means exactly one thing in exactly one context.

## Source Spec

Language spec: `docs/spec/PACT_SPEC_v0.1.md`
Backend examples: `docs/spec/PACT_Examples_Backend.pact`
Lexer design: `docs/superpowers/specs/2026-04-02-pact-lexer-design.md`

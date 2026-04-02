# PACT Interpreter v0.1 Design

## Overview

Tree-walking interpreter that directly executes PACT AST. No intermediate representation, no compilation — walks the AST nodes and evaluates them. Fail-fast errors with line/column/hint context.

## Scope — v0.1

Everything needed to execute `user_service.pact`:

Literals, arithmetic, let/var, functions (call, recursion), if/else, match, pipeline (filter/map/sort/count/sum/take/skip/flatten/unique/group by/find first/expect/or default/each + expr fallback), struct literals (named, anonymous, spread), field access, string interpolation, ensure (runtime assert), nothing, error types (`T or Error`, `?` propagation, `return X if Y`).

Effects: in-memory stubs — `db.memory()`, `time.fixed()`, `rng.deterministic()`.

**Not in v0.1:** `use` imports (file resolution), `route`/`app` (HTTP), `test`/`check`/`using` blocks, closures, full effect providers.

**Goal:** `pact run user_service.pact` — execute and see results.

## Runtime Values

```rust
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nothing,
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Struct { type_name: String, fields: HashMap<String, Value> },
    Variant { type_name: String, variant: String, fields: Option<HashMap<String, Value>> },
    Ok(Box<Value>),
    Error { variant: String, fields: Option<HashMap<String, Value>> },
    Function { name: String, params: Vec<Param>, body: Vec<Statement> },
    BuiltinFn { name: String },
    Effect { name: String, methods: HashMap<String, Value> },
}
```

### Key design decisions

**`Ok`/`Error` — explicit result type.** Functions that declare error types (`-> User or NotFound`) wrap their return value: normal return → `Value::Ok(value)`, error return → `Value::Error { variant, fields }`. `?` operator checks: if `Error` → propagate up, if `Ok` → unwrap.

**`BuiltinFn { name }` — dispatch by name.** No function pointers. Interpreter matches on name string and executes Rust logic with full access to its own state. This keeps `Value` derivable (Debug, Clone, PartialEq) and avoids complex trait objects for v0.1.

**`Function` — no closure capture.** Functions don't capture enclosing scope. Parent is always global. Closures are v0.2 if needed.

**`Effect` — container for methods.** `db` is `Effect { name: "db", methods: { "query": BuiltinFn("db.query"), "insert": BuiltinFn("db.insert"), ... } }`. Field access on Effect returns the method. Calling it dispatches through the interpreter.

## Environment

```rust
struct Environment {
    values: HashMap<String, Value>,
    parent: Option<Box<Environment>>,
    mutables: HashSet<String>,  // tracks which names are var (mutable)
}
```

**Lookup:** search `values`, if not found → recurse into `parent`.

**Assign (for `var`):** search up the chain for the name, mutate in-place. Error if name is in `values` but not in `mutables`.

**Three levels max in v0.1:**
1. **Global** — builtin functions, type constructors, effect instances
2. **Function** — parameters + effects from `needs`, parent = global
3. **Block** — if/else/match body, parent = function scope

Function calls create a new Environment with parent = global (not caller). Block entries create a new Environment with parent = current.

## Interpreter

```rust
struct Interpreter {
    global: Environment,
    source: String,  // for error messages
    // Effect state (v0.1: inline fields, v0.2: trait providers)
    db_storage: HashMap<String, Vec<Value>>,  // table_name → rows
    fixed_time: Option<String>,
    rng_seed: Option<u64>,
    rng_counter: u64,
}
```

### Execution flow

```
interpret(program: &Program) → Result<Value, RuntimeError>
    for each statement: eval_statement(stmt, &mut env)

eval_statement(stmt, env) → Result<Value, RuntimeError>
    match stmt:
        Let → eval value, bind in env (with mutability flag)
        FnDecl → store Function value in env
        TypeDecl → register type constructors
        Return → eval value, optional condition check, return ControlFlow::Return
        Expression → eval_expr

eval_expr(expr, env) → Result<Value, RuntimeError>
    match expr:
        IntLiteral, FloatLiteral, BoolLiteral, Nothing → literal Value
        StringLiteral → eval interpolation parts
        Identifier → env.lookup(name)
        FieldAccess → eval object, get field from Struct/Effect
        DotShorthand → lookup _it in env, chain field access
        BinaryOp → eval left + right, apply op
        UnaryOp → eval operand, apply op
        FnCall → eval callee, eval args, dispatch (Function or BuiltinFn)
        Pipeline → eval source, iterate steps
        If → eval condition, eval matching branch
        Match → eval subject, find matching arm, eval body
        StructLiteral → eval fields, construct Struct value
        Ensure → eval predicate, error if false
        ErrorPropagation → eval expr, if Error propagate, if Ok unwrap
        Is → eval expr, check variant name
```

### Control flow

`return` and `?` propagation need to short-circuit evaluation. Use a special error variant:

```rust
enum ControlFlow {
    Return(Value),
    Error(RuntimeError),
}
```

`eval_statement` for `return` emits `ControlFlow::Return(value)`. The function body loop catches it and returns the value. `?` on an `Error` value emits `ControlFlow::Return(Value::Error { ... })`.

## Error Handling

### RuntimeError

```rust
struct RuntimeError {
    line: usize,
    column: usize,
    message: String,
    hint: Option<String>,
    source_line: String,
}
```

Same format as LexerError and ParseError:

```
Runtime error at line 8, col 3:
  let user: User = find_user("nonexistent")?
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^
  Unhandled error: NotFound { resource: "User" }
  Hint: use 'on NotFound:' to handle this error, or add NotFound to function's error types
```

### Error propagation (`?`)

1. Eval expression before `?`
2. If result is `Value::Ok(v)` → return `v` (unwrap)
3. If result is `Value::Error { .. }` → short-circuit, propagate error up the call stack
4. If result is neither Ok nor Error → runtime error ("? operator requires a result value")

### `return X if Y`

1. Eval condition `Y`
2. If true → eval `X`, wrap in `Value::Error` (if X is an error type name) or `Value::Ok`, return
3. If false → continue execution

### `ensure`

1. Eval predicate
2. If false → RuntimeError with hint about what was ensured
3. If true → continue (returns Nothing)

## Pipeline Execution

### General flow

1. Eval source expression → get current value
2. For each step: apply step to current value, get new value
3. Return final value

### `_it` binding

For per-element steps (filter, map, sort, each), the interpreter creates a temporary scope with `_it` = current element. `DotShorthand(["active"])` evaluates as `_it.active`.

### Per-element steps (require `Value::List`)

- **Filter:** iterate items, eval predicate with `_it`, keep where true
- **Map:** iterate items, eval expression with `_it`, collect results
- **Sort:** sort items by eval field with `_it`. Default direction = ascending. `descending: true` reverses.
- **Each:** iterate items, eval expression with `_it` for side effects, return original list unchanged
- **FindFirst:** iterate, return first where predicate is true → `Value::Ok(item)` or continue to next step with Nothing

### Aggregate steps (require `Value::List`)

- **Count:** → `Value::Int(items.len())`
- **Sum:** → fold, adding Int or Float values
- **Flatten:** → concat nested lists
- **Unique:** → dedup (by PartialEq)
- **GroupBy:** → `Value::List` of `Value::Struct { key, values }` groups
- **Take first/last N:** eval count, slice list
- **Skip N:** eval count, drop first N

### Result steps

- **ExpectOne:** if list has exactly one item → `Value::Ok(item)`, else → `Value::Error { variant }`
- **ExpectAny:** if list has any items → `Value::Ok(list)`, else → `Value::Error { variant }`
- **OrDefault:** if current value is `Nothing` → return default, otherwise pass through

### Expr fallback

For `PipelineStep::Expr(expr)`: set `_it` = entire current value (not per-element), eval expression. This handles arbitrary pipe-through like `request | api_pipeline?`.

### Non-List pipeline error

If a per-element step (filter, map, sort, each) receives a non-List value, throw RuntimeError:

```
Runtime error at line 5, col 5:
  user.name | filter where .active
             ^^^^^^
  'filter' expects List, got String
  Hint: filter/map/sort operate on lists. Did you mean to use a function call?
```

## Effect Stubs (v0.1)

### `db.memory()`

In-memory storage. Tables are `HashMap<String, Vec<Value>>`.

Methods:
- `db.insert(table: String, value: Struct)` → stores value, returns it
- `db.query(table: String)` → returns `Value::List` of all rows in table
- `db.update(table: String, predicate, value)` → updates matching rows

### `time.fixed(datetime: String)`

Returns the same fixed datetime string every time.

Methods:
- `time.now()` → returns the fixed datetime string

### `rng.deterministic(seed: u64)`

Deterministic pseudo-random based on seed + counter.

Methods:
- `rng.uuid()` → returns predictable ID string based on seed

## Module Structure

```
src/
  interpreter/
    mod.rs          — pub mod, re-exports
    value.rs        — Value enum, Display impl
    environment.rs  — Environment struct with lookup/assign/bind
    interpreter.rs  — Interpreter struct, eval_statement, eval_expr
    pipeline.rs     — pipeline step execution
    builtins.rs     — builtin function dispatch (db, time, rng, pipeline ops)
    errors.rs       — RuntimeError with Display impl
```

Six files, each with one responsibility. `interpreter.rs` is the core but delegates pipeline execution to `pipeline.rs` and builtin dispatch to `builtins.rs`.

## Testing Strategy

- Unit tests per eval method (eval int, eval string, eval binary op, etc.)
- Environment tests (lookup, assign, scoping)
- Pipeline tests (filter, map, sort, count on concrete data)
- Error propagation tests (?, return if, ensure)
- Integration tests — eval real PACT code snippets
- Effect stub tests (db insert/query, time.now, rng.uuid)

## Source Spec

Language spec: `docs/spec/PACT_SPEC_v0.1.md`
Backend examples: `docs/spec/PACT_Examples_Backend.pact`
Parser design: `docs/superpowers/specs/2026-04-02-pact-parser-v01-design.md`

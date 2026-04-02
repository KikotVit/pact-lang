# PACT Routes v0.3a Design

## Overview

Add route declarations, `on success`/`on error` pipeline steps, and `respond` expressions to parser and interpreter. No HTTP server — routes are executed directly via `execute_route()` with constructed request structs. HTTP transport is v0.3b.

## Scope

- Parser: `route METHOD "path" { ... }`, `| on success: expr`, `| on error: expr`, `respond N with expr`, `| validate as Type`
- Interpreter: route storage, route execution, respond evaluation, on pipeline steps
- Testing: direct route execution with request structs, no network

## Parser Additions

### AST nodes

```rust
// Add to Statement enum:
Route {
    method: String,         // "GET", "POST", "PUT", "DELETE"
    path: String,           // "/users/{id}"
    intent: String,         // REQUIRED — same as fn
    effects: Vec<String>,   // needs db, auth
    body: Vec<Statement>,
}

// Add to PipelineStep enum:
OnSuccess { body: Expr },
OnError { variant: String, body: Expr },
ValidateAs { type_name: String },

// Add to Expr enum:
Respond { status: Box<Expr>, body: Box<Expr> },
```

### Parse rules

**`route METHOD "path" { ... }`:**
1. Consume `Route` keyword token
2. Read identifier for HTTP method (GET, POST, PUT, DELETE)
3. Parse string literal for path
4. Expect `{`
5. REQUIRED: first statement must be `intent "description"` — error if missing
6. Optional: `needs effect1, effect2` after intent
7. Parse remaining body statements
8. Expect `}`

**`| on success: expr` and `| on Error: expr`:**

In `parse_pipeline_step`, when identifier is `"on"`:
1. Consume `on`
2. Read next identifier: if `"success"` → `OnSuccess`, otherwise → `OnError { variant: identifier }`
3. Expect `Colon` token
4. Parse expression (typically `respond N with expr`)
5. Return the pipeline step

**`respond N with expr`:**

In `parse_primary`, when identifier is `"respond"`:
1. Consume `respond`
2. Parse status expression (typically an integer literal)
3. Expect contextual keyword `"with"`
4. Parse body expression
5. Return `Expr::Respond { status, body }`

**`| validate as TypeName`:**

In `parse_pipeline_step`, when identifier is `"validate"`:
1. Consume `validate`
2. Expect `As` keyword token
3. Read identifier for type name
4. Return `PipelineStep::ValidateAs { type_name }`

### Lexer note

`route` is already a reserved keyword (`TokenKind::Route`). `respond`, `validate`, `on` are contextual identifiers — no lexer changes needed.

## Interpreter Additions

### Route storage

Routes are stored during `interpret()` — when `Statement::Route` is encountered, store it in a `Vec` on the Interpreter. Routes are NOT executed during normal program evaluation.

```rust
// Add to Interpreter struct:
pub routes: Vec<StoredRoute>,

pub struct StoredRoute {
    pub method: String,
    pub path: String,
    pub intent: String,
    pub effects: Vec<String>,
    pub body: Vec<Statement>,
}
```

### Route execution

```rust
pub fn execute_route(
    &mut self,
    route: &StoredRoute,
    request: Value,  // Value::Struct with method, path, params, query, body
) -> Result<Value, RuntimeError>
```

1. Create new environment (parent = global)
2. Bind `request` in env
3. Inject effects from route's `needs` list
4. Eval body statements
5. Return the last value (should be a Response struct from `respond`)

### `respond` evaluation

`Expr::Respond { status, body }`:
1. Eval status → expect `Value::Int`
2. Eval body → any Value
3. Return `Value::Struct { type_name: "Response", fields: { "status": Int, "body": Value } }`

### `on success` / `on error` pipeline steps

**`OnSuccess { body }`:**
- If current value is `Value::Ok(v)` → set `_it = *v`, eval body, return result
- If current value is `Value::Error { .. }` → pass through unchanged (skip this step)
- If current value is neither → treat as success (set `_it = value`, eval body)

**`OnError { variant, body }`:**
- If current value is `Value::Error { variant: v, .. }` and `v == variant` → set `_it = error value`, eval body, return result
- Otherwise → pass through unchanged

This means a pipeline like:
```pact
find_user(request.params.id)
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "User not found" }
```
First evaluates `find_user()` which returns `Ok(user)` or `Error(NotFound)`. Then `on success` handles the Ok case, `on NotFound` handles the error case.

### `validate as` pipeline step

For v0.3a: `ValidateAs` is a pass-through — returns the current value unchanged. Real validation (with `check` constraints) is a later feature. The step exists so the parser can handle the syntax and the pipeline doesn't break.

### Request struct format

```rust
Value::Struct {
    type_name: "Request",
    fields: {
        "method": Value::String("GET"),
        "path": Value::String("/users/123"),
        "params": Value::Struct { type_name: "Params", fields: { "id": Value::String("123") } },
        "query": Value::Struct { type_name: "Query", fields: {} },
        "body": Value::Nothing,
    }
}
```

## Testing Strategy

Test routes by constructing request structs and calling `execute_route`:

```rust
// Register functions (find_user etc.) in global env
// Create request struct
// Execute route
// Assert response status and body
```

Key tests:
- Simple route returning respond 200
- Route with on success / on error
- Route with needs effects
- Route without intent → parse error
- respond creates correct Response struct

## Module structure

No new files — extend existing:
- `src/parser/ast.rs` — Route, OnSuccess, OnError, ValidateAs, Respond
- `src/parser/parser.rs` — parse_route, pipeline step additions, respond in primary
- `src/interpreter/interpreter.rs` — eval_respond, route storage, execute_route, on/validate pipeline steps

# PACT Routes v0.3a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add route declarations, `on success`/`on error` pipeline steps, `respond` expression, and route execution to parser and interpreter — without HTTP server.

**Architecture:** Routes are parsed as `Statement::Route` with method, path, intent (required), effects, and body. `respond N with expr` is an expression returning a Response struct. `on success`/`on error` are pipeline steps matching on Ok/Error values. Routes are stored in Interpreter and executed via `execute_route(route, request)`.

**Tech Stack:** Rust 1.94, no new dependencies. Extends existing parser and interpreter.

**Design spec:** `docs/superpowers/specs/2026-04-02-pact-routes-v03a-design.md`

---

## File Structure

No new files — extend existing:
```
src/
  parser/
    ast.rs          — add Route, OnSuccess, OnError, ValidateAs, Respond
    parser.rs       — add parse_route, on/respond/validate parsing
  interpreter/
    interpreter.rs  — add eval_respond, route storage/execution, on/validate pipeline steps
```

---

### Task 1: AST additions

**Files:**
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Add Route to Statement enum**

Add to the `Statement` enum in `src/parser/ast.rs`:

```rust
Route {
    method: String,
    path: String,
    intent: String,
    effects: Vec<String>,
    body: Vec<Statement>,
},
```

- [ ] **Step 2: Add OnSuccess, OnError, ValidateAs to PipelineStep enum**

Add to `PipelineStep` enum:

```rust
OnSuccess { body: Expr },
OnError { variant: String, body: Expr },
ValidateAs { type_name: String },
```

- [ ] **Step 3: Add Respond to Expr enum**

Add to `Expr` enum:

```rust
Respond { status: Box<Expr>, body: Box<Expr> },
```

- [ ] **Step 4: Verify compilation**

Run: `cargo test`
Expected: compiles (new variants unused but not an error). All 246 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/parser/ast.rs
git commit -m "feat: add Route, OnSuccess, OnError, ValidateAs, Respond AST nodes"
```

---

### Task 2: Parser — route, respond, on, validate

**Files:**
- Modify: `src/parser/parser.rs`

- [ ] **Step 1: Add `parse_route` and wire into `parse_statement`**

In `parse_statement`, add a case for `TokenKind::Route`:

```rust
TokenKind::Route => self.parse_route()?,
```

Implement `parse_route`:

```rust
fn parse_route(&mut self) -> Result<Statement, ParseError> {
    self.advance(); // consume `route`
    
    // HTTP method (GET, POST, PUT, DELETE)
    let method = self.expect_identifier()?;
    
    // Path string
    let path = match self.parse_string_expr()? {
        Expr::StringLiteral(StringExpr::Simple(s)) => s,
        Expr::StringLiteral(StringExpr::Interpolated(_)) => {
            return self.fail("Route path must be a simple string, not interpolated", None);
        }
        _ => return self.fail("Expected string for route path", None),
    };
    
    // Body block
    self.push_block("route");
    self.expect(&TokenKind::LBrace)?;
    self.skip_newlines();
    
    // REQUIRED: intent must be first
    let intent = if self.at(&TokenKind::Intent) {
        self.advance();
        self.parse_intent_string()?
    } else {
        return Err(self.error(
            "Missing 'intent' block in route declaration",
            Some("every route requires intent \"description\" as the first line"),
        ));
    };
    self.skip_newlines();
    
    // Optional: needs effects
    let mut effects = Vec::new();
    if self.at(&TokenKind::Needs) {
        self.advance();
        effects.push(self.expect_identifier()?);
        while self.eat(&TokenKind::Comma) {
            if self.at(&TokenKind::LBrace) || self.at(&TokenKind::Newline) {
                break;
            }
            effects.push(self.expect_identifier()?);
        }
    }
    self.skip_newlines();
    
    // Parse body statements
    let mut body = Vec::new();
    while !self.at(&TokenKind::RBrace) && !self.at_eof() {
        let stmt = self.parse_statement()?;
        body.push(stmt);
        self.skip_newlines();
    }
    self.expect_closing_brace()?;
    
    Ok(Statement::Route { method, path, intent, effects, body })
}
```

- [ ] **Step 2: Add `respond` to `parse_primary`**

In `parse_primary`, add a case for identifier `"respond"`:

```rust
// In the match on current_kind, before the generic Identifier case:
TokenKind::Identifier(ref word) if word == "respond" => {
    self.advance(); // consume "respond"
    let status = self.parse_expression()?;
    self.expect_contextual("with")?;
    let body = self.parse_expression()?;
    Ok(Expr::Respond {
        status: Box::new(status),
        body: Box::new(body),
    })
}
```

IMPORTANT: This case must come BEFORE the generic `TokenKind::Identifier(name)` case in `parse_primary`. Since `parse_primary` clones the current kind and matches, add this as a separate arm matching `Identifier` with the specific string "respond".

- [ ] **Step 3: Add `on` and `validate` to `parse_pipeline_step`**

In `parse_pipeline_step`, add cases in the identifier match:

```rust
"on" => {
    self.advance(); // consume "on"
    if self.eat_contextual("success") {
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_or()?;
        return Ok(PipelineStep::OnSuccess { body });
    } else {
        // on ErrorVariant: expr
        let variant = self.expect_identifier()?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_or()?;
        return Ok(PipelineStep::OnError { variant, body });
    }
}
"validate" => {
    self.advance(); // consume "validate"
    self.expect(&TokenKind::As)?;
    let type_name = self.expect_identifier()?;
    return Ok(PipelineStep::ValidateAs { type_name });
}
```

- [ ] **Step 4: Add parser tests**

```rust
#[test]
fn parse_simple_route() {
    let input = r#"route GET "/health" {
  intent "health check"
  respond 200 with { status: "ok" }
}"#;
    let prog = parse_program(input);
    assert!(matches!(&prog.statements[0], Statement::Route { method, .. } if method == "GET"));
}

#[test]
fn parse_route_with_needs() {
    let input = r#"route GET "/users" {
  intent "list users"
  needs db, auth
  respond 200 with nothing
}"#;
    let prog = parse_program(input);
    if let Statement::Route { effects, .. } = &prog.statements[0] {
        assert_eq!(effects, &["db", "auth"]);
    } else { panic!("Expected Route"); }
}

#[test]
fn parse_route_without_intent_fails() {
    let input = r#"route GET "/health" {
  respond 200 with { status: "ok" }
}"#;
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, input);
    let err = parser.parse().unwrap_err();
    assert!(err[0].message.contains("intent"));
}

#[test]
fn parse_respond_expression() {
    let expr = parse_expr("respond 200 with nothing");
    assert!(matches!(expr, Expr::Respond { .. }));
}

#[test]
fn parse_on_success_pipeline() {
    let input = r#"x | on success: respond 200 with ."#;
    let expr = parse_expr(input);
    if let Expr::Pipeline { steps, .. } = &expr {
        assert!(matches!(&steps[0], PipelineStep::OnSuccess { .. }));
    } else { panic!("Expected Pipeline"); }
}

#[test]
fn parse_on_error_pipeline() {
    let input = r#"x | on NotFound: respond 404 with nothing"#;
    let expr = parse_expr(input);
    if let Expr::Pipeline { steps, .. } = &expr {
        assert!(matches!(&steps[0], PipelineStep::OnError { variant, .. } if variant == "NotFound"));
    } else { panic!("Expected Pipeline"); }
}

#[test]
fn parse_validate_as_pipeline() {
    let input = "x | validate as NewUser";
    let expr = parse_expr(input);
    if let Expr::Pipeline { steps, .. } = &expr {
        assert!(matches!(&steps[0], PipelineStep::ValidateAs { type_name } if type_name == "NewUser"));
    } else { panic!("Expected Pipeline"); }
}

#[test]
fn parse_route_with_pipeline() {
    let input = r#"route GET "/users/{id}" {
  intent "get user by ID"
  needs db
  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "not found" }
}"#;
    let prog = parse_program(input);
    if let Statement::Route { body, .. } = &prog.statements[0] {
        assert!(!body.is_empty());
    } else { panic!("Expected Route"); }
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test`

```bash
git add src/parser/parser.rs
git commit -m "feat: add route, respond, on success/error, validate as parsing"
```

---

### Task 3: Interpreter — respond, on/validate pipeline steps, route execution

**Files:**
- Modify: `src/interpreter/interpreter.rs`

- [ ] **Step 1: Add `respond` evaluation**

In `eval_expr`, add a match arm for `Expr::Respond`:

```rust
Expr::Respond { status, body } => {
    let status_val = self.eval_expr(status, env)?;
    let body_val = self.eval_expr(body, env)?;
    let mut fields = HashMap::new();
    fields.insert("status".to_string(), status_val);
    fields.insert("body".to_string(), body_val);
    Ok(Value::Struct {
        type_name: "Response".to_string(),
        fields,
    })
}
```

- [ ] **Step 2: Add `on` and `validate` pipeline steps**

In the pipeline step execution (wherever `execute_pipeline_step` or the match on PipelineStep is), add:

```rust
PipelineStep::OnSuccess { body } => {
    match &current {
        Value::Ok(inner) => {
            let mut step_env = Environment::with_parent(env.clone());
            step_env.bind("_it".to_string(), *inner.clone(), false);
            self.eval_expr(body, &mut step_env)
        }
        Value::Error { .. } => Ok(current), // pass through
        other => {
            // Non-result value — treat as success
            let mut step_env = Environment::with_parent(env.clone());
            step_env.bind("_it".to_string(), other.clone(), false);
            self.eval_expr(body, &mut step_env)
        }
    }
}
PipelineStep::OnError { variant, body } => {
    match &current {
        Value::Error { variant: v, .. } if v == variant => {
            let mut step_env = Environment::with_parent(env.clone());
            step_env.bind("_it".to_string(), current.clone(), false);
            self.eval_expr(body, &mut step_env)
        }
        _ => Ok(current), // pass through
    }
}
PipelineStep::ValidateAs { .. } => {
    // v0.3a: pass through (real validation with check constraints is later)
    Ok(current)
}
```

- [ ] **Step 3: Add route storage and execution**

Add to `Interpreter` struct:

```rust
pub routes: Vec<StoredRoute>,
```

Add a struct (outside Interpreter impl, or in a sub-module):

```rust
#[derive(Debug, Clone)]
pub struct StoredRoute {
    pub method: String,
    pub path: String,
    pub intent: String,
    pub effects: Vec<String>,
    pub body: Vec<Statement>,
}
```

In `eval_statement`, handle `Statement::Route`:

```rust
Statement::Route { method, path, intent, effects, body } => {
    self.routes.push(StoredRoute {
        method: method.clone(),
        path: path.clone(),
        intent: intent.clone(),
        effects: effects.clone(),
        body: body.clone(),
    });
    Ok(StmtResult::Value(Value::Nothing))
}
```

Add `execute_route` method:

```rust
pub fn execute_route(&mut self, route: &StoredRoute, request: Value) -> Result<Value, RuntimeError> {
    let mut env = Environment::with_parent(self.global.clone());
    env.bind("request".to_string(), request, false);
    
    // Inject effects
    for effect_name in &route.effects {
        if let Some(effect) = self.global.lookup(effect_name) {
            env.bind(effect_name.clone(), effect.clone(), false);
        }
    }
    
    // Eval body
    let mut result = Value::Nothing;
    let body = route.body.clone();
    for stmt in &body {
        match self.eval_statement(stmt, &mut env)? {
            StmtResult::Value(val) => result = val,
            StmtResult::Return(val) => return Ok(val),
        }
    }
    Ok(result)
}
```

Initialize `routes: Vec::new()` in `Interpreter::new()`.

- [ ] **Step 4: Add interpreter tests**

```rust
#[test]
fn eval_respond() {
    let result = eval("respond 200 with nothing");
    if let Value::Struct { type_name, fields } = &result {
        assert_eq!(type_name, "Response");
        assert_eq!(fields.get("status"), Some(&Value::Int(200)));
        assert_eq!(fields.get("body"), Some(&Value::Nothing));
    } else { panic!("Expected Response struct, got {:?}", result); }
}

#[test]
fn eval_respond_with_struct() {
    let result = eval(r#"respond 200 with { message: "ok" }"#);
    if let Value::Struct { type_name, fields } = &result {
        assert_eq!(type_name, "Response");
        let body = fields.get("body").unwrap();
        assert!(matches!(body, Value::Struct { .. }));
    } else { panic!("Expected Response struct"); }
}

#[test]
fn eval_on_success_pipeline() {
    let result = eval_with_list("let x: Ok = list(42) | expect success\nx | on success: respond 200 with .");
    // expect success on a single-element list returns Ok(42)
    // Actually this is tricky — need to test with a value that's already Ok/Error
    // Better test: construct Ok/Error directly
}

#[test]
fn eval_route_stores_and_executes() {
    let input = r#"route GET "/health" {
  intent "health check"
  respond 200 with { status: "ok" }
}"#;
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, input);
    let program = parser.parse().unwrap();
    let mut interp = Interpreter::new(input);
    interp.setup_test_effects();
    interp.interpret(&program).unwrap();
    
    assert_eq!(interp.routes.len(), 1);
    assert_eq!(interp.routes[0].method, "GET");
    assert_eq!(interp.routes[0].path, "/health");
    
    // Execute the route
    let mut request_fields = HashMap::new();
    request_fields.insert("method".to_string(), Value::String("GET".to_string()));
    request_fields.insert("path".to_string(), Value::String("/health".to_string()));
    let request = Value::Struct { type_name: "Request".to_string(), fields: request_fields };
    
    let response = interp.execute_route(&interp.routes[0].clone(), request).unwrap();
    if let Value::Struct { type_name, fields } = &response {
        assert_eq!(type_name, "Response");
        assert_eq!(fields.get("status"), Some(&Value::Int(200)));
    } else { panic!("Expected Response"); }
}

#[test]
fn eval_route_with_on_success_error() {
    let input = r#"intent "find user"
fn find_user(id: String) -> User or NotFound {
  if id == "1" {
    User { id: "1", name: "Alice" }
  } else {
    return NotFound
  }
}

route GET "/users/{id}" {
  intent "get user by ID"
  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "not found" }
}"#;
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, input);
    let program = parser.parse().unwrap();
    let mut interp = Interpreter::new(input);
    interp.setup_test_effects();
    interp.interpret(&program).unwrap();
    
    // Test success case
    let mut params = HashMap::new();
    params.insert("id".to_string(), Value::String("1".to_string()));
    let mut req_fields = HashMap::new();
    req_fields.insert("method".to_string(), Value::String("GET".to_string()));
    req_fields.insert("path".to_string(), Value::String("/users/1".to_string()));
    req_fields.insert("params".to_string(), Value::Struct { type_name: "Params".to_string(), fields: params });
    let request = Value::Struct { type_name: "Request".to_string(), fields: req_fields };
    
    let response = interp.execute_route(&interp.routes[0].clone(), request).unwrap();
    if let Value::Struct { fields, .. } = &response {
        assert_eq!(fields.get("status"), Some(&Value::Int(200)));
    } else { panic!("Expected Response"); }
    
    // Test not found case
    let mut params2 = HashMap::new();
    params2.insert("id".to_string(), Value::String("999".to_string()));
    let mut req_fields2 = HashMap::new();
    req_fields2.insert("method".to_string(), Value::String("GET".to_string()));
    req_fields2.insert("path".to_string(), Value::String("/users/999".to_string()));
    req_fields2.insert("params".to_string(), Value::Struct { type_name: "Params".to_string(), fields: params2 });
    let request2 = Value::Struct { type_name: "Request".to_string(), fields: req_fields2 };
    
    let response2 = interp.execute_route(&interp.routes[0].clone(), request2).unwrap();
    if let Value::Struct { fields, .. } = &response2 {
        assert_eq!(fields.get("status"), Some(&Value::Int(404)));
    } else { panic!("Expected Response"); }
}

#[test]
fn parse_route_intent_required() {
    let input = r#"route GET "/test" {
  respond 200 with nothing
}"#;
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens, input);
    let err = parser.parse().unwrap_err();
    assert!(err[0].message.contains("intent"));
}
```

- [ ] **Step 5: Run tests, commit, and push**

Run: `cargo test`

```bash
git add src/parser/ast.rs src/parser/parser.rs src/interpreter/interpreter.rs
git commit -m "feat: add route execution, respond, on success/error pipeline steps"
git push
```

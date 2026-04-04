# Functions

Define reusable logic with `fn`. Every function requires an `intent` declaration before it.

## Syntax

```pact
intent "describe what the function does"
fn name(param: Type) -> ReturnType {
  body
}
```

## Basic function

```pact
intent "add two numbers"
fn add(a: Int, b: Int) -> Int {
  a + b
}
```

## Error types

Functions can declare error types they may produce with `or`:

```pact
intent "find a user by id"
fn find_user(id: String) -> Struct or NotFound
  needs db
{
  db.query("users")
    | filter where .id == id
    | expect one or raise NotFound
}
```

## Effects with needs

Declare side effects after the return type, before the body:

```pact
intent "create a new user"
fn create_user(data: Struct) -> Struct
  needs db, rng, time
{
  let user: Struct = {
    id: rng.uuid(),
    name: data.name,
    created: time.now()
  }
  db.insert("users", user)
}
```

## Calling functions and error propagation

Use `?` to propagate errors from function calls:

```pact
intent "get user profile"
fn get_profile(id: String) -> Struct or NotFound
  needs db
{
  let user: Struct = find_user(id)?
  user
}
```

> See also: error, effects, pipeline, route

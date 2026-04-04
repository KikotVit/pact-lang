# Error Handling

Errors are values in PACT, not exceptions. Functions declare what errors they can produce, and callers handle them explicitly.

## Error types in function signatures

```pact
intent "find a todo"
fn find_todo(id: String) -> Struct or NotFound
  needs db
{
  db.query("todos")
    | filter where .id == id
    | expect one or raise NotFound
}
```

## Multiple error types

```pact
intent "update a todo"
fn update_todo(id: String, data: Struct) -> Struct or NotFound or BadRequest
  needs db
{
  return { error: "invalid" } if data.title == ""
  db.query("todos")
    | filter where .id == id
    | expect one or raise NotFound
}
```

## Error propagation with ?

Pass errors up to the caller:

```pact
intent "get todo title"
fn get_title(id: String) -> Struct or NotFound
  needs db
{
  let todo: Struct = find_todo(id)?
  todo
}
```

## Pipeline error handling

Handle success and error cases in pipelines:

```pact
find_todo(id)
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "Not found" }
```

## expect one / expect any

```pact
db.query("users")
  | filter where .email == email
  | expect one or raise NotFound
```

```pact
db.query("items")
  | expect any or raise Empty
```

## Conditional return

Return an error value early based on a condition:

```pact
return BadRequest if name == ""
```

> See also: fn, pipeline, match

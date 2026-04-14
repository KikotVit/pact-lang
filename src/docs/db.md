# Database

PACT provides six database operations. Tables are auto-created on first insert. Declare `needs db` to use database operations.

## Insert

```pact
let todo: Struct = { id: "1", title: "Buy milk", done: false }
db.insert("todos", todo)
```

## Query

Returns all rows from a table. Use pipelines to filter:

```pact
db.query("todos")
  | filter where .done == false
  | sort by .title
```

## Find

Find a single record by field match:

```pact
db.find("todos", { id: "1" })
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "Not found" }
```

## Update

Update a record by ID:

```pact
db.update("todos", "1", { id: "1", title: "Buy milk", done: true })
```

## Delete

Delete a record by ID:

```pact
db.delete("todos", "1")
```

## Delete Where

Delete rows matching a filter. Returns the number of deleted rows:

```pact
let deleted: Int = db.delete_where("logs", { before: "2026-04-07T00:00:00Z" })
```

The `before` key compares against the `created_at` field — rows where `created_at < value` are deleted. Useful for scheduled cleanup of old records.

## Full example in a route

```pact
intent "list active todos"
route GET "/todos" {
  needs db
  db.query("todos")
    | filter where .done == false
    | sort by .title
    | respond 200 with .
}
```

## Error handling

Database operations return `DbError` when something goes wrong (disk full, constraint violation, corruption). Use pipeline error handling:

```pact
db.insert("users", user)
  | on success: respond 201 with .
  | on DbError: respond 500 with { error: .message }
```

`DbError` has two fields:
- `.message` — human-readable error description
- `.kind` — `"constraint"` (duplicate, not null) or `"internal"` (disk, corruption)

Use `where` to match specific error kinds:

```pact
db.insert("users", user)
  | on DbError where .kind == "constraint": respond 409 with { error: "Already exists" }
  | on DbError: respond 500 with { error: .message }
```

`NotFound` is a separate error type returned by `db.find`, `db.update`, and `db.delete` when the record doesn't exist:

```pact
db.find("users", { id: id })
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "Not found" }
```

## Testing with in-memory database

Use `db.memory()` in tests for a fast, isolated database:

```pact
test "insert and query" {
  using db = db.memory()
  db.insert("items", { id: "1", name: "test" })
  let items: Struct = db.query("items")
  assert items.length() == 1
}
```

## Watch (SSE streaming)

`db.watch()` returns a stream descriptor for real-time SSE. Used inside `stream` routes:

```pact
intent "stream new items"
stream GET "/items/live" {
  needs db
  send db.watch("items")
}
```

With a filter:

```pact
send db.watch("items", { category: "books" })
```

> See also: route, stream, effects, test, pipeline

# Database

PACT provides five database operations. Tables are auto-created on first insert. Declare `needs db` to use database operations.

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

> See also: route, effects, test, pipeline

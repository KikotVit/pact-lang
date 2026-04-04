# Tests

Write tests with `test` blocks. Tests use mock effects to run without external dependencies.

## Syntax

```pact
test "description" {
  using db = db.memory()
  assert 1 + 1 == 2
}
```

## Mock effects with using

Set up deterministic test environments:

```pact
test "user creation" {
  using db = db.memory()
  using rng = rng.deterministic(42)
  using time = time.fixed("2026-04-04T12:00:00Z")

  let user: Struct = {
    id: rng.hex(8),
    name: "Alice",
    created: time.now()
  }
  db.insert("users", user)

  let found: Struct = db.find("users", { id: user.id })
  assert found.name == "Alice"
}
```

## Assert

Use `assert` to verify conditions:

```pact
test "basic assertions" {
  assert 2 + 2 == 4
  assert "hello".length() == 5
  assert true != false
}
```

## Testing with pipelines

```pact
test "pipeline operations" {
  let n: Int = list(1, 2, 3) | count
  assert n == 3
  let total: Int = list(1, 2, 3) | sum
  assert total == 6
}
```

> See also: effects, db, fn

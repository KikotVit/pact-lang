# Types

Define data structures with `type`. PACT supports struct types and union types.

## Struct types

```pact
type User {
  id: String,
  name: String,
  age: Int,
  active: Bool
}
```

## Union types

```pact
type Role = Admin | Editor | Viewer
```

## Using types in let bindings

```pact
let name: String = "Alice"
let age: Int = 30
let active: Bool = true
let score: Float = 9.5
```

## Struct literals

Create struct values with `{ key: value }` syntax:

```pact
let user: Struct = {
  name: "Alice",
  age: 30,
  active: true
}
```

## Validation constraints

Add constraints to struct fields with `|` syntax:

```pact
type NewUser {
  name: String | minlen 1 | maxlen 100,
  email: String | format email,
  age: Int | min 0 | max 150,
}
```

Constraints are enforced with `| validate as Type`:

```pact
request.body | validate as NewUser
  | on ValidationError: respond 400 with { error: .message }
```

Available constraints:
- `min N` / `max N` — Int range
- `minlen N` / `maxlen N` — String length
- `format email` / `format url` / `format uuid` — String format
- `pattern "..."` — String contains pattern

## Spread syntax

Copy fields from another struct and override specific ones:

```pact
let updated: Struct = { ...user, active: false }
```

> See also: fn, match, db, modules

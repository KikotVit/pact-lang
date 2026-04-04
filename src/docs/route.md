# Routes

Define HTTP endpoints with `route`. Every route requires an `intent` declaration before it.

## Syntax

```pact
intent "describe the route"
route METHOD "/path" {
  needs effect1, effect2
  respond 200 with expr
}
```

## GET with path parameters

```pact
intent "get user by id"
route GET "/users/{id}" {
  needs db
  db.find("users", { id: request.params.id })
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "Not found" }
}
```

## POST with request body

```pact
intent "create a user"
route POST "/users" {
  needs db, rng
  let user: Struct = {
    id: rng.hex(8),
    name: request.body.name,
    active: true
  }
  db.insert("users", user)
  respond 201 with user
}
```

## PUT to update

```pact
intent "update a user"
route PUT "/users/{id}" {
  needs db
  db.update("users", request.params.id, request.body)
  respond 200 with request.body
}
```

## DELETE

```pact
intent "delete a user"
route DELETE "/users/{id}" {
  needs db
  db.delete("users", request.params.id)
  respond 204 with nothing
}
```

The `request` object provides `request.params`, `request.body`, and `request.query` for accessing path parameters, request body, and query string fields.

> See also: fn, pipeline, app, effects

intent "list all users"
route GET "/users" {
  needs db

  db.query("users") | respond 200 with .
}

intent "get user by ID"
route GET "/users/{id}" {
  needs db

  db.find("users", { id: request.params.id })
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "create user"
route POST "/users" {
  needs db

  db.insert("users", request.body)
    | on success: respond 201 with .
}

intent "update user"
route PUT "/users/{id}" {
  needs db

  db.update("users", request.params.id, request.body)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "delete user"
route DELETE "/users/{id}" {
  needs db

  db.delete("users", request.params.id)
    | on success: respond 200 with { message: "Deleted" }
    | on NotFound: respond 404 with { error: "User not found" }
}

app API {
  port: 8080,
  db: "sqlite:///data/api.db",
}

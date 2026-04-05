use models.user.find_user
use models.user.create_user
use models.user.User

intent "get user by id"
route GET "/users/{id}" {
  needs db
  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "create user"
route POST "/users" {
  needs db, rng
  create_user(request.body.name, request.body.email)
    | on success: respond 201 with .
}

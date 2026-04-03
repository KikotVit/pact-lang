// ============================================
// models/user.pact
// ============================================

type Role = Admin | Editor | Viewer

type User {
  id: String,
  name: String,
  email: String,
  age: Int,
  role: Role,
  active: Bool,
  created_at: String,
}

type NewUser {
  name: String,
  email: String,
  age: Int,
}

type UserUpdate {
  name: String,
  email: String,
  age: Int,
}

type UserSummary {
  total: Int,
  active: Int,
}


// ============================================
// errors.pact
// ============================================

type AppError
  = NotFound { resource: String }
  | Forbidden { reason: String }
  | BadRequest { message: String }


// ============================================
// services/user_service.pact
// ============================================

use models.user.User
use models.user.NewUser
use models.user.UserUpdate
use models.user.UserSummary
use errors.AppError

intent "find user by ID"
fn find_user(id: String) -> Struct or NotFound
  needs db
{
  db.find("users", { id: id })
}

intent "find user by email"
fn find_by_email(email: String) -> Struct
  needs db
{
  db.find("users", { email: email })
}

intent "create a new user with default Viewer role"
fn create_user(data: Struct) -> Struct or BadRequest
  needs db, time, rng
{
  let existing: Struct = find_by_email(data.email)
  return BadRequest { message: "Email already taken" } if existing != nothing

  let user: Struct = {
    id: rng.uuid(),
    name: data.name,
    email: data.email,
    age: data.age,
    role: "Viewer",
    active: true,
    created_at: time.now(),
  }

  db.insert("users", user)
}

intent "update an existing user's data"
fn update_user(id: String, updates: Struct) -> Struct or NotFound
  needs db, time
{
  let user: Struct = find_user(id)?

  let updated: Struct = {
    ...user,
    name: updates.name | or default user.name,
    email: updates.email | or default user.email,
    age: updates.age | or default user.age,
    updated_at: time.now(),
  }

  db.update("users", id, updated)
}

intent "deactivate user instead of deleting"
fn deactivate_user(id: String) -> Struct or NotFound
  needs db, time
{
  let user: Struct = find_user(id)?

  db.update("users", id, {
    ...user,
    active: false,
    deactivated_at: time.now(),
  })
}

intent "get user statistics"
fn user_summary() -> Struct
  needs db
{
  let users: List = db.query("users")

  {
    total: users | count,
    active: users | filter where .active | count,
  }
}


// ============================================
// handlers/user_handlers.pact
// ============================================

use services.user_service.*
use models.user.*

// shared pipeline instead of middleware — explicit, visible, testable
intent "authenticate request and return caller"
fn api_pipeline(request: Struct) -> Struct or Unauthorized
  needs auth, log
{
  log.info("{request.method} {request.path}")
  let caller: Struct = auth.require(request)?
  { caller: caller }
}

intent "list active users with pagination"
route GET "/users" {
  needs db, auth, log

  let ctx: Struct = api_pipeline(request)?
  let page: Int = request.query.page | or default 1
  let limit: Int = request.query.limit | or default 20

  db.query("users", { active: true })
    | sort by .created_at descending
    | skip (page - 1) * limit
    | take first limit
    | respond 200 with .
}

intent "get user summary statistics"
route GET "/users/summary" {
  needs db, auth, log

  let ctx: Struct = api_pipeline(request)?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != "Admin"

  let summary: Struct = user_summary()

  respond 200 with summary
}

intent "get single user — public, no auth"
route GET "/users/{id}" {
  needs db

  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "create user"
route POST "/users" {
  needs db, time, rng, auth, log

  let ctx: Struct = api_pipeline(request)?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != "Admin"

  create_user(request.body)
    | on success: respond 201 with .
    | on BadRequest: respond 400 with { error: .message }
}

intent "update user"
route PUT "/users/{id}" {
  needs db, auth, log, time

  let ctx: Struct = api_pipeline(request)?
  let target_id: String = request.params.id

  return Forbidden { reason: "Not allowed" } if ctx.caller.role != "Admin" and ctx.caller.id != target_id

  update_user(target_id, request.body)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

intent "deactivate user (soft delete)"
route DELETE "/users/{id}" {
  needs db, auth, log, time

  let ctx: Struct = api_pipeline(request)?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != "Admin"
  return BadRequest { message: "Cannot deactivate yourself" } if ctx.caller.id == request.params.id

  deactivate_user(request.params.id)
    | on success: respond 200 with { message: "User deactivated" }
    | on NotFound: respond 404 with { error: "User not found" }
}


// ============================================
// tests/user_service_test.pact
// ============================================

use services.user_service.*
use models.user.*

test "create_user assigns Viewer role and sets timestamp" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  let user: Struct = create_user({
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }) | expect success

  assert user.name == "Vitalii"
  assert user.role == "Viewer"
  assert user.active == true
  assert user.created_at == "2026-04-02T12:00:00Z"
}

test "create_user rejects duplicate email" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  let data: Struct = {
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }

  create_user(data) | expect success
  let result: Struct = create_user(data)

  assert result is BadRequest
}

test "find_user returns NotFound for missing ID" {
  using db = db.memory()

  let result: Struct = find_user("nonexistent-id")

  assert result is NotFound
}

test "deactivate_user sets active to false" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  let user: Struct = create_user({
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }) | expect success

  let deactivated: Struct = deactivate_user(user.id) | expect success

  assert deactivated.active == false
  assert deactivated.name == "Vitalii"
}

test "user_summary counts correctly" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(42)
  using db = db.memory()

  create_user({ name: "A", email: "a@test.com", age: 25 }) | expect success
  create_user({ name: "B", email: "b@test.com", age: 30 }) | expect success

  let summary: Struct = user_summary()

  assert summary.total == 2
  assert summary.active == 2
}


// ============================================
// app
// ============================================

app UserService { port: 8080 }

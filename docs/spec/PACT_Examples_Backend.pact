// ============================================
// models/user.pact
// ============================================

type Role = Admin | Editor | Viewer

type User {
  id: ID,
  name: String,
  email: String,
  age: Int,
  role: Role,
  active: Bool,
  created_at: DateTime,
  updated_at: Optional<DateTime>,
}

type NewUser {
  name: String | min 1 | max 100,
  email: String | format email,
  age: Int | min 0 | max 150,
}

type UserUpdate {
  name: Optional<String> | min 1 | max 100,
  email: Optional<String> | format email,
  age: Optional<Int> | min 0 | max 150,
}

type UserSummary {
  total: Int,
  active: Int,
  by_role: List<{ role: Role, count: Int }>,
}


// ============================================
// errors.pact
// ============================================

type AppError
  = NotFound { resource: String }
  | Forbidden { reason: String }
  | BadRequest { message: String }
  | DbError { details: String }


// ============================================
// services/user_service.pact
// ============================================

use models.user.User
use models.user.NewUser
use models.user.UserUpdate
use models.user.UserSummary
use errors.AppError

intent "знайти користувача за ID"
fn find_user(id: ID) -> User or NotFound needs { db } {
  db.query("users", where .id == id)
    | find first where .id == id
    | expect one or raise NotFound { resource: "User" }
}

intent "знайти користувача за email"
fn find_by_email(email: String) -> Optional<User> needs { db } {
  db.query("users", where .email == email)
    | find first where .email == email
}

intent "створити нового користувача з дефолтною роллю Viewer"
fn create_user(data: NewUser) -> User or BadRequest needs { db, time, rng } {
  let existing: Optional<User> = find_by_email(data.email)
  return BadRequest { message: "Email already taken" } if existing != nothing

  let user: User = User {
    id: rng.uuid(),
    name: data.name,
    email: data.email,
    age: data.age,
    role: Viewer,
    active: true,
    created_at: time.now(),
    updated_at: nothing,
  }

  db.insert("users", user)
}

intent "оновити дані існуючого користувача"
fn update_user(id: ID, updates: UserUpdate) -> User or NotFound or DbError needs { db, time } {
  let user: User = find_user(id)?

  let updated: User = User {
    ...user,
    name: updates.name | or default user.name,
    email: updates.email | or default user.email,
    age: updates.age | or default user.age,
    updated_at: time.now(),
  }

  db.update("users", where .id == id, with updated)
}

intent "деактивувати користувача замість видалення"
fn deactivate_user(id: ID) -> User or NotFound needs { db, time } {
  let user: User = find_user(id)?

  db.update("users", where .id == id, with {
    ...user,
    active: false,
    updated_at: time.now(),
  })
}

intent "отримати статистику по користувачах"
fn user_summary() -> UserSummary needs { db } {
  let users: List<User> = db.query("users")

  UserSummary {
    total: users | count,
    active: users | filter where .active | count,
    by_role: users
      | group by .role
      | map to { role: .key, count: .values | count }
      | sort by .count descending,
  }
}


// ============================================
// handlers/user_handlers.pact
// ============================================

use services.user_service.*
use models.user.*

// shared pipeline замість middleware — явний, видимий, тестуємий
fn api_pipeline(request: Request) -> { caller: User } or Unauthorized needs { auth, log, time } {
  log.info("{request.method} {request.path}")
  let caller: User = auth.require(request)?
  { caller }
}

route GET "/users" {
  intent "список активних користувачів з пагінацією"
  needs { db, auth, log, time }

  let ctx = request | api_pipeline?
  let page: Int = request.query.page | or default 1
  let limit: Int = request.query.limit | or default 20

  db.query("users")
    | filter where .active
    | sort by .created_at descending
    | skip (page - 1) * limit
    | take first limit
    | respond 200 with .
}

route GET "/users/summary" {
  intent "статистика по всіх користувачах"
  needs { db, auth, log, time }

  let ctx = request | api_pipeline?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != Admin

  let summary: UserSummary = user_summary()

  respond 200 with summary
}

route GET "/users/{id}" {
  intent "отримати одного користувача — публічний, без auth"
  needs { db }

  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

route POST "/users" {
  intent "створити користувача"
  needs { db, time, rng, auth, log }

  let ctx = request | api_pipeline?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != Admin

  let data: NewUser = request.body | validate as NewUser?

  create_user(data)
    | on success: respond 201 with .
    | on BadRequest: respond 400 with { error: .message }
}

route PUT "/users/{id}" {
  intent "оновити користувача"
  needs { db, auth, log, time }

  let ctx = request | api_pipeline?
  let target_id: ID = request.params.id

  return Forbidden { reason: "Not allowed" } if ctx.caller.role != Admin and ctx.caller.id != target_id

  let updates: UserUpdate = request.body | validate as UserUpdate?

  update_user(target_id, updates)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
    | on DbError: respond 500 with { error: "Update failed" }
}

route DELETE "/users/{id}" {
  intent "деактивувати користувача (soft delete)"
  needs { db, auth, log, time }

  let ctx = request | api_pipeline?
  return Forbidden { reason: "Admin only" } if ctx.caller.role != Admin
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
  using rng = rng.deterministic(seed: 42)
  using db = db.memory()

  let user: User = create_user(NewUser {
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }) | expect success

  assert user.name == "Vitalii"
  assert user.role == Viewer
  assert user.active == true
  assert user.created_at == "2026-04-02T12:00:00Z"
  assert user.updated_at == nothing
}

test "create_user rejects duplicate email" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(seed: 42)
  using db = db.memory()

  let data: NewUser = NewUser {
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }

  create_user(data) | expect success
  let result = create_user(data)

  assert result is BadRequest
  assert result.message == "Email already taken"
}

test "find_user returns NotFound for missing ID" {
  using db = db.memory()

  let result = find_user("nonexistent-id")

  assert result is NotFound
}

test "update_user changes only provided fields" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(seed: 42)
  using db = db.memory()

  let user: User = create_user(NewUser {
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }) | expect success

  using time = time.fixed("2026-04-02T13:00:00Z")

  let updated: User = update_user(user.id, UserUpdate {
    name: "Vitalii K",
    email: nothing,
    age: nothing,
  }) | expect success

  assert updated.name == "Vitalii K"
  assert updated.email == "v@example.com"     // не змінився
  assert updated.age == 30                     // не змінився
  assert updated.updated_at == "2026-04-02T13:00:00Z"
}

test "deactivate_user sets active to false" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(seed: 42)
  using db = db.memory()

  let user: User = create_user(NewUser {
    name: "Vitalii",
    email: "v@example.com",
    age: 30,
  }) | expect success

  let deactivated: User = deactivate_user(user.id) | expect success

  assert deactivated.active == false
  assert deactivated.name == "Vitalii"    // інші поля не змінились
}

test "user_summary counts correctly" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.sequence(["id-1", "id-2", "id-3"])
  using db = db.memory()

  create_user(NewUser { name: "A", email: "a@test.com", age: 25 }) | expect success
  create_user(NewUser { name: "B", email: "b@test.com", age: 30 }) | expect success
  create_user(NewUser { name: "C", email: "c@test.com", age: 35 }) | expect success

  deactivate_user("id-3") | expect success

  let summary: UserSummary = user_summary()

  assert summary.total == 3
  assert summary.active == 2
}

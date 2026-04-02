# PACT Language Specification v0.1

## 1. Базові принципи

- Кожен statement — окремий рядок, без `;`
- Блоки — фігурні дужки `{ }`
- Trailing commas дозволені всюди
- Типи обов'язкові завжди
- Останній вираз у блоці — return value
- `return` — тільки для раннього виходу
- Коментарі: `//` однорядковий

---

## 2. Типи

### Примітиви

```pact
Int
Float
String
Bool
ID
```

### Складні типи

```pact
List<T>
Map<K, V>
Optional<T>       // значення або nothing
T or ErrorType    // result type
```

### Оголошення типів

```pact
type User {
  id: ID,
  name: String,
  email: String,
  age: Int,
  role: Admin | Editor | Viewer,    // inline union
  active: Bool,
}
```

### Union types

```pact
type Role = Admin | Editor | Viewer

type ApiError = NotFound | Forbidden | BadRequest { message: String }
```

---

## 3. Змінні

Завжди з типом. Immutable за замовчуванням.

```pact
let name: String = "Vitalii"
let count: Int = 42
var counter: Int = 0              // mutable — явне ключове слово
```

---

## 4. Функції

```pact
fn add(a: Int, b: Int) -> Int {
  a + b
}
```

### Intent блоки

```pact
intent "перевірити права доступу користувача до ресурсу"
fn can_access(user: User, resource: Resource) -> Bool {
  ensure user.active
  ensure resource.visibility != Hidden

  match user.role {
    Admin => true,
    Editor => resource.owner == user.id,
    Viewer => resource.public,
  }
}
```

### Ensure (контракти)

```pact
fn withdraw(account: Account, amount: Money) -> Account or InsufficientFunds {
  ensure amount > 0
  ensure account.balance >= amount

  return InsufficientFunds if account.balance < amount

  Account { ...account, balance: account.balance - amount }
}
```

### Effect markers

```pact
fn create_user(data: NewUser) -> User needs db, time, rng {
  let now: DateTime = time.now()
  let id: ID = rng.uuid()

  db.insert("users", User {
    id: id,
    name: data.name,
    email: data.email,
    age: data.age,
    role: Viewer,
    active: true,
    created_at: now,
  })
}
```

---

## 5. Pipeline operator `|`

Головний синтаксичний елемент мови. Дані течуть зліва направо.

```pact
fn active_admins(users: List<User>) -> List<String> {
  users
    | filter where .active
    | filter where .role == Admin
    | sort by .name
    | map to .name
}
```

### Pipeline операції

```pact
| filter where <predicate>
| map to <expression>
| sort by <field> ascending/descending
| group by <field>
| take first <n>
| take last <n>
| each <fn>                        // side effect, не змінює дані
| count
| sum
| flatten
| unique
| find first where <predicate>     // -> Optional<T>
| expect one or raise <Error>      // -> T or Error
```

### Pipeline з кількома кроками

```pact
fn order_summary(user_id: ID) -> Summary or NotFound needs db {
  db.query("orders", where .user_id == user_id)
    | expect any or raise NotFound
    | group by .status
    | map to { status: .key, total: .values | map to .amount | sum }
    | sort by .total descending
}
```

---

## 6. Control flow

### If / else

```pact
if age >= 18 {
  "adult"
} else {
  "minor"
}
```

### Ранній вихід

```pact
return NotFound if not user.active
return Forbidden if user.role == Viewer
```

### Match (exhaustive)

```pact
match request.method {
  GET => handle_get(request),
  POST => handle_post(request),
  PUT => handle_put(request),
  DELETE => handle_delete(request),
  _ => MethodNotAllowed,
}
```

---

## 7. Error handling

Помилки — це типи, не exceptions.

### Визначення

```pact
type DbError = ConnectionFailed | QueryFailed { query: String }
type ApiError = NotFound | Forbidden | BadRequest { message: String }
```

### Повернення

```pact
fn find_user(id: ID) -> User or NotFound needs db {
  db.query("users", where .id == id)
    | find first where .id == id
    | expect one or raise NotFound
}
```

### Обробка

```pact
find_user(request.params.id)
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "User not found" }
  | on DbError: respond 500 with { error: "Internal error" }
```

### Propagation

```pact
// ? прокидає помилку вгору, як в Rust
fn get_user_orders(user_id: ID) -> List<Order> or NotFound or DbError needs db {
  let user: User = find_user(user_id)?
  db.query("orders", where .user_id == user.id)?
}
```

---

## 8. Строки

```pact
let simple: String = "hello world"
let interpolated: String = "Hello {user.name}, you have {count} items"
let escaped_braces: String = "JSON: {{key: value}}"
let raw: String = raw"no {interpolation} here, raw braces: {}"
let multiline: String = """
  This is a
  multiline string
  with {interpolation}
"""
```

---

## 9. Imports

Один символ — один шлях — один файл. Ніяких re-exports.

```pact
use models.user.User
use models.order.Order
use utils.validate.email
use handlers.users.create_user
```

Шлях до символу == шлях до файлу:
`use models.user.User` → файл `models/user.pact`, тип `User`

---

## 10. Effects та тестування

### Effect markers

```pact
fn now() -> DateTime needs time {
  time.now()
}

fn generate_id() -> ID needs rng {
  rng.uuid()
}

fn save(user: User) -> User needs db {
  db.insert("users", user)
}

fn send_email(to: String, body: String) -> Bool needs email {
  email.send(to, body)
}
```

### Тести з mock effects

```pact
test "create user sets correct timestamp" {
  using time = time.fixed("2026-04-02T12:00:00Z")
  using rng = rng.deterministic(seed: 42)
  using db = db.memory()

  let user: User = create_user(NewUser {
    name: "Vitalii",
    email: "v@test.com",
    age: 30,
  })

  assert user.created_at == "2026-04-02T12:00:00Z"
  assert user.role == Viewer
  assert user.active == true
}
```

---

## 11. HTTP / Backend

### Routes

```pact
route GET "/users" {
  intent "отримати список активних користувачів"
  needs db, auth

  let caller: User = auth.require(request)?
  return Forbidden if caller.role == Viewer

  let users: List<User> = db.query("users")
    | filter where .active
    | sort by .name

  respond 200 with users
}

route GET "/users/{id}" {
  intent "отримати користувача за ID"
  needs db

  find_user(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "User not found" }
}

route POST "/users" {
  intent "створити нового користувача"
  needs db, time, rng, auth

  let caller: User = auth.require(request)?
  return Forbidden if caller.role != Admin

  let data: NewUser = request.body | validate as NewUser?

  create_user(data)
    | on success: respond 201 with .
    | on ValidationError: respond 400 with { error: .message }
}

route PUT "/users/{id}" {
  intent "оновити дані користувача"
  needs db, auth, time

  let caller: User = auth.require(request)?
  let target: User = find_user(request.params.id)?

  return Forbidden if caller.role != Admin and caller.id != target.id

  let updates: UserUpdate = request.body | validate as UserUpdate?

  db.update("users", where .id == target.id, with {
    ...target,
    ...updates,
    updated_at: time.now(),
  })
    | on success: respond 200 with .
    | on DbError: respond 500 with { error: "Update failed" }
}

route DELETE "/users/{id}" {
  intent "деактивувати користувача"
  needs db, auth, time

  let caller: User = auth.require(request)?
  return Forbidden if caller.role != Admin

  let target: User = find_user(request.params.id)?

  db.update("users", where .id == target.id, with {
    ...target,
    active: false,
    deactivated_at: time.now(),
  })
    | on success: respond 200 with { message: "User deactivated" }
    | on DbError: respond 500 with { error: "Deactivation failed" }
}
```

### Validation

```pact
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
```

### Shared Pipelines (замість middleware)

PACT не має middleware. Middleware — implicit: ти дивишся на route і не бачиш
хто модифікував request до тебе. Замість цього — shared pipeline функції,
які route викликає явно. Кожен крок видимий, порядок очевидний.

```pact
// shared pipeline — звичайна функція, нічого магічного
fn api_pipeline(request: Request) -> { caller: User } or Unauthorized needs auth, log, time {
  let start: DateTime = time.now()
  log.info("{request.method} {request.path}")

  let caller: User = auth.require(request)?

  { caller }
}

// route з auth — явно викликає api_pipeline
route GET "/users" {
  intent "список користувачів"
  needs db, auth, log, time

  let ctx = request | api_pipeline?

  db.query("users")
    | filter where .active
    | respond 200 with .
}

// route без auth — видно одразу, бо api_pipeline відсутній
route GET "/health" {
  intent "health check"
  

  respond 200 with { status: "ok" }
}
```

### App

```pact
app UserService {
  port: 8080,

  routes: [
    users_routes,
  ],
}
```

---

## Вбудовані effects

| Effect | Опис |
|--------|------|
| `db` | Доступ до бази даних |
| `time` | Поточний час |
| `rng` | Генерація випадкових чисел |
| `log` | Логування |
| `auth` | Автентифікація/авторизація |
| `email` | Відправка пошти |
| `http` | HTTP клієнт |
| `io` | Файлова система |
| `env` | Змінні оточення |

---

## Конвенції

- Файли: `snake_case.pact`
- Типи: `PascalCase`
- Функції, змінні: `snake_case`
- Константи: `UPPER_SNAKE_CASE`
- Один тип на файл (рекомендовано)
- Тести поруч з кодом, в тому ж файлі
- Максимум 3 рівні вкладеності
- Pipeline замість вкладених викликів де можливо

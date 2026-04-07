// Deep type checker demo — every line with a comment has an intentional bug

type User {
  id: String,
  name: String,
  age: Int,
  active: Bool,
}

intent "demo struct literal errors"
fn make_bad_user() -> User
  needs rng
{
  User {
    id: rng.uuid(),
    name: 42,            // ERROR: field 'name' expects String, got Int
    age: "thirty",       // ERROR: field 'age' expects Int, got String
    email: "a@b.com",   // ERROR: unknown field 'email' on User
  }
}

intent "demo field access errors"
fn bad_access() -> Int {
  let u: User = User { id: "1", name: "Alice", age: 30, active: true }
  u.email                // ERROR: Type 'User' has no field 'email'
}

intent "demo operator errors"
fn bad_ops() -> String {
  let x: String = "hello" + 5     // ERROR: Cannot apply 'Add' to String and Int
  let y: Bool = not 42            // ERROR: Cannot apply 'not' to Int
  x
}

intent "demo effect without needs"
fn forgot_needs() -> String {
  time.now()             // WARNING: Effect 'time' used without 'needs time'
}

intent "demo method on wrong type"
fn bad_method() -> Int {
  let x: Int = 42
  x.length()             // ERROR: Int has no methods
}

intent "demo unknown method"
fn unknown_method() -> String {
  let s: String = "hello"
  s.banana()             // ERROR: String has no method 'banana'
}

app CheckerDemo { port: 9999 }

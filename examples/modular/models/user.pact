type User {
  id: String,
  name: String,
  email: String,
  active: Bool
}

intent "find user by id"
fn find_user(id: String) -> User or NotFound
  needs db
{
  db.find("users", { id: id })
}

intent "create new user"
fn create_user(name: String, email: String) -> User
  needs db, rng
{
  let user: User = {
    id: rng.hex(8),
    name: name,
    email: email,
    active: true
  }
  db.insert("users", user)
}

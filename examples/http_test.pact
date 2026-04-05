test "mock http get" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: list({ name: "Alice", active: true }) }
  })

  let res: Struct = http.get("https://api.example.com/users")
  assert res.status == 200
  assert res.body.length() == 1
}

test "http get with pipeline" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 200, body: list(
        { name: "Alice", active: true },
        { name: "Bob", active: false }
      )}
  })

  let names: List = http.get("https://api.example.com/users")
    | .body
    | filter where .active
    | map to .name

  assert names.length() == 1
  assert names.first() == "Alice"
}

test "http error handling" {
  using http = http.mock({})

  let result: String = http.get("https://unknown.url")
    | on HttpError: "caught"

  assert result == "caught"
}

test "http post with body" {
  using http = http.mock({
    "https://api.example.com/users":
      { status: 201, body: { id: 1, name: "Alice" } }
  })

  let res: Struct = http.post("https://api.example.com/users", {
    body: { name: "Alice" }
  })
  assert res.status == 201
  assert res.body.name == "Alice"
}

test "http multiple urls" {
  using http = http.mock({
    "https://api.example.com/users": { status: 200, body: "users" },
    "https://api.example.com/roles": { status: 200, body: "roles" }
  })

  let u: Struct = http.get("https://api.example.com/users")
  let r: Struct = http.get("https://api.example.com/roles")
  assert u.body == "users"
  assert r.body == "roles"
}

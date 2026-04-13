// Generic type checker demo — every line with a comment has an intentional bug

type User {
  id: String,
  name: String,
  age: Int,
}

intent "demo: wrong element type in list"
fn bad_list() -> List<Int> {
  list(1, "two", 3)
}

intent "demo: list type mismatch"
fn wrong_list() -> List<String> {
  list(1, 2, 3)
}

intent "demo: split returns List<String>, not List<Int>"
fn bad_split() -> List<Int> {
  "hello,world".split(",")
}

intent "demo: first() returns element type"
fn bad_first() -> String {
  list(1, 2, 3).first()
}

intent "demo: correct generics — no errors"
fn good_list() -> List<Int> {
  list(1, 2, 3)
}

intent "demo: correct split"
fn good_split() -> List<String> {
  "a,b,c".split(",")
}

intent "demo: correct first"
fn good_first() -> Int {
  list(1, 2, 3).first()
}

intent "demo: reverse preserves type"
fn good_reverse() -> List<Int> {
  list(3, 2, 1).reverse()
}

app GenericsDemo { port: 9999 }

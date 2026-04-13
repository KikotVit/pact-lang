// Test generics with let bindings

intent "test let bindings"
fn test_lets() -> Int {
  let nums: List<Int> = list(1, "two", 3)
  let names: List<String> = list(1, 2, 3)
  let parts: List<Int> = "a,b".split(",")
  let first: String = list(1, 2, 3).first()
  let good: List<Int> = list(1, 2, 3)
  0
}

app Demo { port: 9999 }

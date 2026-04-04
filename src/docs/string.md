# Strings

PACT strings use double quotes. Interpolation uses `{expr}` inside the string.

## Simple strings

```pact
let greeting: String = "hello world"
```

## String interpolation

```pact
let name: String = "Alice"
let msg: String = "Hello {name}"
```

```pact
let user: Struct = { name: "Bob", age: 30 }
let info: String = "Name: {user.name}"
```

## String methods

```pact
let s: String = "Hello World"
s.length()
s.contains("World")
s.to_upper()
s.to_lower()
s.trim()
s.split(" ")
s.replace("World", "PACT")
```

## Example

```pact
test "string operations" {
  let s: String = "  hello  "
  assert s.trim() == "hello"
  assert "abc".length() == 3
  assert "hello".contains("ell") == true
  assert "hello".to_upper() == "HELLO"
}
```

> See also: list, type, pipeline

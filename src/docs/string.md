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
s.starts_with("Hello")
s.ends_with("World")
s.chars()
s.code()
```

## Character operations

`chars()` splits a string into a list of single characters. `code()` returns the Unicode code point of the first character:

```pact
"abc".chars()        // list("a", "b", "c")
"a".code()           // 97
```

Use `| chars` as a pipeline step:

```pact
"hello" | chars | map to _it.code() | sum   // sum of char codes
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

# Lists

Create lists with `list()` and use pipeline operations to transform them.

## Creating lists

```pact
let numbers: List = list(1, 2, 3)
let names: List = list("Alice", "Bob", "Charlie")
let empty: List = list()
```

## List methods

```pact
let items: List = list(1, 2, 3)
items.length()
items.contains(2)
items.first()
items.last()
items.get(0)
items.is_empty()
items.push(4)
items.join(", ")
```

## Pipeline operations on lists

```pact
list(1, 2, 3, 4, 5)
  | filter where . > 2
  | map to . * 10
  | sort by . descending
```

## Example

```pact
test "list operations" {
  let nums: List = list(3, 1, 2)
  assert nums.length() == 3
  assert nums.contains(2) == true
  assert nums.first() == 3
  let total: Int = list(1, 2, 3) | sum
  assert total == 6
}
```

> See also: pipeline, string, fn

# Pipelines

Chain operations on lists and values using `|`. Each step transforms or filters the data flowing through.

## Syntax

```pact
source | step1 | step2 | step3
```

## Filtering

```pact
list(1, 2, 3, 4, 5) | filter where . > 3
```

```pact
db.query("users") | filter where .active == true
```

```pact
db.query("users") | find first where .id == id
```

## Transforming

```pact
list(1, 2, 3) | map to . * 2
```

```pact
list(3, 1, 2) | sort by .
```

```pact
list(3, 1, 2) | sort by . descending
```

```pact
list(list(1, 2), list(3, 4)) | flatten
```

```pact
list(1, 2, 2, 3) | unique
```

## Aggregating

```pact
list(1, 2, 3) | count
```

```pact
list(10, 20, 30) | sum
```

## Slicing

```pact
list(1, 2, 3, 4, 5) | take first 3
```

```pact
list(1, 2, 3, 4, 5) | take last 2
```

```pact
list(1, 2, 3, 4, 5) | skip 2
```

## Grouping

```pact
db.query("users") | group by .role
```

## Error handling in pipelines

```pact
db.query("users")
  | find first where .id == id
  | expect one or raise NotFound
```

```pact
db.query("items")
  | expect any or raise Empty
```

```pact
find_user(id)
  | on success: respond 200 with .
  | on NotFound: respond 404 with { error: "Not found" }
```

```pact
list() | count | or default 0
```

## Utility

```pact
list(1, 2, 3) | each { print(.) }
```

## Validation (parsed, not enforced yet)

```pact
request.body | validate as User
```

## Expression as pipeline step

Any expression can be a pipeline step. The `.` refers to the current value:

```pact
db.insert("users", request.body) | respond 201 with .
```

## Chaining multiple steps

```pact
db.query("users")
  | filter where .active == true
  | sort by .name
  | take first 10
```

> See also: fn, route, error, db

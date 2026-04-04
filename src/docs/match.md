# Match

Match expressions branch on a value. Each arm maps a pattern to a result.

## Syntax

```pact
match expr {
  Pattern1 => result1
  Pattern2 => result2
  _ => default
}
```

## Matching union variants

```pact
type Status = Active | Inactive | Banned

let msg: String = match status {
  Active => "welcome"
  Inactive => "please reactivate"
  Banned => "access denied"
  _ => "unknown"
}
```

## Matching literals

```pact
let label: String = match code {
  200 => "ok"
  404 => "not found"
  500 => "server error"
  _ => "other"
}
```

## Wildcard

Use `_` as a catch-all pattern:

```pact
match role {
  Admin => "full access"
  _ => "limited access"
}
```

> See also: type, error, fn

# Schedule

Run background tasks on a recurring interval. Like `route` and `fn`, every schedule requires an `intent` declaration.

## Syntax

```
intent "description"
schedule every <duration> {
  needs effect1, effect2
  // body
}
```

## Duration units

| Unit | Example | Meaning |
|------|---------|---------|
| `ms` | `500ms` | milliseconds |
| `s` | `30s` | seconds |
| `m` | `5m` | minutes |
| `h` | `24h` | hours |
| `d` | `1d` | days |

## Example: daily cleanup

```pact
intent "delete old records"
schedule every 1d {
  needs db, time
  let cutoff: String = time.days_ago(7)
  db.delete_where("pastes", { before: cutoff })
}
```

## Behavior

- First execution runs immediately when the app starts
- Subsequent executions run after each interval
- Runs in a background thread — does not block HTTP routes
- If the body errors, the error is logged and the schedule continues
- Each execution gets a fresh interpreter with its own DB connection

## Effects

Schedules support `needs` just like routes:

```pact
intent "refresh cache"
schedule every 5m {
  needs db, http
  let data: Struct = http.get("https://api.example.com/data")
  db.insert("cache", data)
}
```

> See also: route, effects, db, app

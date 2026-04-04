# App

Declare your application with `app`. This starts the HTTP server and binds routes.

## Syntax

```pact
app MyApp { port: 8080 }
```

## With SQLite database

```pact
app MyApp { port: 8080, db: "sqlite://data.db" }
```

## Full example

```pact
intent "health check"
route GET "/health" {
  respond 200 with { status: "ok" }
}

app API { port: 3000 }
```

On startup, PACT binds all declared routes to the specified port. If a `db` URL is provided, it connects to SQLite with WAL mode enabled. Without a `db` URL, an in-memory database is used.

> See also: route, db, effects

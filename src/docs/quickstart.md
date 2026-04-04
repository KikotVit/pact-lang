# Quickstart

Build a simple notes API in under 20 lines.

```pact
intent "list all notes"
route GET "/notes" {
  needs db
  db.query("notes") | respond 200 with .
}

intent "create a note"
route POST "/notes" {
  needs db
  db.insert("notes", request.body)
    | on success: respond 201 with .
}

intent "get note by id"
route GET "/notes/{id}" {
  needs db
  db.find("notes", { id: request.params.id })
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "Not found" }
}

app Notes {
  port: 8080,
  db: "sqlite:///data/notes.db",
}
```

Save as `notes.pact` and run:

```
pact run notes.pact
```

Your API is live at `http://localhost:8080`.

> See also: route, fn, db, app

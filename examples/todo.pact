// Todo List API

type Todo {
  id: String,
  title: String,
  done: Bool
}

intent "find a todo by id"
fn find_todo(id: String) -> Struct or NotFound
  needs db
{
  db.query("todos")
    | filter where .id == id
    | expect one or raise NotFound
}

route GET "/health" {
  intent "health check"
  respond 200 with { status: "ok" }
}

route GET "/todos" {
  intent "list all todos"
  needs db
  let todos: List = db.query("todos")
  respond 200 with todos
}

route GET "/todos/{id}" {
  intent "get todo by id"
  needs db
  find_todo(request.params.id)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "Not found" }
}

route POST "/todos" {
  intent "create a new todo"
  needs db, rng
  let todo: Struct = {
    id: rng.hex(8),
    title: request.body.title,
    done: false
  }
  db.insert("todos", todo)
  respond 201 with todo
}

route POST "/todos/{id}/complete" {
  intent "mark todo as done"
  needs db
  find_todo(request.params.id)
    | on success: {
        let found: Struct = .
        let updated: Struct = {
          id: found.id,
          title: found.title,
          done: true
        }
        db.insert("todos", updated)
        respond 200 with updated
      }
    | on NotFound: respond 404 with { error: "Not found" }
}

app TodoApp { port: 8086 }

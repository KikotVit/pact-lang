use handlers.repos.*

intent "health check"
route GET "/health" {
  respond 200 with { status: "ok" }
}

app RepoTracker { port: 8092, db: "sqlite://repos.db" }

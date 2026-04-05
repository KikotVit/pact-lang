use handlers.users.*

app UserService {
  port: 8090,
  db: "sqlite://users.db"
}

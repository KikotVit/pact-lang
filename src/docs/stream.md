# Stream Routes (SSE)

Stream routes provide real-time Server-Sent Events (SSE) streaming. Clients receive new data automatically as it's inserted into the database.

## Syntax

```pact
intent "description"
stream METHOD "/path" {
  needs effect1, effect2
  send db.watch("table_name", optional_filter)
}
```

## How it works

1. The stream body executes **once** per connection (for auth, setup)
2. `db.watch(table, filter)` returns a stream descriptor
3. `send` tells the server to start streaming
4. The server polls the database every 500ms and sends new rows as SSE events
5. Connection stays open until the client disconnects

## Example: Real-time messages

```pact
intent "stream new messages"
stream GET "/rooms/{room_id}/live" {
  needs db, auth
  auth.require(request)

  send db.watch("messages", { room_id: request.params.room_id })
}
```

## Client usage

**curl:**
```
curl -N -H "Authorization: Bearer token" http://localhost:8080/rooms/abc/live
```

**Browser (EventSource):**
```
const es = new EventSource('/rooms/abc/live');
es.onmessage = (e) => console.log(JSON.parse(e.data));
```

## SSE event format

Each new database row is sent as:
```
id: 42
data: {"id":"msg-1","text":"hello","room_id":"abc","created_at":"2026-04-06T12:00:00Z"}
```

The `id` field is the SQLite rowid. On reconnection, the browser sends `Last-Event-ID` and the server resumes from that point.

## db.watch()

`db.watch(table)` — watch all rows in a table
`db.watch(table, filter)` — watch rows matching a filter struct

Returns a `DbWatch` value that `send` passes to the server.

## Notes

- Stream routes use `stream` instead of `route`
- `send` is only meaningful inside stream blocks
- SSE requires a `db:` config in the app declaration (SQLite only)
- Each SSE connection gets its own database reader
- `intent` is required before `stream`, same as `route`

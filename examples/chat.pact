type Message {
  id: String,
  room_id: String,
  sender: String,
  text: String,
  created_at: String,
}

type Room {
  id: String,
  name: String,
  created_by: String,
}

type Member {
  room_id: String,
  user_id: String,
  role: String,
}

type RefreshToken {
  id: String,
  user_id: String,
  created_at: String,
}

// ── Auth: login, refresh, logout ───────────────────────────────────

intent "login and get access + refresh tokens"
route POST "/login" {
  needs db, auth, rng, time
  let access: String = auth.sign({ id: "user-1", name: "Alice", role: "member", kind: "access", exp: 900 })
  let refresh_id: String = rng.uuid()
  let refresh: String = auth.sign({ id: "user-1", role: "member", kind: "refresh", family: refresh_id, exp: 604800 })

  db.insert("refresh_tokens", {
    id: refresh_id,
    user_id: "user-1",
    created_at: time.now(),
  })

  respond 200 with { access: access, refresh: refresh }
}

intent "refresh access token with rotation"
route POST "/refresh" {
  needs db, auth, rng, time
  let claims: Struct = auth.verify(request.body.refresh_token)
    | on Unauthorized: respond 401 with { error: "Invalid refresh token" }

  return respond 401 with { error: "Not a refresh token" }
    if claims.kind != "refresh"

  let stored: RefreshToken = db.find("refresh_tokens", { id: claims.family })
    | on NotFound: respond 401 with { error: "Token revoked" }

  db.delete("refresh_tokens", stored.id)

  let new_family: String = rng.uuid()
  let access: String = auth.sign({ id: claims.id, role: claims.role, kind: "access", exp: 900 })
  let refresh: String = auth.sign({ id: claims.id, role: claims.role, kind: "refresh", family: new_family, exp: 604800 })

  db.insert("refresh_tokens", {
    id: new_family,
    user_id: claims.id,
    created_at: time.now(),
  })

  respond 200 with { access: access, refresh: refresh }
}

intent "logout and revoke refresh token"
route POST "/logout" {
  needs db, auth
  let claims: Struct = auth.verify(request.body.refresh_token)
    | on Unauthorized: respond 401 with { error: "Invalid token" }

  db.delete("refresh_tokens", claims.family)
  respond 200 with { message: "Logged out" }
}

// ── Rooms ──────────────────────────────────────────────────────────

intent "create a new chat room"
route POST "/rooms" {
  needs db, rng, time, auth
  let user: User = auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  let room: Room = {
    id: rng.uuid(),
    name: request.body.name,
    created_by: user.id,
  }
  db.insert("rooms", room)
  db.insert("members", { room_id: room.id, user_id: user.id, role: "owner" })
  respond 201 with room
}

intent "list all rooms"
route GET "/rooms" {
  needs db, auth
  auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  db.query("rooms") | respond 200 with .
}

// ── Messages ───────────────────────────────────────────────────────

intent "send a message to a room"
route POST "/rooms/{room_id}/messages" {
  needs db, rng, time, auth
  let user: User = auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  db.find("members", { room_id: request.params.room_id, user_id: user.id })
    | on NotFound: respond 403 with { error: "Not a member of this room" }

  let msg: Message = {
    id: rng.uuid(),
    room_id: request.params.room_id,
    sender: user.id,
    text: request.body.text,
    created_at: time.now(),
  }
  db.insert("messages", msg) | on success: respond 201 with .
}

intent "get messages from a room"
route GET "/rooms/{room_id}/messages" {
  needs db, auth
  let user: User = auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  db.find("members", { room_id: request.params.room_id, user_id: user.id })
    | on NotFound: respond 403 with { error: "Not a member of this room" }

  db.query("messages", { room_id: request.params.room_id })
    | sort by .created_at descending
    | take first 50
    | respond 200 with .
}

intent "delete own message, or admin/owner can delete any"
route DELETE "/rooms/{room_id}/messages/{id}" {
  needs db, auth
  let user: User = auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }
  let member: Member = db.find("members", { room_id: request.params.room_id, user_id: user.id })
    | on NotFound: respond 403 with { error: "Not a member of this room" }
  let msg: Message = db.find("messages", { id: request.params.id })
    | on NotFound: respond 404 with { error: "Message not found" }

  return respond 403 with { error: "Cannot delete another user's message" }
    if msg.sender != user.id and member.role != "admin" and member.role != "owner"

  db.delete("messages", msg.id) | on success: respond 200 with { deleted: true }
}

// ── SSE streaming ──────────────────────────────────────────────────

intent "stream new messages in real-time via SSE"
stream GET "/rooms/{room_id}/live" {
  needs db, auth
  auth.require(request)
    | on Unauthorized: respond 401 with { error: "Not authenticated" }

  send db.watch("messages", { room_id: request.params.room_id })
}

app Chat { port: 8080, db: "sqlite://chat.db" }

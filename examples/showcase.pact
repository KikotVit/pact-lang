// ─────────────────────────────────────────────────────────────────────────────
// showcase.pact — self-documenting API
// Exercises: schedule, respond as, rng.short_id, db.delete_where,
//            time.days_ago, str.chars(), str.code(), | chars, | count, | sum
// ─────────────────────────────────────────────────────────────────────────────

// ── Types ─────────────────────────────────────────────────────────────────────

type Paste {
  id: String,
  content: String,
  created_at: String,
}

type Link {
  code: String,
  url: String,
  created_at: String,
}

// ── 1. Health check ───────────────────────────────────────────────────────────

intent "health check — returns status and version"
route GET "/health" {
  respond 200 with { status: "ok", version: "0.5.0" }
}

// ── 2. Paste bin ──────────────────────────────────────────────────────────────

intent "create a new paste"
route POST "/paste" {
  needs db, rng, time

  return respond 400 with { error: "content is required" }
    if request.body.content == ""

  let paste: Paste = {
    id: rng.uuid(),
    content: request.body.content,
    created_at: time.now(),
  }
  db.insert("pastes", paste)
  respond 201 with paste
}

intent "retrieve a paste by id and return plain text"
route GET "/paste/{id}" {
  needs db

  let found: Paste = db.find("pastes", { id: request.params.id })
    | on NotFound: respond 404 with { error: "Paste not found" }

  respond 200 with found.content as "text/plain"
}

// ── 3. URL shortener ──────────────────────────────────────────────────────────

intent "shorten a URL and return the short code"
route POST "/shorten" {
  needs db, rng, time

  return respond 400 with { error: "url is required" }
    if request.body.url == ""

  let link: Link = {
    code: rng.short_id(),
    url: request.body.url,
    created_at: time.now(),
  }
  db.insert("links", link)
  respond 201 with { code: link.code, short_url: "/s/" + link.code }
}

intent "redirect to the original URL for a short code"
route GET "/s/{code}" {
  needs db

  let link: Link = db.find("links", { code: request.params.code })
    | on NotFound: respond 404 with { error: "Short link not found" }

  respond 302 with { location: link.url }
}

// ── 4. SVG Avatar ─────────────────────────────────────────────────────────────

intent "generate a deterministic SVG identicon from a name"
route GET "/avatar/{name}" {
  needs db

  let name: String = request.params.name

  // Compute seed = sum of char code points
  let seed: Int = name | chars | map to _it.code() | sum

  // Pick foreground color from a small palette using seed value
  // Use bits of seed to pick one of 5 colors
  // seed bit 0 (odd/even) and bit 1, combined: 0-3 -> 4 colors, else 5th
  let c0: Bool = seed > 400
  let c1: Bool = seed > 800
  let c2: Bool = seed > 1200
  let c3: Bool = seed > 1600

  let fg: String = if c3 { "#86efac" } else {
    if c2 { "#818cf8" } else {
      if c1 { "#fbbf24" } else {
        if c0 { "#f87171" } else { "#2dd4bf" }
      }
    }
  }

  let bg: String = "#1e293b"

  // Use bits of seed to determine which cells are filled (5x5 mirrored grid)
  // Columns 0,1,2 are generated; 3 mirrors 1, 4 mirrors 0
  // 15 unique cells: rows 0-4, cols 0-2
  // Use different offsets of seed to get varied patterns

  let r0c0: Bool = if seed + 7 > 250 { true } else { false }
  let r0c1: Bool = if seed + 13 > 300 { true } else { false }
  let r0c2: Bool = if seed + 3 > 270 { true } else { false }
  let r1c0: Bool = if seed + 17 > 280 { true } else { false }
  let r1c1: Bool = if seed + 11 > 240 { true } else { false }
  let r1c2: Bool = if seed + 5 > 260 { true } else { false }
  let r2c0: Bool = if seed + 19 > 310 { true } else { false }
  let r2c1: Bool = if seed + 23 > 290 { true } else { false }
  let r2c2: Bool = if seed + 2 > 230 { true } else { false }
  let r3c0: Bool = if seed + 29 > 320 { true } else { false }
  let r3c1: Bool = if seed + 37 > 270 { true } else { false }
  let r3c2: Bool = if seed + 41 > 295 { true } else { false }
  let r4c0: Bool = if seed + 43 > 260 { true } else { false }
  let r4c1: Bool = if seed + 47 > 285 { true } else { false }
  let r4c2: Bool = if seed + 53 > 275 { true } else { false }

  // Build rect elements for filled cells (each cell = 20x20px, gap 5px)

  let svg_r0c0: String = if r0c0 { "<rect x='5' y='5' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r0c1: String = if r0c1 { "<rect x='30' y='5' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r0c2: String = if r0c2 { "<rect x='55' y='5' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r0c3: String = if r0c1 { "<rect x='80' y='5' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r0c4: String = if r0c0 { "<rect x='105' y='5' width='20' height='20' fill='" + fg + "'/>" } else { "" }

  let svg_r1c0: String = if r1c0 { "<rect x='5' y='30' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r1c1: String = if r1c1 { "<rect x='30' y='30' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r1c2: String = if r1c2 { "<rect x='55' y='30' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r1c3: String = if r1c1 { "<rect x='80' y='30' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r1c4: String = if r1c0 { "<rect x='105' y='30' width='20' height='20' fill='" + fg + "'/>" } else { "" }

  let svg_r2c0: String = if r2c0 { "<rect x='5' y='55' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r2c1: String = if r2c1 { "<rect x='30' y='55' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r2c2: String = if r2c2 { "<rect x='55' y='55' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r2c3: String = if r2c1 { "<rect x='80' y='55' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r2c4: String = if r2c0 { "<rect x='105' y='55' width='20' height='20' fill='" + fg + "'/>" } else { "" }

  let svg_r3c0: String = if r3c0 { "<rect x='5' y='80' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r3c1: String = if r3c1 { "<rect x='30' y='80' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r3c2: String = if r3c2 { "<rect x='55' y='80' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r3c3: String = if r3c1 { "<rect x='80' y='80' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r3c4: String = if r3c0 { "<rect x='105' y='80' width='20' height='20' fill='" + fg + "'/>" } else { "" }

  let svg_r4c0: String = if r4c0 { "<rect x='5' y='105' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r4c1: String = if r4c1 { "<rect x='30' y='105' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r4c2: String = if r4c2 { "<rect x='55' y='105' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r4c3: String = if r4c1 { "<rect x='80' y='105' width='20' height='20' fill='" + fg + "'/>" } else { "" }
  let svg_r4c4: String = if r4c0 { "<rect x='105' y='105' width='20' height='20' fill='" + fg + "'/>" } else { "" }

  let svg: String = "<svg xmlns='http://www.w3.org/2000/svg' width='130' height='130'>"
    + "<rect width='130' height='130' fill='" + bg + "'/>"
    + svg_r0c0 + svg_r0c1 + svg_r0c2 + svg_r0c3 + svg_r0c4
    + svg_r1c0 + svg_r1c1 + svg_r1c2 + svg_r1c3 + svg_r1c4
    + svg_r2c0 + svg_r2c1 + svg_r2c2 + svg_r2c3 + svg_r2c4
    + svg_r3c0 + svg_r3c1 + svg_r3c2 + svg_r3c3 + svg_r3c4
    + svg_r4c0 + svg_r4c1 + svg_r4c2 + svg_r4c3 + svg_r4c4
    + "</svg>"

  respond 200 with svg as "image/svg+xml"
}

// ── 5. Statistics ─────────────────────────────────────────────────────────────

intent "return aggregate statistics for pastes and short links"
route GET "/stats" {
  needs db

  let paste_count: Int = db.query("pastes") | count
  let link_count: Int = db.query("links") | count

  respond 200 with { pastes: paste_count, links: link_count }
}

// ── 6. Scheduled cleanup ──────────────────────────────────────────────────────

intent "delete pastes and links older than 7 days"
schedule every 1d {
  needs db
  db.delete_where("pastes", { before: "2000-01-01T00:00:00Z" })
  db.delete_where("links", { before: "2000-01-01T00:00:00Z" })
}

// ── 7. Landing page ───────────────────────────────────────────────────────────

intent "serve the landing page with endpoint documentation"
route GET "/" {
  let html: String = "<!DOCTYPE html>"
    + "<html lang='en'><head>"
    + "<meta charset='UTF-8'/>"
    + "<meta name='viewport' content='width=device-width, initial-scale=1'/>"
    + "<title>Showcase API</title>"
    + "<style>"
    + "body{{font-family:system-ui,sans-serif;max-width:700px;margin:40px auto;padding:0 20px;background:#0f172a;color:#e2e8f0}}"
    + "h1{{color:#38bdf8}}h2{{color:#7dd3fc;margin-top:2rem}}"
    + "code{{background:#1e293b;padding:2px 8px;border-radius:4px;color:#f0abfc}}"
    + "table{{width:100%;border-collapse:collapse;margin-top:1rem}}"
    + "th,td{{text-align:left;padding:8px 12px;border-bottom:1px solid #334155}}"
    + "th{{color:#94a3b8}}tr:hover{{background:#1e293b}}"
    + ".badge{{display:inline-block;padding:2px 8px;border-radius:4px;font-size:0.75rem;font-weight:bold}}"
    + ".get{{background:#166534;color:#bbf7d0}}.post{{background:#92400e;color:#fef3c7}}"
    + ".get302{{background:#1e3a5f;color:#bfdbfe}}"
    + "</style>"
    + "</head><body>"
    + "<h1>Showcase API</h1>"
    + "<p>A self-documenting API built with <strong>PACT 0.5.0</strong>.</p>"
    + "<h2>Endpoints</h2>"
    + "<table><thead><tr><th>Method</th><th>Path</th><th>Description</th></tr></thead><tbody>"
    + "<tr><td><span class='badge get'>GET</span></td><td><code>/health</code></td><td>Health check</td></tr>"
    + "<tr><td><span class='badge post'>POST</span></td><td><code>/paste</code></td><td>Create a paste</td></tr>"
    + "<tr><td><span class='badge get'>GET</span></td><td><code>/paste/{{id}}</code></td><td>Get paste as plain text</td></tr>"
    + "<tr><td><span class='badge post'>POST</span></td><td><code>/shorten</code></td><td>Shorten a URL</td></tr>"
    + "<tr><td><span class='badge get302'>GET 302</span></td><td><code>/s/{{code}}</code></td><td>Redirect to original URL</td></tr>"
    + "<tr><td><span class='badge get'>GET</span></td><td><code>/avatar/{{name}}</code></td><td>SVG identicon</td></tr>"
    + "<tr><td><span class='badge get'>GET</span></td><td><code>/stats</code></td><td>Paste and link counts</td></tr>"
    + "</tbody></table>"
    + "<h2>Features used</h2>"
    + "<ul>"
    + "<li><code>schedule every 1d</code> — background cleanup</li>"
    + "<li><code>respond 200 with ... as text/plain</code> — custom content type</li>"
    + "<li><code>rng.short_id()</code> — 8-char random ID</li>"
    + "<li><code>db.delete_where(table, filter)</code> — bulk delete</li>"
    + "<li><code>time.days_ago(7)</code> — relative timestamp</li>"
    + "<li><code>str | chars | map | sum</code> — char code arithmetic</li>"
    + "</ul>"
    + "</body></html>"

  respond 200 with html as "text/html"
}

// ── 8. App declaration ────────────────────────────────────────────────────────

app Showcase {
  port: 8080,
  db: "sqlite://data/showcase.db",
}

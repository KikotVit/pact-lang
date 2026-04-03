intent "Generate a short code from URL"
fn make_code(url: String) -> String
  needs rng
{
  rng.hex(6)
}

intent "Shorten a URL and store it"
fn shorten_url(url: String) -> Struct
  needs db, rng
{
  let code: String = make_code(url)
  let link: Struct = { url: url, code: code }
  db.insert("links", link)
  { short: code, url: url }
}

intent "Find link by short code"
fn find_link(code: String) -> Struct or NotFound
  needs db
{
  db.query("links")
    | filter where .code == code
    | expect one or raise NotFound
}

route POST "/shorten" {
  intent "Create a shortened URL"
  needs db, rng
  let url: String = request.body.url
  let result: Struct = shorten_url(url)
  respond 201 with result
}

route GET "/links/{code}" {
  intent "Redirect to original URL"
  needs db
  find_link(request.params.code)
    | on success: respond 200 with .
    | on NotFound: respond 404 with { error: "Link not found" }
}

route GET "/health" {
  intent "Health check"
  respond 200 with { status: "ok" }
}

app Shortener { port: 8085 }

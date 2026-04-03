type Link {
  id: ID,
  url: String,
  short: String,
  clicks: Int
}

intent "Create a short link from a URL"
fn shorten(url: String) -> Link needs db, rng {
  let short: String = rng.hex(6)
  let link: Link = Link { id: rng.uuid(), url: url, short: short, clicks: 0 }
  db.insert("links", link)
  link
}

route POST "/shorten" {
  intent "Shorten a URL"
  needs db, rng

  let link: Link = shorten(request.body.url)
  respond 201 with link
}

route GET "/{short}" {
  intent "Redirect to original URL"
  needs db

  let link: Link = db.query("links")
    | find first where .short == request.params.short
  if link == nothing {
    respond 404 with { error: "Link not found" }
  } else {
    respond 302 with { location: link.url }
  }
}

app PACTLinks {
  port: 8083,
}

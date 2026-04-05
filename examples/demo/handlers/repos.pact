use models.repo.Repo

// Fetch trending repos from GitHub, transform with pipeline, respond.
// This is what PACT does in 15 lines that takes 50+ in Express/Flask.

intent "fetch trending Rust repos from GitHub"
route GET "/trending" {
  needs http

  http.get("https://api.github.com/search/repositories?q=language:Rust+sort:stars&per_page=10", {
    headers: { "User-Agent": "pact-demo" }
  })
    | on HttpError: respond 502 with { error: "GitHub API unavailable" }
    | .body
    | .items
    | sort by .stargazers_count descending
    | take first 5
    | map to {
        name: .full_name,
        stars: .stargazers_count,
        language: .language,
        url: .html_url
      }
    | respond 200 with .
}

intent "list all saved repos"
route GET "/repos" {
  needs db
  db.query("repos")
    | respond 200 with .
}

intent "save a repo"
route POST "/repos" {
  needs db, rng
  let repo: Repo = {
    name: request.body.name,
    stars: request.body.stars,
    language: request.body.language,
    url: request.body.url
  }
  db.insert("repos", repo)
    | on success: respond 201 with .
    | on DbError: respond 500 with { error: .message }
}

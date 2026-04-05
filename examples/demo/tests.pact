test "fetch and transform GitHub data via pipeline" {
  using http = http.mock({
    "https://api.github.com/search/repositories?q=language:Rust+sort:stars&per_page=10": {
      status: 200,
      body: {
        items: list(
          { full_name: "big/repo", stargazers_count: 50000, language: "Rust", html_url: "https://github.com/big/repo" },
          { full_name: "small/repo", stargazers_count: 100, language: "Rust", html_url: "https://github.com/small/repo" },
          { full_name: "mid/repo", stargazers_count: 5000, language: "Rust", html_url: "https://github.com/mid/repo" }
        )
      }
    }
  })

  let res: Struct = http.get("https://api.github.com/search/repositories?q=language:Rust+sort:stars&per_page=10", {
    headers: { "User-Agent": "test" }
  })

  // Same pipeline as the route
  let repos: List = res.body.items
    | sort by .stargazers_count descending
    | take first 2
    | map to { name: .full_name, stars: .stargazers_count }

  assert repos.length() == 2
  assert repos.first().name == "big/repo"
  assert repos.first().stars == 50000
}

test "save and list repos via db" {
  using db = db.memory()

  db.insert("repos", { name: "test/repo", stars: 42, language: "Rust", url: "https://example.com" })

  let all: List = db.query("repos")
  assert all.length() == 1
  assert all.first().name == "test/repo"
}

test "http error returns structured error" {
  using http = http.mock({})

  let result: Struct = http.get("https://unknown.api/data")
    | on HttpError: { error: "caught" }

  assert result.error == "caught"
}

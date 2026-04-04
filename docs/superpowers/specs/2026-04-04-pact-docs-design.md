# `pact docs` + MCP tool `pact_docs` — Design Spec

## Goal

Add built-in documentation to the PACT binary: `pact docs [topic]` CLI command and `pact_docs` MCP tool, both backed by the same content source.

## Architecture

One new file `src/docs.rs` is the single source of truth. Two entry points (CLI and MCP) call the same functions.

```
pact docs pipeline   ->  main.rs  ->  docs::get_doc("pipeline")  ->  stdout
pact mcp (pact_docs) ->  mcp.rs   ->  docs::get_doc("pipeline")  ->  JSON-RPC response
```

### Public API (`src/docs.rs`)

```rust
/// Returns markdown documentation for a topic, or None if topic doesn't exist.
pub fn get_doc(topic: &str) -> Option<&'static str>

/// Returns list of (topic_name, one_line_description) for all available topics.
pub fn list_topics() -> Vec<(&'static str, &'static str)>
```

### Content storage

- Each topic is a separate `.md` file in `src/docs/` directory
- `src/docs.rs` uses `include_str!()` to embed them at compile time
- `src/docs.rs` stays small (~60 lines): match + include_str! + list_topics
- Each topic file: heading, brief description, syntax section, 1-2 working examples, `> See also:` cross-references at the bottom
- ~25-50 lines per topic file

### Cross-references

Every topic ends with a `> See also:` line linking to related topics. Example:
```
> See also: pipeline (filter, map, sort on lists), string (.split() returns List)
```

## Topics

13 topics, each with syntax reference and working examples:

| Topic | Description |
|-------|-------------|
| **`quickstart`** | Working example: route + fn + db + app. Copy-paste and run. |
| `pipeline` | All 21 pipeline steps (filter, map, sort, take, skip, etc.) |
| `route` | HTTP routes with path parameters, respond |
| `fn` | Functions, intent, needs, error types |
| `type` | Struct types, union types, optional fields |
| `db` | insert, query, find, update, delete |
| `test` | Test blocks, using (mock effects), assert |
| `effects` | All 6 built-in effects: db, time, rng, auth, log, env |
| `match` | Match expressions, patterns, wildcard |
| `error` | Error handling: ?, on ErrorType:, error propagation |
| `string` | Interpolation, raw strings, multiline, methods |
| `list` | list(), methods, pipeline operations on lists |
| `app` | App declaration, port, db config |

## CLI integration (`main.rs`)

### `pact docs` (no argument)

Prints formatted list of all topics with descriptions:

```
PACT documentation topics:

  pipeline   All 21 pipeline steps
  route      HTTP routes with path parameters
  fn         Functions, intent, needs, error types
  ...

Usage: pact docs <topic>
```

### `pact docs <topic>` (known topic)

Prints the markdown content to stdout. Markdown is readable as-is in terminal.

### `pact docs <unknown>` (unknown topic)

```
Unknown topic 'pip'. Did you mean 'pipeline'?

Available topics: pipeline, route, fn, type, db, test, effects, match, error, string, list, app
```

Did-you-mean: prefix match (`topic.starts_with(input)`). No Levenshtein — overkill for 13 topics.

### Help text update

Add `pact docs [topic]` line to the existing help output in main.rs.

## MCP integration (`mcp.rs`)

Third tool `pact_docs` alongside existing `pact_run` and `pact_check`.

### Tool definition

```json
{
  "name": "pact_docs",
  "description": "Get PACT language documentation. Returns markdown reference for a topic, or lists all available topics if no topic is specified.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "topic": {
        "type": "string",
        "description": "Topic name (e.g. 'pipeline', 'route', 'db'). Omit to list all topics."
      }
    },
    "additionalProperties": false
  }
}
```

### Behavior

- `topic` present and valid: returns markdown content via `make_tool_result(text, false)`
- `topic` absent/empty: returns topic list via `make_tool_result(list_text, false)`
- `topic` unknown: returns error with available topics via `make_tool_result(msg, true)`

## Testing

### Unit tests in `src/docs.rs`

- Each of 13 topics exists (get_doc returns Some)
- Each topic content is non-empty
- `list_topics()` returns exactly 13 entries
- Unknown topic returns None
- All topic names in list_topics match get_doc keys
- `test_quickstart_contains_app` — quickstart has app declaration
- `test_code_examples_parse` — extract all ````pact` blocks from every topic, run through Lexer+Parser. If an example doesn't parse, the test fails. This guarantees agents won't copy broken code.

### Unit tests in `src/mcp.rs`

- `pact_docs` with valid topic returns markdown content, no isError
- `pact_docs` without topic returns topic list, no isError
- `pact_docs` with unknown topic returns isError: true
- `tools/list` now returns 3 tools (was 2)

### Manual verification

```bash
pact docs              # prints topic list
pact docs pipeline     # prints pipeline documentation
pact docs unknown      # error with hint
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"pact_docs","arguments":{"topic":"route"}}}' | pact mcp
```

## Out of scope

- Documentation search (`pact docs --search`)
- HTML/PDF rendering
- Auto-generating docs from source code or AST

# PACT Language for VS Code

Syntax highlighting, diagnostics, hover, and autocomplete for the [PACT programming language](https://github.com/KikotVit/pact-lang).

## Features

- **Syntax highlighting** — keywords, types, strings, comments, pipeline operators, intent declarations
- **Real-time diagnostics** — type errors and warnings as you type
- **Hover information** — type signatures for functions, types, and built-in effects
- **Autocomplete** — keywords, pipeline steps, effect methods (`db.`, `auth.`, etc.), user-defined symbols

## Requirements

The `pact` binary must be installed. The extension auto-discovers it in:
- `~/bin/pact`
- `~/.local/bin/pact`
- `/usr/local/bin/pact`
- Workspace `target/release/pact` or `target/debug/pact`

Install pact:
```sh
curl -fsSL https://raw.githubusercontent.com/KikotVit/pact-lang/master/scripts/install.sh | sh
```

Or set the path manually in VS Code settings:
```json
{
  "pact.path": "/path/to/pact"
}
```

## Install from .vsix

```sh
code --install-extension pact-lang-0.1.0.vsix
```

Build from source:
```sh
cd editors/vscode
npm install
npx tsc -p ./
npx @vscode/vsce package
code --install-extension pact-lang-0.1.0.vsix
```

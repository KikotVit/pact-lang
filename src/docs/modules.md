## Modules

Import functions and types from other `.pact` files.

### Syntax

```pact
use models.user.User
use models.user.find_user
use utils.math.*
```

### File resolution

`use models.user.User` resolves to:
- File: `models/user.pact` (relative to the main file)
- Symbol: `User`

Path components map to directories. The second-to-last component is the filename.

### Example structure

```
myapp/
  main.pact              ← pact run main.pact
  models/user.pact       ← use models.user.User
  handlers/users.pact    ← use handlers.users.*
```

### Caching

Each module is loaded and evaluated once. Multiple imports from the same file reuse the cached result.

### Error handling

- Missing file: `Cannot import 'models/user.pact': file not found`
- Missing symbol: `Symbol 'Foo' not found in module 'models/user.pact'`
- Circular import: `Circular import detected: models/user.pact is already being loaded`

> See also: fn (functions to import), type (types to import), app (entry point)

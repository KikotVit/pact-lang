// Built-in functions and effect stubs are implemented as methods on Interpreter
// in interpreter.rs. Supported builtins:
//   - list(args...) -> List
//   - db.insert(table, value) -> value
//   - db.query(table) -> List
//   - time.now() -> String
//   - rng.uuid() -> String
//
// Effects (db, time, rng) are set up via Interpreter::setup_test_effects().

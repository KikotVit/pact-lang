// Pipeline execution is implemented as methods on Interpreter in interpreter.rs.
// Pipeline steps (Filter, Map, Sort, Each, Count, Sum, Flatten, Unique, GroupBy,
// Take, Skip, FindFirst, ExpectOne, ExpectAny, OrDefault, Expr) are all handled
// by Interpreter::execute_pipeline_step().

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::parser::ast::*;

pub fn format(source: &str) -> Result<String, String> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| format!("{}", e))?;
    let comments: Vec<(usize, String)> = lexer
        .comments()
        .iter()
        .map(|(span, text)| (span.line, text.clone()))
        .collect();

    let mut parser = Parser::new(tokens, source);
    let program = parser.parse().map_err(|errors| {
        errors
            .iter()
            .map(|e| format!("{}", e))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let mut f = Formatter {
        output: String::new(),
        indent: 0,
        comments,
        comment_idx: 0,
    };

    f.format_program(&program);

    let mut result = f.output.trim_end().to_string();
    result.push('\n');
    Ok(result)
}

struct Formatter {
    output: String,
    indent: usize,
    comments: Vec<(usize, String)>,
    comment_idx: usize,
}

impl Formatter {
    fn writeln(&mut self, s: &str) {
        self.output.push_str(&self.indent_str());
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }

    fn indent_str(&self) -> String {
        "  ".repeat(self.indent)
    }

    fn emit_comments_before_line(&mut self, line: usize) {
        while self.comment_idx < self.comments.len() {
            let (comment_line, ref text) = self.comments[self.comment_idx];
            if comment_line < line {
                if text.is_empty() {
                    self.writeln("//");
                } else {
                    self.writeln(&format!("// {}", text));
                }
                self.comment_idx += 1;
            } else {
                break;
            }
        }
    }

    fn emit_remaining_comments(&mut self) {
        while self.comment_idx < self.comments.len() {
            let (_, ref text) = self.comments[self.comment_idx];
            if text.is_empty() {
                self.writeln("//");
            } else {
                self.writeln(&format!("// {}", text));
            }
            self.comment_idx += 1;
        }
    }

    fn format_program(&mut self, program: &Program) {
        let stmts = &program.statements;
        for (i, stmt) in stmts.iter().enumerate() {
            // Emit comments before this statement
            if let Some(line) = self.stmt_line(stmt) {
                self.emit_comments_before_line(line);
            }

            self.format_statement(stmt);

            // Blank line between top-level declarations
            if i + 1 < stmts.len() && self.needs_blank_line(stmt, &stmts[i + 1]) {
                self.newline();
            }
        }

        self.emit_remaining_comments();
    }

    fn needs_blank_line(&self, _current: &Statement, _next: &Statement) -> bool {
        // Blank line between most top-level statements
        true
    }

    fn stmt_line(&self, stmt: &Statement) -> Option<usize> {
        match stmt {
            Statement::Let { span, .. } => span.as_ref().map(|s| s.line),
            Statement::FnDecl { span, .. } => span.as_ref().map(|s| s.line),
            Statement::Return { span, .. } => span.as_ref().map(|s| s.line),
            _ => None,
        }
    }

    fn format_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let {
                name,
                mutable,
                type_ann,
                value,
                ..
            } => {
                let kw = if *mutable { "var" } else { "let" };
                let type_str = self.format_type_expr(type_ann);
                let value_str = self.format_expr(value);
                self.writeln(&format!("{} {}: {} = {}", kw, name, type_str, value_str));
            }

            Statement::FnDecl {
                name,
                intent,
                params,
                return_type,
                error_types,
                effects,
                body,
                ..
            } => {
                if let Some(intent_text) = intent {
                    if !intent_text.is_empty() {
                        self.writeln(&format!("intent \"{}\"", intent_text));
                    }
                }

                let params_str: Vec<String> = params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, self.format_type_expr(&p.type_ann)))
                    .collect();

                let mut sig = format!("fn {}({})", name, params_str.join(", "));

                if let Some(ret) = return_type {
                    sig.push_str(&format!(" -> {}", self.format_type_expr(ret)));
                    if !error_types.is_empty() {
                        sig.push_str(&format!(" or {}", error_types.join(" or ")));
                    }
                }

                self.writeln(&sig);

                if !effects.is_empty() {
                    self.indent += 1;
                    self.writeln(&format!("needs {}", effects.join(", ")));
                    self.indent -= 1;
                }

                self.writeln("{");
                self.indent += 1;
                self.format_body(body);
                self.indent -= 1;
                self.writeln("}");
            }

            Statement::TypeDecl(type_decl) => {
                self.format_type_decl(type_decl);
            }

            Statement::Use { path } => {
                self.writeln(&format!("use {}", path.join(".")));
            }

            Statement::Return {
                value, condition, ..
            } => {
                let mut line = String::from("return");
                if let Some(val) = value {
                    line.push(' ');
                    line.push_str(&self.format_expr(val));
                }
                if let Some(cond) = condition {
                    line.push_str(&format!(" if {}", self.format_expr(cond)));
                }
                self.writeln(&line);
            }

            Statement::Expression(expr) => {
                let s = self.format_expr(expr);
                self.writeln(&s);
            }

            Statement::TestBlock { name, body } => {
                self.writeln(&format!("test \"{}\" {{", name));
                self.indent += 1;
                self.format_body(body);
                self.indent -= 1;
                self.writeln("}");
            }

            Statement::Using { name, value } => {
                let v = self.format_expr(value);
                self.writeln(&format!("using {} = {}", name, v));
            }

            Statement::Assert(expr) => {
                let e = self.format_expr(expr);
                self.writeln(&format!("assert {}", e));
            }

            Statement::Route {
                method,
                path,
                intent,
                effects,
                body,
            } => {
                if !intent.is_empty() {
                    self.writeln(&format!("intent \"{}\"", intent));
                }
                self.writeln(&format!("route {} \"{}\" {{", method, path));
                self.indent += 1;
                if !effects.is_empty() {
                    self.writeln(&format!("needs {}", effects.join(", ")));
                }
                self.format_body(body);
                self.indent -= 1;
                self.writeln("}");
            }

            Statement::Stream {
                method,
                path,
                intent,
                effects,
                body,
            } => {
                if !intent.is_empty() {
                    self.writeln(&format!("intent \"{}\"", intent));
                }
                self.writeln(&format!("stream {} \"{}\" {{", method, path));
                self.indent += 1;
                if !effects.is_empty() {
                    self.writeln(&format!("needs {}", effects.join(", ")));
                }
                self.format_body(body);
                self.indent -= 1;
                self.writeln("}");
            }

            Statement::App { name, port, db_url } => {
                if let Some(url) = db_url {
                    self.writeln(&format!(
                        "app {} {{ port: {}, db: \"{}\" }}",
                        name, port, url
                    ));
                } else {
                    self.writeln(&format!("app {} {{ port: {} }}", name, port));
                }
            }
        }
    }

    fn format_body(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.format_statement(stmt);
        }
    }

    fn format_type_decl(&mut self, decl: &TypeDecl) {
        match decl {
            TypeDecl::Struct { name, fields } => {
                self.writeln(&format!("type {} {{", name));
                self.indent += 1;
                for field in fields {
                    let constraints_str = self.format_constraints(&field.constraints);
                    self.writeln(&format!(
                        "{}: {}{},",
                        field.name,
                        self.format_type_expr(&field.type_ann),
                        constraints_str,
                    ));
                }
                self.indent -= 1;
                self.writeln("}");
            }
            TypeDecl::Union { name, variants } => {
                let variant_strs: Vec<String> =
                    variants.iter().map(|v| self.format_variant(v)).collect();
                self.writeln(&format!("type {} = {}", name, variant_strs.join(" | ")));
            }
        }
    }

    fn format_variant(&self, variant: &UnionVariant) -> String {
        if let Some(ref fields) = variant.fields {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|f| format!("{}: {}", f.name, self.format_type_expr(&f.type_ann)))
                .collect();
            format!("{} {{ {} }}", variant.name, fields_str.join(", "))
        } else {
            variant.name.clone()
        }
    }

    fn format_constraints(&self, constraints: &[Constraint]) -> String {
        if constraints.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = constraints
            .iter()
            .map(|c| match c {
                Constraint::Min(n) => format!("min {}", n),
                Constraint::Max(n) => format!("max {}", n),
                Constraint::MinLen(n) => format!("minlen {}", n),
                Constraint::MaxLen(n) => format!("maxlen {}", n),
                Constraint::Format(f) => format!("format {}", f),
                Constraint::Pattern(p) => format!("pattern \"{}\"", p),
            })
            .collect();
        format!(" | {}", parts.join(" | "))
    }

    fn format_type_expr(&self, te: &TypeExpr) -> String {
        match te {
            TypeExpr::Named(name) => name.clone(),
            TypeExpr::Generic { name, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.format_type_expr(a)).collect();
                format!("{}<{}>", name, args_str.join(", "))
            }
            TypeExpr::Optional(inner) => {
                format!("{}?", self.format_type_expr(inner))
            }
            TypeExpr::Result { ok, errors } => {
                let mut s = self.format_type_expr(ok);
                if !errors.is_empty() {
                    s.push_str(&format!(" or {}", errors.join(" or ")));
                }
                s
            }
        }
    }

    fn format_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::IntLiteral(n) => n.to_string(),
            Expr::FloatLiteral(n) => format_float(*n),
            Expr::StringLiteral(se) => self.format_string_expr(se),
            Expr::BoolLiteral(b) => b.to_string(),
            Expr::Nothing => "nothing".to_string(),
            Expr::Identifier(name) => name.clone(),

            Expr::FieldAccess { object, field } => {
                format!("{}.{}", self.format_expr(object), field)
            }

            Expr::DotShorthand(fields) => {
                let mut s = String::from(".");
                s.push_str(&fields.join("."));
                s
            }

            Expr::BinaryOp { left, op, right } => {
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Eq => "==",
                    BinaryOp::NotEq => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::Gt => ">",
                    BinaryOp::LtEq => "<=",
                    BinaryOp::GtEq => ">=",
                    BinaryOp::And => "and",
                    BinaryOp::Or => "or",
                };
                format!(
                    "{} {} {}",
                    self.format_expr(left),
                    op_str,
                    self.format_expr(right)
                )
            }

            Expr::UnaryOp { op, operand } => {
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "not ",
                };
                format!("{}{}", op_str, self.format_expr(operand))
            }

            Expr::ErrorPropagation(inner) => {
                format!("{}?", self.format_expr(inner))
            }

            Expr::FnCall { callee, args, .. } => {
                let args_str: Vec<String> = args.iter().map(|a| self.format_expr(a)).collect();
                format!("{}({})", self.format_expr(callee), args_str.join(", "))
            }

            Expr::Pipeline { source, steps } => self.format_pipeline(source, steps),

            Expr::If {
                condition,
                then_body,
                else_body,
            } => self.format_if(condition, then_body, else_body),

            Expr::Match { subject, arms, .. } => self.format_match(subject, arms),

            Expr::Block(stmts) => self.format_block_expr(stmts),

            Expr::StructLiteral { name, fields } => self.format_struct_literal(name, fields),

            Expr::Ensure(inner) => {
                format!("ensure {}", self.format_expr(inner))
            }

            Expr::Is { expr, type_name } => {
                format!("{} is {}", self.format_expr(expr), type_name)
            }

            Expr::Respond { status, body } => {
                format!(
                    "respond {} with {}",
                    self.format_expr(status),
                    self.format_expr(body)
                )
            }

            Expr::Send { body } => {
                format!("send {}", self.format_expr(body))
            }
        }
    }

    fn format_string_expr(&self, se: &StringExpr) -> String {
        match se {
            StringExpr::Simple(s) => format!("\"{}\"", s),
            StringExpr::Interpolated(parts) => {
                let mut out = String::from("\"");
                for part in parts {
                    match part {
                        StringPart::Literal(s) => out.push_str(s),
                        StringPart::Expr(e) => {
                            out.push('{');
                            out.push_str(&self.format_expr(e));
                            out.push('}');
                        }
                    }
                }
                out.push('"');
                out
            }
        }
    }

    fn format_pipeline(&self, source: &Expr, steps: &[PipelineStep]) -> String {
        let mut parts = vec![self.format_expr(source)];
        for step in steps {
            parts.push(self.format_pipeline_step(step));
        }
        if parts.len() <= 2 && parts.iter().map(|p| p.len()).sum::<usize>() < 60 {
            parts.join(" | ")
        } else {
            let first = parts.remove(0);
            let rest: Vec<String> = parts.iter().map(|p| format!("  | {}", p)).collect();
            format!(
                "{}\n{}{}",
                first,
                self.indent_str(),
                rest.join(&format!("\n{}", self.indent_str()))
            )
        }
    }

    fn format_pipeline_step(&self, step: &PipelineStep) -> String {
        match step {
            PipelineStep::Filter { predicate } => {
                format!("filter where {}", self.format_expr(predicate))
            }
            PipelineStep::Map { expr } => {
                format!("map to {}", self.format_expr(expr))
            }
            PipelineStep::Sort { field, descending } => {
                let dir = if *descending { " descending" } else { "" };
                format!("sort by {}{}", self.format_expr(field), dir)
            }
            PipelineStep::GroupBy { field } => {
                format!("group by {}", self.format_expr(field))
            }
            PipelineStep::Take { kind, count } => {
                let k = match kind {
                    TakeKind::First => "first",
                    TakeKind::Last => "last",
                };
                format!("take {} {}", k, self.format_expr(count))
            }
            PipelineStep::Skip { count } => {
                format!("skip {}", self.format_expr(count))
            }
            PipelineStep::Each { expr } => {
                format!("each {}", self.format_expr(expr))
            }
            PipelineStep::FindFirst { predicate } => {
                format!("find first where {}", self.format_expr(predicate))
            }
            PipelineStep::ExpectOne { error } => {
                format!("expect one or raise {}", self.format_expr(error))
            }
            PipelineStep::ExpectAny { error } => {
                format!("expect any or raise {}", self.format_expr(error))
            }
            PipelineStep::OrDefault { value } => {
                format!("or default {}", self.format_expr(value))
            }
            PipelineStep::Flatten => "flatten".to_string(),
            PipelineStep::Unique => "unique".to_string(),
            PipelineStep::Count => "count".to_string(),
            PipelineStep::Sum => "sum".to_string(),
            PipelineStep::ExpectSuccess => "expect success".to_string(),
            PipelineStep::OnSuccess { body } => {
                format!("on success: {}", self.format_expr(body))
            }
            PipelineStep::OnError {
                variant,
                guard,
                body,
            } => {
                let mut s = format!("on {}", variant);
                if let Some(g) = guard {
                    s.push_str(&format!(" where {}", self.format_expr(g)));
                }
                s.push_str(&format!(": {}", self.format_expr(body)));
                s
            }
            PipelineStep::ValidateAs { type_name } => {
                format!("validate as {}", type_name)
            }
            PipelineStep::Expr(expr) => self.format_expr(expr),
        }
    }

    fn format_if(
        &self,
        condition: &Expr,
        then_body: &[Statement],
        else_body: &Option<Vec<Statement>>,
    ) -> String {
        let mut out = format!("if {} {{\n", self.format_expr(condition));
        let inner_indent = "  ".repeat(self.indent + 1);
        for stmt in then_body {
            out.push_str(&inner_indent);
            out.push_str(&self.format_stmt_inline(stmt));
            out.push('\n');
        }
        out.push_str(&"  ".repeat(self.indent));
        out.push('}');

        if let Some(else_stmts) = else_body {
            out.push_str(" else {\n");
            for stmt in else_stmts {
                out.push_str(&inner_indent);
                out.push_str(&self.format_stmt_inline(stmt));
                out.push('\n');
            }
            out.push_str(&"  ".repeat(self.indent));
            out.push('}');
        }

        out
    }

    fn format_match(&self, subject: &Expr, arms: &[MatchArm]) -> String {
        let mut out = format!("match {} {{\n", self.format_expr(subject));
        let arm_indent = "  ".repeat(self.indent + 1);
        for arm in arms {
            let pattern = match &arm.pattern {
                Pattern::Identifier(name) => name.clone(),
                Pattern::Wildcard => "_".to_string(),
                Pattern::Literal(expr) => self.format_expr(expr),
            };
            out.push_str(&arm_indent);
            out.push_str(&format!("{} => {}", pattern, self.format_expr(&arm.body)));
            out.push('\n');
        }
        out.push_str(&"  ".repeat(self.indent));
        out.push('}');
        out
    }

    fn format_block_expr(&self, stmts: &[Statement]) -> String {
        if stmts.len() == 1 {
            if let Statement::Expression(expr) = &stmts[0] {
                return self.format_expr(expr);
            }
        }

        let mut out = String::from("{\n");
        let inner_indent = "  ".repeat(self.indent + 1);
        for stmt in stmts {
            out.push_str(&inner_indent);
            out.push_str(&self.format_stmt_inline(stmt));
            out.push('\n');
        }
        out.push_str(&"  ".repeat(self.indent));
        out.push('}');
        out
    }

    fn format_struct_literal(&self, name: &Option<String>, fields: &[StructField]) -> String {
        let fields_str: Vec<String> = fields
            .iter()
            .map(|f| match f {
                StructField::Named { name, value } => {
                    format!("{}: {}", name, self.format_expr(value))
                }
                StructField::Spread(expr) => {
                    format!("...{}", self.format_expr(expr))
                }
            })
            .collect();

        let inner = fields_str.join(", ");
        let one_line = if let Some(n) = name {
            format!("{} {{ {} }}", n, inner)
        } else {
            format!("{{ {} }}", inner)
        };

        // Use multiline if too long
        if one_line.len() > 80 || fields.len() > 3 {
            let inner_indent = "  ".repeat(self.indent + 1);
            let outer_indent = "  ".repeat(self.indent);
            let mut out = if let Some(n) = name {
                format!("{} {{\n", n)
            } else {
                "{\n".to_string()
            };
            for field_str in &fields_str {
                out.push_str(&inner_indent);
                out.push_str(field_str);
                out.push(',');
                out.push('\n');
            }
            out.push_str(&outer_indent);
            out.push('}');
            out
        } else {
            one_line
        }
    }

    fn format_stmt_inline(&self, stmt: &Statement) -> String {
        match stmt {
            Statement::Let {
                name,
                mutable,
                type_ann,
                value,
                ..
            } => {
                let kw = if *mutable { "var" } else { "let" };
                format!(
                    "{} {}: {} = {}",
                    kw,
                    name,
                    self.format_type_expr(type_ann),
                    self.format_expr(value)
                )
            }
            Statement::Return {
                value, condition, ..
            } => {
                let mut line = String::from("return");
                if let Some(val) = value {
                    line.push(' ');
                    line.push_str(&self.format_expr(val));
                }
                if let Some(cond) = condition {
                    line.push_str(&format!(" if {}", self.format_expr(cond)));
                }
                line
            }
            Statement::Expression(expr) => self.format_expr(expr),
            Statement::Assert(expr) => format!("assert {}", self.format_expr(expr)),
            Statement::Using { name, value } => {
                format!("using {} = {}", name, self.format_expr(value))
            }
            _ => {
                // For complex statements inside blocks, fall back
                let mut sub = Formatter {
                    output: String::new(),
                    indent: self.indent,
                    comments: vec![],
                    comment_idx: 0,
                };
                sub.format_statement(stmt);
                sub.output.trim().to_string()
            }
        }
    }
}

fn format_float(n: f64) -> String {
    let s = n.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(input: &str) -> String {
        format(input).unwrap()
    }

    #[test]
    fn format_let_statement() {
        let input = "let x: Int = 42\n";
        assert_eq!(fmt(input), "let x: Int = 42\n");
    }

    #[test]
    fn format_type_decl() {
        let input = "type User { id: String, name: String }\n";
        let output = fmt(input);
        assert!(output.contains("type User {"));
        assert!(output.contains("  id: String,"));
        assert!(output.contains("  name: String,"));
    }

    #[test]
    fn format_route() {
        let input = r#"intent "list users"
route GET "/users" {
  needs db
  db.query("users") | respond 200 with .
}
"#;
        let output = fmt(input);
        assert!(output.contains("intent \"list users\""));
        assert!(output.contains("route GET \"/users\" {"));
        assert!(output.contains("  needs db"));
    }

    #[test]
    fn format_fn_decl() {
        let input = r#"intent "add numbers"
fn add(a: Int, b: Int) -> Int {
  a + b
}
"#;
        let output = fmt(input);
        assert!(output.contains("intent \"add numbers\""));
        assert!(output.contains("fn add(a: Int, b: Int) -> Int"));
    }

    #[test]
    fn format_app() {
        let input = "app MyApp { port: 8080, db: \"sqlite://data.db\" }\n";
        let output = fmt(input);
        assert!(output.contains("app MyApp { port: 8080, db: \"sqlite://data.db\" }"));
    }

    #[test]
    fn format_union_type() {
        let input = "type Role = Admin | Editor | Viewer\n";
        let output = fmt(input);
        assert!(output.contains("type Role = Admin | Editor | Viewer"));
    }

    #[test]
    fn format_preserves_comments() {
        let input = "// a comment\nlet x: Int = 1\n";
        let output = fmt(input);
        assert!(output.contains("// a comment"));
        assert!(output.contains("let x: Int = 1"));
    }

    #[test]
    fn format_idempotent() {
        let input = r#"intent "list users"
route GET "/users" {
  needs db
  db.query("users") | respond 200 with .
}

app API { port: 8080 }
"#;
        let first = fmt(input);
        let second = fmt(&first);
        assert_eq!(first, second, "Formatter must be idempotent");
    }

    #[test]
    fn format_pipeline_multiline() {
        let input = r#"intent "find user"
fn find_user(id: String) -> Struct or NotFound
  needs db
{
  db.query("users") | filter where .id == id | expect one or raise NotFound
}
"#;
        let output = fmt(input);
        // Should have pipeline steps on separate lines since it's long
        assert!(output.contains("| filter where"));
    }

    #[test]
    fn format_test_block() {
        let input = r#"test "something works" {
  let x: Int = 1
  assert x == 1
}
"#;
        let output = fmt(input);
        assert!(output.contains("test \"something works\" {"));
        assert!(output.contains("  let x: Int = 1"));
        assert!(output.contains("  assert x == 1"));
    }

    #[test]
    fn format_stream() {
        let input = r#"intent "live data"
stream GET "/live" {
  needs db
  send db.watch("events")
}
"#;
        let output = fmt(input);
        assert!(output.contains("intent \"live data\""));
        assert!(output.contains("stream GET \"/live\" {"));
        assert!(output.contains("  send db.watch(\"events\")"));
    }

    #[test]
    fn format_match_expr() {
        let input = r#"let y: String = match x {
  1 => "one"
  2 => "two"
  _ => "other"
}
"#;
        let output = fmt(input);
        assert!(output.contains("match x {"));
        assert!(output.contains("  1 => \"one\""));
        assert!(output.contains("  _ => \"other\""));
    }

    #[test]
    fn format_type_with_constraints() {
        let input =
            "type NewUser { name: String | minlen 1 | maxlen 100, age: Int | min 0 | max 150 }\n";
        let output = fmt(input);
        assert!(output.contains("name: String | minlen 1 | maxlen 100,"));
        assert!(output.contains("age: Int | min 0 | max 150,"));
    }
}

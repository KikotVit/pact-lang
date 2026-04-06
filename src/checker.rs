use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::lexer::token::Span;
use crate::parser::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
    pub source_line: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.severity {
            Severity::Error => "Type error",
            Severity::Warning => "Warning",
        };
        writeln!(f, "{} at line {}, col {}:", prefix, self.line, self.column)?;
        writeln!(f, "  {}", self.source_line)?;
        let padding = self.column - 1 + 2;
        writeln!(f, "{:>width$}^", "", width = padding)?;
        write!(f, "  {}", self.message)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ResolvedType {
    Int,
    Float,
    String,
    Bool,
    Nothing,
    List,
    Map,
    Struct(std::string::String),
    Optional(Box<ResolvedType>),
    Result {
        ok: Box<ResolvedType>,
        errors: Vec<std::string::String>,
    },
    Unknown,
}

impl fmt::Display for ResolvedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolvedType::Int => write!(f, "Int"),
            ResolvedType::Float => write!(f, "Float"),
            ResolvedType::String => write!(f, "String"),
            ResolvedType::Bool => write!(f, "Bool"),
            ResolvedType::Nothing => write!(f, "Nothing"),
            ResolvedType::List => write!(f, "List"),
            ResolvedType::Map => write!(f, "Map"),
            ResolvedType::Struct(name) => write!(f, "{}", name),
            ResolvedType::Optional(inner) => write!(f, "{}?", inner),
            ResolvedType::Result { ok, errors } => {
                write!(f, "{}", ok)?;
                if !errors.is_empty() {
                    write!(f, " or {}", errors.join(", "))?;
                }
                Ok(())
            }
            ResolvedType::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used in collect_declarations, will be read in v2 for field access checking
enum TypeDef {
    Struct {
        fields: Vec<(std::string::String, ResolvedType)>,
    },
    Union {
        variants: Vec<std::string::String>,
    },
}

#[derive(Debug, Clone)]
struct FnSig {
    params: Vec<(std::string::String, ResolvedType)>,
    return_type: ResolvedType,
}

struct Checker<'a> {
    source: &'a str,
    scopes: Vec<HashMap<std::string::String, ResolvedType>>,
    fn_sigs: HashMap<std::string::String, FnSig>,
    type_defs: HashMap<std::string::String, TypeDef>,
    diagnostics: Vec<Diagnostic>,
    current_fn_return: Option<ResolvedType>,
    base_dir: Option<PathBuf>,
    module_type_cache: HashMap<
        PathBuf,
        (
            HashMap<std::string::String, TypeDef>,
            HashMap<std::string::String, FnSig>,
        ),
    >,
    loading_modules: HashSet<PathBuf>,
}

impl<'a> Checker<'a> {
    fn new(source: &'a str, base_dir: Option<PathBuf>) -> Self {
        Checker {
            source,
            scopes: vec![HashMap::new()],
            fn_sigs: HashMap::new(),
            type_defs: HashMap::new(),
            diagnostics: Vec::new(),
            current_fn_return: None,
            base_dir,
            module_type_cache: HashMap::new(),
            loading_modules: HashSet::new(),
        }
    }

    fn resolve_type(&self, type_expr: &TypeExpr) -> ResolvedType {
        match type_expr {
            TypeExpr::Named(name) => match name.as_str() {
                "Int" => ResolvedType::Int,
                "Float" => ResolvedType::Float,
                "String" => ResolvedType::String,
                "Bool" => ResolvedType::Bool,
                "Nothing" => ResolvedType::Nothing,
                "List" => ResolvedType::List,
                "Map" => ResolvedType::Map,
                other => {
                    if self.type_defs.contains_key(other) {
                        ResolvedType::Struct(other.to_string())
                    } else {
                        ResolvedType::Unknown
                    }
                }
            },
            TypeExpr::Generic { name, .. } => match name.as_str() {
                "List" => ResolvedType::List,
                "Map" => ResolvedType::Map,
                _ => ResolvedType::Unknown,
            },
            TypeExpr::Optional(inner) => ResolvedType::Optional(Box::new(self.resolve_type(inner))),
            TypeExpr::Result { ok, errors } => ResolvedType::Result {
                ok: Box::new(self.resolve_type(ok)),
                errors: errors.clone(),
            },
        }
    }

    fn types_compatible(expected: &ResolvedType, actual: &ResolvedType) -> bool {
        if matches!(expected, ResolvedType::Unknown) || matches!(actual, ResolvedType::Unknown) {
            return true;
        }
        match (expected, actual) {
            (ResolvedType::Int, ResolvedType::Int) => true,
            (ResolvedType::Float, ResolvedType::Float) => true,
            (ResolvedType::String, ResolvedType::String) => true,
            (ResolvedType::Bool, ResolvedType::Bool) => true,
            (ResolvedType::Nothing, ResolvedType::Nothing) => true,
            (ResolvedType::List, ResolvedType::List) => true,
            (ResolvedType::Map, ResolvedType::Map) => true,
            (ResolvedType::Struct(a), ResolvedType::Struct(b)) => a == b,
            // Nothing is compatible with Optional(_)
            (ResolvedType::Optional(_), ResolvedType::Nothing) => true,
            // Optional(T) is compatible with T
            (ResolvedType::Optional(inner), actual) => Self::types_compatible(inner, actual),
            _ => false,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: std::string::String, resolved_type: ResolvedType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, resolved_type);
        }
    }

    fn lookup(&self, name: &str) -> ResolvedType {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return t.clone();
            }
        }
        ResolvedType::Unknown
    }

    fn emit(
        &mut self,
        severity: Severity,
        span: &Option<Span>,
        message: std::string::String,
        hint: Option<std::string::String>,
    ) {
        let (line, column, source_line) = if let Some(s) = span {
            let src_line = self
                .source
                .lines()
                .nth(s.line - 1)
                .unwrap_or("")
                .to_string();
            (s.line, s.column, src_line)
        } else {
            (0, 0, std::string::String::new())
        };
        self.diagnostics.push(Diagnostic {
            severity,
            line,
            column,
            message,
            hint,
            source_line,
        });
    }
}

/// Analyze a parsed program for type errors. Returns all diagnostics found.
/// Empty vec = no issues.
pub fn check(program: &Program, source: &str, base_dir: Option<&Path>) -> Vec<Diagnostic> {
    let mut checker = Checker::new(source, base_dir.map(|p| p.to_path_buf()));
    checker.collect_declarations(program);
    checker.check_statements(&program.statements);
    checker.diagnostics
}

impl<'a> Checker<'a> {
    fn collect_declarations(&mut self, program: &Program) {
        for stmt in &program.statements {
            match stmt {
                Statement::TypeDecl(TypeDecl::Struct { name, fields }) => {
                    let resolved_fields: Vec<(String, ResolvedType)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), self.resolve_type(&f.type_ann)))
                        .collect();
                    self.type_defs.insert(
                        name.clone(),
                        TypeDef::Struct {
                            fields: resolved_fields,
                        },
                    );
                }
                Statement::TypeDecl(TypeDecl::Union { name, variants }) => {
                    let variant_names: Vec<String> =
                        variants.iter().map(|v| v.name.clone()).collect();
                    self.type_defs.insert(
                        name.clone(),
                        TypeDef::Union {
                            variants: variant_names,
                        },
                    );
                }
                Statement::FnDecl {
                    name,
                    params,
                    return_type,
                    error_types,
                    ..
                } => {
                    let resolved_params: Vec<(String, ResolvedType)> = params
                        .iter()
                        .map(|p| (p.name.clone(), self.resolve_type(&p.type_ann)))
                        .collect();
                    let resolved_return = if let Some(rt) = return_type {
                        if error_types.is_empty() {
                            self.resolve_type(rt)
                        } else {
                            ResolvedType::Result {
                                ok: Box::new(self.resolve_type(rt)),
                                errors: error_types.clone(),
                            }
                        }
                    } else {
                        ResolvedType::Nothing
                    };
                    self.fn_sigs.insert(
                        name.clone(),
                        FnSig {
                            params: resolved_params,
                            return_type: resolved_return,
                        },
                    );
                }
                Statement::Use { path } => {
                    self.resolve_use(path);
                }
                _ => {}
            }
        }
    }

    fn resolve_use(&mut self, path: &[String]) {
        if path.is_empty() {
            return;
        }

        let symbol_name = &path[path.len() - 1];
        let file_parts = &path[..path.len() - 1];

        if file_parts.is_empty() {
            return;
        }

        let base = match &self.base_dir {
            Some(d) => d.clone(),
            None => return, // no base_dir means we can't resolve files
        };

        let mut file_path = base;
        for part in file_parts {
            file_path.push(part);
        }
        file_path.set_extension("pact");

        // Check cache
        if let Some((cached_types, cached_fns)) = self.module_type_cache.get(&file_path) {
            if symbol_name == "*" {
                for (name, td) in cached_types {
                    self.type_defs.insert(name.clone(), td.clone());
                }
                for (name, sig) in cached_fns {
                    self.fn_sigs.insert(name.clone(), sig.clone());
                }
            } else {
                if let Some(td) = cached_types.get(symbol_name) {
                    self.type_defs.insert(symbol_name.clone(), td.clone());
                }
                if let Some(sig) = cached_fns.get(symbol_name) {
                    self.fn_sigs.insert(symbol_name.clone(), sig.clone());
                }
            }
            return;
        }

        // Circular dependency check
        let canonical = file_path.canonicalize().unwrap_or(file_path.clone());
        if self.loading_modules.contains(&canonical) {
            self.diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                line: 0,
                column: 0,
                message: format!(
                    "Circular import detected: {} is already being checked",
                    file_path.display()
                ),
                hint: Some(format!("File path resolved from: use {}", path.join("."))),
                source_line: String::new(),
            });
            return;
        }
        self.loading_modules.insert(canonical.clone());

        // Read, lex, parse the module
        let source = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(_) => {
                self.loading_modules.remove(&canonical);
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    line: 0,
                    column: 0,
                    message: format!(
                        "Cannot resolve import '{}': file not found",
                        file_path.display()
                    ),
                    hint: Some(format!("File path resolved from: use {}", path.join("."))),
                    source_line: String::new(),
                });
                return;
            }
        };

        let mut lexer = crate::lexer::Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                self.loading_modules.remove(&canonical);
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    line: 0,
                    column: 0,
                    message: format!(
                        "Lex error in imported '{}': {}",
                        file_path.display(),
                        e.message
                    ),
                    hint: Some(format!("File path resolved from: use {}", path.join("."))),
                    source_line: String::new(),
                });
                return;
            }
        };
        let mut parser = crate::parser::Parser::new(tokens, &source);
        let program = match parser.parse() {
            Ok(p) => p,
            Err(errors) => {
                self.loading_modules.remove(&canonical);
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    line: 0,
                    column: 0,
                    message: format!(
                        "Parse error in imported '{}': {}",
                        file_path.display(),
                        errors[0].message
                    ),
                    hint: Some(format!("File path resolved from: use {}", path.join("."))),
                    source_line: String::new(),
                });
                return;
            }
        };

        // Collect declarations from the module (Pass 1 only)
        let mut module_checker = Checker::new(&source, self.base_dir.clone());
        module_checker.loading_modules = self.loading_modules.clone();
        module_checker.collect_declarations(&program);

        // Propagate diagnostics from sub-checker
        self.diagnostics.extend(module_checker.diagnostics);

        self.loading_modules.remove(&canonical);

        // Cache the module's type info
        let module_types = module_checker.type_defs;
        let module_fns = module_checker.fn_sigs;
        self.module_type_cache
            .insert(file_path, (module_types.clone(), module_fns.clone()));

        // Import requested symbols
        if symbol_name == "*" {
            for (name, td) in &module_types {
                self.type_defs.insert(name.clone(), td.clone());
            }
            for (name, sig) in &module_fns {
                self.fn_sigs.insert(name.clone(), sig.clone());
            }
        } else {
            if let Some(td) = module_types.get(symbol_name) {
                self.type_defs.insert(symbol_name.clone(), td.clone());
            }
            if let Some(sig) = module_fns.get(symbol_name) {
                self.fn_sigs.insert(symbol_name.clone(), sig.clone());
            }
        }
    }

    fn check_statements(&mut self, statements: &[Statement]) {
        for stmt in statements {
            self.check_statement(stmt);
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        match stmt {
            // Check 1: Let binding type mismatch
            Statement::Let {
                name,
                type_ann,
                value,
                span,
                ..
            } => {
                let expected = self.resolve_type(type_ann);
                let actual = self.infer_expr(value);
                if !Self::types_compatible(&expected, &actual) {
                    self.emit(
                        Severity::Error,
                        span,
                        format!("Type mismatch: expected {}, got {}", expected, actual),
                        Some(format!(
                            "The variable '{}' is declared as {} but the value is {}",
                            name, expected, actual
                        )),
                    );
                }
                // Check nested expressions (e.g. FnCall args in the value)
                self.check_expr(value);
                // Bind regardless so later lookups work
                self.bind(name.clone(), expected);
            }
            // Check 3: Return type mismatch + Check 2 in fn bodies
            Statement::FnDecl {
                name,
                params,
                return_type,
                error_types,
                body,
                span,
                ..
            } => {
                let resolved_return = if let Some(rt) = return_type {
                    if error_types.is_empty() {
                        self.resolve_type(rt)
                    } else {
                        ResolvedType::Result {
                            ok: Box::new(self.resolve_type(rt)),
                            errors: error_types.clone(),
                        }
                    }
                } else {
                    ResolvedType::Nothing
                };
                let prev_fn_return = self.current_fn_return.take();
                self.current_fn_return = Some(resolved_return.clone());
                self.push_scope();
                // Bind params
                for p in params {
                    let pt = self.resolve_type(&p.type_ann);
                    self.bind(p.name.clone(), pt);
                }
                // Check body statements
                self.check_statements(body);
                // Check implicit return (last expression in body)
                if return_type.is_some() {
                    if let Some(Statement::Expression(expr)) = body.last() {
                        let actual = self.infer_expr(expr);
                        // For Result return types, compare against the ok type
                        let check_type = match &resolved_return {
                            ResolvedType::Result { ok, .. } => ok.as_ref(),
                            other => other,
                        };
                        if !Self::types_compatible(check_type, &actual) {
                            self.emit(
                                Severity::Error,
                                span,
                                format!(
                                    "Function '{}' should return {}, got {}",
                                    name, check_type, actual
                                ),
                                None,
                            );
                        }
                    }
                }
                self.pop_scope();
                self.current_fn_return = prev_fn_return;
            }
            // Check 3: Explicit return
            Statement::Return { value, span, .. } => {
                if let (Some(val_expr), Some(fn_ret)) = (value, &self.current_fn_return.clone()) {
                    let actual = self.infer_expr(val_expr);
                    let check_type = match fn_ret {
                        ResolvedType::Result { ok, .. } => ok.as_ref(),
                        other => other,
                    };
                    if !Self::types_compatible(check_type, &actual) {
                        self.emit(
                            Severity::Error,
                            span,
                            format!(
                                "Return type mismatch: expected {}, got {}",
                                check_type, actual
                            ),
                            None,
                        );
                    }
                    // Check nested expressions (e.g. FnCall args in the value)
                    self.check_expr(val_expr);
                }
            }
            // Check route body
            Statement::Route { body, .. } | Statement::Stream { body, .. } => {
                self.push_scope();
                self.bind("request".to_string(), ResolvedType::Unknown);
                self.check_statements(body);
                self.pop_scope();
            }
            // Recurse into expressions for Check 2 and Check 4
            Statement::Expression(expr) => {
                self.check_expr(expr);
            }
            Statement::TestBlock { body, .. } => {
                self.push_scope();
                self.check_statements(body);
                self.pop_scope();
            }
            Statement::Assert(expr) => {
                self.check_expr(expr);
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            // Check 2: Function argument type mismatch
            Expr::FnCall { callee, args, span } => {
                if let Expr::Identifier(name) = callee.as_ref() {
                    if let Some(sig) = self.fn_sigs.get(name).cloned() {
                        if args.len() == sig.params.len() {
                            for (arg, (param_name, param_type)) in
                                args.iter().zip(sig.params.iter())
                            {
                                let actual = self.infer_expr(arg);
                                if !Self::types_compatible(param_type, &actual) {
                                    self.emit(
                                        Severity::Error,
                                        span,
                                        format!(
                                            "Argument '{}' expects {}, got {}",
                                            param_name, param_type, actual
                                        ),
                                        Some(format!(
                                            "Function '{}' parameter '{}' is declared as {}",
                                            name, param_name, param_type
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }
                // Also check nested expressions in args
                for arg in args {
                    self.check_expr(arg);
                }
            }
            // Check 4: Match exhaustiveness
            Expr::Match {
                subject,
                arms,
                span,
            } => {
                let subject_type = self.infer_expr(subject);
                if let ResolvedType::Struct(name) = &subject_type {
                    if let Some(TypeDef::Union { variants }) = self.type_defs.get(name) {
                        // Check if any arm is a wildcard
                        let has_wildcard =
                            arms.iter().any(|a| matches!(a.pattern, Pattern::Wildcard));
                        if !has_wildcard {
                            let covered: Vec<&str> = arms
                                .iter()
                                .filter_map(|a| {
                                    if let Pattern::Identifier(n) = &a.pattern {
                                        Some(n.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            let missing: Vec<&String> = variants
                                .iter()
                                .filter(|v| !covered.contains(&v.as_str()))
                                .collect();
                            if !missing.is_empty() {
                                let missing_str: Vec<&str> =
                                    missing.iter().map(|s| s.as_str()).collect();
                                self.emit(
                                    Severity::Warning,
                                    span,
                                    format!(
                                        "Match on {} does not cover: {}",
                                        name,
                                        missing_str.join(", ")
                                    ),
                                    Some(format!(
                                        "Add arms for {} or use '_' wildcard to cover remaining variants",
                                        missing_str.join(", ")
                                    )),
                                );
                            }
                        }
                    }
                }
                // Check nested expressions in arms
                for arm in arms {
                    self.check_expr(&arm.body);
                }
            }
            // Recurse into subexpressions
            Expr::BinaryOp { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::UnaryOp { operand, .. } => {
                self.check_expr(operand);
            }
            Expr::If {
                condition,
                then_body,
                else_body,
            } => {
                self.check_expr(condition);
                self.push_scope();
                self.check_statements(then_body);
                self.pop_scope();
                if let Some(else_stmts) = else_body {
                    self.push_scope();
                    self.check_statements(else_stmts);
                    self.pop_scope();
                }
            }
            Expr::Block(stmts) => {
                self.push_scope();
                self.check_statements(stmts);
                self.pop_scope();
            }
            Expr::ErrorPropagation(inner) => {
                self.check_expr(inner);
            }
            Expr::Pipeline { source, .. } => {
                self.check_expr(source);
            }
            Expr::Send { body } => {
                self.check_expr(body);
            }
            Expr::Respond { status, body } => {
                self.check_expr(status);
                self.check_expr(body);
            }
            _ => {}
        }
    }

    fn infer_expr(&self, expr: &Expr) -> ResolvedType {
        match expr {
            Expr::IntLiteral(_) => ResolvedType::Int,
            Expr::FloatLiteral(_) => ResolvedType::Float,
            Expr::StringLiteral(_) => ResolvedType::String,
            Expr::BoolLiteral(_) => ResolvedType::Bool,
            Expr::Nothing => ResolvedType::Nothing,
            Expr::Identifier(name) => self.lookup(name),
            Expr::FnCall { callee, .. } => {
                if let Expr::Identifier(name) = callee.as_ref() {
                    if let Some(sig) = self.fn_sigs.get(name) {
                        return sig.return_type.clone();
                    }
                }
                ResolvedType::Unknown
            }
            Expr::StructLiteral { name: Some(n), .. } => ResolvedType::Struct(n.clone()),
            Expr::BinaryOp { left, op, right } => {
                let left_t = self.infer_expr(left);
                let right_t = self.infer_expr(right);
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        match (&left_t, &right_t) {
                            (ResolvedType::Int, ResolvedType::Int) => ResolvedType::Int,
                            (ResolvedType::Float, _) | (_, ResolvedType::Float) => {
                                ResolvedType::Float
                            }
                            _ if matches!(op, BinaryOp::Add)
                                && matches!(left_t, ResolvedType::String) =>
                            {
                                ResolvedType::String
                            }
                            _ => ResolvedType::Unknown,
                        }
                    }
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::Gt
                    | BinaryOp::LtEq
                    | BinaryOp::GtEq => ResolvedType::Bool,
                    BinaryOp::And | BinaryOp::Or => ResolvedType::Bool,
                }
            }
            Expr::UnaryOp { op, operand } => match op {
                UnaryOp::Neg => {
                    let t = self.infer_expr(operand);
                    match t {
                        ResolvedType::Int => ResolvedType::Int,
                        ResolvedType::Float => ResolvedType::Float,
                        _ => ResolvedType::Unknown,
                    }
                }
                UnaryOp::Not => ResolvedType::Bool,
            },
            Expr::ErrorPropagation(inner) => {
                let t = self.infer_expr(inner);
                if let ResolvedType::Result { ok, .. } = t {
                    *ok
                } else {
                    ResolvedType::Unknown
                }
            }
            Expr::Respond { .. } => ResolvedType::Nothing,
            Expr::Send { body } => self.infer_expr(body),
            _ => ResolvedType::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_check(source: &str) -> Vec<Diagnostic> {
        let mut lexer = crate::lexer::Lexer::new(source);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = crate::parser::Parser::new(tokens, source);
        let program = parser.parse().expect("parse failed");
        check(&program, source, None)
    }

    #[test]
    fn test_resolve_primitives() {
        let checker = Checker::new("", None);
        assert_eq!(
            checker.resolve_type(&TypeExpr::Named("Int".to_string())),
            ResolvedType::Int
        );
        assert_eq!(
            checker.resolve_type(&TypeExpr::Named("String".to_string())),
            ResolvedType::String
        );
        assert_eq!(
            checker.resolve_type(&TypeExpr::Named("Bool".to_string())),
            ResolvedType::Bool
        );
        assert_eq!(
            checker.resolve_type(&TypeExpr::Named("Float".to_string())),
            ResolvedType::Float
        );
        assert_eq!(
            checker.resolve_type(&TypeExpr::Named("Nothing".to_string())),
            ResolvedType::Nothing
        );
    }

    #[test]
    fn test_types_compatible_primitives() {
        assert!(Checker::types_compatible(
            &ResolvedType::Int,
            &ResolvedType::Int
        ));
        assert!(!Checker::types_compatible(
            &ResolvedType::Int,
            &ResolvedType::String
        ));
        assert!(Checker::types_compatible(
            &ResolvedType::Unknown,
            &ResolvedType::Int
        ));
        assert!(Checker::types_compatible(
            &ResolvedType::Int,
            &ResolvedType::Unknown
        ));
    }

    #[test]
    fn test_types_compatible_optional() {
        // Optional(Int) is compatible with Int
        assert!(Checker::types_compatible(
            &ResolvedType::Optional(Box::new(ResolvedType::Int)),
            &ResolvedType::Int
        ));
        // Nothing is compatible with Optional(_)
        assert!(Checker::types_compatible(
            &ResolvedType::Optional(Box::new(ResolvedType::Int)),
            &ResolvedType::Nothing
        ));
    }

    #[test]
    fn test_scope_management() {
        let mut checker = Checker::new("", None);
        checker.bind("x".to_string(), ResolvedType::Int);
        assert_eq!(checker.lookup("x"), ResolvedType::Int);
        assert_eq!(checker.lookup("y"), ResolvedType::Unknown);

        checker.push_scope();
        checker.bind("y".to_string(), ResolvedType::String);
        assert_eq!(checker.lookup("y"), ResolvedType::String);
        assert_eq!(checker.lookup("x"), ResolvedType::Int); // visible from outer

        checker.pop_scope();
        assert_eq!(checker.lookup("y"), ResolvedType::Unknown); // gone
    }

    #[test]
    fn test_empty_program() {
        let diags = parse_and_check("");
        assert!(diags.is_empty());
    }

    fn make_checker(source: &str) -> (Checker<'_>, Program) {
        let mut lexer = crate::lexer::Lexer::new(source);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = crate::parser::Parser::new(tokens, source);
        let program = parser.parse().expect("parse failed");
        let mut checker = Checker::new(source, None);
        checker.collect_declarations(&program);
        (checker, program)
    }

    #[test]
    fn test_collect_struct_type() {
        let (checker, _) = make_checker("type User {\n  name: String\n  age: Int\n}");
        assert!(checker.type_defs.contains_key("User"));
        if let TypeDef::Struct { fields } = &checker.type_defs["User"] {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0], ("name".to_string(), ResolvedType::String));
            assert_eq!(fields[1], ("age".to_string(), ResolvedType::Int));
        } else {
            panic!("Expected Struct typedef");
        }
    }

    #[test]
    fn test_collect_union_type() {
        let (checker, _) = make_checker("type Status = Active | Inactive");
        assert!(checker.type_defs.contains_key("Status"));
        if let TypeDef::Union { variants } = &checker.type_defs["Status"] {
            assert_eq!(variants, &["Active", "Inactive"]);
        } else {
            panic!("Expected Union typedef");
        }
    }

    #[test]
    fn test_collect_fn_sig() {
        let (checker, _) =
            make_checker("intent \"add numbers\"\nfn foo(x: Int, y: String) -> Bool {\n  true\n}");
        assert!(checker.fn_sigs.contains_key("foo"));
        let sig = &checker.fn_sigs["foo"];
        assert_eq!(
            sig.params,
            vec![
                ("x".to_string(), ResolvedType::Int),
                ("y".to_string(), ResolvedType::String),
            ]
        );
        assert_eq!(sig.return_type, ResolvedType::Bool);
    }

    #[test]
    fn test_infer_literals() {
        let checker = Checker::new("", None);
        assert_eq!(checker.infer_expr(&Expr::IntLiteral(42)), ResolvedType::Int);
        assert_eq!(
            checker.infer_expr(&Expr::FloatLiteral(3.14)),
            ResolvedType::Float
        );
        assert_eq!(
            checker.infer_expr(&Expr::StringLiteral(StringExpr::Simple("hi".to_string()))),
            ResolvedType::String
        );
        assert_eq!(
            checker.infer_expr(&Expr::BoolLiteral(true)),
            ResolvedType::Bool
        );
        assert_eq!(checker.infer_expr(&Expr::Nothing), ResolvedType::Nothing);
    }

    #[test]
    fn test_infer_binary_ops() {
        let checker = Checker::new("", None);
        // Int + Int = Int
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::IntLiteral(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::IntLiteral(2)),
        };
        assert_eq!(checker.infer_expr(&expr), ResolvedType::Int);
        // == returns Bool
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::IntLiteral(1)),
            op: BinaryOp::Eq,
            right: Box::new(Expr::IntLiteral(2)),
        };
        assert_eq!(checker.infer_expr(&expr), ResolvedType::Bool);
    }

    #[test]
    fn test_infer_identifier() {
        let mut checker = Checker::new("", None);
        checker.bind("x".to_string(), ResolvedType::Int);
        assert_eq!(
            checker.infer_expr(&Expr::Identifier("x".to_string())),
            ResolvedType::Int
        );
        assert_eq!(
            checker.infer_expr(&Expr::Identifier("unknown".to_string())),
            ResolvedType::Unknown
        );
    }

    #[test]
    fn test_infer_fn_call() {
        let mut checker = Checker::new("", None);
        checker.fn_sigs.insert(
            "foo".to_string(),
            FnSig {
                params: vec![("x".to_string(), ResolvedType::Int)],
                return_type: ResolvedType::String,
            },
        );
        let expr = Expr::FnCall {
            callee: Box::new(Expr::Identifier("foo".to_string())),
            args: vec![Expr::IntLiteral(1)],
            span: None,
        };
        assert_eq!(checker.infer_expr(&expr), ResolvedType::String);
        // Unknown function returns Unknown
        let expr = Expr::FnCall {
            callee: Box::new(Expr::Identifier("bar".to_string())),
            args: vec![],
            span: None,
        };
        assert_eq!(checker.infer_expr(&expr), ResolvedType::Unknown);
    }

    // --- Check 1: Let binding type mismatch ---

    #[test]
    fn test_let_int_gets_string() {
        let diags = parse_and_check("let x: Int = \"hello\"");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("expected Int"));
        assert!(diags[0].message.contains("got String"));
    }

    #[test]
    fn test_let_string_gets_int() {
        let diags = parse_and_check("let x: String = 42");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn test_let_bool_gets_int() {
        let diags = parse_and_check("let x: Bool = 1");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn test_let_int_gets_int() {
        let diags = parse_and_check("let x: Int = 42");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_let_unknown_rhs() {
        let diags = parse_and_check("let x: Int = some_var");
        assert!(diags.is_empty()); // Unknown → no false positive
    }

    #[test]
    fn test_let_struct_type() {
        let diags = parse_and_check(
            "type User {\n  name: String\n}\nlet u: User = User { name: \"Alice\" }",
        );
        assert!(diags.is_empty());
    }

    // --- Check 2: Function argument type mismatch ---

    #[test]
    fn test_fn_call_wrong_arg_type() {
        let diags =
            parse_and_check("intent \"test\"\nfn foo(x: Int) -> Int {\n  x\n}\nfoo(\"hello\")");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("'x'"));
        assert!(diags[0].message.contains("expects Int"));
    }

    #[test]
    fn test_fn_call_correct_types() {
        let diags = parse_and_check("intent \"test\"\nfn foo(x: Int) -> Int {\n  x\n}\nfoo(42)");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_fn_call_unknown_function() {
        let diags = parse_and_check("bar(42)");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_fn_call_multiple_args() {
        let diags = parse_and_check(
            "intent \"add\"\nfn add(a: Int, b: Int) -> Int {\n  a\n}\nadd(1, \"two\")",
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("'b'"));
    }

    // --- Check 3: Return type mismatch ---

    #[test]
    fn test_fn_returns_wrong_type() {
        let diags = parse_and_check("intent \"test\"\nfn foo() -> Int {\n  \"hello\"\n}");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("foo"));
    }

    #[test]
    fn test_fn_returns_correct_type() {
        let diags = parse_and_check("intent \"test\"\nfn foo() -> Int {\n  42\n}");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_fn_no_return_type() {
        let diags = parse_and_check("intent \"test\"\nfn foo() {\n  42\n}");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_fn_returns_unknown() {
        let diags = parse_and_check("intent \"test\"\nfn foo() -> Int {\n  some_call()\n}");
        assert!(diags.is_empty());
    }

    // --- Check 4: Match exhaustiveness ---

    #[test]
    fn test_match_missing_variant() {
        let diags = parse_and_check(
            "type Status = Active | Inactive | Banned\nlet s: Status = Active\nmatch s {\n  Active => true\n  Inactive => false\n}",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Banned"));
    }

    #[test]
    fn test_match_all_variants() {
        let diags = parse_and_check(
            "type Status = Active | Inactive | Banned\nlet s: Status = Active\nmatch s {\n  Active => 1\n  Inactive => 2\n  Banned => 3\n}",
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn test_match_with_wildcard() {
        let diags = parse_and_check(
            "type Status = Active | Inactive | Banned\nlet s: Status = Active\nmatch s {\n  Active => 1\n  _ => 0\n}",
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn test_match_non_union() {
        let diags = parse_and_check("let x: Int = 1\nmatch x {\n  1 => true\n  _ => false\n}");
        assert!(diags.is_empty());
    }

    // --- Route body checking ---

    #[test]
    fn test_type_error_in_route() {
        let diags = parse_and_check(
            "intent \"test\"\nroute GET \"/test\" {\n  let x: Int = \"hello\"\n  respond 200 with x\n}",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    // --- Integration tests ---

    #[test]
    fn test_multiple_errors() {
        let diags = parse_and_check("let a: Int = \"x\"\nlet b: Bool = 42\nlet c: String = true");
        assert_eq!(diags.len(), 3);
        assert!(diags.iter().all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn test_forward_reference() {
        // bar() calls foo() which is declared later — two-pass should handle this
        let diags = parse_and_check(
            "intent \"b\"\nfn bar() -> Int {\n  foo()\n}\nintent \"f\"\nfn foo() -> Int {\n  42\n}\nbar()",
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn test_nested_scope() {
        // Variable declared inside fn body doesn't leak out
        let diags = parse_and_check(
            "intent \"test\"\nfn foo() -> Int {\n  let x: Int = 42\n  x\n}\nlet y: Int = x",
        );
        // 'x' is Unknown outside fn, so let y: Int = x → no error (Unknown escape)
        assert!(diags.is_empty());
    }

    #[test]
    fn test_diagnostic_display() {
        let diag = Diagnostic {
            severity: Severity::Error,
            line: 3,
            column: 5,
            message: "Type mismatch: expected Int, got String".to_string(),
            hint: Some("The variable 'x' is declared as Int".to_string()),
            source_line: "let x: Int = \"hello\"".to_string(),
        };
        let output = format!("{}", diag);
        assert!(output.contains("Type error at line 3, col 5"));
        assert!(output.contains("let x: Int = \"hello\""));
        assert!(output.contains("Type mismatch"));
        assert!(output.contains("Hint:"));
    }

    // --- Regression: Check 2 inside let and return ---

    #[test]
    fn test_fn_call_wrong_arg_inside_let() {
        // Bug: check_expr was not called on let value, so FnCall arg mismatches were silent
        let diags = parse_and_check(
            "intent \"greet\"\nfn greet(name: String) -> String {\n  \"hello\"\n}\nlet x: String = greet(42)",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("'name'"));
        assert!(diags[0].message.contains("expects String"));
    }

    #[test]
    fn test_fn_call_wrong_arg_inside_return() {
        // Bug: check_expr was not called on return value
        let diags = parse_and_check(
            "intent \"greet\"\nfn greet(name: String) -> String {\n  \"hello\"\n}\nintent \"wrap\"\nfn wrap() -> String {\n  return greet(42)\n}",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("'name'"));
    }

    // --- Module resolution tests ---

    fn parse_and_check_with_base(source: &str, base_dir: &std::path::Path) -> Vec<Diagnostic> {
        let mut lexer = crate::lexer::Lexer::new(source);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = crate::parser::Parser::new(tokens, source);
        let program = parser.parse().expect("parse failed");
        check(&program, source, Some(base_dir))
    }

    #[test]
    fn test_check_imported_type() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_imported_type");
        let _ = fs::create_dir_all(dir.join("models"));
        fs::write(
            dir.join("models/user.pact"),
            "type User {\n  name: String,\n  age: Int\n}\n",
        )
        .unwrap();

        let source = "use models.user.User\nlet u: User = User { name: \"Alice\", age: 30 }";
        let diags = parse_and_check_with_base(source, &dir);
        // User type should be resolved — no errors
        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_imported_fn_sig() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_imported_fn");
        let _ = fs::create_dir_all(dir.join("utils"));
        fs::write(
            dir.join("utils/math.pact"),
            "intent \"add\"\nfn add(a: Int, b: Int) -> Int {\n  a + b\n}\n",
        )
        .unwrap();

        // Call add with wrong type — checker should catch it
        let source = "use utils.math.add\nadd(\"hello\", 2)";
        let diags = parse_and_check_with_base(source, &dir);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("expects Int"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_import_file_not_found() {
        let dir = std::env::temp_dir().join("pact_check_missing_import");
        let _ = std::fs::create_dir_all(&dir);

        let source = "use nonexistent.module.Foo";
        let diags = parse_and_check_with_base(source, &dir);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("file not found"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_wildcard_import_types() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_wildcard");
        let _ = fs::create_dir_all(dir.join("defs"));
        fs::write(
            dir.join("defs/types.pact"),
            "type Color = Red | Green | Blue\nintent \"get id\"\nfn get_id(x: Int) -> Int {\n  x\n}\n",
        )
        .unwrap();

        // Wildcard import should bring in both the type and the fn sig
        let source = "use defs.types.*\nget_id(\"wrong\")";
        let diags = parse_and_check_with_base(source, &dir);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("expects Int"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_circular_import() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_circular");
        let _ = fs::create_dir_all(dir.join("cyc"));
        fs::write(
            dir.join("cyc/a.pact"),
            "use cyc.b.b_fn\nintent \"a\"\nfn a_fn() -> Int { 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("cyc/b.pact"),
            "use cyc.a.a_fn\nintent \"b\"\nfn b_fn() -> Int { 2 }\n",
        )
        .unwrap();

        let source = "use cyc.a.a_fn";
        let diags = parse_and_check_with_base(source, &dir);
        // Should get a warning about circular import, not stack overflow
        assert!(
            diags.iter().any(|d| d.message.contains("Circular import")),
            "Expected circular import warning, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_import_lex_error() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_lex_err");
        let _ = fs::create_dir_all(dir.join("bad"));
        fs::write(dir.join("bad/mod.pact"), "@@@ not valid pact").unwrap();

        let source = "use bad.mod.Foo";
        let diags = parse_and_check_with_base(source, &dir);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Lex error"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_check_import_parse_error() {
        use std::fs;

        let dir = std::env::temp_dir().join("pact_check_parse_err");
        let _ = fs::create_dir_all(dir.join("bad"));
        fs::write(dir.join("bad/mod.pact"), "fn {{{").unwrap();

        let source = "use bad.mod.Foo";
        let diags = parse_and_check_with_base(source, &dir);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("Parse error"));

        let _ = fs::remove_dir_all(dir);
    }
}

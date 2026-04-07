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
        if !self.source_line.is_empty() {
            writeln!(f, "  {}", self.source_line)?;
            let padding = self.column.saturating_sub(1) + 2;
            writeln!(f, "{:>width$}^", "", width = padding)?;
        }
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
    List(Box<ResolvedType>),
    Map(Box<ResolvedType>, Box<ResolvedType>),
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
            ResolvedType::List(inner) => {
                if matches!(inner.as_ref(), ResolvedType::Unknown) {
                    write!(f, "List")
                } else {
                    write!(f, "List<{}>", inner)
                }
            }
            ResolvedType::Map(k, v) => {
                if matches!(k.as_ref(), ResolvedType::Unknown) {
                    write!(f, "Map")
                } else {
                    write!(f, "Map<{}, {}>", k, v)
                }
            }
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

const KNOWN_EFFECTS: &[&str] = &["db", "auth", "log", "time", "rng", "env", "http"];

struct Checker<'a> {
    source: &'a str,
    scopes: Vec<HashMap<std::string::String, ResolvedType>>,
    fn_sigs: HashMap<std::string::String, FnSig>,
    type_defs: HashMap<std::string::String, TypeDef>,
    diagnostics: Vec<Diagnostic>,
    current_fn_return: Option<ResolvedType>,
    current_fn_effects: Option<Vec<std::string::String>>,
    current_stmt_span: Option<Span>,
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
            current_fn_effects: None,
            current_stmt_span: None,
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
                "List" => ResolvedType::List(Box::new(ResolvedType::Unknown)),
                "Map" => ResolvedType::Map(
                    Box::new(ResolvedType::Unknown),
                    Box::new(ResolvedType::Unknown),
                ),
                other => {
                    if self.type_defs.contains_key(other) {
                        ResolvedType::Struct(other.to_string())
                    } else {
                        ResolvedType::Unknown
                    }
                }
            },
            TypeExpr::Generic { name, .. } => match name.as_str() {
                "List" => ResolvedType::List(Box::new(ResolvedType::Unknown)),
                "Map" => ResolvedType::Map(
                    Box::new(ResolvedType::Unknown),
                    Box::new(ResolvedType::Unknown),
                ),
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
            (ResolvedType::List(_), ResolvedType::List(_)) => true,
            (ResolvedType::Map(_, _), ResolvedType::Map(_, _)) => true,
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
        let effective_span = span.as_ref().or(self.current_stmt_span.as_ref());
        let (line, column, source_line) = if let Some(s) = effective_span {
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

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Function,
    Type,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub type_info: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
}

/// Analyze and return both diagnostics and symbol information (for LSP).
pub fn check_with_symbols(program: &Program, source: &str, base_dir: Option<&Path>) -> CheckResult {
    let mut checker = Checker::new(source, base_dir.map(|p| p.to_path_buf()));
    checker.collect_declarations(program);
    checker.check_statements(&program.statements);

    let mut symbols = Vec::new();

    for (name, sig) in &checker.fn_sigs {
        let params_str: Vec<String> = sig
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, t))
            .collect();
        symbols.push(Symbol {
            name: name.clone(),
            kind: SymbolKind::Function,
            type_info: format!("fn ({}) -> {}", params_str.join(", "), sig.return_type),
            detail: None,
        });
    }

    for (name, type_def) in &checker.type_defs {
        let info = match type_def {
            TypeDef::Struct { fields } => {
                let fs: Vec<String> = fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t))
                    .collect();
                format!("type {{ {} }}", fs.join(", "))
            }
            TypeDef::Union { variants } => {
                format!("type = {}", variants.join(" | "))
            }
        };
        symbols.push(Symbol {
            name: name.clone(),
            kind: SymbolKind::Type,
            type_info: info,
            detail: None,
        });
    }

    CheckResult {
        diagnostics: checker.diagnostics,
        symbols,
    }
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
                    // Check constraint validity
                    for field in fields {
                        let rt = self.resolve_type(&field.type_ann);
                        self.check_field_constraints(name, field, &rt);
                    }
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

    fn find_field_line(&self, field_name: &str) -> (usize, usize, String) {
        for (i, line) in self.source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with(field_name) && trimmed[field_name.len()..].starts_with(':') {
                let col = line.len() - trimmed.len() + 1;
                return (i + 1, col, line.to_string());
            }
        }
        (0, 0, String::new())
    }

    fn check_field_constraints(&mut self, type_name: &str, field: &Field, resolved: &ResolvedType) {
        let is_int = matches!(resolved, ResolvedType::Int);
        let is_string = matches!(resolved, ResolvedType::String);
        let (fline, fcol, fsource) = self.find_field_line(&field.name);
        let mut min_val: Option<i64> = None;
        let mut max_val: Option<i64> = None;
        let mut minlen_val: Option<usize> = None;
        let mut maxlen_val: Option<usize> = None;

        for c in &field.constraints {
            match c {
                Constraint::Min(n) => {
                    if !is_int {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'min' constraint on non-Int field",
                                type_name, field.name
                            ),
                            hint: Some("'min' only applies to Int fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                    min_val = Some(*n);
                }
                Constraint::Max(n) => {
                    if !is_int {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'max' constraint on non-Int field",
                                type_name, field.name
                            ),
                            hint: Some("'max' only applies to Int fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                    max_val = Some(*n);
                }
                Constraint::MinLen(n) => {
                    if !is_string {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'minlen' constraint on non-String field",
                                type_name, field.name
                            ),
                            hint: Some("'minlen' only applies to String fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                    minlen_val = Some(*n);
                }
                Constraint::MaxLen(n) => {
                    if !is_string {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'maxlen' constraint on non-String field",
                                type_name, field.name
                            ),
                            hint: Some("'maxlen' only applies to String fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                    maxlen_val = Some(*n);
                }
                Constraint::Format(fmt) => {
                    if !is_string {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'format {}' constraint on non-String field",
                                type_name, field.name, fmt
                            ),
                            hint: Some("'format' only applies to String fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                }
                Constraint::Pattern(_) => {
                    if !is_string {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            line: fline,
                            column: fcol,
                            message: format!(
                                "{}.{}: 'pattern' constraint on non-String field",
                                type_name, field.name
                            ),
                            hint: Some("'pattern' only applies to String fields".to_string()),
                            source_line: fsource.clone(),
                        });
                    }
                }
            }
        }

        // Check contradictions
        if let (Some(min), Some(max)) = (min_val, max_val) {
            if min > max {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    line: fline,
                    column: fcol,
                    message: format!(
                        "{}.{}: min ({}) > max ({}), no value can satisfy both",
                        type_name, field.name, min, max
                    ),
                    hint: None,
                    source_line: fsource.clone(),
                });
            }
        }
        if let (Some(minl), Some(maxl)) = (minlen_val, maxlen_val) {
            if minl > maxl {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    line: fline,
                    column: fcol,
                    message: format!(
                        "{}.{}: minlen ({}) > maxlen ({}), no value can satisfy both",
                        type_name, field.name, minl, maxl
                    ),
                    hint: None,
                    source_line: fsource.clone(),
                });
            }
        }
    }

    fn check_statements(&mut self, statements: &[Statement]) {
        for stmt in statements {
            self.check_statement(stmt);
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        // Set fallback span for diagnostics without their own span.
        // Only save/restore when this statement provides a span.
        let has_span = match stmt {
            Statement::Let { span, .. }
            | Statement::FnDecl { span, .. }
            | Statement::Return { span, .. } => span.is_some(),
            _ => false,
        };
        let prev_stmt_span = if has_span {
            let prev = self.current_stmt_span.take();
            self.current_stmt_span = match stmt {
                Statement::Let { span, .. }
                | Statement::FnDecl { span, .. }
                | Statement::Return { span, .. } => span.clone(),
                _ => None,
            };
            prev
        } else {
            None
        };

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
                effects,
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
                let prev_fn_effects = self.current_fn_effects.take();
                self.current_fn_return = Some(resolved_return.clone());
                self.current_fn_effects = Some(effects.clone());
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
                self.current_fn_effects = prev_fn_effects;
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
            Statement::Route { effects, body, .. } | Statement::Stream { effects, body, .. } => {
                let prev_fn_effects = self.current_fn_effects.take();
                self.current_fn_effects = Some(effects.clone());
                self.push_scope();
                self.bind("request".to_string(), ResolvedType::Unknown);
                self.check_statements(body);
                self.pop_scope();
                self.current_fn_effects = prev_fn_effects;
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
        if has_span {
            self.current_stmt_span = prev_stmt_span;
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
                // Check 10: Method calls on wrong types
                if let Expr::FieldAccess { object, field } = callee.as_ref() {
                    let obj_type = self.infer_expr(object);
                    match &obj_type {
                        ResolvedType::String => {
                            const STRING_METHODS: &[&str] = &[
                                "length",
                                "contains",
                                "to_upper",
                                "to_lower",
                                "trim",
                                "split",
                                "starts_with",
                                "ends_with",
                                "replace",
                            ];
                            if !STRING_METHODS.contains(&field.as_str()) {
                                self.emit(
                                    Severity::Error,
                                    span,
                                    format!(
                                        "String has no method '{}'. Available: {}",
                                        field,
                                        STRING_METHODS.join(", ")
                                    ),
                                    None,
                                );
                            }
                        }
                        ResolvedType::List(_) => {
                            const LIST_METHODS: &[&str] = &[
                                "length", "contains", "push", "get", "join", "is_empty", "first",
                                "last", "reverse",
                            ];
                            if !LIST_METHODS.contains(&field.as_str()) {
                                self.emit(
                                    Severity::Error,
                                    span,
                                    format!(
                                        "List has no method '{}'. Available: {}",
                                        field,
                                        LIST_METHODS.join(", ")
                                    ),
                                    None,
                                );
                            }
                        }
                        ResolvedType::Int | ResolvedType::Float | ResolvedType::Bool => {
                            self.emit(
                                Severity::Error,
                                span,
                                format!("{} has no methods", obj_type),
                                Some("Methods are available on String and List values".to_string()),
                            );
                        }
                        _ => {} // Unknown, Struct, effect — skip
                    }
                }
                // Check callee (for FieldAccess effect checks, etc.)
                self.check_expr(callee);
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
            // Check 7: Binary operator type checking
            Expr::BinaryOp { left, op, right } => {
                self.check_expr(left);
                self.check_expr(right);
                let left_t = self.infer_expr(left);
                let right_t = self.infer_expr(right);
                if matches!(left_t, ResolvedType::Unknown)
                    || matches!(right_t, ResolvedType::Unknown)
                {
                    return;
                }
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        let valid = matches!(
                            (&left_t, &right_t),
                            (ResolvedType::Int, ResolvedType::Int)
                                | (ResolvedType::Float, ResolvedType::Float)
                                | (ResolvedType::Int, ResolvedType::Float)
                                | (ResolvedType::Float, ResolvedType::Int)
                        ) || (matches!(op, BinaryOp::Add)
                            && matches!(left_t, ResolvedType::String)
                            && matches!(right_t, ResolvedType::String));
                        if !valid {
                            self.emit(
                                Severity::Error,
                                &None,
                                format!("Cannot apply '{:?}' to {} and {}", op, left_t, right_t),
                                None,
                            );
                        }
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if !matches!(left_t, ResolvedType::Bool) {
                            self.emit(
                                Severity::Error,
                                &None,
                                format!("Left side of '{:?}' must be Bool, got {}", op, left_t),
                                None,
                            );
                        }
                        if !matches!(right_t, ResolvedType::Bool) {
                            self.emit(
                                Severity::Error,
                                &None,
                                format!("Right side of '{:?}' must be Bool, got {}", op, right_t),
                                None,
                            );
                        }
                    }
                    _ => {} // Comparison operators — lenient for v1
                }
            }
            // Check 8: Unary operator type checking
            Expr::UnaryOp { op, operand } => {
                self.check_expr(operand);
                let t = self.infer_expr(operand);
                if matches!(t, ResolvedType::Unknown) {
                    return;
                }
                match op {
                    UnaryOp::Neg => {
                        if !matches!(t, ResolvedType::Int | ResolvedType::Float) {
                            self.emit(Severity::Error, &None, format!("Cannot negate {}", t), None);
                        }
                    }
                    UnaryOp::Not => {
                        if !matches!(t, ResolvedType::Bool) {
                            self.emit(
                                Severity::Error,
                                &None,
                                format!("Cannot apply 'not' to {}", t),
                                None,
                            );
                        }
                    }
                }
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
            // Check 5: Struct literal field validation
            Expr::StructLiteral {
                name: Some(type_name),
                fields,
            } => {
                // Recurse into field values first
                for field in fields {
                    match field {
                        StructField::Named { value, .. } => self.check_expr(value),
                        StructField::Spread(expr) => self.check_expr(expr),
                    }
                }
                if let Some(TypeDef::Struct { fields: def_fields }) = self.type_defs.get(type_name)
                {
                    let def_fields = def_fields.clone();
                    let has_spread = fields.iter().any(|f| matches!(f, StructField::Spread(_)));

                    for field in fields {
                        if let StructField::Named { name, value } = field {
                            if let Some((_, expected_type)) =
                                def_fields.iter().find(|(n, _)| n == name)
                            {
                                // Check field value type
                                let actual = self.infer_expr(value);
                                if !Self::types_compatible(expected_type, &actual) {
                                    self.emit(
                                        Severity::Error,
                                        &None,
                                        format!(
                                            "Field '{}' of {} expects {}, got {}",
                                            name, type_name, expected_type, actual
                                        ),
                                        None,
                                    );
                                }
                            } else {
                                // Unknown field
                                let available: Vec<&str> =
                                    def_fields.iter().map(|(n, _)| n.as_str()).collect();
                                self.emit(
                                    Severity::Error,
                                    &None,
                                    format!(
                                        "Unknown field '{}' on type {}. Available: {}",
                                        name,
                                        type_name,
                                        available.join(", ")
                                    ),
                                    None,
                                );
                            }
                        }
                    }

                    // Check missing required fields (skip if spread present)
                    if !has_spread {
                        let provided: Vec<&str> = fields
                            .iter()
                            .filter_map(|f| match f {
                                StructField::Named { name, .. } => Some(name.as_str()),
                                _ => None,
                            })
                            .collect();
                        let missing: Vec<&str> = def_fields
                            .iter()
                            .filter(|(name, typ)| {
                                !provided.contains(&name.as_str())
                                    && !matches!(typ, ResolvedType::Optional(_))
                            })
                            .map(|(name, _)| name.as_str())
                            .collect();
                        if !missing.is_empty() {
                            self.emit(
                                Severity::Error,
                                &None,
                                format!(
                                    "Missing required field(s) on {}: {}",
                                    type_name,
                                    missing.join(", ")
                                ),
                                None,
                            );
                        }
                    }
                }
            }
            // Check 6: Field access validation + Check 9: effect usage
            Expr::FieldAccess { object, field } => {
                // Check 9: effect usage without needs
                if let Expr::Identifier(name) = object.as_ref() {
                    if KNOWN_EFFECTS.contains(&name.as_str()) {
                        if let Some(ref declared) = self.current_fn_effects {
                            if !declared.iter().any(|e| e == name) {
                                self.emit(
                                    Severity::Warning,
                                    &None,
                                    format!(
                                        "Effect '{}' used without 'needs {}' declaration",
                                        name, name
                                    ),
                                    Some(format!(
                                        "Add 'needs {}' to the function or route signature",
                                        name
                                    )),
                                );
                            }
                        }
                    }
                }
                self.check_expr(object);
                let obj_type = self.infer_expr(object);
                match &obj_type {
                    ResolvedType::Struct(type_name) => {
                        if let Some(TypeDef::Struct { fields: def_fields }) =
                            self.type_defs.get(type_name)
                        {
                            if !def_fields.iter().any(|(n, _)| n == field) {
                                let available: Vec<&str> =
                                    def_fields.iter().map(|(n, _)| n.as_str()).collect();
                                self.emit(
                                    Severity::Error,
                                    &None,
                                    format!(
                                        "Type '{}' has no field '{}'. Available: {}",
                                        type_name,
                                        field,
                                        available.join(", ")
                                    ),
                                    None,
                                );
                            }
                        }
                    }
                    ResolvedType::Int
                    | ResolvedType::Float
                    | ResolvedType::Bool
                    | ResolvedType::Nothing => {
                        self.emit(
                            Severity::Error,
                            &None,
                            format!("Cannot access field '{}' on {}", field, obj_type),
                            None,
                        );
                    }
                    _ => {} // Unknown, Map, List, effect, etc. — don't warn
                }
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
                // Method call return types
                if let Expr::FieldAccess { object, field } = callee.as_ref() {
                    let obj_type = self.infer_expr(object);
                    match (&obj_type, field.as_str()) {
                        (ResolvedType::String, "length") => return ResolvedType::Int,
                        (ResolvedType::String, "contains" | "starts_with" | "ends_with") => {
                            return ResolvedType::Bool;
                        }
                        (ResolvedType::String, "to_upper" | "to_lower" | "trim" | "replace") => {
                            return ResolvedType::String;
                        }
                        (ResolvedType::String, "split") => {
                            return ResolvedType::List(Box::new(ResolvedType::Unknown));
                        }
                        (ResolvedType::List(_), "length") => return ResolvedType::Int,
                        (ResolvedType::List(_), "contains" | "is_empty") => {
                            return ResolvedType::Bool;
                        }
                        (ResolvedType::List(_), "join") => return ResolvedType::String,
                        _ => {}
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
            Expr::FieldAccess { object, field } => {
                let obj_type = self.infer_expr(object);
                if let ResolvedType::Struct(type_name) = &obj_type {
                    if let Some(TypeDef::Struct { fields: def_fields }) =
                        self.type_defs.get(type_name)
                    {
                        if let Some((_, field_type)) = def_fields.iter().find(|(n, _)| n == field) {
                            return field_type.clone();
                        }
                    }
                }
                ResolvedType::Unknown
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

    #[test]
    fn struct_literal_field_type_mismatch() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn make() -> User {
  User { name: "Alice", age: "thirty" }
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("age"));
        assert!(errors[0].message.contains("Int"));
        assert!(errors[0].message.contains("String"));
    }

    #[test]
    fn struct_literal_unknown_field() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn make() -> User {
  User { name: "Alice", age: 30, email: "x" }
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Unknown field"));
        assert!(errors[0].message.contains("email"));
    }

    #[test]
    fn struct_literal_missing_required_field() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn make() -> User {
  User { name: "Alice" }
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Missing required"));
        assert!(errors[0].message.contains("age"));
    }

    #[test]
    fn struct_literal_valid_no_errors() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn make() -> User {
  User { name: "Alice", age: 30 }
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn struct_literal_optional_field_can_be_omitted() {
        let source = r#"
type Profile { name: String, bio: Optional<String> }
intent "test"
fn make() -> Profile {
  Profile { name: "Alice" }
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn field_access_nonexistent_field() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn test_fn() -> String {
  let u: User = User { name: "Alice", age: 30 }
  u.email
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("no field 'email'"));
        assert!(errors[0].message.contains("name, age"));
    }

    #[test]
    fn field_access_valid() {
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn test_fn() -> String {
  let u: User = User { name: "Alice", age: 30 }
  u.name
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn field_access_type_propagation() {
        // u.name is String, assigning to Int should error
        let source = r#"
type User { name: String, age: Int }
intent "test"
fn test_fn() -> Int {
  let u: User = User { name: "Alice", age: 30 }
  let n: Int = u.name
  n
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Int"));
        assert!(errors[0].message.contains("String"));
    }

    #[test]
    fn field_access_on_primitive() {
        let source = r#"
intent "test"
fn test_fn() -> Int {
  let x: Int = 42
  x.name
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Cannot access field"));
        assert!(errors[0].message.contains("Int"));
    }

    #[test]
    fn binary_op_string_plus_int() {
        let source = r#"
intent "test"
fn test_fn() -> String {
  let x: String = "hello" + 5
  x
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.len() >= 1);
        assert!(errors.iter().any(|e| e.message.contains("Cannot apply")));
    }

    #[test]
    fn binary_op_valid_arithmetic() {
        let source = r#"
intent "test"
fn test_fn() -> Int {
  let x: Int = 1 + 2
  let y: Float = 1.5 + 2.5
  let z: String = "a" + "b"
  x
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn unary_negate_string() {
        let source = r#"
intent "test"
fn test_fn() -> String {
  let x: String = -"hello"
  x
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.iter().any(|e| e.message.contains("Cannot negate")));
    }

    #[test]
    fn unary_not_int() {
        let source = r#"
intent "test"
fn test_fn() -> Bool {
  let x: Bool = not 42
  x
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Cannot apply 'not'"))
        );
    }

    #[test]
    fn effect_without_needs_warns() {
        let source = r#"
intent "test"
fn bad() -> String {
  time.now()
}
"#;
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("needs time"));
    }

    #[test]
    fn effect_with_needs_no_warning() {
        let source = r#"
intent "test"
fn good() -> String
  needs time
{
  time.now()
}
"#;
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(warnings.is_empty());
    }

    #[test]
    fn effect_in_route_without_needs_warns() {
        let source = r#"
intent "list"
route GET "/items" {
  db.query("items")
}
"#;
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("needs db"));
    }

    #[test]
    fn effect_in_route_with_needs_no_warning() {
        let source = r#"
intent "list"
route GET "/items" {
  needs db
  db.query("items")
}
"#;
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(warnings.is_empty());
    }

    #[test]
    fn method_call_on_int() {
        let source = r#"
intent "test"
fn test_fn() -> Int {
  let x: Int = 42
  x.length()
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.iter().any(|e| e.message.contains("has no methods")));
    }

    #[test]
    fn method_call_unknown_string_method() {
        let source = r#"
intent "test"
fn test_fn() -> String {
  let s: String = "hello"
  s.banana()
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("no method 'banana'"))
        );
    }

    #[test]
    fn method_call_valid_string_length() {
        let source = r#"
intent "test"
fn test_fn() -> Int {
  let s: String = "hello"
  s.length()
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn method_return_type_inferred() {
        // s.length() returns Int, assigning to String should error
        let source = r#"
intent "test"
fn test_fn() -> String {
  let s: String = "hello"
  let n: String = s.length()
  n
}
"#;
        let diags = parse_and_check(source);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.iter().any(|e| e.message.contains("Int")));
    }

    #[test]
    fn constraint_min_on_string_warns() {
        let source = "type Bad { name: String | min 1 }\n";
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("non-Int"));
    }

    #[test]
    fn constraint_min_greater_than_max_warns() {
        let source = "type Bad { age: Int | min 100 | max 10 }\n";
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("min (100) > max (10)"));
    }

    #[test]
    fn constraint_valid_no_warnings() {
        let source =
            "type Good { name: String | minlen 1 | maxlen 100, age: Int | min 0 | max 150 }\n";
        let diags = parse_and_check(source);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert!(warnings.is_empty());
    }

    #[test]
    fn display_list_map_generic_types() {
        assert_eq!(
            format!("{}", ResolvedType::List(Box::new(ResolvedType::Unknown))),
            "List"
        );
        assert_eq!(
            format!("{}", ResolvedType::List(Box::new(ResolvedType::Int))),
            "List<Int>"
        );
        assert_eq!(
            format!(
                "{}",
                ResolvedType::Map(
                    Box::new(ResolvedType::Unknown),
                    Box::new(ResolvedType::Unknown)
                )
            ),
            "Map"
        );
        assert_eq!(
            format!(
                "{}",
                ResolvedType::Map(Box::new(ResolvedType::String), Box::new(ResolvedType::Int))
            ),
            "Map<String, Int>"
        );
        assert_eq!(
            format!(
                "{}",
                ResolvedType::List(Box::new(ResolvedType::List(Box::new(ResolvedType::Int))))
            ),
            "List<List<Int>>"
        );
    }
}

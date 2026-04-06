/// PACT Abstract Syntax Tree types
use crate::lexer::token::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        mutable: bool,
        type_ann: TypeExpr,
        value: Expr,
        span: Option<Span>,
    },
    FnDecl {
        name: String,
        intent: Option<String>,
        params: Vec<Param>,
        return_type: Option<TypeExpr>,
        error_types: Vec<String>,
        effects: Vec<String>,
        body: Vec<Statement>,
        span: Option<Span>,
    },
    TypeDecl(TypeDecl),
    Use {
        path: Vec<String>,
    },
    Return {
        value: Option<Expr>,
        condition: Option<Expr>,
        span: Option<Span>,
    },
    Expression(Expr),
    TestBlock {
        name: String,
        body: Vec<Statement>,
    },
    Using {
        name: String,
        value: Expr,
    },
    Assert(Expr),
    Route {
        method: String,
        path: String,
        intent: String,
        effects: Vec<String>,
        body: Vec<Statement>,
    },
    Stream {
        method: String,
        path: String,
        intent: String,
        effects: Vec<String>,
        body: Vec<Statement>,
    },
    App {
        name: String,
        port: u16,
        db_url: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_ann: TypeExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeDecl {
    Struct {
        name: String,
        fields: Vec<Field>,
    },
    Union {
        name: String,
        variants: Vec<UnionVariant>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub type_ann: TypeExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionVariant {
    pub name: String,
    pub fields: Option<Vec<Field>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(StringExpr),
    BoolLiteral(bool),
    Nothing,
    Identifier(String),
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    DotShorthand(Vec<String>),
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    ErrorPropagation(Box<Expr>),
    FnCall {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Option<Span>,
    },
    Pipeline {
        source: Box<Expr>,
        steps: Vec<PipelineStep>,
    },
    If {
        condition: Box<Expr>,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Option<Span>,
    },
    Block(Vec<Statement>),
    StructLiteral {
        name: Option<String>,
        fields: Vec<StructField>,
    },
    Ensure(Box<Expr>),
    Is {
        expr: Box<Expr>,
        type_name: String,
    },
    Respond {
        status: Box<Expr>,
        body: Box<Expr>,
    },
    Send {
        body: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructField {
    Named { name: String, value: Expr },
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringExpr {
    Simple(String),
    Interpolated(Vec<StringPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(String),
    Wildcard,
    Literal(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStep {
    Filter {
        predicate: Expr,
    },
    Map {
        expr: Expr,
    },
    Sort {
        field: Expr,
        descending: bool,
    },
    GroupBy {
        field: Expr,
    },
    Take {
        kind: TakeKind,
        count: Expr,
    },
    Skip {
        count: Expr,
    },
    Each {
        expr: Expr,
    },
    FindFirst {
        predicate: Expr,
    },
    ExpectOne {
        error: Expr,
    },
    ExpectAny {
        error: Expr,
    },
    OrDefault {
        value: Expr,
    },
    Flatten,
    Unique,
    Count,
    Sum,
    ExpectSuccess,
    OnSuccess {
        body: Expr,
    },
    OnError {
        variant: String,
        guard: Option<Expr>,
        body: Expr,
    },
    ValidateAs {
        type_name: String,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TakeKind {
    First,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Generic {
        name: String,
        args: Vec<TypeExpr>,
    },
    Optional(Box<TypeExpr>),
    Result {
        ok: Box<TypeExpr>,
        errors: Vec<String>,
    },
}

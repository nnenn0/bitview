use crate::{error::Span, html::ElementSpec, resolve::FunctionId};

pub(crate) struct Def {
    pub(crate) name: String,
    pub(crate) params: Vec<(String, Span)>,
    pub(crate) body: Syntax,
    pub(crate) span: Span,
}

pub(crate) enum Syntax {
    Str(String),
    Name(String, Span),
    List(Vec<Syntax>, Span),
    Record(Vec<(String, Syntax, Span)>),
    Field(Box<Syntax>, String, Span),
    /// Only names can be called, so the callee is the name and its position.
    Call(String, Vec<Syntax>, Span),
    If(Box<Syntax>, Box<Syntax>, Box<Syntax>, Span),
}

pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) body: Expr,
    pub(crate) span: Span,
    /// Kept so that the recursion check and `Program::functions_used_by` need not walk the body.
    pub(crate) calls: Vec<(FunctionId, Span)>,
}

pub(crate) enum Expr {
    Str(String),
    /// The position of the parameter. Every call passes as many arguments as the function has
    /// parameters, so the argument is always there.
    Param(usize),
    List(Vec<Expr>, Span),
    Record(Vec<(String, Expr)>),
    Field(Box<Expr>, String, Span),
    Call(Callee, Vec<Expr>, Span),
    If(Box<Expr>, Box<Expr>, Box<Expr>, Span),
}

pub(crate) enum Callee {
    User(FunctionId),
    Element(&'static ElementSpec),
    Concat,
    /// The function is resolved when the program loads, so only the list is left as an argument.
    Map(FunctionId),
}

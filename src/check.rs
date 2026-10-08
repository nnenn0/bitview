//! Evaluation with types in place of values. There is no recursion, so following every call
//! terminates, and checking both sides of every `if` and the body of every `map` finds the errors
//! that rendering would meet only with some data.

use crate::{
    ast::{Callee, Expr, Function},
    content::Content,
    error::{Error, ErrorKind, Span},
    html::{self, ElementSpec},
    types::Ty,
};
use std::collections::HashMap;

pub(crate) struct Checker<'a> {
    functions: &'a [Function],
    /// A function called again with the same argument types has the same result.
    checked: HashMap<(usize, Vec<Ty>), Ty>,
}

impl<'a> Checker<'a> {
    pub(crate) fn new(functions: &'a [Function]) -> Self {
        Self {
            functions,
            checked: HashMap::new(),
        }
    }

    pub(crate) fn call(&mut self, function: usize, args: Vec<Ty>) -> Result<Ty, Error> {
        let key = (function, args);
        if let Some(result) = self.checked.get(&key) {
            return Ok(result.clone());
        }
        let functions = self.functions;
        let body = &functions
            .get(function)
            .ok_or_else(|| Error::new(ErrorKind::Name, "internal error: unknown function"))?
            .body;
        let result = self.check(body, &key.1)?;
        self.checked.insert(key, result.clone());
        Ok(result)
    }

    fn call_at(&mut self, function: usize, args: Vec<Ty>, site: &Span) -> Result<Ty, Error> {
        self.call(function, args).map_err(|error| {
            let name = self
                .functions
                .get(function)
                .map_or("?", |function| function.name.as_str());
            error.in_function(name, Some(site))
        })
    }

    fn check(&mut self, expr: &Expr, args: &[Ty]) -> Result<Ty, Error> {
        match expr {
            Expr::Str(_) => Ok(Ty::String),
            Expr::Param(position) => args
                .get(*position)
                .cloned()
                .ok_or_else(|| Error::new(ErrorKind::Name, "internal error: missing argument")),
            Expr::List(items, span) => {
                let mut list = Ty::Never;
                for item in items {
                    let item = self.check(item, args)?;
                    list = list.join(&item).ok_or_else(|| {
                        Error::at(
                            ErrorKind::Type,
                            span,
                            format!("the items of a list have different types: {list} and {item}"),
                        )
                    })?;
                }
                Ok(Ty::List(Box::new(list)))
            }
            Expr::Record(fields) => fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), self.check(value, args)?)))
                .collect::<Result<_, Error>>()
                .map(Ty::Record),
            Expr::Field(record, name, span) => {
                field(&self.check(record, args)?, name).map_err(|error| error.with_span(span))
            }
            Expr::If(condition, then, otherwise, span) => {
                let condition = self.check(condition, args)?;
                if !matches!(condition, Ty::Bool | Ty::Never) {
                    return Err(Error::at(
                        ErrorKind::Type,
                        span,
                        format!("if needs a Bool condition, but got {condition}"),
                    ));
                }
                let then = self.check(then, args)?;
                let otherwise = self.check(otherwise, args)?;
                then.join(&otherwise).ok_or_else(|| {
                    Error::at(
                        ErrorKind::Type,
                        span,
                        format!("the two sides of if have different types: {then} and {otherwise}"),
                    )
                })
            }
            Expr::Call(callee, call_args, span) => {
                let types = call_args
                    .iter()
                    .map(|arg| self.check(arg, args))
                    .collect::<Result<Vec<_>, _>>()?;
                match callee {
                    Callee::User(function) => self.call_at(*function, types, span),
                    Callee::Map(function) => self.map(*function, types, span),
                    Callee::Concat => concat(&types).map_err(|error| error.with_span(span)),
                    Callee::Element(spec) => {
                        element(spec, &types).map_err(|error| error.with_span(span))
                    }
                }
            }
        }
    }

    fn map(&mut self, function: usize, types: Vec<Ty>, span: &Span) -> Result<Ty, Error> {
        let item = match types.into_iter().next() {
            Some(Ty::List(item)) => *item,
            Some(Ty::Never) => Ty::Never,
            other => {
                return Err(Error::at(
                    ErrorKind::Type,
                    span,
                    format!("map needs a List, but got {}", other.unwrap_or(Ty::Never)),
                ));
            }
        };
        let result = self.call_at(function, vec![item], span)?;
        Ok(Ty::List(Box::new(result)))
    }
}

fn field(record: &Ty, name: &str) -> Result<Ty, Error> {
    match record {
        Ty::Record(fields) => fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| {
                let available = fields
                    .iter()
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Error::new(
                    ErrorKind::Field,
                    format!("unknown field {name:?} (fields: {available})"),
                )
            }),
        Ty::Never => Ok(Ty::Never),
        other => Err(Error::new(
            ErrorKind::Type,
            format!("cannot read field {name} of a {other}; only records have fields"),
        )),
    }
}

fn concat(types: &[Ty]) -> Result<Ty, Error> {
    match types
        .iter()
        .find(|ty| !matches!(ty, Ty::String | Ty::Never))
    {
        Some(other) => Err(Error::new(
            ErrorKind::Type,
            format!("concat joins Strings, but got {other}"),
        )),
        None => Ok(Ty::String),
    }
}

fn element(spec: &ElementSpec, types: &[Ty]) -> Result<Ty, Error> {
    let children = match types.split_first() {
        Some((Ty::Record(attrs), children)) => {
            for (name, ty) in attrs {
                html::check_attribute(spec, name)?;
                if !matches!(ty, Ty::String | Ty::Never) {
                    return Err(Error::new(
                        ErrorKind::Type,
                        format!(
                            "attribute {name} of <{}> needs a String, but got {ty}",
                            spec.name
                        ),
                    ));
                }
            }
            children
        }
        _ => types,
    };
    if spec.void && !children.is_empty() {
        return Err(Error::new(
            ErrorKind::Html,
            format!("<{}> is a void element and takes no children", spec.name),
        ));
    }
    let mut content = Content::default();
    for child in children {
        let Some(child_content) = child.content() else {
            let hint = if matches!(child, Ty::Record(_)) {
                "; attributes must be the first argument"
            } else {
                ""
            };
            return Err(Error::new(
                ErrorKind::Type,
                format!("a {child} cannot be a child of <{}>{hint}", spec.name),
            ));
        };
        content = content.union(child_content);
    }
    Ok(Ty::Html(html::place(spec, content)?))
}

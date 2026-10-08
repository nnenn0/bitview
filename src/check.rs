//! Evaluation with types in place of values. There is no recursion, so following every call
//! terminates, and checking both sides of every `if` and the body of every `map` finds the errors
//! that rendering would meet only with some data.

use crate::{
    ast::{Callee, Expr},
    content::Content,
    error::{Error, ErrorKind, Span},
    html::{self, ElementSpec},
    resolve::{FunctionId, Functions},
    types::Ty,
};
use std::collections::HashMap;

pub(crate) struct Checker<'a> {
    functions: &'a Functions,
    /// A function called again with the same argument types has the same result.
    checked: HashMap<(FunctionId, Vec<Ty>), Ty>,
}

impl<'a> Checker<'a> {
    pub(crate) fn new(functions: &'a Functions) -> Self {
        Self {
            functions,
            checked: HashMap::new(),
        }
    }

    pub(crate) fn call(&mut self, function: FunctionId, args: Vec<Ty>) -> Result<Ty, Error> {
        let key = (function, args);
        if let Some(result) = self.checked.get(&key) {
            return Ok(result.clone());
        }
        let functions = self.functions;
        let result = self.check(&functions.get(function).body, &key.1)?;
        self.checked.insert(key, result.clone());
        Ok(result)
    }

    fn call_at(&mut self, function: FunctionId, args: Vec<Ty>, site: &Span) -> Result<Ty, Error> {
        self.call(function, args)
            .map_err(|error| error.in_function(&self.functions.get(function).name, Some(site)))
    }

    fn check(&mut self, expr: &Expr, args: &[Ty]) -> Result<Ty, Error> {
        match expr {
            Expr::Str(_) => Ok(Ty::String),
            #[expect(clippy::indexing_slicing, reason = "every call passes every argument")]
            Expr::Param(position) => Ok(args[*position].clone()),
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
                    return Err(Error::not_a_condition(span, condition));
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

    fn map(&mut self, function: FunctionId, types: Vec<Ty>, span: &Span) -> Result<Ty, Error> {
        let item = match types.into_iter().next() {
            Some(Ty::List(item)) => *item,
            Some(Ty::Never) => Ty::Never,
            other => return Err(Error::not_a_list(span, other.unwrap_or(Ty::Never))),
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
            .ok_or_else(|| Error::unknown_field(name, fields.iter().map(|(key, _)| key.as_str()))),
        Ty::Never => Ok(Ty::Never),
        other => Err(Error::not_a_record(name, other)),
    }
}

fn concat(types: &[Ty]) -> Result<Ty, Error> {
    match types
        .iter()
        .find(|ty| !matches!(ty, Ty::String | Ty::Never))
    {
        Some(other) => Err(Error::not_a_string_to_concat(other)),
        None => Ok(Ty::String),
    }
}

fn element(spec: &ElementSpec, types: &[Ty]) -> Result<Ty, Error> {
    let children = match types.split_first() {
        Some((Ty::Record(attrs), children)) => {
            for (name, ty) in attrs {
                html::check_attribute(spec, name)?;
                if !matches!(ty, Ty::String | Ty::Never) {
                    return Err(Error::not_a_string_attribute(spec, name, ty));
                }
            }
            children
        }
        _ => types,
    };
    if spec.void && !children.is_empty() {
        return Err(Error::void_with_children(spec));
    }
    let mut content = Content::default();
    for child in children {
        let Some(child_content) = child.content() else {
            return Err(Error::not_a_child(
                spec,
                child,
                matches!(child, Ty::Record(_)),
            ));
        };
        content = content.union(child_content);
    }
    Ok(Ty::Html(html::place(spec, content)?))
}

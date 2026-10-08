//! Checks everything that can be known before evaluation: names, arities, `map` targets, and the
//! absence of recursion. Functions are not values and there are no local bindings besides
//! parameters, so every call target is fixed here.

use crate::{
    ast::{Callee, Def, Expr, Function, Syntax},
    error::{Error, ErrorKind, Span},
    html,
};
use std::collections::HashMap;

pub(crate) type Index = HashMap<String, usize>;

pub(crate) fn resolve(defs: Vec<Def>) -> Result<(Vec<Function>, Index), Error> {
    let mut index = Index::new();
    for (position, def) in defs.iter().enumerate() {
        if is_builtin(&def.name) {
            return Err(Error::at(
                ErrorKind::Name,
                &def.span,
                format!(
                    "{} is a built-in function and cannot be redefined",
                    def.name
                ),
            ));
        }
        if let Some(previous) = index.insert(def.name.clone(), position) {
            let previous = defs.get(previous).map_or_else(String::new, |def| {
                format!(" (first defined at {})", def.span)
            });
            return Err(Error::at(
                ErrorKind::Name,
                &def.span,
                format!("function {} is defined twice{previous}", def.name),
            ));
        }
    }
    let arities = defs.iter().map(|def| def.params.len()).collect::<Vec<_>>();
    let functions = defs
        .into_iter()
        .map(|def| resolve_function(def, &index, &arities))
        .collect::<Result<Vec<_>, _>>()?;
    check_recursion(&functions)?;
    Ok((functions, index))
}

fn is_builtin(name: &str) -> bool {
    matches!(name, "map" | "concat") || html::element_spec(name).is_some()
}

fn resolve_function(def: Def, index: &Index, arities: &[usize]) -> Result<Function, Error> {
    let mut params = Vec::with_capacity(def.params.len());
    for (name, span) in def.params {
        if params.contains(&name) {
            return Err(Error::at(
                ErrorKind::Name,
                &span,
                format!("parameter {name} is declared twice"),
            ));
        }
        params.push(name);
    }
    let scope = Scope {
        params: &params,
        index,
        arities,
    };
    let body = scope.expr(def.body)?;
    let mut calls = Vec::new();
    collect_calls(&body, &mut calls);
    Ok(Function {
        body,
        name: def.name,
        arity: params.len(),
        span: def.span,
        calls,
    })
}

struct Scope<'a> {
    params: &'a [String],
    index: &'a Index,
    arities: &'a [usize],
}

impl Scope<'_> {
    fn expr(&self, syntax: Syntax) -> Result<Expr, Error> {
        Ok(match syntax {
            Syntax::Str(text) => Expr::Str(text),
            Syntax::Name(name, span) => self.name(&name, &span)?,
            Syntax::List(items, span) => Expr::List(self.exprs(items)?, span),
            Syntax::Record(fields) => {
                let mut resolved: Vec<(String, Expr)> = Vec::with_capacity(fields.len());
                for (key, value, span) in fields {
                    if resolved.iter().any(|(existing, _)| *existing == key) {
                        return Err(Error::at(
                            ErrorKind::Name,
                            &span,
                            format!("field {key} is written twice in the record"),
                        ));
                    }
                    resolved.push((key, self.expr(value)?));
                }
                Expr::Record(resolved)
            }
            Syntax::Field(record, field, span) => {
                Expr::Field(Box::new(self.expr(*record)?), field, span)
            }
            Syntax::Call(name, args, span) => self.call(&name, args, span)?,
            Syntax::If(condition, then, otherwise, span) => Expr::If(
                Box::new(self.expr(*condition)?),
                Box::new(self.expr(*then)?),
                Box::new(self.expr(*otherwise)?),
                span,
            ),
        })
    }

    fn exprs(&self, items: Vec<Syntax>) -> Result<Vec<Expr>, Error> {
        items.into_iter().map(|item| self.expr(item)).collect()
    }

    fn name(&self, name: &str, span: &Span) -> Result<Expr, Error> {
        if let Some(position) = self.params.iter().position(|param| param == name) {
            return Ok(Expr::Param(position));
        }
        let message = if self.index.contains_key(name) || is_builtin(name) {
            format!("{name} is a function; functions are not values, so call it as {name}(...)")
        } else {
            format!("unknown name {name}")
        };
        Err(Error::at(ErrorKind::Name, span, message))
    }

    fn call(&self, name: &str, args: Vec<Syntax>, span: Span) -> Result<Expr, Error> {
        if self.params.iter().any(|param| param == name) {
            return Err(Error::at(
                ErrorKind::Name,
                &span,
                format!("{name} is a parameter, not a function"),
            ));
        }
        let callee = if let Some(&function) = self.index.get(name) {
            let arity = self.arities.get(function).copied().unwrap_or_default();
            if args.len() != arity {
                return Err(Error::at(
                    ErrorKind::Arity,
                    &span,
                    format!(
                        "{name} takes {arity} argument(s), but {} were given",
                        args.len()
                    ),
                ));
            }
            Callee::User(function)
        } else if name == "map" {
            return self.map(args, span);
        } else if name == "concat" {
            if args.is_empty() {
                return Err(Error::at(
                    ErrorKind::Arity,
                    &span,
                    "concat takes at least one argument",
                ));
            }
            Callee::Concat
        } else if let Some(spec) = html::element_spec(name) {
            Callee::Element(spec)
        } else {
            return Err(Error::at(
                ErrorKind::Name,
                &span,
                format!("unknown function {name}"),
            ));
        };
        Ok(Expr::Call(callee, self.exprs(args)?, span))
    }

    fn map(&self, args: Vec<Syntax>, span: Span) -> Result<Expr, Error> {
        let usage = "map takes a list and the name of a function with one parameter, as in map(items, item-view)";
        let [list, Syntax::Name(function, function_span)] =
            <[Syntax; 2]>::try_from(args).map_err(|_| Error::at(ErrorKind::Arity, &span, usage))?
        else {
            return Err(Error::at(ErrorKind::Type, &span, usage));
        };
        if self.params.contains(&function) {
            return Err(Error::at(
                ErrorKind::Name,
                &function_span,
                format!("{function} is a parameter, not a function"),
            ));
        }
        let Some(&index) = self.index.get(&function) else {
            let message = if is_builtin(&function) {
                format!("map can only apply functions defined in the templates, not {function}")
            } else {
                format!("unknown function {function}")
            };
            return Err(Error::at(ErrorKind::Name, &function_span, message));
        };
        if self.arities.get(index) != Some(&1) {
            return Err(Error::at(
                ErrorKind::Arity,
                &function_span,
                format!(
                    "map needs a function with one parameter, but {function} has a different number"
                ),
            ));
        }
        Ok(Expr::Call(Callee::Map(index), vec![self.expr(list)?], span))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unvisited,
    Visiting,
    Done,
}

fn check_recursion(functions: &[Function]) -> Result<(), Error> {
    let mut search = Search {
        functions,
        states: vec![State::Unvisited; functions.len()],
        path: Vec::new(),
    };
    for start in 0..functions.len() {
        search.visit(start)?;
    }
    Ok(())
}

fn collect_calls(expr: &Expr, calls: &mut Vec<(usize, Span)>) {
    match expr {
        Expr::Str(_) | Expr::Param(_) => {}
        Expr::List(items, _) => items.iter().for_each(|item| collect_calls(item, calls)),
        Expr::Record(fields) => fields
            .iter()
            .for_each(|(_, value)| collect_calls(value, calls)),
        Expr::Field(record, _, _) => collect_calls(record, calls),
        Expr::Call(callee, args, span) => {
            if let Callee::User(function) | Callee::Map(function) = callee {
                calls.push((*function, span.clone()));
            }
            for arg in args {
                collect_calls(arg, calls);
            }
        }
        Expr::If(condition, then, otherwise, _) => {
            collect_calls(condition, calls);
            collect_calls(then, calls);
            collect_calls(otherwise, calls);
        }
    }
}

struct Search<'a> {
    functions: &'a [Function],
    states: Vec<State>,
    path: Vec<usize>,
}

impl Search<'_> {
    fn visit(&mut self, function: usize) -> Result<(), Error> {
        if self.states.get(function) != Some(&State::Unvisited) {
            return Ok(());
        }
        self.set(function, State::Visiting);
        self.path.push(function);
        let functions = self.functions;
        let calls = functions
            .get(function)
            .map_or(&[][..], |function| function.calls.as_slice());
        for (callee, span) in calls {
            match self.states.get(*callee) {
                Some(State::Visiting) => return Err(self.cycle(*callee, span)),
                Some(State::Unvisited) => self.visit(*callee)?,
                _ => {}
            }
        }
        self.path.pop();
        self.set(function, State::Done);
        Ok(())
    }

    fn set(&mut self, function: usize, state: State) {
        if let Some(slot) = self.states.get_mut(function) {
            *slot = state;
        }
    }

    fn cycle(&self, callee: usize, span: &Span) -> Error {
        let names = self
            .path
            .iter()
            .skip_while(|&&function| function != callee)
            .chain([&callee])
            .filter_map(|&function| self.functions.get(function))
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>();
        Error::at(
            ErrorKind::Recursion,
            span,
            format!("recursive calls are not allowed: {}", names.join(" -> ")),
        )
    }
}

pub(crate) fn used_by(functions: &[Function], entry: usize) -> Vec<usize> {
    let mut visited = vec![false; functions.len()];
    let mut order = Vec::new();
    post_order(functions, entry, &mut visited, &mut order);
    order
}

fn post_order(
    functions: &[Function],
    function: usize,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) {
    match visited.get_mut(function) {
        Some(seen @ false) => *seen = true,
        _ => return,
    }
    for (callee, _) in functions
        .get(function)
        .map_or(&[][..], |function| function.calls.as_slice())
    {
        post_order(functions, *callee, visited, order);
    }
    order.push(function);
}

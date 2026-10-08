//! Checks everything that can be known before evaluation: names, arities, `map` targets, and the
//! absence of recursion. Functions are not values and there are no local bindings besides
//! parameters, so every call target is fixed here.

use crate::{
    ast::{Callee, Def, Expr, Function, Syntax},
    error::{Error, ErrorKind, Span},
    html,
};
use std::collections::{HashMap, HashSet, hash_map::Entry};

/// Names a function in [`Functions`]. Only this module issues ids, and only for the functions it
/// resolves, so every id names one.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FunctionId(usize);

/// The resolved functions, in the order of the sources, with their names.
pub(crate) struct Functions {
    list: Vec<Function>,
    index: HashMap<String, FunctionId>,
}

impl Functions {
    #[expect(
        clippy::indexing_slicing,
        reason = "every id is issued for a function in the list"
    )]
    pub(crate) fn get(&self, id: FunctionId) -> &Function {
        &self.list[id.0]
    }

    pub(crate) fn find(&self, name: &str) -> Option<FunctionId> {
        self.index.get(name).copied()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (FunctionId, &Function)> {
        self.list
            .iter()
            .enumerate()
            .map(|(position, function)| (FunctionId(position), function))
    }

    /// The functions that `entry` may call, including itself, each after the functions it calls.
    pub(crate) fn used_by(&self, entry: FunctionId) -> Vec<FunctionId> {
        let mut order = Vec::new();
        self.post_order(entry, &mut HashSet::new(), &mut order);
        order
    }

    fn post_order(
        &self,
        id: FunctionId,
        visited: &mut HashSet<FunctionId>,
        order: &mut Vec<FunctionId>,
    ) {
        if !visited.insert(id) {
            return;
        }
        for (callee, _) in &self.get(id).calls {
            self.post_order(*callee, visited, order);
        }
        order.push(id);
    }
}

/// What resolving needs to know about a function before its body is resolved.
struct Signature {
    id: FunctionId,
    arity: usize,
    span: Span,
}

pub(crate) fn resolve(defs: Vec<Def>) -> Result<Functions, Error> {
    let mut signatures: HashMap<String, Signature> = HashMap::new();
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
        match signatures.entry(def.name.clone()) {
            Entry::Occupied(first) => {
                return Err(Error::at(
                    ErrorKind::Name,
                    &def.span,
                    format!(
                        "function {} is defined twice (first defined at {})",
                        def.name,
                        first.get().span
                    ),
                ));
            }
            Entry::Vacant(slot) => {
                slot.insert(Signature {
                    id: FunctionId(position),
                    arity: def.params.len(),
                    span: def.span.clone(),
                });
            }
        }
    }
    let list = defs
        .into_iter()
        .map(|def| resolve_function(def, &signatures))
        .collect::<Result<Vec<_>, _>>()?;
    let functions = Functions {
        list,
        index: signatures
            .into_iter()
            .map(|(name, signature)| (name, signature.id))
            .collect(),
    };
    check_recursion(&functions)?;
    Ok(functions)
}

fn is_builtin(name: &str) -> bool {
    matches!(name, "map" | "concat") || html::element_spec(name).is_some()
}

fn resolve_function(def: Def, signatures: &HashMap<String, Signature>) -> Result<Function, Error> {
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
        signatures,
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
    signatures: &'a HashMap<String, Signature>,
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
        let message = if self.signatures.contains_key(name) || is_builtin(name) {
            format!("{name} is a function; functions are not values, so call it as ({name} ...)")
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
        let callee = if let Some(&Signature { id, arity, .. }) = self.signatures.get(name) {
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
            Callee::User(id)
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
        let usage = "map takes a list and the name of a function with one parameter, as in (map items item-view)";
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
        let Some(&Signature { id, arity, .. }) = self.signatures.get(&function) else {
            let message = if is_builtin(&function) {
                format!("map can only apply functions defined in the templates, not {function}")
            } else {
                format!("unknown function {function}")
            };
            return Err(Error::at(ErrorKind::Name, &function_span, message));
        };
        if arity != 1 {
            return Err(Error::at(
                ErrorKind::Arity,
                &function_span,
                format!(
                    "map needs a function with one parameter, but {function} has a different number"
                ),
            ));
        }
        Ok(Expr::Call(Callee::Map(id), vec![self.expr(list)?], span))
    }
}

fn check_recursion(functions: &Functions) -> Result<(), Error> {
    let mut search = Search {
        functions,
        done: HashSet::new(),
        path: Vec::new(),
    };
    for (id, _) in functions.iter() {
        search.visit(id)?;
    }
    Ok(())
}

fn collect_calls(expr: &Expr, calls: &mut Vec<(FunctionId, Span)>) {
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
    functions: &'a Functions,
    done: HashSet<FunctionId>,
    /// The functions being visited, each called by the one before it.
    path: Vec<FunctionId>,
}

impl Search<'_> {
    fn visit(&mut self, id: FunctionId) -> Result<(), Error> {
        if self.done.contains(&id) {
            return Ok(());
        }
        self.path.push(id);
        let functions = self.functions;
        for (callee, span) in &functions.get(id).calls {
            if self.path.contains(callee) {
                return Err(self.cycle(*callee, span));
            }
            self.visit(*callee)?;
        }
        self.path.pop();
        self.done.insert(id);
        Ok(())
    }

    fn cycle(&self, callee: FunctionId, span: &Span) -> Error {
        let names = self
            .path
            .iter()
            .skip_while(|&&id| id != callee)
            .chain([&callee])
            .map(|&id| self.functions.get(id).name.as_str())
            .collect::<Vec<_>>();
        Error::at(
            ErrorKind::Recursion,
            span,
            format!("recursive calls are not allowed: {}", names.join(" -> ")),
        )
    }
}

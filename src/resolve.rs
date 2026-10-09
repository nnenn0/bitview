//! Checks everything that can be known before evaluation: names, arities, `map` targets, and the
//! absence of recursion. Functions are not values and there are no local bindings besides
//! parameters, so every call target is fixed here.

use crate::{
    Type,
    ast::{Callee, Def, Expr, Function, Param, Syntax, TypeSyntax},
    error::{Error, ErrorKind, Span},
    html,
    types::{self, Ty},
};
use std::collections::{HashMap, HashSet};

/// Names a function in [`Functions`]. Only this module issues ids, and only for the functions it
/// resolves, so every id names one.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FunctionId(usize);

/// The resolved functions, in the order of the sources. Only the public ones can be found by
/// name; a private function is reached only through the calls in its own source.
pub(crate) struct Functions {
    list: Vec<Function>,
    public: HashMap<String, FunctionId>,
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
        self.public.get(name).copied()
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
    /// The position of the source that defines it.
    source: usize,
    span: Span,
}

/// The functions each source can call by name.
struct Names {
    public: HashMap<String, Signature>,
    /// The private functions of each source, by the position of the source.
    private: Vec<HashMap<String, Signature>>,
}

impl Names {
    /// What `name` calls in `source`. A private function hides a public one of the same name
    /// defined in another source, so adding a public function elsewhere never changes what a
    /// source calls.
    fn get(&self, source: usize, name: &str) -> Option<&Signature> {
        self.private
            .get(source)
            .and_then(|private| private.get(name))
            .or_else(|| self.public.get(name))
    }

    /// A private function `name` of some other source, for an error that says why it is out of
    /// reach.
    fn private_elsewhere(&self, name: &str) -> Option<&Signature> {
        self.private.iter().find_map(|private| private.get(name))
    }
}

/// Resolves the definitions of each source, given in the order of the sources.
pub(crate) fn resolve(
    sources: Vec<Vec<Def>>,
    host_types: &[(&str, Type)],
) -> Result<Functions, Error> {
    let types = TypeNames::new(host_types)?;
    let mut names = Names {
        public: HashMap::new(),
        private: Vec::with_capacity(sources.len()),
    };
    let mut position = 0;
    for (source, defs) in sources.iter().enumerate() {
        let mut private = HashMap::new();
        for def in defs {
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
            // Public functions share one namespace; a private one shares only its source's.
            let first = private.get(&def.name).or_else(|| {
                names
                    .public
                    .get(&def.name)
                    .filter(|first: &&Signature| def.public || first.source == source)
            });
            if let Some(first) = first {
                return Err(Error::at(
                    ErrorKind::Name,
                    &def.span,
                    format!(
                        "function {} is defined twice (first defined at {})",
                        def.name, first.span
                    ),
                ));
            }
            let signature = Signature {
                id: FunctionId(position),
                arity: def.params.len(),
                source,
                span: def.span.clone(),
            };
            position += 1;
            if def.public {
                names.public.insert(def.name.clone(), signature);
            } else {
                private.insert(def.name.clone(), signature);
            }
        }
        names.private.push(private);
    }
    let list = sources
        .into_iter()
        .enumerate()
        .flat_map(|(source, defs)| defs.into_iter().map(move |def| (source, def)))
        .map(|(source, def)| resolve_function(def, &names, &types, source))
        .collect::<Result<Vec<_>, _>>()?;
    let functions = Functions {
        list,
        public: names
            .public
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

/// The types that parameters can name: the built-in ones, then the host's.
struct TypeNames(Vec<(String, Ty)>);

impl TypeNames {
    fn new(host: &[(&str, Type)]) -> Result<Self, Error> {
        let mut names = types::BUILT_IN
            .iter()
            .map(|(name, ty)| ((*name).to_owned(), Ty::from(ty)))
            .collect::<Vec<_>>();
        for (name, ty) in host {
            let mut characters = name.chars();
            let well_formed = characters
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
                && characters.all(|character| character.is_ascii_alphanumeric());
            if !well_formed {
                return Err(Error::new(
                    ErrorKind::Name,
                    format!(
                        "type name {name:?} must be an uppercase letter followed by letters and digits, as in Entry"
                    ),
                ));
            }
            if names.iter().any(|(existing, _)| existing == name) {
                return Err(Error::new(
                    ErrorKind::Name,
                    format!("type {name} is defined twice or is built in"),
                ));
            }
            names.push(((*name).to_owned(), Ty::from(ty)));
        }
        Ok(Self(names))
    }

    fn resolve(&self, syntax: TypeSyntax) -> Result<Ty, Error> {
        Ok(match syntax {
            TypeSyntax::Name(name, span) => self
                .0
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, ty)| ty.clone())
                .ok_or_else(|| {
                    let known = self
                        .0
                        .iter()
                        .map(|(known, _)| known.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Error::at(
                        ErrorKind::Name,
                        &span,
                        format!("unknown type {name} (types: {known})"),
                    )
                })?,
            TypeSyntax::List(item) => Ty::List(Box::new(self.resolve(*item)?)),
            TypeSyntax::Record(fields) => Ty::Record(
                fields
                    .into_iter()
                    .map(|(key, ty)| Ok((key, self.resolve(ty)?)))
                    .collect::<Result<_, Error>>()?,
            ),
        })
    }
}

fn resolve_function(
    def: Def,
    names: &Names,
    types: &TypeNames,
    source: usize,
) -> Result<Function, Error> {
    let mut param_names = Vec::with_capacity(def.params.len());
    let mut params = Vec::with_capacity(def.params.len());
    for param in def.params {
        if param_names.contains(&param.name) {
            return Err(Error::at(
                ErrorKind::Name,
                &param.span,
                format!("parameter {} is declared twice", param.name),
            ));
        }
        param_names.push(param.name.clone());
        params.push(Param {
            name: param.name,
            ty: types.resolve(param.ty)?,
        });
    }
    let scope = Scope {
        params: &param_names,
        names,
        source,
    };
    let body = scope.expr(def.body)?;
    let mut calls = Vec::new();
    collect_calls(&body, &mut calls);
    Ok(Function {
        body,
        name: def.name,
        public: def.public,
        params,
        span: def.span,
        calls,
    })
}

struct Scope<'a> {
    params: &'a [String],
    names: &'a Names,
    source: usize,
}

impl Scope<'_> {
    fn function(&self, name: &str) -> Option<&Signature> {
        self.names.get(self.source, name)
    }

    fn unknown_function(&self, name: &str, span: &Span) -> Error {
        let message = match self.names.private_elsewhere(name) {
            Some(private) => format!(
                "{name} is defined with defn- in {}, so only that source can call it",
                private.span.source()
            ),
            None => format!("unknown function {name}"),
        };
        Error::at(ErrorKind::Name, span, message)
    }

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
        let message = if self.function(name).is_some() || is_builtin(name) {
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
        let callee = if let Some(&Signature { id, arity, .. }) = self.function(name) {
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
            return Err(self.unknown_function(name, &span));
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
        let Some(&Signature { id, arity, .. }) = self.function(&function) else {
            if is_builtin(&function) {
                return Err(Error::at(
                    ErrorKind::Name,
                    &function_span,
                    format!(
                        "map can only apply functions defined in the templates, not {function}"
                    ),
                ));
            }
            return Err(self.unknown_function(&function, &function_span));
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

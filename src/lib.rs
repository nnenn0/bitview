//! bitview: a small, pure functional language that builds HTML from data.
//!
//! A program is a set of functions written in one or more sources. The host reads the sources,
//! passes them to [`Program::parse`], and renders a page by calling one function with one value:
//!
//! ```
//! use bitview::{Html, Program, Source, Value};
//!
//! let program = Program::parse(&[Source {
//!     name: "page.bv",
//!     text: r#"fn page(ctx) => html(body(h1(ctx.title), ctx.content))"#,
//! }])?;
//! let ctx = Value::record([
//!     ("title", Value::from("<Hello>")),
//!     ("content", Value::from(Html::text("Body"))),
//! ]);
//! let page = program.render("page", ctx)?.to_document()?;
//! assert_eq!(page, "<!doctype html><html><body><h1>&lt;Hello&gt;</h1>Body</body></html>");
//! # Ok::<(), bitview::Error>(())
//! ```
//!
//! Evaluation has no input or output and always terminates: there is no recursion.

mod ast;
mod check;
mod content;
mod error;
mod eval;
mod html;
mod lexer;
mod parser;
mod resolve;
mod serializer;
mod types;
mod value;

pub use error::{Error, ErrorKind, Frame, Span};
pub use html::Html;
pub use types::{HtmlType, Type};
pub use value::Value;

use ast::Function;
use resolve::Index;
use std::sync::Arc;

/// A template source and the name errors use for it, such as `views/pages/page.bv`.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// Parsed and checked functions from all sources, sharing one namespace.
pub struct Program {
    functions: Vec<Function>,
    index: Index,
}

impl Program {
    /// Parses all sources and checks names, arities, and the absence of recursion.
    ///
    /// # Errors
    ///
    /// Returns the first syntax, name, arity, or recursion error, with its position.
    pub fn parse(sources: &[Source<'_>]) -> Result<Self, Error> {
        let mut defs = Vec::new();
        for source in sources {
            let name: Arc<str> = Arc::from(source.name);
            defs.extend(parser::parse(lexer::tokenize(&name, source.text)?)?);
        }
        let (functions, index) = resolve::resolve(defs)?;
        Ok(Self { functions, index })
    }

    /// Whether `name` is a function with one parameter, which [`Program::render`] can call.
    #[must_use]
    pub fn has_entry(&self, name: &str) -> bool {
        self.function(name)
            .is_some_and(|(_, function)| function.arity == 1)
    }

    /// Where the function `name` is defined, which lets a host tie functions to the files that
    /// define them.
    #[must_use]
    pub fn defined_at(&self, name: &str) -> Option<&Span> {
        self.function(name).map(|(_, function)| &function.span)
    }

    /// Checks that each entry renders every value of its type without a type or field error: every
    /// field it may read exists, every value fits where it is used, and the result is Html. Both
    /// sides of every `if` and the function of every `map` are checked, so an error that rendering
    /// would meet only with some data is found here. Errors that depend on the values themselves,
    /// such as a URL with a disallowed scheme, remain for rendering.
    ///
    /// A function is checked only through the calls that reach it, so a function that no entry may
    /// call is an error: nothing could tell whether it fits the data it is meant for.
    ///
    /// # Errors
    ///
    /// Returns the first error, with its position and the functions that were being checked.
    pub fn check(&self, entries: &[(&str, Type)]) -> Result<(), Error> {
        let mut checker = check::Checker::new(&self.functions);
        let mut reached = vec![false; self.functions.len()];
        for (entry, ctx) in entries {
            let (index, function) = self.entry(entry)?;
            let result = checker
                .call(index, vec![types::Ty::from(ctx)])
                .map_err(|error| error.in_function(entry, None))?;
            if !result.is_html() {
                return Err(Error::at(
                    ErrorKind::Type,
                    &function.span,
                    format!("{entry} must return Html, but returns {result}"),
                ));
            }
            for used in resolve::used_by(&self.functions, index) {
                if let Some(reached) = reached.get_mut(used) {
                    *reached = true;
                }
            }
        }
        match self
            .functions
            .iter()
            .zip(reached)
            .find(|(_, reached)| !reached)
        {
            Some((function, _)) => Err(Error::at(
                ErrorKind::Name,
                &function.span,
                format!(
                    "function {} is not called from {}, so it cannot be checked; call it or remove it",
                    function.name,
                    entries
                        .iter()
                        .map(|(entry, _)| *entry)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
            None => Ok(()),
        }
    }

    /// Calls the function `entry` with `ctx` and returns the HTML it builds.
    ///
    /// # Errors
    ///
    /// Fails if `entry` is not a function with one parameter, if evaluation fails, or if the
    /// function returns something that does not stand for HTML. Evaluation errors carry the
    /// functions that were running.
    pub fn render(&self, entry: &str, ctx: Value) -> Result<Html, Error> {
        let (index, function) = self.entry(entry)?;
        let evaluator = eval::Evaluator {
            functions: &self.functions,
        };
        let result = evaluator
            .call(index, &[ctx])
            .map_err(|error| error.in_function(entry, None))?;
        eval::into_html([result]).map_err(|other| {
            Error::at(
                ErrorKind::Type,
                &function.span,
                format!(
                    "{entry} must return Html, but returned a {}",
                    other.type_name()
                ),
            )
        })
    }

    fn entry(&self, name: &str) -> Result<(usize, &Function), Error> {
        let (index, function) = self.function(name).ok_or_else(|| {
            Error::new(
                ErrorKind::Name,
                format!("the templates define no function {name}"),
            )
        })?;
        if function.arity != 1 {
            return Err(Error::at(
                ErrorKind::Arity,
                &function.span,
                format!("{name} must take exactly one parameter to render a page"),
            ));
        }
        Ok((index, function))
    }

    /// The functions that rendering `entry` may call, including `entry` itself. Each function
    /// comes after the functions it calls, and functions called side by side keep the order of
    /// the calls in the source. Both branches of every `if` count, so the list depends only on
    /// the program, not on the data. Returns `None` if there is no function `entry`.
    ///
    /// A host can use it to attach resources to functions, such as one CSS file per function,
    /// in an order where the callers come last.
    #[must_use]
    pub fn functions_used_by(&self, entry: &str) -> Option<Vec<&str>> {
        let (index, _) = self.function(entry)?;
        Some(
            resolve::used_by(&self.functions, index)
                .into_iter()
                .filter_map(|function| self.functions.get(function))
                .map(|function| function.name.as_str())
                .collect(),
        )
    }

    fn function(&self, name: &str) -> Option<(usize, &Function)> {
        let index = *self.index.get(name)?;
        Some((index, self.functions.get(index)?))
    }
}

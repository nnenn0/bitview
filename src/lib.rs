//! bitview: a small, pure functional language that builds HTML from data.
//!
//! A program is a set of functions written in one or more sources. The host reads the sources,
//! passes them to [`Program::parse`] with the types it names for the templates, and renders a
//! page by calling one function with one value:
//!
//! ```
//! use bitview::{Html, HtmlType, Program, Source, Type, Value};
//!
//! let page_type = Type::record([
//!     ("title", Type::String),
//!     ("content", Type::Html(HtmlType::Flow)),
//! ]);
//! let program = Program::parse(
//!     &[Source {
//!         name: "page.bv",
//!         text: r#"(defn page [ctx Page] (html (body (h1 ctx.title) ctx.content)))"#,
//!     }],
//!     &[("Page", page_type.clone())],
//! )?;
//! program.check(&[("page", page_type)])?;
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
pub use types::{HtmlType, Type, declare_types};
pub use value::Value;

use ast::Function;
use resolve::{FunctionId, Functions};
use std::sync::Arc;

/// A template source and the name errors use for it, such as `views/pages/page.bv`.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// Parsed and checked functions from all sources.
///
/// Functions defined with `defn` share one namespace across the sources, and the host calls them
/// by name. A function defined with `defn-` is private to its source: only that source can call
/// it, a private function of another source may share its name, and within its source it hides
/// a public function of the same name. The host cannot name a private function.
pub struct Program {
    functions: Functions,
}

impl Program {
    /// Parses all sources and checks them: names, arities, the absence of recursion, and the
    /// types of every function. Each parameter has a type, written with the built-in types and
    /// `types`, the host's named types, such as `("Entry", Type::record(…))`. A function is checked
    /// once, with its parameters' types in place of values: every field it reads exists, every
    /// value fits where it is used, and every call passes arguments that fit. Both sides of every
    /// `if` and the function of every `map` are checked, so an error that rendering would meet only
    /// with some data is found here. Errors that depend on the values themselves, such as a URL
    /// with a disallowed scheme, remain for rendering.
    ///
    /// # Errors
    ///
    /// Returns the first error, with its position and the function that has it.
    pub fn parse(sources: &[Source<'_>], types: &[(&str, Type)]) -> Result<Self, Error> {
        let defs = sources
            .iter()
            .map(|source| {
                let name: Arc<str> = Arc::from(source.name);
                parser::parse(lexer::tokenize(&name, source.text)?)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let functions = resolve::resolve(defs, types)?;
        check::Checker::new(&functions).check_all()?;
        Ok(Self { functions })
    }

    /// Whether `name` is a public function with one parameter, which [`Program::render`] can
    /// call.
    #[must_use]
    pub fn has_entry(&self, name: &str) -> bool {
        self.function(name)
            .is_some_and(|function| function.params.len() == 1)
    }

    /// Where the public function `name` is defined, which lets a host tie functions to the files
    /// that define them.
    #[must_use]
    pub fn defined_at(&self, name: &str) -> Option<&Span> {
        self.function(name).map(|function| &function.span)
    }

    /// Checks that the host can render each entry with values of the type it gives: the type
    /// fits the entry's parameter, and the entry returns Html.
    ///
    /// # Errors
    ///
    /// Returns the first entry that is missing, takes a type that the given one does not fit, or
    /// does not return Html.
    pub fn check(&self, entries: &[(&str, Type)]) -> Result<(), Error> {
        let mut checker = check::Checker::new(&self.functions);
        for (entry, ctx) in entries {
            let (id, function) = self.entry(entry)?;
            for param in &function.params {
                types::Ty::from(ctx)
                    .fit(&param.ty, &param.name)
                    .map_err(|error| error.with_span(&function.span).in_function(entry, None))?;
            }
            let result = checker.result(id)?;
            if !result.is_html() {
                return Err(Error::not_html(entry, &function.span, result));
            }
        }
        Ok(())
    }

    /// Calls the public function `entry` with `ctx` and returns the HTML it builds.
    ///
    /// # Errors
    ///
    /// Fails if `entry` is not a public function with one parameter, if evaluation fails, or if the
    /// function returns something that does not stand for HTML. Evaluation errors carry the
    /// functions that were running.
    pub fn render(&self, entry: &str, ctx: Value) -> Result<Html, Error> {
        let (id, function) = self.entry(entry)?;
        let result = eval::Evaluator::new(&self.functions)
            .call(id, vec![ctx])
            .map_err(|error| error.with_span(&function.span).in_function(entry, None))?;
        eval::into_html([result])
            .map_err(|other| Error::not_html(entry, &function.span, other.type_name()))
    }

    fn entry(&self, name: &str) -> Result<(FunctionId, &Function), Error> {
        let id = self.functions.find(name).ok_or_else(|| {
            let private = self
                .functions
                .iter()
                .find(|(_, function)| function.name == name);
            match private {
                Some((_, function)) => Error::at(
                    ErrorKind::Name,
                    &function.span,
                    format!("{name} is defined with defn-, so only its source can call it"),
                ),
                None => Error::new(
                    ErrorKind::Name,
                    format!("the templates define no function {name}"),
                ),
            }
        })?;
        let function = self.functions.get(id);
        if function.params.len() != 1 {
            return Err(Error::at(
                ErrorKind::Arity,
                &function.span,
                format!("{name} must take exactly one parameter to render a page"),
            ));
        }
        Ok((id, function))
    }

    /// The public functions that rendering `entry` may call, including `entry` itself. Each
    /// function comes after the functions it calls, and functions called side by side keep the
    /// order of the calls in the source. Calls through private functions count, but the private
    /// functions themselves are left out: their names are not unique, and they belong to the
    /// public functions of their source. Both branches of every `if` count, so the list depends
    /// only on the program, not on the data. Returns `None` if there is no public function
    /// `entry`.
    ///
    /// A host can use it to attach resources to functions, such as one CSS file per function,
    /// in an order where the callers come last.
    #[must_use]
    pub fn functions_used_by(&self, entry: &str) -> Option<Vec<&str>> {
        let entry = self.functions.find(entry)?;
        Some(
            self.functions
                .used_by(entry)
                .into_iter()
                .map(|id| self.functions.get(id))
                .filter(|function| function.public)
                .map(|function| function.name.as_str())
                .collect(),
        )
    }

    fn function(&self, name: &str) -> Option<&Function> {
        self.functions.find(name).map(|id| self.functions.get(id))
    }
}

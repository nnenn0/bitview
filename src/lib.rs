//! bitview: a small, pure functional language that builds HTML from data.
//!
//! A program is a set of functions written in one or more sources. The host reads the sources,
//! passes them to [`Program::parse`], and renders a page by calling one function with one value:
//!
//! ```
//! use bitview::{Html, Program, Source, Value};
//!
//! let program = Program::parse(&[Source {
//!     name: "page.bitview",
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
mod error;
mod eval;
mod html;
mod lexer;
mod parser;
mod resolve;
mod serializer;
mod value;

pub use error::{Error, ErrorKind, Frame, Span};
pub use html::Html;
pub use value::Value;

use ast::Function;
use resolve::Index;
use std::sync::Arc;

/// A template source and the name errors use for it, such as `templates/page.bitview`.
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
        eval::into_html(result).map_err(|other| {
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

    fn function(&self, name: &str) -> Option<(usize, &Function)> {
        let index = *self.index.get(name)?;
        Some((index, self.functions.get(index)?))
    }
}

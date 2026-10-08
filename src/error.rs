use std::{fmt, sync::Arc};

/// A position in a template source, counted in characters from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    source: Arc<str>,
    line: u32,
    column: u32,
}

impl Span {
    pub(crate) fn new(source: Arc<str>, line: u32, column: u32) -> Self {
        Self {
            source,
            line,
            column,
        }
    }

    /// The name the source was given in [`crate::Source`].
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn line(&self) -> u32 {
        self.line
    }

    #[must_use]
    pub fn column(&self) -> u32 {
        self.column
    }
}

impl fmt::Display for Span {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}:{}", self.source, self.line, self.column)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The text is not a valid program.
    Syntax,
    /// A name is unknown, duplicated, or used as the wrong kind of thing.
    Name,
    /// A call has the wrong number of arguments.
    Arity,
    /// Functions call each other in a cycle.
    Recursion,
    /// A value has the wrong type.
    Type,
    /// A record has no field of the given name.
    Field,
    /// HTML that the standard library refuses to build or serialize.
    Html,
}

/// A function that was running when an evaluation error occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    function: String,
    call_site: Option<Span>,
}

impl Frame {
    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }

    /// Where the function was called, or `None` for the entry function.
    #[must_use]
    pub fn call_site(&self) -> Option<&Span> {
        self.call_site.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    span: Option<Span>,
    trace: Vec<Frame>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            span: None,
            trace: Vec::new(),
        }
    }

    pub(crate) fn at(kind: ErrorKind, span: &Span, message: impl Into<String>) -> Self {
        Self::new(kind, message).with_span(span)
    }

    /// The innermost position is the most precise, so outer calls keep it.
    #[must_use]
    pub(crate) fn with_span(mut self, span: &Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span.clone());
        }
        self
    }

    #[must_use]
    pub(crate) fn in_function(mut self, function: &str, call_site: Option<&Span>) -> Self {
        self.trace.push(Frame {
            function: function.to_owned(),
            call_site: call_site.cloned(),
        });
        self
    }

    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn span(&self) -> Option<&Span> {
        self.span.as_ref()
    }

    /// The functions that were running, innermost first. Empty for errors found before evaluation.
    #[must_use]
    pub fn trace(&self) -> &[Frame] {
        &self.trace
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = &self.span {
            write!(formatter, "{span}: ")?;
        }
        formatter.write_str(&self.message)?;
        for frame in &self.trace {
            write!(formatter, "\n  in {}", frame.function)?;
            if let Some(site) = &frame.call_site {
                write!(formatter, " (called at {site})")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

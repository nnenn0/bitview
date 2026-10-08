use crate::{
    Value,
    content::{Category, Content, FLOW, PHRASING},
    error::{Error, ErrorKind},
};
use std::fmt;

/// The type of a value a host passes to a template, for [`crate::Program::check`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    String,
    Bool,
    Html(HtmlType),
    List(Box<Type>),
    Record(Vec<(String, Type)>),
}

/// Where HTML that a host passes may go, so that templates can be checked to place it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HtmlType {
    /// Text and elements that go in a `<body>`, such as paragraphs, lists, and links.
    Flow,
    /// Text and elements that go in a line of text, such as `<span>`, `<a>`, and `<em>`.
    Phrasing,
    /// Elements that go in a `<head>`, such as `<meta>`, styles, and JSON.
    Metadata,
}

impl HtmlType {
    fn content(self) -> Content {
        match self {
            Self::Flow => Content::any_of(FLOW),
            Self::Phrasing => Content::any_of(PHRASING),
            Self::Metadata => Content::any_of(&[Category::Metadata]),
        }
    }
}

impl Type {
    #[must_use]
    pub fn list(item: Type) -> Self {
        Self::List(Box::new(item))
    }

    #[must_use]
    pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Type)>) -> Self {
        Self::Record(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    /// Whether `value` has exactly this type: records have the same fields, each once, and every item of a
    /// list has the item type. A host that checked its views with this type can confirm the values
    /// it renders with, so that the check holds for them.
    ///
    /// # Errors
    ///
    /// Names the first part of `value` that differs, by its path from the top.
    pub fn validate(&self, value: &Value) -> Result<(), Error> {
        validate(self, value, "value")
    }
}

fn validate(ty: &Type, value: &Value, path: &str) -> Result<(), Error> {
    let mismatch = || {
        Error::new(
            ErrorKind::Type,
            format!(
                "{path} is a {}, but the type is {}",
                value.type_name(),
                Ty::from(ty)
            ),
        )
    };
    match (ty, value) {
        (Type::String, Value::String(_)) | (Type::Bool, Value::Bool(_)) => Ok(()),
        (Type::Html(html), Value::Html(value)) => {
            if value.content().fits(html.content()) {
                Ok(())
            } else {
                Err(Error::new(
                    ErrorKind::Type,
                    format!(
                        "{path} is Html with {}, but the type is {html:?} Html",
                        value.content().description()
                    ),
                ))
            }
        }
        (Type::List(item), Value::List(items)) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, value)| validate(item, value, &format!("{path}[{index}]"))),
        (Type::Record(types), Value::Record(values)) => {
            // Templates read the first of repeated fields, so a later one would go unchecked.
            if let Some((name, _)) = values
                .iter()
                .enumerate()
                .find(|(position, (name, _))| {
                    values
                        .iter()
                        .take(*position)
                        .any(|(earlier, _)| earlier == name)
                })
                .map(|(_, field)| field)
            {
                return Err(Error::new(
                    ErrorKind::Field,
                    format!("{path}.{name} is given twice"),
                ));
            }
            if let Some((name, _)) = values
                .iter()
                .find(|(name, _)| !types.iter().any(|(key, _)| key == name))
            {
                return Err(Error::new(
                    ErrorKind::Field,
                    format!("{path}.{name} is not a field of the type"),
                ));
            }
            types.iter().try_for_each(|(name, ty)| {
                let value = values
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        Error::new(ErrorKind::Field, format!("{path}.{name} is missing"))
                    })?;
                validate(ty, value, &format!("{path}.{name}"))
            })
        }
        _ => Err(mismatch()),
    }
}

/// Types while checking. `Never` is the item type of an empty list literal: no value has it, so it
/// is a subtype of every type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Ty {
    Never,
    String,
    Bool,
    Html(Content),
    List(Box<Ty>),
    Record(Vec<(String, Ty)>),
}

impl From<&Type> for Ty {
    fn from(value: &Type) -> Self {
        match value {
            Type::String => Self::String,
            Type::Bool => Self::Bool,
            Type::Html(html) => Self::Html(html.content()),
            Type::List(item) => Self::List(Box::new(Self::from(item.as_ref()))),
            Type::Record(fields) => Self::Record(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from(value)))
                    .collect(),
            ),
        }
    }
}

impl Ty {
    pub(crate) fn join(&self, other: &Self) -> Option<Self> {
        let structural = match (self, other) {
            (left, right) if left == right => Some(left.clone()),
            (Self::Never, other) | (other, Self::Never) => Some(other.clone()),
            (Self::List(left), Self::List(right)) => {
                left.join(right).map(|item| Self::List(Box::new(item)))
            }
            (Self::Record(left), Self::Record(right)) if left.len() == right.len() => left
                .iter()
                .map(|(key, value)| {
                    let (_, other) = right.iter().find(|(name, _)| name == key)?;
                    Some((key.clone(), value.join(other)?))
                })
                .collect::<Option<_>>()
                .map(Self::Record),
            _ => None,
        };
        structural.or_else(|| Some(Self::Html(self.content()?.union(other.content()?))))
    }

    /// What values of this type are when they stand for HTML: text becomes a text node, and a
    /// list of HTML becomes a fragment. `None` if they cannot stand for HTML.
    pub(crate) fn content(&self) -> Option<Content> {
        match self {
            Self::Never => Some(Content::default()),
            Self::String => Some(Content::of(Category::Text)),
            Self::Html(content) => Some(*content),
            Self::List(item) => item.content(),
            Self::Bool | Self::Record(_) => None,
        }
    }

    pub(crate) fn is_html(&self) -> bool {
        self.content().is_some()
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Never => formatter.write_str("nothing"),
            Self::String => formatter.write_str("String"),
            Self::Bool => formatter.write_str("Bool"),
            Self::Html(_) => formatter.write_str("Html"),
            // An empty list literal is also the empty fragment, so it is shown as written.
            Self::List(item) if **item == Self::Never => formatter.write_str("[]"),
            Self::List(item) => write!(formatter, "List of {item}"),
            Self::Record(fields) => {
                let names = fields
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                write!(formatter, "Record {{{}}}", names.join(", "))
            }
        }
    }
}

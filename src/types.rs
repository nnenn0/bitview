use crate::{
    Value,
    error::{Error, ErrorKind},
};
use std::fmt;

/// The type of a value a host passes to a template, for [`crate::Program::check`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    String,
    Bool,
    Html,
    List(Box<Type>),
    Record(Vec<(String, Type)>),
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

    /// Whether `value` has exactly this type: records have the same fields, and every item of a
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
        (Type::String, Value::String(_))
        | (Type::Bool, Value::Bool(_))
        | (Type::Html, Value::Html(_)) => Ok(()),
        (Type::List(item), Value::List(items)) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, value)| validate(item, value, &format!("{path}[{index}]"))),
        (Type::Record(types), Value::Record(values)) => {
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
    Html,
    List(Box<Ty>),
    Record(Vec<(String, Ty)>),
}

impl From<&Type> for Ty {
    fn from(value: &Type) -> Self {
        match value {
            Type::String => Self::String,
            Type::Bool => Self::Bool,
            Type::Html => Self::Html,
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
        structural.or_else(|| (self.is_html() && other.is_html()).then_some(Self::Html))
    }

    /// Whether values of this type can stand for HTML: text becomes a text node, and a list of
    /// HTML becomes a fragment.
    pub(crate) fn is_html(&self) -> bool {
        match self {
            Self::Never | Self::String | Self::Html => true,
            Self::List(item) => item.is_html(),
            Self::Bool | Self::Record(_) => false,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Never => formatter.write_str("nothing"),
            Self::String => formatter.write_str("String"),
            Self::Bool => formatter.write_str("Bool"),
            Self::Html => formatter.write_str("Html"),
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

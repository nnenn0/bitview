use crate::Html;

/// The data that templates work with. Functions are not values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Bool(bool),
    List(Vec<Value>),
    /// Fields in order. In attribute records, the order is the order of the attributes.
    Record(Vec<(String, Value)>),
    Html(Html),
}

impl Value {
    #[must_use]
    pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Value)>) -> Self {
        Self::Record(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "String",
            Self::Bool(_) => "Bool",
            Self::List(_) => "List",
            Self::Record(_) => "Record",
            Self::Html(_) => "Html",
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<Html> for Value {
    fn from(value: Html) -> Self {
        Self::Html(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Self::List(value)
    }
}

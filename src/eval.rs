use crate::{
    Html, Value,
    ast::{Callee, Expr},
    error::{Error, ErrorKind, Span},
    html::{self, ElementSpec, Node},
    resolve::{FunctionId, Functions},
};

pub(crate) struct Evaluator<'a> {
    pub(crate) functions: &'a Functions,
}

impl Evaluator<'_> {
    pub(crate) fn call(&self, function: FunctionId, args: &[Value]) -> Result<Value, Error> {
        self.eval(&self.functions.get(function).body, args)
    }

    fn call_at(&self, function: FunctionId, args: &[Value], site: &Span) -> Result<Value, Error> {
        self.call(function, args)
            .map_err(|error| error.in_function(&self.functions.get(function).name, Some(site)))
    }

    fn eval(&self, expr: &Expr, args: &[Value]) -> Result<Value, Error> {
        match expr {
            Expr::Str(text) => Ok(Value::String(text.clone())),
            Expr::Param(_) | Expr::Field(..) => self.read(expr, args),
            Expr::List(items, _) => self.eval_all(items, args).map(Value::List),
            Expr::Record(fields) => fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), self.eval(value, args)?)))
                .collect::<Result<_, Error>>()
                .map(Value::Record),
            Expr::If(condition, then, otherwise, span) => match self.eval(condition, args)? {
                Value::Bool(true) => self.eval(then, args),
                Value::Bool(false) => self.eval(otherwise, args),
                other => Err(Error::at(
                    ErrorKind::Type,
                    span,
                    format!("if needs a Bool condition, but got {}", other.type_name()),
                )),
            },
            Expr::Call(callee, call_args, span) => {
                let values = self.eval_all(call_args, args)?;
                match callee {
                    Callee::User(function) => self.call_at(*function, &values, span),
                    Callee::Map(function) => self.map(*function, values, span),
                    Callee::Concat => concat(values).map_err(|error| error.with_span(span)),
                    Callee::Element(spec) => element(spec, values)
                        .map(Value::Html)
                        .map_err(|error| error.with_span(span)),
                }
            }
        }
    }

    /// Paths such as `ctx.article.title` are followed by reference, so that only the value at the
    /// end is copied, not the whole argument with the article's HTML.
    fn read(&self, expr: &Expr, args: &[Value]) -> Result<Value, Error> {
        if let Some(value) = borrow_path(expr, args)? {
            return Ok(value.clone());
        }
        let Expr::Field(record, name, span) = expr else {
            return Err(Error::new(ErrorKind::Name, "internal error: not a path"));
        };
        // The record is built here and owned, so the field can be moved out of it.
        let record = self.eval(record, args)?;
        into_field(record, name).map_err(|error| error.with_span(span))
    }

    fn eval_all(&self, exprs: &[Expr], args: &[Value]) -> Result<Vec<Value>, Error> {
        exprs.iter().map(|expr| self.eval(expr, args)).collect()
    }

    fn map(&self, function: FunctionId, values: Vec<Value>, span: &Span) -> Result<Value, Error> {
        match values.into_iter().next() {
            Some(Value::List(items)) => items
                .into_iter()
                .map(|item| self.call_at(function, &[item], span))
                .collect::<Result<_, _>>()
                .map(Value::List),
            other => Err(Error::at(
                ErrorKind::Type,
                span,
                format!(
                    "map needs a List, but got {}",
                    other.as_ref().map_or("nothing", Value::type_name)
                ),
            )),
        }
    }
}

fn borrow_path<'v>(expr: &Expr, args: &'v [Value]) -> Result<Option<&'v Value>, Error> {
    match expr {
        #[expect(clippy::indexing_slicing, reason = "every call passes every argument")]
        Expr::Param(position) => Ok(Some(&args[*position])),
        Expr::Field(record, name, span) => match borrow_path(record, args)? {
            Some(record) => field(record, name)
                .map(Some)
                .map_err(|error| error.with_span(span)),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

fn field<'v>(record: &'v Value, name: &str) -> Result<&'v Value, Error> {
    let Value::Record(fields) = record else {
        return Err(not_a_record(record, name));
    };
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| unknown_field(fields, name))
}

fn into_field(record: Value, name: &str) -> Result<Value, Error> {
    let Value::Record(mut fields) = record else {
        return Err(not_a_record(&record, name));
    };
    match fields.iter().position(|(key, _)| key == name) {
        Some(position) => Ok(fields.swap_remove(position).1),
        None => Err(unknown_field(&fields, name)),
    }
}

fn not_a_record(value: &Value, name: &str) -> Error {
    Error::new(
        ErrorKind::Type,
        format!(
            "cannot read field {name} of a {}; only records have fields",
            value.type_name()
        ),
    )
}

fn unknown_field(fields: &[(String, Value)], name: &str) -> Error {
    let available = fields
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Error::new(
        ErrorKind::Field,
        format!("unknown field {name:?} (fields: {available})"),
    )
}

fn concat(values: Vec<Value>) -> Result<Value, Error> {
    let mut text = String::new();
    for value in values {
        let Value::String(part) = value else {
            return Err(Error::new(
                ErrorKind::Type,
                format!("concat joins Strings, but got {}", value.type_name()),
            ));
        };
        text.push_str(&part);
    }
    Ok(Value::String(text))
}

fn element(spec: &'static ElementSpec, values: Vec<Value>) -> Result<Html, Error> {
    let mut values = values.into_iter().peekable();
    let attrs = match values.next_if(|value| matches!(value, Value::Record(_))) {
        Some(Value::Record(fields)) => attributes(spec, fields)?,
        _ => Vec::new(),
    };
    if spec.void && values.peek().is_some() {
        return Err(Error::new(
            ErrorKind::Html,
            format!("<{}> is a void element and takes no children", spec.name),
        ));
    }
    let children = into_html(values).map_err(|other| {
        let hint = if matches!(other, Value::Record(_)) {
            "; attributes must be the first argument"
        } else {
            ""
        };
        Error::new(
            ErrorKind::Type,
            format!(
                "a {} cannot be a child of <{}>{hint}",
                other.type_name(),
                spec.name
            ),
        )
    })?;
    html::build_element(spec, attrs, children)
}

fn attributes(
    spec: &ElementSpec,
    fields: Vec<(String, Value)>,
) -> Result<Vec<(String, String)>, Error> {
    fields
        .into_iter()
        .map(|(name, value)| match value {
            Value::String(text) => Ok((name, text)),
            other => Err(Error::new(
                ErrorKind::Type,
                format!(
                    "attribute {name} of <{}> needs a String, but got {}",
                    spec.name,
                    other.type_name()
                ),
            )),
        })
        .collect()
}

/// Text becomes a text node and a list a fragment, as the types allow. Returns the first value
/// that cannot stand for HTML.
pub(crate) fn into_html(values: impl IntoIterator<Item = Value>) -> Result<Html, Value> {
    fn push(nodes: &mut Vec<Node>, value: Value) -> Result<(), Value> {
        match value {
            Value::String(text) => nodes.push(Node::Text(text)),
            Value::Html(html) => nodes.extend(html.0),
            Value::List(items) => {
                for item in items {
                    push(nodes, item)?;
                }
            }
            other @ (Value::Bool(_) | Value::Record(_)) => return Err(other),
        }
        Ok(())
    }
    let mut nodes = Vec::new();
    for value in values {
        push(&mut nodes, value)?;
    }
    Ok(Html(nodes))
}

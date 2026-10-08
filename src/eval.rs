use crate::{
    Html, Value,
    ast::{Callee, Expr},
    error::{Error, Span},
    html::{self, ElementSpec, Node},
    resolve::{FunctionId, Functions},
};

pub(crate) struct Evaluator<'a> {
    functions: &'a Functions,
}

impl<'a> Evaluator<'a> {
    pub(crate) fn new(functions: &'a Functions) -> Self {
        Self { functions }
    }

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
            #[expect(clippy::indexing_slicing, reason = "every call passes every argument")]
            Expr::Param(position) => Ok(args[*position].clone()),
            Expr::Field(record, name, span) => self
                .read_field(record, name, args)
                .map_err(|error| error.with_span(span)),
            Expr::List(items, _) => self.eval_all(items, args).map(Value::List),
            Expr::Record(fields) => fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), self.eval(value, args)?)))
                .collect::<Result<_, Error>>()
                .map(Value::Record),
            Expr::If(condition, then, otherwise, span) => match self.eval(condition, args)? {
                Value::Bool(true) => self.eval(then, args),
                Value::Bool(false) => self.eval(otherwise, args),
                other => Err(Error::not_a_condition(span, other.type_name())),
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
    /// end is copied, not the whole argument with the article's HTML. A record built here, such as
    /// the result of a call, is owned, so the field is moved out of it instead.
    fn read_field(&self, record: &Expr, name: &str, args: &[Value]) -> Result<Value, Error> {
        match borrow_path(record, args)? {
            Some(record) => field(record, name).cloned(),
            None => into_field(self.eval(record, args)?, name),
        }
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
            other => Err(Error::not_a_list(
                span,
                other.as_ref().map_or("nothing", Value::type_name),
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
        return Err(Error::not_a_record(name, record.type_name()));
    };
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| unknown_field(name, fields))
}

fn into_field(record: Value, name: &str) -> Result<Value, Error> {
    let Value::Record(mut fields) = record else {
        return Err(Error::not_a_record(name, record.type_name()));
    };
    match fields.iter().position(|(key, _)| key == name) {
        Some(position) => Ok(fields.swap_remove(position).1),
        None => Err(unknown_field(name, &fields)),
    }
}

fn unknown_field(name: &str, fields: &[(String, Value)]) -> Error {
    Error::unknown_field(name, fields.iter().map(|(key, _)| key.as_str()))
}

fn concat(values: Vec<Value>) -> Result<Value, Error> {
    let mut text = String::new();
    for value in values {
        let Value::String(part) = value else {
            return Err(Error::not_a_string_to_concat(value.type_name()));
        };
        text.push_str(&part);
    }
    Ok(Value::String(text))
}

fn element(spec: &'static ElementSpec, values: Vec<Value>) -> Result<Html, Error> {
    let mut values = values.into_iter().peekable();
    let attributes = match values.next_if(|value| matches!(value, Value::Record(_))) {
        Some(Value::Record(fields)) => into_attributes(spec, fields)?,
        _ => Vec::new(),
    };
    if spec.void && values.peek().is_some() {
        return Err(Error::void_with_children(spec));
    }
    let children = into_html(values).map_err(|other| {
        Error::not_a_child(spec, other.type_name(), matches!(other, Value::Record(_)))
    })?;
    html::build_element(spec, attributes, children)
}

fn into_attributes(
    spec: &ElementSpec,
    fields: Vec<(String, Value)>,
) -> Result<Vec<(String, String)>, Error> {
    fields
        .into_iter()
        .map(|(name, value)| match value {
            Value::String(text) => Ok((name, text)),
            other => Err(Error::not_a_string_attribute(
                spec,
                &name,
                other.type_name(),
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

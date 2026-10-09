use crate::{
    HtmlType, Type,
    ast::{Def, Param, Syntax},
    error::{Error, ErrorKind, Span},
    lexer::{NAME_RULE, Spanned, Token, Tokens},
    types::Ty,
};
use std::{iter::Peekable, vec};

/// How deeply expressions may nest. The parser and the evaluator recurse once per level, so this
/// keeps both far from the end of the stack.
const MAX_DEPTH: usize = 128;

pub(crate) fn parse(tokens: Tokens) -> Result<Vec<Def>, Error> {
    let mut parser = Parser {
        tokens: tokens.list.into_iter().peekable(),
        end: tokens.end,
    };
    let mut defs = Vec::new();
    while parser.peek().is_some() {
        defs.push(parser.def()?);
    }
    Ok(defs)
}

struct Parser {
    tokens: Peekable<vec::IntoIter<Spanned>>,
    end: Span,
}

impl Parser {
    fn peek(&mut self) -> Option<&Token> {
        self.tokens.peek().map(|spanned| &spanned.token)
    }

    /// Takes the next token if it is `token`.
    fn eat(&mut self, token: &Token) -> Option<Span> {
        self.tokens
            .next_if(|spanned| spanned.token == *token)
            .map(|spanned| spanned.span)
    }

    /// Takes the next token, or fails with `expected` at the end of the file.
    fn next(&mut self, expected: &str) -> Result<Spanned, Error> {
        self.tokens
            .next()
            .ok_or_else(|| unexpected(expected, None, &self.end))
    }

    fn expect(&mut self, token: &Token, expected: &str) -> Result<Span, Error> {
        match self.next(expected)? {
            Spanned { token: found, span } if found == *token => Ok(span),
            Spanned { token: found, span } => Err(unexpected(expected, Some(&found), &span)),
        }
    }

    fn name(&mut self, expected: &str) -> Result<(String, Span), Error> {
        match self.next(expected)? {
            Spanned {
                token: Token::Name(name),
                span,
            } => Ok((name, span)),
            Spanned { token, span } => Err(unexpected(expected, Some(&token), &span)),
        }
    }

    /// Reads items up to `close`. A bracket left open reports where it was opened, since the end
    /// of the file says little about which one is missing.
    fn until<T>(
        &mut self,
        close: &Token,
        open: &Span,
        mut item: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut items = Vec::new();
        loop {
            if self.eat(close).is_some() {
                return Ok(items);
            }
            if self.peek().is_none() {
                return Err(Error::at(
                    ErrorKind::Syntax,
                    open,
                    format!("no {close} closes this bracket"),
                ));
            }
            items.push(item(self)?);
        }
    }

    fn def(&mut self) -> Result<Def, Error> {
        let expected = "a function definition, as in (defn page [ctx] ...)";
        let span = self.expect(&Token::LParen, expected)?;
        let public = match self.next(expected)? {
            Spanned {
                token: Token::Defn, ..
            } => true,
            Spanned {
                token: Token::DefnPrivate,
                ..
            } => false,
            Spanned { token, span } => return Err(unexpected(expected, Some(&token), &span)),
        };
        let (name, _) = self.name("a function name")?;
        let params_open = self.expect(&Token::LBracket, "`[` and the parameters")?;
        let params = self.until(&Token::RBracket, &params_open, Self::param)?;
        let body = self.until(&Token::RParen, &span, |parser| parser.expr(0))?;
        let Ok([body]) = <[Syntax; 1]>::try_from(body) else {
            return Err(Error::at(
                ErrorKind::Syntax,
                &span,
                "a function body is one expression",
            ));
        };
        Ok(Def {
            name,
            public,
            params,
            body,
            span,
        })
    }

    /// A parameter name, and its type if one follows.
    fn param(&mut self) -> Result<Param, Error> {
        let (name, span) = self.name("a parameter name")?;
        let ty = match self.peek() {
            Some(Token::Type(_) | Token::LBracket | Token::LBrace) => Some(Ty::from(&self.ty(0)?)),
            _ => None,
        };
        Ok(Param { name, span, ty })
    }

    /// A type: one of the [`Type`]s a host passes, written as in `{:tags [String]}`.
    fn ty(&mut self, depth: usize) -> Result<Type, Error> {
        let expected = "a type";
        let Spanned { token, span } = self.next(expected)?;
        let depth = Self::nest(depth, &span)?;
        match token {
            Token::Type(name) => match name.as_str() {
                "String" => Ok(Type::String),
                "Bool" => Ok(Type::Bool),
                "Flow" => Ok(Type::Html(HtmlType::Flow)),
                "Phrasing" => Ok(Type::Html(HtmlType::Phrasing)),
                "Metadata" => Ok(Type::Html(HtmlType::Metadata)),
                _ => Err(Error::at(
                    ErrorKind::Syntax,
                    &span,
                    format!("unknown type {name}; {TYPES}"),
                )),
            },
            Token::LBracket => {
                let item = self.ty(depth)?;
                self.expect(
                    &Token::RBracket,
                    "`]`; a list type has one item type, as in [String]",
                )?;
                Ok(Type::list(item))
            }
            Token::LBrace => {
                let fields = self.until(&Token::RBrace, &span, |parser| {
                    let expected = "a key, as in :title";
                    match parser.next(expected)? {
                        Spanned {
                            token: Token::Key(key),
                            span,
                        } => Ok((key, span, parser.ty(depth)?)),
                        Spanned { token, span } => Err(unexpected(expected, Some(&token), &span)),
                    }
                })?;
                for (position, (key, span, _)) in fields.iter().enumerate() {
                    if fields
                        .iter()
                        .take(position)
                        .any(|(earlier, _, _)| earlier == key)
                    {
                        return Err(Error::at(
                            ErrorKind::Name,
                            span,
                            format!("field {key} is written twice in the type"),
                        ));
                    }
                }
                Ok(Type::record(
                    fields.into_iter().map(|(key, _, ty)| (key, ty)),
                ))
            }
            other => Err(Error::at(
                ErrorKind::Syntax,
                &span,
                format!("expected a type, found {other}; {TYPES}"),
            )),
        }
    }

    /// The depth one level inside `depth`.
    fn nest(depth: usize, span: &Span) -> Result<usize, Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::at(
                ErrorKind::Syntax,
                span,
                format!("expressions are nested more than {MAX_DEPTH} levels deep"),
            ));
        }
        Ok(depth + 1)
    }

    /// `depth` is how many expressions enclose this one.
    fn expr(&mut self, depth: usize) -> Result<Syntax, Error> {
        let expected = "an expression";
        let Spanned { token, span } = self.next(expected)?;
        let depth = Self::nest(depth, &span)?;
        match token {
            Token::Str(text) => Ok(Syntax::Str(text)),
            Token::Name(name) => self.fields(Syntax::Name(name, span), depth),
            Token::LBracket => {
                let items = self.until(&Token::RBracket, &span, |parser| parser.expr(depth))?;
                Ok(Syntax::List(items, span))
            }
            Token::LBrace => Ok(Syntax::Record(self.until(
                &Token::RBrace,
                &span,
                |parser| parser.entry(depth),
            )?)),
            Token::LParen => self.form(&span, depth),
            Token::Type(name) => Err(Error::at(
                ErrorKind::Syntax,
                &span,
                format!(
                    "{name} is a type, which is written only after a parameter name; {NAME_RULE}"
                ),
            )),
            Token::Key(_) => Err(Error::at(
                ErrorKind::Syntax,
                &span,
                "a key is written only before a value in a record, as in {:lang \"ja\"}",
            )),
            other => Err(unexpected(expected, Some(&other), &span)),
        }
    }

    /// Each field read wraps the expression before it, so it counts as one level of nesting.
    fn fields(&mut self, mut expr: Syntax, mut depth: usize) -> Result<Syntax, Error> {
        while let Some(Spanned {
            token: Token::Field(field),
            span,
        }) = self
            .tokens
            .next_if(|spanned| matches!(spanned.token, Token::Field(_)))
        {
            depth = Self::nest(depth, &span)?;
            expr = Syntax::Field(Box::new(expr), field, span);
        }
        Ok(expr)
    }

    /// A parenthesized form: `if` or a call. `open` is the position of `(`.
    fn form(&mut self, open: &Span, depth: usize) -> Result<Syntax, Error> {
        let expected = "a function name or `if` after `(`";
        let Spanned { token, span } = self.next(expected)?;
        match token {
            Token::If => {
                let args = self.until(&Token::RParen, open, |parser| parser.expr(depth))?;
                let Ok([condition, then, otherwise]) = <[Syntax; 3]>::try_from(args) else {
                    return Err(Error::at(
                        ErrorKind::Syntax,
                        &span,
                        "if takes a condition, the value when it holds, and the value otherwise",
                    ));
                };
                Ok(Syntax::If(
                    Box::new(condition),
                    Box::new(then),
                    Box::new(otherwise),
                    span,
                ))
            }
            Token::Name(name) => {
                if let Some(Token::Field(_)) = self.peek() {
                    return Err(Error::at(
                        ErrorKind::Syntax,
                        &span,
                        "only a function name can be called",
                    ));
                }
                let args = self.until(&Token::RParen, open, |parser| parser.expr(depth))?;
                Ok(Syntax::Call(name, args, span))
            }
            other => Err(unexpected(expected, Some(&other), &span)),
        }
    }

    fn entry(&mut self, depth: usize) -> Result<(String, Syntax, Span), Error> {
        let expected = "a key, as in :lang";
        match self.next(expected)? {
            Spanned {
                token: Token::Key(key),
                span,
            } => Ok((key, self.expr(depth)?, span)),
            Spanned { token, span } => Err(unexpected(expected, Some(&token), &span)),
        }
    }
}

const TYPES: &str = "the types are String, Bool, Flow, Phrasing, Metadata, lists such as [String], and records such as {:title String}";

fn unexpected(expected: &str, found: Option<&Token>, span: &Span) -> Error {
    let message = match found {
        Some(token) => format!("expected {expected}, found {token}"),
        None => format!("expected {expected}, found the end of the file"),
    };
    Error::at(ErrorKind::Syntax, span, message)
}

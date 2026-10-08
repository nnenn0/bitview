use crate::{
    ast::{Def, Syntax},
    error::{Error, ErrorKind, Span},
    lexer::{Spanned, Token, Tokens},
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

    fn span(&mut self) -> Span {
        self.tokens
            .peek()
            .map_or_else(|| self.end.clone(), |spanned| spanned.span.clone())
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

    fn unexpected(&mut self, expected: &str) -> Error {
        let span = self.span();
        unexpected(expected, self.peek(), &span)
    }

    fn expect(&mut self, token: &Token, expected: &str) -> Result<Span, Error> {
        self.eat(token).ok_or_else(|| self.unexpected(expected))
    }

    fn ident(&mut self, expected: &str) -> Result<(String, Span), Error> {
        match self.next(expected)? {
            Spanned {
                token: Token::Ident(name),
                span,
            } => Ok((name, span)),
            Spanned { token, span } => Err(unexpected(expected, Some(&token), &span)),
        }
    }

    fn separated<T>(
        &mut self,
        close: &Token,
        expected: &str,
        mut item: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut items = Vec::new();
        loop {
            if self.eat(close).is_some() {
                return Ok(items);
            }
            items.push(item(self)?);
            if self.eat(&Token::Comma).is_none() {
                self.expect(close, expected)?;
                return Ok(items);
            }
        }
    }

    fn def(&mut self) -> Result<Def, Error> {
        let span = self.expect(&Token::Fn, "a function definition starting with `fn`")?;
        let (name, _) = self.ident("a function name")?;
        self.expect(&Token::LParen, "`(`")?;
        let params = self.separated(&Token::RParen, "`,` or `)`", |parser| {
            parser.ident("a parameter name")
        })?;
        self.expect(&Token::Arrow, "`=>`")?;
        let body = self.expr(0)?;
        Ok(Def {
            name,
            params,
            body,
            span,
        })
    }

    /// The depth one level inside `depth`.
    fn nest(&mut self, depth: usize) -> Result<usize, Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::at(
                ErrorKind::Syntax,
                &self.span(),
                format!("expressions are nested more than {MAX_DEPTH} levels deep"),
            ));
        }
        Ok(depth + 1)
    }

    /// `depth` is how many expressions enclose this one.
    fn expr(&mut self, depth: usize) -> Result<Syntax, Error> {
        let depth = self.nest(depth)?;
        if self.peek() == Some(&Token::If) {
            self.if_expr(depth)
        } else {
            self.postfix(depth)
        }
    }

    fn if_expr(&mut self, depth: usize) -> Result<Syntax, Error> {
        let span = self.expect(&Token::If, "`if`")?;
        let condition = self.expr(depth)?;
        self.expect(&Token::Then, "`then`")?;
        let then = self.expr(depth)?;
        self.expect(&Token::Else, "`else`")?;
        let otherwise = self.expr(depth)?;
        Ok(Syntax::If(
            Box::new(condition),
            Box::new(then),
            Box::new(otherwise),
            span,
        ))
    }

    /// Each field read wraps the expression before it, so it counts as one level of nesting.
    fn postfix(&mut self, mut depth: usize) -> Result<Syntax, Error> {
        let mut expr = self.primary(depth)?;
        loop {
            match self.peek() {
                Some(Token::LParen) => {
                    let Syntax::Name(name, span) = expr else {
                        return Err(Error::at(
                            ErrorKind::Syntax,
                            &self.span(),
                            "only a function name can be called",
                        ));
                    };
                    self.tokens.next();
                    let args =
                        self.separated(&Token::RParen, "`,` or `)`", |parser| parser.expr(depth))?;
                    expr = Syntax::Call(name, args, span);
                }
                Some(Token::Dot) => {
                    depth = self.nest(depth)?;
                    self.tokens.next();
                    let (field, span) = self.ident("a field name")?;
                    expr = Syntax::Field(Box::new(expr), field, span);
                }
                _ => return Ok(expr),
            }
        }
    }

    fn primary(&mut self, depth: usize) -> Result<Syntax, Error> {
        let expected = "an expression";
        let Spanned { token, span } = self.next(expected)?;
        match token {
            Token::Str(text) => Ok(Syntax::Str(text)),
            Token::Ident(name) => Ok(Syntax::Name(name, span)),
            Token::LBracket => {
                let items =
                    self.separated(&Token::RBracket, "`,` or `]`", |parser| parser.expr(depth))?;
                Ok(Syntax::List(items, span))
            }
            Token::LBrace => Ok(Syntax::Record(self.separated(
                &Token::RBrace,
                "`,` or `}`",
                |parser| parser.field(depth),
            )?)),
            Token::LParen => {
                let expr = self.expr(depth)?;
                self.expect(&Token::RParen, "`)`")?;
                Ok(expr)
            }
            other => Err(unexpected(expected, Some(&other), &span)),
        }
    }

    fn field(&mut self, depth: usize) -> Result<(String, Syntax, Span), Error> {
        let expected = "a field name";
        let (key, span) = match self.next(expected)? {
            Spanned {
                token: Token::Ident(name) | Token::Str(name),
                span,
            } => (name, span),
            Spanned { token, span } => return Err(unexpected(expected, Some(&token), &span)),
        };
        self.expect(&Token::Colon, "`:`")?;
        Ok((key, self.expr(depth)?, span))
    }
}

fn unexpected(expected: &str, found: Option<&Token>, span: &Span) -> Error {
    let message = match found {
        Some(token) => format!("expected {expected}, found {token}"),
        None => format!("expected {expected}, found the end of the file"),
    };
    Error::at(ErrorKind::Syntax, span, message)
}

use crate::{
    ast::{Def, Syntax},
    error::{Error, ErrorKind, Span},
    lexer::{Token, Tokens},
};

/// How deeply expressions may nest. The parser and the evaluator recurse once per level, so this
/// keeps both far from the end of the stack.
pub(crate) const MAX_DEPTH: usize = 128;

pub(crate) fn parse(tokens: Tokens) -> Result<Vec<Def>, Error> {
    let mut parser = Parser {
        tokens,
        position: 0,
        depth: 0,
    };
    let mut defs = Vec::new();
    while parser.peek().is_some() {
        defs.push(parser.def()?);
    }
    Ok(defs)
}

struct Parser {
    tokens: Tokens,
    position: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens
            .list
            .get(self.position)
            .map(|spanned| &spanned.token)
    }

    fn span(&self) -> Span {
        self.tokens
            .list
            .get(self.position)
            .map_or_else(|| self.tokens.end.clone(), |spanned| spanned.span.clone())
    }

    fn advance(&mut self) {
        self.position = self.position.saturating_add(1);
    }

    fn unexpected(&self, expected: &str) -> Error {
        let found = self
            .peek()
            .map_or_else(|| "the end of the file".to_owned(), Token::describe);
        Error::at(
            ErrorKind::Syntax,
            &self.span(),
            format!("expected {expected}, found {found}"),
        )
    }

    fn expect(&mut self, token: &Token, expected: &str) -> Result<Span, Error> {
        if self.peek() != Some(token) {
            return Err(self.unexpected(expected));
        }
        let span = self.span();
        self.advance();
        Ok(span)
    }

    fn ident(&mut self, expected: &str) -> Result<(String, Span), Error> {
        let Some(Token::Ident(name)) = self.peek() else {
            return Err(self.unexpected(expected));
        };
        let name = name.clone();
        let span = self.span();
        self.advance();
        Ok((name, span))
    }

    fn separated<T>(
        &mut self,
        close: &Token,
        expected: &str,
        mut item: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut items = Vec::new();
        loop {
            if self.peek() == Some(close) {
                self.advance();
                return Ok(items);
            }
            items.push(item(self)?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
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
        let body = self.expr()?;
        Ok(Def {
            name,
            params,
            body,
            span,
        })
    }

    fn expr(&mut self) -> Result<Syntax, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(Error::at(
                ErrorKind::Syntax,
                &self.span(),
                format!("expressions are nested more than {MAX_DEPTH} levels deep"),
            ));
        }
        self.depth += 1;
        let expr = if self.peek() == Some(&Token::If) {
            self.if_expr()
        } else {
            self.postfix()
        };
        self.depth -= 1;
        expr
    }

    fn if_expr(&mut self) -> Result<Syntax, Error> {
        let span = self.expect(&Token::If, "`if`")?;
        let condition = self.expr()?;
        self.expect(&Token::Then, "`then`")?;
        let then = self.expr()?;
        self.expect(&Token::Else, "`else`")?;
        let otherwise = self.expr()?;
        Ok(Syntax::If(
            Box::new(condition),
            Box::new(then),
            Box::new(otherwise),
            span,
        ))
    }

    fn postfix(&mut self) -> Result<Syntax, Error> {
        let mut expr = self.primary()?;
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
                    self.advance();
                    let args = self.separated(&Token::RParen, "`,` or `)`", Self::expr)?;
                    expr = Syntax::Call(name, args, span);
                }
                Some(Token::Dot) => {
                    self.advance();
                    let (field, span) = self.ident("a field name")?;
                    expr = Syntax::Field(Box::new(expr), field, span);
                }
                _ => return Ok(expr),
            }
        }
    }

    fn primary(&mut self) -> Result<Syntax, Error> {
        let span = self.span();
        let expr = match self.peek() {
            Some(Token::Str(text)) => Syntax::Str(text.clone()),
            Some(Token::Ident(name)) => Syntax::Name(name.clone(), span),
            Some(Token::LBracket) => {
                self.advance();
                return Ok(Syntax::List(self.separated(
                    &Token::RBracket,
                    "`,` or `]`",
                    Self::expr,
                )?));
            }
            Some(Token::LBrace) => {
                self.advance();
                return Ok(Syntax::Record(self.separated(
                    &Token::RBrace,
                    "`,` or `}`",
                    Self::field,
                )?));
            }
            Some(Token::LParen) => {
                self.advance();
                let expr = self.expr()?;
                self.expect(&Token::RParen, "`)`")?;
                return Ok(expr);
            }
            _ => return Err(self.unexpected("an expression")),
        };
        self.advance();
        Ok(expr)
    }

    fn field(&mut self) -> Result<(String, Syntax, Span), Error> {
        let span = self.span();
        let key = match self.peek() {
            Some(Token::Ident(name) | Token::Str(name)) => name.clone(),
            _ => return Err(self.unexpected("a field name")),
        };
        self.advance();
        self.expect(&Token::Colon, "`:`")?;
        Ok((key, self.expr()?, span))
    }
}

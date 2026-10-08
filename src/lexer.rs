use crate::error::{Error, ErrorKind, Span};
use std::{fmt, iter::Peekable, str::Chars, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Name(String),
    /// A field read, written right after a name or another field, as `.title` in `ctx.title`.
    Field(String),
    /// A record key, as `:lang` in `{:lang "ja"}`.
    Key(String),
    Str(String),
    Defn,
    /// `defn-`, which defines a function that only its own source can call.
    DefnPrivate,
    If,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
}

impl fmt::Display for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self {
            Self::Name(name) => return write!(formatter, "`{name}`"),
            Self::Field(name) => return write!(formatter, "`.{name}`"),
            Self::Key(name) => return write!(formatter, "`:{name}`"),
            Self::Str(_) => return formatter.write_str("a string"),
            Self::Defn => "defn",
            Self::DefnPrivate => "defn-",
            Self::If => "if",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::LBrace => "{",
            Self::RBrace => "}",
        };
        write!(formatter, "`{symbol}`")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Spanned {
    pub(crate) token: Token,
    pub(crate) span: Span,
}

pub(crate) struct Tokens {
    pub(crate) list: Vec<Spanned>,
    /// The position just after the last character, for errors at the end of the source.
    pub(crate) end: Span,
}

pub(crate) fn tokenize(source: &Arc<str>, text: &str) -> Result<Tokens, Error> {
    let mut lexer = Lexer {
        source,
        chars: text.chars().peekable(),
        line: 1,
        column: 1,
    };
    let mut list = Vec::new();
    loop {
        lexer.skip_trivia();
        let span = lexer.span();
        let Some(character) = lexer.bump() else {
            return Ok(Tokens { list, end: span });
        };
        let token = match character {
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '"' => Token::Str(lexer.string(&span)?),
            ':' => {
                let key_span = lexer.span();
                match lexer.bump() {
                    Some(first @ 'a'..='z') => Token::Key(lexer.ident(first, &key_span)?),
                    _ => {
                        return Err(Error::at(
                            ErrorKind::Syntax,
                            &span,
                            "a record key is a name after `:`, as in :lang",
                        ));
                    }
                }
            }
            'a'..='z' => {
                // `defn-` is the only word that ends with a hyphen.
                let word = lexer.word(character);
                let token = if word == "defn-" {
                    Token::DefnPrivate
                } else {
                    keyword_or_name(lexer.name(word, &span)?)
                };
                if !matches!(token, Token::Name(_)) && lexer.chars.peek() == Some(&'.') {
                    return Err(Error::at(
                        ErrorKind::Syntax,
                        &span,
                        format!("{token} is a keyword and has no fields"),
                    ));
                }
                list.push(Spanned { token, span });
                lexer.fields(&mut list)?;
                continue;
            }
            'A'..='Z' | '_' => return Err(Error::at(ErrorKind::Syntax, &span, NAME_RULE)),
            other => {
                return Err(Error::at(
                    ErrorKind::Syntax,
                    &span,
                    format!("unexpected character {other:?}"),
                ));
            }
        };
        list.push(Spanned { token, span });
    }
}

/// Names look like CSS class names, so that a function and its CSS file can share one.
const NAME_RULE: &str =
    "names are lowercase letters and digits, with words joined by single hyphens, as in entry-list";

fn keyword_or_name(name: String) -> Token {
    match name.as_str() {
        "defn" => Token::Defn,
        "if" => Token::If,
        _ => Token::Name(name),
    }
}

struct Lexer<'a> {
    source: &'a Arc<str>,
    chars: Peekable<Chars<'a>>,
    line: u32,
    column: u32,
}

impl Lexer<'_> {
    fn span(&self) -> Span {
        Span::new(Arc::clone(self.source), self.line, self.column)
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.chars.next()?;
        if character == '\n' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else {
            self.column = self.column.saturating_add(1);
        }
        Some(character)
    }

    fn eat(&mut self, expected: char) -> bool {
        let found = self.chars.peek() == Some(&expected);
        if found {
            self.bump();
        }
        found
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.chars.peek().copied() {
                Some(' ' | '\t' | '\n' | '\r') => {
                    self.bump();
                }
                Some(';') => {
                    while self
                        .chars
                        .peek()
                        .is_some_and(|character| *character != '\n')
                    {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    /// Reads the fields after a name, each a `.` followed by a name with no space between.
    fn fields(&mut self, list: &mut Vec<Spanned>) -> Result<(), Error> {
        while self.eat('.') {
            let span = self.span();
            let Some(first @ 'a'..='z') = self.bump() else {
                return Err(Error::at(
                    ErrorKind::Syntax,
                    &span,
                    "expected a field name after `.`",
                ));
            };
            let token = Token::Field(self.ident(first, &span)?);
            list.push(Spanned { token, span });
        }
        Ok(())
    }

    fn ident(&mut self, first: char, start: &Span) -> Result<String, Error> {
        let word = self.word(first);
        self.name(word, start)
    }

    /// Reads the lowercase letters, digits, and hyphens that start with `first`.
    fn word(&mut self, first: char) -> String {
        let mut word = String::from(first);
        while let Some(&character) = self.chars.peek() {
            if !(character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-') {
                break;
            }
            word.push(character);
            self.bump();
        }
        word
    }

    /// `name` if the word just read follows [`NAME_RULE`].
    fn name(&mut self, name: String, start: &Span) -> Result<String, Error> {
        let continues = self
            .chars
            .peek()
            .is_some_and(|character| character.is_ascii_uppercase() || *character == '_');
        if continues || name.ends_with('-') || name.contains("--") {
            return Err(Error::at(ErrorKind::Syntax, start, NAME_RULE));
        }
        Ok(name)
    }

    fn string(&mut self, start: &Span) -> Result<String, Error> {
        let mut text = String::new();
        loop {
            let span = self.span();
            match self.bump() {
                None => return Err(Error::at(ErrorKind::Syntax, start, "unterminated string")),
                Some('"') => return Ok(text),
                Some('\\') => match self.bump() {
                    Some('"') => text.push('"'),
                    Some('\\') => text.push('\\'),
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some('u') => text.push(self.unicode_escape(&span)?),
                    _ => {
                        return Err(Error::at(
                            ErrorKind::Syntax,
                            &span,
                            "unknown escape; use \\\", \\\\, \\n, \\t, or \\u{...}",
                        ));
                    }
                },
                Some(character) => text.push(character),
            }
        }
    }

    fn unicode_escape(&mut self, span: &Span) -> Result<char, Error> {
        let invalid = || {
            Error::at(
                ErrorKind::Syntax,
                span,
                "\\u{...} needs 1 to 6 hexadecimal digits of a Unicode scalar value",
            )
        };
        if !self.eat('{') {
            return Err(invalid());
        }
        let mut hex = String::new();
        loop {
            match self.bump() {
                Some('}') => break,
                Some(digit) if digit.is_ascii_hexdigit() && hex.len() < 6 => hex.push(digit),
                _ => return Err(invalid()),
            }
        }
        u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(invalid)
    }
}

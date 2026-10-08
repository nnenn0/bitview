//! HTML as a value. Templates and Rust build it only through the checked constructors here, and
//! only `serializer` turns it into text.

use crate::{
    error::{Error, ErrorKind},
    serializer,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ElementSpec {
    pub(crate) name: &'static str,
    pub(crate) void: bool,
}

const fn normal(name: &'static str) -> ElementSpec {
    ElementSpec { name, void: false }
}

const fn void(name: &'static str) -> ElementSpec {
    ElementSpec { name, void: true }
}

/// The elements that templates and Rust can build: those that genbit's templates and its Markdown
/// output use. Add an element when a template needs it. Elements that run code or change how the whole
/// page resolves URLs (`script`, `iframe`, `form`, `base`, ...) are left out on purpose; `<style>`
/// and JSON data come only from [`Html::style`] and [`Html::json`].
const ELEMENTS: &[ElementSpec] = &[
    normal("html"),
    normal("head"),
    normal("body"),
    normal("title"),
    void("meta"),
    void("link"),
    normal("header"),
    normal("footer"),
    normal("main"),
    normal("nav"),
    normal("section"),
    normal("article"),
    normal("aside"),
    normal("h1"),
    normal("h2"),
    normal("h3"),
    normal("h4"),
    normal("h5"),
    normal("h6"),
    normal("p"),
    normal("div"),
    normal("ul"),
    normal("ol"),
    normal("li"),
    normal("dl"),
    normal("dt"),
    normal("dd"),
    normal("blockquote"),
    normal("pre"),
    void("hr"),
    normal("a"),
    normal("span"),
    normal("time"),
    normal("strong"),
    normal("em"),
    normal("code"),
    void("br"),
    void("img"),
    normal("table"),
    normal("thead"),
    normal("tbody"),
    normal("tr"),
    normal("th"),
    normal("td"),
];

pub(crate) fn element_spec(name: &str) -> Option<&'static ElementSpec> {
    ELEMENTS.iter().find(|spec| spec.name == name)
}

/// How deeply elements may nest. Serializing and dropping HTML recurse once per level, so this
/// keeps both far from the end of the stack, even for Markdown with thousands of nested quotes.
const MAX_DEPTH: u16 = 256;

/// Attributes whose values browsers follow as URLs.
const URL_ATTRIBUTES: [&str; 3] = ["href", "src", "cite"];
const URL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawText {
    Style,
    Json { media_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Text(String),
    Element {
        spec: &'static ElementSpec,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
        /// Kept on each element so that checking the depth never walks the tree again.
        depth: u16,
    },
    RawText {
        kind: RawText,
        text: String,
    },
}

/// A fragment of HTML: zero or more nodes. Its contents can only be built, not inspected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Html(pub(crate) Vec<Node>);

impl Html {
    /// Text, escaped when serialized.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self(vec![Node::Text(text.into())])
    }

    /// One element with `children` inside.
    ///
    /// # Errors
    ///
    /// Fails if `name` is not in the element table, an attribute name is invalid, repeated, `style`,
    /// or starts with `on`, a URL attribute has a scheme other than http, https, or mailto, a void
    /// element has children, or elements would nest more than 256 levels deep.
    pub fn element(
        name: &str,
        attrs: Vec<(String, String)>,
        children: Self,
    ) -> Result<Self, Error> {
        let spec = element_spec(name)
            .ok_or_else(|| Error::new(ErrorKind::Html, format!("unknown element <{name}>")))?;
        build_element(spec, attrs, children)
    }

    /// A `<style>` element with `css` inside.
    ///
    /// # Errors
    ///
    /// Fails if `css` contains `</style`, `</script`, or `<!--`, which could end the element early.
    pub fn style(css: impl Into<String>) -> Result<Self, Error> {
        raw_text(RawText::Style, css.into())
    }

    /// A `<script>` element of type `media_type` with `json` inside, such as JSON-LD with
    /// `application/ld+json`. `json` must already be serialized with `<`, `>`, and `&` escaped as
    /// `\u003c`, `\u003e`, and `\u0026`.
    ///
    /// # Errors
    ///
    /// Fails unless `media_type` is `application/json` or `application/<name>+json`, which browsers
    /// never run as a script, or if `json` contains `</style`, `</script`, or `<!--`, which could end
    /// the element early.
    pub fn json(media_type: &str, json: impl Into<String>) -> Result<Self, Error> {
        let name = media_type
            .strip_prefix("application/")
            .and_then(|subtype| subtype.strip_suffix("+json"));
        let valid = media_type == "application/json"
            || name.is_some_and(|name| {
                name.starts_with(|character: char| character.is_ascii_alphanumeric())
                    && name.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || matches!(character, '.' | '-')
                    })
            });
        if !valid {
            return Err(Error::new(
                ErrorKind::Html,
                format!("{media_type:?} is not a JSON media type such as application/ld+json"),
            ));
        }
        raw_text(
            RawText::Json {
                media_type: media_type.to_owned(),
            },
            json.into(),
        )
    }

    fn depth(&self) -> u16 {
        self.0
            .iter()
            .map(|node| match node {
                Node::Element { depth, .. } => *depth,
                Node::Text(_) | Node::RawText { .. } => 0,
            })
            .max()
            .unwrap_or(0)
    }

    /// Appends the nodes of `other`.
    pub fn push(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Serializes a whole page, adding `<!doctype html>`.
    ///
    /// # Errors
    ///
    /// Fails unless the fragment is exactly one `html` element.
    pub fn to_document(&self) -> Result<String, Error> {
        serializer::document(&self.0)
    }

    /// Serializes the nodes as they are.
    #[must_use]
    pub fn to_fragment(&self) -> String {
        serializer::fragment(&self.0)
    }
}

impl FromIterator<Html> for Html {
    fn from_iter<I: IntoIterator<Item = Html>>(fragments: I) -> Self {
        Self(fragments.into_iter().flat_map(|html| html.0).collect())
    }
}

pub(crate) fn build_element(
    spec: &'static ElementSpec,
    attrs: Vec<(String, String)>,
    children: Html,
) -> Result<Html, Error> {
    let depth = children.depth().saturating_add(1);
    if depth > MAX_DEPTH {
        return Err(Error::new(
            ErrorKind::Html,
            format!("elements are nested more than {MAX_DEPTH} levels deep"),
        ));
    }
    if spec.void && !children.is_empty() {
        return Err(Error::new(
            ErrorKind::Html,
            format!("<{}> is a void element and cannot have children", spec.name),
        ));
    }
    for (position, (name, value)) in attrs.iter().enumerate() {
        check_attribute_name(name)?;
        if attrs
            .iter()
            .take(position)
            .any(|(earlier, _)| earlier == name)
        {
            return Err(Error::new(
                ErrorKind::Html,
                format!("attribute {name} is given twice"),
            ));
        }
        if URL_ATTRIBUTES.contains(&name.as_str()) {
            check_url(name, value)?;
        }
    }
    Ok(Html(vec![Node::Element {
        spec,
        attrs,
        children: children.0,
        depth,
    }]))
}

pub(crate) fn check_attribute_name(name: &str) -> Result<(), Error> {
    // Scripts and CSS live outside the HTML: in no page at all, and in the CSS files.
    if name.starts_with("on") || name == "style" {
        return Err(Error::new(
            ErrorKind::Html,
            format!("attribute {name} is not allowed; scripts and CSS do not go in attributes"),
        ));
    }
    let mut characters = name.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::Html,
            format!("invalid attribute name {name:?}; use lowercase letters, digits, and -"),
        ))
    }
}

/// Rejects URLs whose scheme is not allowed. Browsers strip leading and trailing control
/// characters and spaces and remove tabs and newlines before reading the scheme, so this does the
/// same to see the scheme the browser would see.
fn check_url(attribute: &str, value: &str) -> Result<(), Error> {
    let url = value
        .trim_matches(|character: char| character <= ' ')
        .chars()
        .filter(|character| !matches!(character, '\t' | '\n' | '\r'))
        .collect::<String>();
    let before_path = url.split(['/', '?', '#']).next().unwrap_or_default();
    if let Some((scheme, _)) = before_path.split_once(':')
        && is_scheme(scheme)
        && !URL_SCHEMES
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    {
        return Err(Error::new(
            ErrorKind::Html,
            format!(
                "{attribute} has the URL scheme {scheme}:, but only {} and relative URLs are allowed",
                URL_SCHEMES.join(":, ") + ":"
            ),
        ));
    }
    Ok(())
}

fn is_scheme(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn raw_text(kind: RawText, text: String) -> Result<Html, Error> {
    let lowercase = text.to_ascii_lowercase();
    if let Some(sequence) = ["</style", "</script", "<!--"]
        .into_iter()
        .find(|sequence| lowercase.contains(sequence))
    {
        return Err(Error::new(
            ErrorKind::Html,
            format!("{} must not contain {sequence}", kind.description()),
        ));
    }
    Ok(Html(vec![Node::RawText { kind, text }]))
}

impl RawText {
    fn description(&self) -> &'static str {
        match self {
            Self::Style => "CSS",
            Self::Json { .. } => "JSON",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ELEMENTS, Html, check_url};
    use crate::error::Error;

    #[test]
    fn url_attributes_allow_only_web_and_mail_schemes() {
        for allowed in [
            "/entries/a/",
            "../a",
            "a.png",
            "#top",
            "?q=1",
            "",
            "//example.com/",
            "https://example.com/",
            "HTTP://example.com/",
            "mailto:a@example.com",
            "a/b:c",
            "a?b:c",
            "1a:b",
        ] {
            assert!(check_url("href", allowed).is_ok(), "{allowed}");
        }
        for rejected in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "\u{1}javascript:alert(1)",
            "java\nscript:alert(1)",
            "java\tscript:alert(1)",
            "data:text/html,<p>",
            "vbscript:x",
            "custom+v1.2-test:target",
            "tel:+81-3-0000-0000",
        ] {
            assert!(check_url("href", rejected).is_err(), "{rejected:?}");
        }
    }

    #[test]
    fn raw_text_cannot_end_its_element() -> Result<(), Error> {
        assert!(Html::style("a</STYLE>").is_err());
        assert!(Html::style("a</script").is_err());
        assert!(Html::json("application/json", "\"</Script>\"").is_err());
        assert!(Html::json("application/ld+json", "<!--").is_err());
        for media_type in [
            "text/javascript",
            "module",
            "",
            "application/ld+json ",
            "application/+json",
            "Application/JSON",
        ] {
            assert!(Html::json(media_type, "{}").is_err(), "{media_type:?}");
        }
        assert_eq!(
            Html::json("application/ld+json", "{\"a\":\"\\u003c/script\\u003e\"}")?.to_fragment(),
            "<script type=\"application/ld+json\">{\"a\":\"\\u003c/script\\u003e\"}</script>"
        );
        Ok(())
    }

    #[test]
    fn elements_check_names_attributes_and_void_children() -> Result<(), Error> {
        assert!(Html::element("script", Vec::new(), Html::default()).is_err());
        assert!(Html::element("img", Vec::new(), Html::text("x")).is_err());
        let attrs = |name: &str| vec![(name.to_owned(), "x".to_owned())];
        assert!(Html::element("p", attrs("Class"), Html::default()).is_err());
        assert!(Html::element("p", attrs("a b"), Html::default()).is_err());
        assert!(Html::element("p", attrs("\"x"), Html::default()).is_err());
        assert!(Html::element("p", attrs("onclick"), Html::default()).is_err());
        assert!(Html::element("p", attrs("style"), Html::default()).is_err());
        let twice = vec![
            ("id".to_owned(), "a".to_owned()),
            ("id".to_owned(), "b".to_owned()),
        ];
        assert!(Html::element("p", twice, Html::default()).is_err());
        assert!(
            Html::element(
                "a",
                vec![("href".to_owned(), "javascript:x".to_owned())],
                Html::default()
            )
            .is_err()
        );
        let image = Html::element(
            "img",
            vec![("src".to_owned(), "/a.png".to_owned())],
            Html::default(),
        )?;
        assert_eq!(image.to_fragment(), "<img src=\"/a.png\">");
        Ok(())
    }

    #[test]
    fn elements_nest_at_most_256_levels() -> Result<(), Error> {
        let mut html = Html::text("deep");
        for _ in 0..256 {
            html = Html::element("div", Vec::new(), html)?;
        }
        assert!(html.to_fragment().starts_with("<div><div>"));
        let error = Html::element("div", Vec::new(), html.clone())
            .err()
            .map(|error| error.to_string());
        assert_eq!(
            error.as_deref(),
            Some("elements are nested more than 256 levels deep")
        );
        let beside = [Html::text("x"), html].into_iter().collect::<Html>();
        assert!(Html::element("p", Vec::new(), beside).is_err());
        Ok(())
    }

    /// The VS Code grammar lists the element functions to color them as tags. It lives in this
    /// repository so that both change together; this keeps them from drifting apart.
    #[test]
    fn the_vs_code_grammar_highlights_exactly_the_elements() {
        let grammar = include_str!("../editors/vscode/syntaxes/bitview.tmLanguage.json");
        let mut highlighted = grammar
            .split("(?<![a-z0-9.-])(")
            .skip(1)
            .filter_map(|rest| rest.split_once(')').map(|(group, _)| group))
            .find(|group| group.split('|').any(|name| name == "html"))
            .map(|group| group.split('|').collect::<Vec<_>>())
            .unwrap_or_default();
        let mut elements = ELEMENTS.iter().map(|spec| spec.name).collect::<Vec<_>>();
        highlighted.sort_unstable();
        elements.sort_unstable();
        assert_eq!(highlighted, elements);
    }
}

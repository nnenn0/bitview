//! The only place that turns HTML into text, and so the only place that escapes.

use crate::{
    error::{Error, ErrorKind},
    html::{Node, RawText},
};

pub(crate) fn document(nodes: &[Node]) -> Result<String, Error> {
    let [Node::Element { spec, .. }] = nodes else {
        return Err(Error::new(
            ErrorKind::Html,
            "a page must be exactly one <html> element",
        ));
    };
    if spec.name != "html" {
        return Err(Error::new(
            ErrorKind::Html,
            format!("a page must be an <html> element, not <{}>", spec.name),
        ));
    }
    let mut output = String::from("<!doctype html>");
    write_nodes(&mut output, nodes);
    Ok(output)
}

pub(crate) fn fragment(nodes: &[Node]) -> String {
    let mut output = String::new();
    write_nodes(&mut output, nodes);
    output
}

fn write_nodes(output: &mut String, nodes: &[Node]) {
    for node in nodes {
        match node {
            Node::Text(text) => escape(output, text),
            Node::Element {
                spec,
                attrs,
                children,
                ..
            } => {
                output.push('<');
                output.push_str(spec.name);
                for (name, value) in attrs {
                    output.push(' ');
                    output.push_str(name);
                    output.push_str("=\"");
                    escape(output, value);
                    output.push('"');
                }
                output.push('>');
                if !spec.void {
                    write_nodes(output, children);
                    output.push_str("</");
                    output.push_str(spec.name);
                    output.push('>');
                }
            }
            Node::RawText { kind, text } => {
                match kind {
                    RawText::Style => output.push_str("<style>"),
                    RawText::Json { media_type } => {
                        output.push_str("<script type=\"");
                        output.push_str(media_type);
                        output.push_str("\">");
                    }
                }
                output.push_str(text);
                output.push_str(match kind {
                    RawText::Style => "</style>",
                    RawText::Json { .. } => "</script>",
                });
            }
        }
    }
}

/// Attribute values are always double-quoted, so one escaping serves them and element content.
fn escape(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            other => output.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Html, error::Error};

    #[test]
    fn escapes_text_and_attributes() -> Result<(), Error> {
        let html = Html::element(
            "a",
            vec![
                ("href".to_owned(), "/?a=1&b=\"2\"".to_owned()),
                ("title".to_owned(), "'<x>'".to_owned()),
            ],
            Html::text("<script>alert('&')</script>"),
        )?;
        assert_eq!(
            html.to_fragment(),
            "<a href=\"/?a=1&amp;b=&quot;2&quot;\" title=\"&#39;&lt;x&gt;&#39;\">&lt;script&gt;alert(&#39;&amp;&#39;)&lt;/script&gt;</a>"
        );
        Ok(())
    }

    #[test]
    fn documents_are_one_html_element() -> Result<(), Error> {
        let page = Html::element("html", Vec::new(), Html::default())?;
        assert_eq!(page.to_document()?, "<!doctype html><html></html>");
        assert!(Html::text("x").to_document().is_err());
        assert!(Html::default().to_document().is_err());
        assert!(
            Html::element("body", Vec::new(), Html::default())?
                .to_document()
                .is_err()
        );
        let twice = [page.clone(), page].into_iter().collect::<Html>();
        assert!(twice.to_document().is_err());
        Ok(())
    }
}

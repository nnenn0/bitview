use bitview::{Error as BitviewError, ErrorKind, Html, Program, Source, Value};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn parse(text: &str) -> std::result::Result<Program, BitviewError> {
    Program::parse(&[Source {
        name: "t.bitview",
        text,
    }])
}

fn render(text: &str, ctx: Value) -> std::result::Result<String, BitviewError> {
    Ok(parse(text)?.render("page", ctx)?.to_fragment())
}

fn parse_error(text: &str) -> Result<BitviewError> {
    parse(text)
        .err()
        .ok_or_else(|| format!("parsed: {text}").into())
}

fn render_error(text: &str, ctx: Value) -> Result<BitviewError> {
    render(text, ctx)
        .err()
        .ok_or_else(|| format!("rendered: {text}").into())
}

fn empty() -> Value {
    Value::record::<&str>([])
}

#[test]
fn strings_are_escaped_and_html_is_inserted_as_built() -> Result<()> {
    let ctx = Value::record([
        ("text", Value::from("<script>alert('&')</script>")),
        (
            "content",
            Value::from(Html::element("em", Vec::new(), Html::text("<b>"))?),
        ),
    ]);
    assert_eq!(
        render("fn page(ctx) => p(ctx.text, ctx.content)", ctx)?,
        "<p>&lt;script&gt;alert(&#39;&amp;&#39;)&lt;/script&gt;<em>&lt;b&gt;</em></p>"
    );
    Ok(())
}

#[test]
fn attributes_are_escaped_in_order() -> Result<()> {
    assert_eq!(
        render(
            r#"fn page(ctx) => a({title: "\"><x", href: "/a?b=1&c=2", class: "x"}, "t")"#,
            empty()
        )?,
        "<a title=\"&quot;&gt;&lt;x\" href=\"/a?b=1&amp;c=2\" class=\"x\">t</a>"
    );
    assert_eq!(
        render(
            r#"fn page(ctx) => meta({http-equiv: "x", "content": "y"})"#,
            empty()
        )?,
        "<meta http-equiv=\"x\" content=\"y\">"
    );
    Ok(())
}

#[test]
fn templates_cannot_write_scripts_styles_or_unsafe_urls() -> Result<()> {
    for text in [
        r#"fn page(ctx) => a({onclick: "x"}, "t")"#,
        r#"fn page(ctx) => p({style: "color: red"}, "t")"#,
        r#"fn page(ctx) => a({href: "javascript:alert(1)"}, "t")"#,
        r#"fn page(ctx) => a({href: " JAVA\tSCRIPT:alert(1)"}, "t")"#,
        r#"fn page(ctx) => img({src: "data:image/png;base64,AAAA"})"#,
    ] {
        let error = render_error(text, empty())?;
        assert_eq!(error.kind(), ErrorKind::Html, "{text}: {error}");
        assert_eq!(error.span().map(bitview::Span::line), Some(1), "{error}");
    }
    let ctx = Value::record([("url", Value::from("javascript:alert(1)"))]);
    assert!(render_error(r#"fn page(ctx) => a({href: ctx.url}, "t")"#, ctx).is_ok());
    for text in [
        r#"fn page(ctx) => script("x")"#,
        r#"fn page(ctx) => style("x")"#,
        r#"fn page(ctx) => raw("<b>")"#,
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Name, "{text}");
    }
    Ok(())
}

#[test]
fn void_elements_have_no_children_or_end_tag() -> Result<()> {
    assert_eq!(
        render(r#"fn page(ctx) => p("a", br(), "b")"#, empty())?,
        "<p>a<br>b</p>"
    );
    let error = render_error(r#"fn page(ctx) => img({src: "/a.png"}, "x")"#, empty())?;
    assert_eq!(error.kind(), ErrorKind::Html);
    Ok(())
}

#[test]
fn children_flatten_lists_and_reject_other_values() -> Result<()> {
    assert_eq!(
        render(r#"fn page(ctx) => p(["a", ["b", []], "c"])"#, empty())?,
        "<p>abc</p>"
    );
    let ctx = Value::record([("flag", Value::from(true))]);
    assert_eq!(
        render_error("fn page(ctx) => p(ctx.flag)", ctx)?.kind(),
        ErrorKind::Type
    );
    let error = render_error(r#"fn page(ctx) => p("a", {class: "x"})"#, empty())?;
    assert!(error.message().contains("first argument"), "{error}");
    Ok(())
}

#[test]
fn if_needs_a_bool_and_else_can_render_nothing() -> Result<()> {
    let text = r#"fn page(ctx) => p(if ctx.draft then span("draft") else [])"#;
    let with = |draft: Value| Value::record([("draft", draft)]);
    assert_eq!(
        render(text, with(Value::from(true)))?,
        "<p><span>draft</span></p>"
    );
    assert_eq!(render(text, with(Value::from(false)))?, "<p></p>");
    let error = render_error(text, with(Value::from("yes")))?;
    assert_eq!(error.kind(), ErrorKind::Type);
    assert!(error.message().contains("Bool"), "{error}");
    Ok(())
}

#[test]
fn map_applies_a_named_function() -> Result<()> {
    let text = "fn page(ctx) => ul(map(ctx.items, item))\nfn item(x) => li(x.name)";
    let items = |names: &[&str]| {
        Value::record([(
            "items",
            Value::from(
                names
                    .iter()
                    .map(|name| Value::record([("name", Value::from(*name))]))
                    .collect::<Vec<_>>(),
            ),
        )])
    };
    assert_eq!(
        render(text, items(&["a", "b"]))?,
        "<ul><li>a</li><li>b</li></ul>"
    );
    assert_eq!(render(text, items(&[]))?, "<ul></ul>");
    let nested = "fn page(ctx) => div(map(ctx.rows, row))\nfn row(r) => p(map(r.cells, cell))\nfn cell(c) => span(c)";
    let row = |cells: &[&str]| {
        Value::record([(
            "cells",
            Value::from(
                cells
                    .iter()
                    .map(|cell| Value::from(*cell))
                    .collect::<Vec<_>>(),
            ),
        )])
    };
    let ctx = Value::record([("rows", Value::from(vec![row(&["a", "b"]), row(&["c"])]))]);
    assert_eq!(
        render(nested, ctx)?,
        "<div><p><span>a</span><span>b</span></p><p><span>c</span></p></div>"
    );
    for text in [
        "fn page(ctx) => ul(map(ctx.items, p))",
        "fn page(ctx) => ul(map(ctx.items, missing))",
        "fn page(ctx) => ul(map(ctx.items))",
        "fn page(ctx) => ul(map(ctx.items, two))\nfn two(x, y) => x",
        "fn page(ctx) => ul(map(ctx.items, ctx))",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
    assert_eq!(
        render_error(text, Value::record([("items", Value::from("x"))]))?.kind(),
        ErrorKind::Type
    );
    Ok(())
}

#[test]
fn concat_joins_strings_only() -> Result<()> {
    let ctx = Value::record([("title", Value::from("A & B"))]);
    assert_eq!(
        render(
            r#"fn page(ctx) => title(concat(ctx.title, " | ", "Site"))"#,
            ctx
        )?,
        "<title>A &amp; B | Site</title>"
    );
    let ctx = Value::record([("flag", Value::from(true))]);
    assert_eq!(
        render_error(r#"fn page(ctx) => p(concat("a", ctx.flag))"#, ctx)?.kind(),
        ErrorKind::Type
    );
    assert_eq!(
        parse_error("fn page(ctx) => p(concat())")?.kind(),
        ErrorKind::Arity
    );
    Ok(())
}

#[test]
fn layouts_are_functions_that_take_the_parts_of_a_page() -> Result<()> {
    let text = r#"
        fn layout(page-title, content) => html(head(title(page-title)), body(content))
        fn page(ctx) => layout(concat("Post | ", ctx.site), [h1("Post"), p("Body")])
    "#;
    let page = parse(text)?
        .render("page", Value::record([("site", Value::from("Blog"))]))?
        .to_document()?;
    assert_eq!(
        page,
        "<!doctype html><html><head><title>Post | Blog</title></head><body><h1>Post</h1><p>Body</p></body></html>"
    );
    Ok(())
}

#[test]
fn records_and_fields() -> Result<()> {
    let ctx = Value::record([(
        "post",
        Value::record([("meta", Value::record([("title", Value::from("T"))]))]),
    )]);
    assert_eq!(
        render("fn page(ctx) => p(ctx.post.meta.title)", ctx.clone())?,
        "<p>T</p>"
    );
    assert_eq!(
        render(
            "fn page(ctx) => p(pick({a: \"x\", b: ctx.post.meta.title}))\nfn pick(r) => r.b",
            ctx.clone()
        )?,
        "<p>T</p>"
    );
    assert_eq!(
        render(
            "fn page(ctx) => p(wrap(ctx.post).inner.meta.title)\nfn wrap(x) => {inner: x}",
            ctx.clone()
        )?,
        "<p>T</p>"
    );
    let error = render_error(
        "fn page(ctx) => p(wrap(ctx.post).inner.titel)\nfn wrap(x) => {inner: x}",
        ctx.clone(),
    )?;
    assert_eq!(error.kind(), ErrorKind::Field);
    assert_eq!(error.span().map(bitview::Span::column), Some(40));
    let error = render_error("fn page(ctx) => p(ctx.post.titel)", ctx.clone())?;
    assert_eq!(error.kind(), ErrorKind::Field);
    assert!(error.message().contains("(fields: meta)"), "{error}");
    let error = render_error("fn page(ctx) => p(ctx.post.meta.title.x)", ctx)?;
    assert_eq!(error.kind(), ErrorKind::Type);
    assert_eq!(
        parse_error("fn page(ctx) => p({a: \"x\", a: \"y\"}.a)")?.kind(),
        ErrorKind::Name
    );
    Ok(())
}

#[test]
fn errors_point_to_the_source_and_the_running_functions() -> Result<()> {
    let text =
        "fn page(ctx) =>\n  div(meta-line(ctx.post))\n\nfn meta-line(post) =>\n  p(post.titel)";
    let ctx = Value::record([("post", Value::record([("title", Value::from("T"))]))]);
    let error = render_error(text, ctx)?;
    assert_eq!(
        error.to_string(),
        "t.bitview:5:10: unknown field \"titel\" (fields: title)\n  in meta-line (called at t.bitview:2:7)\n  in page"
    );
    let error = parse_error("fn page(ctx) =>\n  p(\"a\" \"b\")")?;
    assert_eq!(
        error.to_string(),
        "t.bitview:2:9: expected `,` or `)`, found a string"
    );
    Ok(())
}

#[test]
fn names_are_checked_before_rendering() -> Result<()> {
    for (text, kind) in [
        ("fn page(ctx) => p(missing)", ErrorKind::Name),
        ("fn page(ctx) => missing(ctx)", ErrorKind::Name),
        (
            "fn page(ctx) => p(helper)\nfn helper(x) => x",
            ErrorKind::Name,
        ),
        ("fn page(ctx) => ctx(1)", ErrorKind::Syntax),
        ("fn page(ctx) => ctx()", ErrorKind::Name),
        (
            "fn page(ctx) => p(helper(ctx))\nfn helper(ctx) => ctx\nfn page(x) => x",
            ErrorKind::Name,
        ),
        (
            "fn page(ctx) => p(helper(ctx, ctx))\nfn helper(x) => x",
            ErrorKind::Arity,
        ),
        ("fn p(ctx) => ctx", ErrorKind::Name),
        ("fn page(x, x) => x", ErrorKind::Name),
        ("fn page(ctx) => P(ctx)", ErrorKind::Syntax),
        ("fn page(ctx) => p(1)", ErrorKind::Syntax),
        ("fn page(ctx) => p(\"a)", ErrorKind::Syntax),
        ("fn page(ctx) => p(\"\\q\")", ErrorKind::Syntax),
        ("fn page(ctx) => p(\"\\u{110000}\")", ErrorKind::Syntax),
        ("fn page(ctx) =>", ErrorKind::Syntax),
        ("page(ctx)", ErrorKind::Syntax),
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), kind, "{text}: {error}");
        assert!(error.span().is_some(), "{text}: {error}");
    }
    assert_eq!(
        render(r#"fn page(ctx) => p("\u{1F600}\n\t\"\\")"#, empty())?,
        "<p>\u{1F600}\n\t&quot;\\</p>"
    );
    Ok(())
}

#[test]
fn names_are_lowercase_words_joined_by_hyphens() -> Result<()> {
    let text =
        "fn page(ctx) => entry-list(ctx.created-at)\nfn entry-list(created-at) => p(created-at)";
    let ctx = Value::record([("created-at", Value::from("today"))]);
    assert_eq!(render(text, ctx)?, "<p>today</p>");
    for text in [
        "fn page(ctx) => entry_list(ctx)",
        "fn page(ctx) => p(ctx.created_at)",
        "fn page(ctx) => entry-(ctx)",
        "fn page(ctx) => entry--list(ctx)",
        "fn page(ctx) => -entry(ctx)",
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Syntax, "{text}: {error}");
    }
    let error = parse_error("fn page(ctx) => entry_list(ctx)")?;
    assert!(error.message().contains("entry-list"), "{error}");
    assert_eq!(error.span().map(bitview::Span::column), Some(17));
    Ok(())
}

#[test]
fn parameters_hide_functions_of_the_same_name() -> Result<()> {
    let text = "fn page(ctx) => heading(ctx.title)\nfn heading(title) => h1(title)";
    let ctx = Value::record([("title", Value::from("T"))]);
    assert_eq!(render(text, ctx)?, "<h1>T</h1>");
    for text in [
        "fn page(title) => html(head(title(title)))",
        "fn page(item) => ul(map(item, item))\nfn item(x) => li(x)",
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Name, "{text}");
        assert!(
            error.message().ends_with("is a parameter, not a function"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn comments_run_from_two_hyphens_to_the_end_of_the_line() -> Result<()> {
    let text = "-- A page.\nfn page(ctx) => -- the body\n  p(\"-- not a comment\") --";
    assert_eq!(render(text, empty())?, "<p>-- not a comment</p>");
    for text in ["fn page(ctx) => p(ctx.title--)", "fn page(ctx) => - p(ctx)"] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Syntax, "{text}");
    }
    Ok(())
}

#[test]
fn recursion_is_rejected_before_rendering() -> Result<()> {
    for text in [
        "fn page(ctx) => page(ctx)",
        "fn page(ctx) => f(ctx)\nfn f(x) => g(x)\nfn g(x) => f(x)",
        "fn page(ctx) => ul(map(ctx, item))\nfn item(x) => ul(map(x, item))",
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Recursion, "{text}");
    }
    let error = parse_error("fn page(ctx) => f(ctx)\nfn f(x) => g(x)\nfn g(x) => f(x)")?;
    assert!(error.message().ends_with("f -> g -> f"), "{error}");
    Ok(())
}

#[test]
fn deep_nesting_fails_without_exhausting_the_stack() -> Result<()> {
    let depth = 10_000;
    for (open, close) in [("p(", ")"), ("[", "]"), ("(", ")")] {
        let text = format!(
            "fn page(ctx) => {}ctx{}",
            open.repeat(depth),
            close.repeat(depth)
        );
        let error = parse_error(&text)?;
        assert_eq!(error.kind(), ErrorKind::Syntax);
        assert!(error.message().contains("nested"), "{error}");
    }
    let text = format!(
        "fn page(ctx) => {}\"x\"{}",
        "p(".repeat(100),
        ")".repeat(100)
    );
    assert!(render(&text, empty())?.starts_with("<p><p>"));
    Ok(())
}

#[test]
fn entry_functions_take_one_value_and_return_html() -> Result<()> {
    let program = parse("fn page(ctx) => {a: \"text\"}\nfn two(x, y) => x")?;
    assert_eq!(
        program
            .render("page", empty())
            .err()
            .as_ref()
            .map(BitviewError::kind),
        Some(ErrorKind::Type)
    );
    assert_eq!(
        program
            .render("two", empty())
            .err()
            .as_ref()
            .map(BitviewError::kind),
        Some(ErrorKind::Arity)
    );
    assert_eq!(
        program
            .render("none", empty())
            .err()
            .as_ref()
            .map(BitviewError::kind),
        Some(ErrorKind::Name)
    );
    assert!(!program.has_entry("two"));
    Ok(())
}

#[test]
fn functions_share_one_namespace_across_sources() -> Result<()> {
    let program = Program::parse(&[
        Source {
            name: "a.bitview",
            text: "fn page(ctx) => p(helper(ctx))",
        },
        Source {
            name: "b.bitview",
            text: "fn helper(x) => x.name",
        },
    ])?;
    let ctx = Value::record([("name", Value::from("n"))]);
    assert_eq!(program.render("page", ctx)?.to_fragment(), "<p>n</p>");
    let error = Program::parse(&[
        Source {
            name: "a.bitview",
            text: "fn page(ctx) => ctx",
        },
        Source {
            name: "b.bitview",
            text: "\nfn page(ctx) => ctx",
        },
    ])
    .err()
    .ok_or("accepted a duplicate function")?;
    assert_eq!(
        error.to_string(),
        "b.bitview:2:1: function page is defined twice (first defined at a.bitview:1:1)"
    );
    Ok(())
}

#[test]
fn entry_results_that_stand_for_html_render_as_fragments() -> Result<()> {
    assert_eq!(
        render(r#"fn page(ctx) => [p("a"), "b", []]"#, empty())?,
        "<p>a</p>b"
    );
    assert_eq!(render(r#"fn page(ctx) => "text""#, empty())?, "text");
    Ok(())
}

#[test]
fn void_elements_take_no_child_arguments() -> Result<()> {
    for text in [r#"fn page(ctx) => br("x")"#, "fn page(ctx) => br([])"] {
        assert_eq!(
            render_error(text, empty())?.kind(),
            ErrorKind::Html,
            "{text}"
        );
    }
    Ok(())
}

use bitview::{
    Error as BitviewError, ErrorKind, Html, HtmlType, Program, Source, Type, Value, declare_types,
};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn parse(text: &str) -> std::result::Result<Program, BitviewError> {
    Program::parse(&[Source { name: "t.bv", text }], &[("Post", post_type())])
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

fn check_error(text: &str, ctx: &Type) -> Result<BitviewError> {
    parse(text)?
        .check(&[("page", ctx.clone())])
        .err()
        .ok_or_else(|| format!("checked: {text}").into())
}

fn empty() -> Value {
    Value::record::<&str>([])
}

/// The host type that the tests name `Post`.
fn post_type() -> Type {
    Type::record([
        ("flag", Type::Bool),
        ("title", Type::String),
        ("items", Type::list(Type::record([("name", Type::String)]))),
    ])
}

fn post(flag: bool, items: &[&str]) -> Value {
    Value::record([
        ("flag", Value::from(flag)),
        ("title", Value::from("T")),
        (
            "items",
            Value::from(
                items
                    .iter()
                    .map(|name| Value::record([("name", Value::from(*name))]))
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}

fn sources<'a>(texts: &[(&'a str, &'a str)]) -> Vec<Source<'a>> {
    texts
        .iter()
        .map(|&(name, text)| Source { name, text })
        .collect()
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
        render(
            "(defn page [ctx {:text String :content Phrasing}] (p ctx.text ctx.content))",
            ctx
        )?,
        "<p>&lt;script&gt;alert(&#39;&amp;&#39;)&lt;/script&gt;<em>&lt;b&gt;</em></p>"
    );
    Ok(())
}

#[test]
fn attributes_are_escaped_in_order() -> Result<()> {
    assert_eq!(
        render(
            r#"(defn page [ctx {}] (a {:title "\"><x" :href "/a?b=1&c=2" :class "x"} "t"))"#,
            empty()
        )?,
        "<a title=\"&quot;&gt;&lt;x\" href=\"/a?b=1&amp;c=2\" class=\"x\">t</a>"
    );
    assert_eq!(
        render(
            r#"(defn page [ctx {}] (span {:aria-label "x" :data-id "y"}))"#,
            empty()
        )?,
        "<span aria-label=\"x\" data-id=\"y\"></span>"
    );
    Ok(())
}

#[test]
fn templates_cannot_write_scripts_styles_or_unsafe_urls() -> Result<()> {
    for text in [
        r#"(defn page [ctx {}] (a {:onclick "x"} "t"))"#,
        r#"(defn page [ctx {}] (p {:style "color: red"} "t"))"#,
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Html, "{text}: {error}");
        assert_eq!(error.span().map(bitview::Span::line), Some(1), "{error}");
    }
    // A URL is a value, so its scheme is checked when the page renders.
    for text in [
        r#"(defn page [ctx {}] (a {:href "javascript:alert(1)"} "t"))"#,
        r#"(defn page [ctx {}] (a {:href " JAVA\tSCRIPT:alert(1)"} "t"))"#,
        r#"(defn page [ctx {}] (img {:src "data:image/png;base64,AAAA"}))"#,
    ] {
        let error = render_error(text, empty())?;
        assert_eq!(error.kind(), ErrorKind::Html, "{text}: {error}");
        assert_eq!(error.span().map(bitview::Span::line), Some(1), "{error}");
    }
    let ctx = Value::record([("url", Value::from("javascript:alert(1)"))]);
    assert!(
        render_error(
            r#"(defn page [ctx {:url String}] (a {:href ctx.url} "t"))"#,
            ctx
        )
        .is_ok()
    );
    for text in [
        r#"(defn page [ctx {}] (script "x"))"#,
        r#"(defn page [ctx {}] (style "x"))"#,
        r#"(defn page [ctx {}] (raw "<b>"))"#,
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Name, "{text}");
    }
    Ok(())
}

#[test]
fn void_elements_have_no_children_or_end_tag() -> Result<()> {
    assert_eq!(
        render(r#"(defn page [ctx {}] (p "a" (br) "b"))"#, empty())?,
        "<p>a<br>b</p>"
    );
    for text in [
        r#"(defn page [ctx {}] (img {:src "/a.png"} "x"))"#,
        r#"(defn page [ctx {}] (br "x"))"#,
        "(defn page [ctx {}] (br []))",
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Html, "{text}");
    }
    Ok(())
}

#[test]
fn children_flatten_lists_and_reject_other_values() -> Result<()> {
    assert_eq!(
        render(r#"(defn page [ctx {}] (p ["a" ["b" []] "c"]))"#, empty())?,
        "<p>abc</p>"
    );
    assert_eq!(
        parse_error("(defn page [ctx {:flag Bool}] (p ctx.flag))")?.kind(),
        ErrorKind::Type
    );
    let error = parse_error(r#"(defn page [ctx {}] (p "a" {:class "x"}))"#)?;
    assert!(error.message().contains("first argument"), "{error}");
    Ok(())
}

#[test]
fn if_needs_a_bool_and_else_can_render_nothing() -> Result<()> {
    let text = r#"(defn page [ctx {:draft Bool}] (p (if ctx.draft (span "draft") [])))"#;
    let with = |draft: Value| Value::record([("draft", draft)]);
    assert_eq!(
        render(text, with(Value::from(true)))?,
        "<p><span>draft</span></p>"
    );
    assert_eq!(render(text, with(Value::from(false)))?, "<p></p>");
    let error = parse_error(r#"(defn page [ctx {:draft String}] (p (if ctx.draft "a" "b")))"#)?;
    assert_eq!(error.kind(), ErrorKind::Type);
    assert!(error.message().contains("Bool"), "{error}");
    Ok(())
}

#[test]
fn map_applies_a_named_function() -> Result<()> {
    let text = "(defn page [ctx Post] (ul (map ctx.items item)))\n(defn item [x {:name String}] (li x.name))";
    assert_eq!(
        render(text, post(true, &["a", "b"]))?,
        "<ul><li>a</li><li>b</li></ul>"
    );
    assert_eq!(render(text, post(true, &[]))?, "<ul></ul>");
    let nested = "(defn page [ctx {:rows [{:cells [String]}]}] (div (map ctx.rows row)))\n(defn row [r {:cells [String]}] (p (map r.cells cell)))\n(defn cell [c String] (span c))";
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
        "(defn page [ctx Post] (ul (map ctx.items p)))",
        "(defn page [ctx Post] (ul (map ctx.items missing)))",
        "(defn page [ctx Post] (ul (map ctx.items)))",
        "(defn page [ctx Post] (ul (map ctx.items two)))\n(defn two [x {} y {}] (li))",
        "(defn page [ctx Post] (ul (map ctx.items ctx)))",
        "(defn page [ctx Post] (ul (map ctx.title item)))\n(defn item [x String] (li x))",
        "(defn page [ctx Post] (ul (map ctx.items item)))\n(defn item [x {:title String}] (li x.title))",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
    Ok(())
}

#[test]
fn concat_joins_strings_only() -> Result<()> {
    let ctx = Value::record([("title", Value::from("A & B"))]);
    assert_eq!(
        render(
            r#"(defn page [ctx {:title String}] (title (concat ctx.title " | " "Site")))"#,
            ctx
        )?,
        "<title>A &amp; B | Site</title>"
    );
    assert_eq!(
        parse_error(r#"(defn page [ctx Post] (p (concat "a" ctx.flag)))"#)?.kind(),
        ErrorKind::Type
    );
    assert_eq!(
        parse_error("(defn page [ctx {}] (p (concat)))")?.kind(),
        ErrorKind::Arity
    );
    Ok(())
}

#[test]
fn layouts_are_functions_that_take_the_parts_of_a_page() -> Result<()> {
    let text = r#"(defn layout [page-title String content Flow] (html (head (title page-title)) (body content)))
(defn page [ctx {:site String}] (layout (concat "Post | " ctx.site) [(h1 "Post") (p "Body")]))"#;
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
    let page = "(defn page [ctx {:post {:meta {:title String}}}]";
    assert_eq!(
        render(&format!("{page} (p ctx.post.meta.title))"), ctx.clone())?,
        "<p>T</p>"
    );
    assert_eq!(
        render(
            &format!(
                "{page} (p (pick {{:a \"x\" :b ctx.post.meta.title}})))\n(defn pick [r {{:b String}}] r.b)"
            ),
            ctx.clone()
        )?,
        "<p>T</p>"
    );
    // Only a name has fields; a record a function returns is read through a parameter.
    let wrapped = format!(
        "{page} (p (title-of (wrap ctx.post))))\n(defn wrap [x {{:meta {{:title String}}}}] {{:inner x}})"
    );
    let inner = "(defn title-of [w {:inner {:meta {:title String}}}]";
    assert_eq!(
        render(&format!("{wrapped}\n{inner} w.inner.meta.title)"), ctx)?,
        "<p>T</p>"
    );
    let error = parse_error(&format!("{wrapped}\n{inner} w.inner.titel)"))?;
    assert_eq!(error.kind(), ErrorKind::Field);
    assert_eq!(error.span().map(bitview::Span::column), Some(61));
    assert_eq!(
        parse_error(&format!("{wrapped}\n{inner} (wrap w).inner)"))?.kind(),
        ErrorKind::Syntax
    );
    let error = parse_error(&format!("{page} (p ctx.post.titel))"))?;
    assert_eq!(error.kind(), ErrorKind::Field);
    assert!(error.message().contains("(fields: meta)"), "{error}");
    let error = parse_error(&format!("{page} (p ctx.post.meta.title.x))"))?;
    assert_eq!(error.kind(), ErrorKind::Type);
    assert_eq!(
        parse_error(
            "(defn page [ctx {}] (p (pick {:a \"x\" :a \"y\"})))\n(defn pick [r {:a String}] r.a)"
        )?
        .kind(),
        ErrorKind::Name
    );
    Ok(())
}

#[test]
fn fields_are_read_from_records_that_functions_return() -> Result<()> {
    let text = "(defn page [ctx {:note String}] (show (info ctx)))\n(defn show [i {:title String :note String}] (p i.title i.note))\n(defn info [ctx {:note String}] {:title \"T\" :note ctx.note})";
    let ctx = Value::record([("note", Value::from("n"))]);
    assert_eq!(render(text, ctx)?, "<p>Tn</p>");
    let error = parse_error(
        "(defn page [ctx {}] (show (info ctx)))\n(defn show [i {:title String}] (p i.other))\n(defn info [ctx {}] {:title \"T\"})",
    )?;
    assert_eq!(error.message(), "unknown field \"other\" (fields: title)");
    Ok(())
}

#[test]
fn errors_point_to_the_source_and_the_function() -> Result<()> {
    let text = "(defn page [ctx {:post {:title String}}]\n  (div (meta-line ctx.post)))\n\n(defn meta-line [post {:title String}]\n  (p post.titel))";
    assert_eq!(
        parse_error(text)?.to_string(),
        "t.bv:5:11: unknown field \"titel\" (fields: title)\n  in meta-line"
    );
    // Errors that depend on values come from rendering, with the functions that were running.
    let text = "(defn page [ctx {:post {:url String}}]\n  (div (link-to ctx.post)))\n\n(defn link-to [post {:url String}]\n  (a {:href post.url} \"x\"))";
    let ctx = Value::record([(
        "post",
        Value::record([("url", Value::from("javascript:x"))]),
    )]);
    let error = render_error(text, ctx)?;
    assert_eq!(error.span().map(bitview::Span::line), Some(5));
    assert!(
        error
            .to_string()
            .ends_with("\n  in link-to (called at t.bv:2:9)\n  in page"),
        "{error}"
    );
    // A bracket left open is reported where it opens, not at the end of the file.
    let error = parse_error("(defn page [ctx {}]\n  (div\n    (p \"a\")\n")?;
    assert_eq!(error.to_string(), "t.bv:2:3: no `)` closes this bracket");
    Ok(())
}

#[test]
fn names_are_checked_before_rendering() -> Result<()> {
    for (text, kind) in [
        ("(defn page [ctx {}] (p missing))", ErrorKind::Name),
        ("(defn page [ctx {}] (missing ctx))", ErrorKind::Name),
        (
            "(defn page [ctx {}] (p helper))\n(defn helper [x String] x)",
            ErrorKind::Name,
        ),
        ("(defn page [ctx {}] (ctx 1))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (ctx))", ErrorKind::Name),
        (
            "(defn page [ctx {}] (p (helper ctx)))\n(defn helper [ctx {}] \"x\")\n(defn page [x {}] \"y\")",
            ErrorKind::Name,
        ),
        (
            "(defn page [ctx {}] (p (helper ctx ctx)))\n(defn helper [x {}] \"x\")",
            ErrorKind::Arity,
        ),
        ("(defn p [ctx {}] \"x\")", ErrorKind::Name),
        ("(defn page [x {} x {}] \"x\")", ErrorKind::Name),
        ("(defn page [ctx {}] (P ctx))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p 1))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p \"a))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p \"\\q\"))", ErrorKind::Syntax),
        (
            "(defn page [ctx {}] (p \"\\u{110000}\"))",
            ErrorKind::Syntax,
        ),
        ("(defn page [ctx {}])", ErrorKind::Syntax),
        ("(defn page [ctx {}] ctx ctx)", ErrorKind::Syntax),
        ("(page ctx)", ErrorKind::Syntax),
        ("page(ctx)", ErrorKind::Syntax),
        ("(defn page [ctx {}] ())", ErrorKind::Syntax),
        ("(defn page [ctx Post] (ctx.title))", ErrorKind::Syntax),
        ("(defn page [ctx Post] ctx .title)", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p :id))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p {id \"x\"}))", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p {:id}))", ErrorKind::Syntax),
        (
            "(defn page [ctx Post] (if ctx.flag \"x\"))",
            ErrorKind::Syntax,
        ),
        ("(defn if [ctx {}] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p if.x))", ErrorKind::Syntax),
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), kind, "{text}: {error}");
        assert!(error.span().is_some(), "{text}: {error}");
    }
    assert_eq!(
        render(r#"(defn page [ctx {}] (p "\u{1F600}\n\t\"\\"))"#, empty())?,
        "<p>\u{1F600}\n\t&quot;\\</p>"
    );
    Ok(())
}

#[test]
fn names_are_lowercase_words_joined_by_hyphens() -> Result<()> {
    let text = "(defn page [ctx {:created-at String}] (entry-list ctx.created-at))\n(defn entry-list [created-at String] (p created-at))";
    let ctx = Value::record([("created-at", Value::from("today"))]);
    assert_eq!(render(text, ctx)?, "<p>today</p>");
    for text in [
        "(defn page [ctx {}] (entry_list ctx))",
        "(defn page [ctx {}] (p ctx.created_at))",
        "(defn page [ctx {}] (entry- ctx))",
        "(defn page [ctx {}] (entry--list ctx))",
        "(defn page [ctx {}] (-entry ctx))",
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Syntax, "{text}: {error}");
    }
    let error = parse_error("(defn page [ctx {}] (entry_list ctx))")?;
    assert!(error.message().contains("entry-list"), "{error}");
    assert_eq!(error.span().map(bitview::Span::column), Some(22));
    Ok(())
}

#[test]
fn parameters_hide_functions_of_the_same_name() -> Result<()> {
    let text = "(defn page [ctx {:title String}] (heading ctx.title))\n(defn heading [title String] (h1 title))";
    let ctx = Value::record([("title", Value::from("T"))]);
    assert_eq!(render(text, ctx)?, "<h1>T</h1>");
    for text in [
        "(defn page [title String] (html (head (title title))))",
        "(defn page [item [String]] (ul (map item item)))\n(defn item [x String] (li x))",
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
fn comments_run_from_a_semicolon_to_the_end_of_the_line() -> Result<()> {
    let text = "; A page.\n(defn page [ctx {}] ; the body\n  (p \"; not a comment\")) ;";
    assert_eq!(render(text, empty())?, "<p>; not a comment</p>");
    for text in [
        "(defn page [ctx Post] (p ctx.title--))",
        "(defn page [ctx {}] - (p \"x\"))",
        "-- A page.\n(defn page [ctx {}] \"x\")",
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Syntax, "{text}");
    }
    Ok(())
}

#[test]
fn recursion_is_rejected_before_rendering() -> Result<()> {
    let cycle = "(defn page [ctx {}] (f ctx))\n(defn f [x {}] (g x))\n(defn g [x {}] (f x))";
    for text in [
        "(defn page [ctx {}] (page ctx))",
        cycle,
        "(defn page [ctx [String]] (ul (map ctx item)))\n(defn item [x String] (ul (map [x] item)))",
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Recursion, "{text}");
    }
    let error = parse_error(cycle)?;
    assert!(error.message().ends_with("f -> g -> f"), "{error}");
    Ok(())
}

#[test]
fn deep_nesting_fails_without_exhausting_the_stack() -> Result<()> {
    let depth = 10_000;
    for (open, close) in [("(p ", ")"), ("[", "]"), ("{:a ", "}")] {
        let text = format!(
            "(defn page [ctx {{}}] {}\"x\"{})",
            open.repeat(depth),
            close.repeat(depth)
        );
        let error = parse_error(&text)?;
        assert_eq!(error.kind(), ErrorKind::Syntax);
        assert!(error.message().contains("nested"), "{error}");
    }
    for text in [
        format!("(defn page [ctx Post] ctx{})", ".a".repeat(depth)),
        format!(
            "(defn page [ctx {}String{}] \"x\")",
            "[".repeat(depth),
            "]".repeat(depth)
        ),
    ] {
        let error = parse_error(&text)?;
        assert_eq!(error.kind(), ErrorKind::Syntax);
        assert!(error.message().contains("nested"), "{error}");
    }
    let text = format!(
        "(defn page [ctx {{}}] {}\"x\"{})",
        "(div ".repeat(100),
        ")".repeat(100)
    );
    assert!(render(&text, empty())?.starts_with("<div><div>"));
    Ok(())
}

#[test]
fn every_parameter_has_a_type() -> Result<()> {
    let error = parse_error("(defn page [ctx] \"x\")")?;
    assert_eq!(
        error.to_string(),
        "t.bv:1:13: parameter ctx needs a type after it, as in [ctx String]"
    );
    for (text, kind) in [
        ("(defn page [ctx Strin] \"x\")", ErrorKind::Name),
        ("(defn page [ctx [String Bool]] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx []] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx {:a}] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx {a String}] \"x\")", ErrorKind::Syntax),
        (
            "(defn page [ctx {:a String :a Bool}] \"x\")",
            ErrorKind::Name,
        ),
        ("(defn page [String] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx String x] \"x\")", ErrorKind::Syntax),
        ("(defn page [ctx {}] (p String))", ErrorKind::Syntax),
    ] {
        assert_eq!(parse_error(text)?.kind(), kind, "{text}");
    }
    let error = parse_error("(defn page [ctx Strin] \"x\")")?;
    assert_eq!(
        error.message(),
        "unknown type Strin (types: String, Bool, Flow, Phrasing, Metadata, Post)"
    );
    Ok(())
}

#[test]
fn host_types_are_named_for_the_templates() -> Result<()> {
    let tag = Type::record([("name", Type::String), ("url", Type::String)]);
    let page = Type::record([("title", Type::String), ("tags", Type::list(tag.clone()))]);
    let text = "(defn page [ctx Page] (div (h1 ctx.title) (ul (map ctx.tags tag-link))))\n(defn- tag-link [tag Tag] (li (a {:href tag.url} tag.name)))";
    let program = Program::parse(
        &[Source { name: "t.bv", text }],
        &[("Tag", tag), ("Page", page.clone())],
    )?;
    program.check(&[("page", page)])?;
    let ctx = Value::record([
        ("title", Value::from("T")),
        (
            "tags",
            Value::from(vec![Value::record([
                ("name", Value::from("rust")),
                ("url", Value::from("/rust/")),
            ])]),
        ),
    ]);
    assert_eq!(
        program.render("page", ctx)?.to_fragment(),
        "<div><h1>T</h1><ul><li><a href=\"/rust/\">rust</a></li></ul></div>"
    );
    for (types, message) in [
        (
            vec![("tag", Type::String)],
            "type name \"tag\" must be an uppercase letter followed by letters and digits, as in Entry",
        ),
        (
            vec![("My-Tag", Type::String)],
            "type name \"My-Tag\" must be an uppercase letter followed by letters and digits, as in Entry",
        ),
        (
            vec![("String", Type::Bool)],
            "type String is defined twice or is built in",
        ),
        (
            vec![("Tag", Type::String), ("Tag", Type::Bool)],
            "type Tag is defined twice or is built in",
        ),
    ] {
        let error = Program::parse(&[], &types)
            .err()
            .ok_or("accepted a wrong type name")?;
        assert_eq!(error.message(), message);
    }
    Ok(())
}

#[test]
fn host_types_are_declared_in_template_syntax() -> Result<()> {
    let tag = Type::record([("name", Type::String), ("url", Type::String)]);
    let types = [
        ("Title", Type::String),
        ("Tag", tag.clone()),
        (
            "TagsPage",
            Type::record([
                ("title", Type::String),
                ("style", Type::Html(HtmlType::Metadata)),
                ("tags", Type::list(tag)),
                ("meta", Type::record([("draft", Type::Bool)])),
            ]),
        ),
        ("Empty", Type::record::<&str>([])),
    ];
    let declared = declare_types(&types);
    assert_eq!(
        declared,
        concat!(
            "Title String\n",
            "\n",
            "Tag {:name String\n",
            "     :url String}\n",
            "\n",
            "TagsPage {:title String\n",
            "          :style Metadata\n",
            "          :tags [Tag]\n",
            "          :meta {:draft Bool}}\n",
            "\n",
            "Empty {}\n",
        )
    );
    // Each declaration is a type that templates can write.
    for declaration in declared.split("\n\n") {
        let (_, ty) = declaration
            .split_once(' ')
            .ok_or("a declaration without a type")?;
        let text = format!("(defn page [ctx {}] (p))", ty.trim_end());
        Program::parse(
            &[Source {
                name: "t.bv",
                text: &text,
            }],
            &types,
        )?;
    }
    Ok(())
}

#[test]
fn parameter_types_list_what_a_function_reads() -> Result<()> {
    // Records with more fields than the type lists fit, so one function serves both.
    let text = "(defn page [ctx {:article {:draft Bool} :entries [{:title String :draft Bool}]}] (div (badge ctx.article) (ul (map ctx.entries item))))\n(defn item [entry {:title String :draft Bool}] (li entry.title (badge entry)))\n(defn badge [entry {:draft Bool}] (if entry.draft (span \"draft\") []))";
    let ctx_type = Type::record([
        (
            "article",
            Type::record([("title", Type::String), ("draft", Type::Bool)]),
        ),
        (
            "entries",
            Type::list(Type::record([
                ("title", Type::String),
                ("url", Type::String),
                ("draft", Type::Bool),
            ])),
        ),
    ]);
    let ctx = Value::record([
        (
            "article",
            Value::record([("title", Value::from("A")), ("draft", Value::from(true))]),
        ),
        (
            "entries",
            Value::from(vec![Value::record([
                ("title", Value::from("E")),
                ("url", Value::from("/e")),
                ("draft", Value::from(false)),
            ])]),
        ),
    ]);
    parse(text)?.check(&[("page", ctx_type)])?;
    assert_eq!(
        render(text, ctx)?,
        "<div><span>draft</span><ul><li>E</li></ul></div>"
    );
    Ok(())
}

#[test]
fn parameter_types_are_contracts_on_both_sides() -> Result<()> {
    for (text, message) in [
        // The function sees only the fields its type lists.
        (
            "(defn page [ctx Post] (badge ctx))\n(defn badge [entry {:flag Bool}] (p entry.title))",
            "t.bv:2:43: unknown field \"title\" (fields: flag)\n  in badge",
        ),
        // A caller must pass what the type lists.
        (
            "(defn page [ctx Post] (badge ctx))\n(defn badge [entry {:draft Bool}] (p \"x\"))",
            "t.bv:1:24: entry has no field \"draft\", which its type lists\n  in page",
        ),
        (
            "(defn page [ctx Post] (badge ctx))\n(defn badge [entry {:flag String}] (p entry.flag))",
            "t.bv:1:24: entry.flag must be String, but got Bool\n  in page",
        ),
        (
            "(defn page [ctx Post] (p (inline (div ctx.title))))\n(defn inline [x Phrasing] (span x))",
            "t.bv:1:27: x must hold only text, phrasing elements such as <span> and <a>, but holds block elements such as <p> and <div>\n  in page",
        ),
        // The body is checked with the type, not with what a caller happens to pass.
        (
            "(defn page [ctx Post] (block ctx.title))\n(defn block [x Flow] (p x))",
            "t.bv:2:23: <p> cannot contain block elements such as <p> and <div>; it takes text, phrasing elements such as <span> and <a>\n  in block",
        ),
    ] {
        assert_eq!(parse_error(text)?.to_string(), message, "{text}");
    }
    // Text and fragments stand for Html, and an empty list fits a list of any type.
    let text = "(defn page [ctx Post] (div (block ctx.title) (block [(p \"a\") \"b\"]) (tags [])))\n(defn block [x Flow] (div x))\n(defn tags [x [String]] (ul (map x tag)))\n(defn tag [x String] (li x))";
    assert_eq!(
        render(text, post(true, &[]))?,
        "<div><div>T</div><div><p>a</p>b</div><ul></ul></div>"
    );
    Ok(())
}

#[test]
fn functions_no_entry_calls_are_checked_too() -> Result<()> {
    let page = "(defn page [ctx Post] (p ctx.title))";
    parse(&format!(
        "{page}\n(defn card [entry {{:title String}}] (div (heading entry.title)))\n(defn- heading [title String] (h2 title))"
    ))?;
    assert_eq!(
        parse_error(&format!(
            "{page}\n(defn- card [entry {{:title String}}] (div entry.titel))"
        ))?
        .to_string(),
        "t.bv:2:48: unknown field \"titel\" (fields: title)\n  in card"
    );
    Ok(())
}

#[test]
fn entry_functions_take_one_value_and_return_html() -> Result<()> {
    let program = parse("(defn page [ctx {}] {:a \"text\"})\n(defn two [x {} y {}] (p))")?;
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
            .check(&[("page", Type::record::<&str>([]))])
            .err()
            .map(|error| error.to_string()),
        Some("t.bv:1:1: page must return Html, but returns a Record {a}".to_owned())
    );
    for (entry, kind) in [("two", ErrorKind::Arity), ("none", ErrorKind::Name)] {
        assert_eq!(
            program
                .render(entry, empty())
                .err()
                .as_ref()
                .map(BitviewError::kind),
            Some(kind),
            "{entry}"
        );
    }
    assert!(!program.has_entry("two"));
    assert_eq!(
        parse("(defn page [ctx {}] (p \"x\"))")?
            .check(&[("missing", post_type())])
            .err()
            .as_ref()
            .map(BitviewError::kind),
        Some(ErrorKind::Name)
    );
    Ok(())
}

#[test]
fn entry_results_that_stand_for_html_render_as_fragments() -> Result<()> {
    assert_eq!(
        render(r#"(defn page [ctx {}] [(p "a") "b" []])"#, empty())?,
        "<p>a</p>b"
    );
    assert_eq!(render(r#"(defn page [ctx {}] "text")"#, empty())?, "text");
    Ok(())
}

#[test]
fn check_and_render_hold_host_values_to_the_entry_type_alike() -> Result<()> {
    let host_type = Type::record([("title", Type::String), ("flag", Type::Bool)]);
    let ctx = Value::record([("title", Value::from("t")), ("flag", Value::from(true))]);
    for (text, message) in [
        (
            "(defn page [ctx {:items [String]}] (ul (map ctx.items item)))\n(defn item [x String] (li x))",
            "t.bv:1:1: ctx has no field \"items\", which its type lists\n  in page",
        ),
        (
            "(defn page [ctx {:flag String}] (p ctx.flag))",
            "t.bv:1:1: ctx.flag must be String, but got Bool\n  in page",
        ),
        (
            "(defn page [ctx Bool] (p))",
            "t.bv:1:1: ctx must be Bool, but got Record {title, flag}\n  in page",
        ),
    ] {
        assert_eq!(
            check_error(text, &host_type)?.to_string(),
            message,
            "{text}"
        );
        let rendered = render_error(text, ctx.clone())?.to_string();
        // A value names only its kind, where a type also lists its fields.
        assert_eq!(
            rendered,
            message.replace("Record {title, flag}", "Record"),
            "{text}"
        );
    }
    // The entry sees only the fields its type lists.
    assert_eq!(
        render("(defn page [ctx {:title String}] (p ctx.title))", ctx)?,
        "<p>t</p>"
    );
    Ok(())
}

#[test]
fn host_html_goes_only_where_its_type_allows() -> Result<()> {
    let ctx_type = |html: HtmlType| Type::record([("part", Type::Html(html))]);
    let program = parse("(defn page [ctx {:part Phrasing}] (p ctx.part))")?;
    program.check(&[("page", ctx_type(HtmlType::Phrasing))])?;
    for html in [HtmlType::Flow, HtmlType::Metadata] {
        assert!(
            program.check(&[("page", ctx_type(html))]).is_err(),
            "{html:?}"
        );
    }
    let block = Html::element("div", Vec::new(), Html::text("x"))?;
    assert_eq!(
        program
            .render(
                "page",
                Value::record([("part", Value::from(block.clone()))])
            )
            .err()
            .map(|error| error.kind()),
        Some(ErrorKind::Html)
    );
    assert!(
        Type::Html(HtmlType::Flow)
            .validate(&Value::from(block.clone()))
            .is_ok()
    );
    let error = Type::Html(HtmlType::Phrasing)
        .validate(&Value::from(block))
        .err()
        .ok_or("validated a block as phrasing")?;
    assert_eq!(
        error.message(),
        "value is Html with block elements such as <p> and <div>, but the type is Phrasing Html"
    );
    let style = Html::style("p{}")?;
    assert!(
        Type::Html(HtmlType::Metadata)
            .validate(&Value::from(style.clone()))
            .is_ok()
    );
    assert!(Html::element("p", Vec::new(), style).is_err());
    Ok(())
}

#[test]
fn functions_share_one_namespace_across_sources() -> Result<()> {
    let program = Program::parse(
        &sources(&[
            ("a.bv", "(defn page [ctx {:name String}] (p (helper ctx)))"),
            ("b.bv", "(defn helper [x {:name String}] x.name)"),
        ]),
        &[],
    )?;
    let ctx = Value::record([("name", Value::from("n"))]);
    assert_eq!(program.render("page", ctx)?.to_fragment(), "<p>n</p>");
    let defined = program
        .defined_at("helper")
        .ok_or("helper is not defined")?;
    assert_eq!(
        (defined.source(), defined.line(), defined.column()),
        ("b.bv", 1, 1)
    );
    assert!(program.defined_at("p").is_none());
    let error = Program::parse(
        &sources(&[
            ("a.bv", "(defn page [ctx {}] (p))"),
            ("b.bv", "\n(defn page [ctx {}] (p))"),
        ]),
        &[],
    )
    .err()
    .ok_or("accepted a duplicate function")?;
    assert_eq!(
        error.to_string(),
        "b.bv:2:1: function page is defined twice (first defined at a.bv:1:1)"
    );
    Ok(())
}

#[test]
fn private_functions_belong_to_their_source() -> Result<()> {
    // Both sources keep a private item, and a.bv's hides the public item of c.bv.
    let program = Program::parse(
        &sources(&[
            (
                "a.bv",
                "(defn page [ctx {:items [String]}] (div (ul (map ctx.items item)) (tags ctx)))\n(defn- item [x String] (li x))",
            ),
            (
                "b.bv",
                "(defn tags [ctx {:items [String]}] (ol (map ctx.items item)))\n(defn- item [x String] (li (b x)))",
            ),
            (
                "c.bv",
                "(defn item [x String] (p x))\n(defn b [x String] (span x))",
            ),
        ]),
        &[],
    )?;
    let ctx = Value::record([("items", Value::from(vec![Value::from("x")]))]);
    let ctx_type = Type::record([("items", Type::list(Type::String))]);
    assert_eq!(
        program.render("page", ctx.clone())?.to_fragment(),
        "<div><ul><li>x</li></ul><ol><li><span>x</span></li></ol></div>"
    );
    program.check(&[("page", ctx_type), ("item", Type::String)])?;
    // The host names only public functions.
    assert!(program.has_entry("tags"));
    assert!(!program.has_entry("missing"));
    let error = Program::parse(
        &sources(&[(
            "a.bv",
            "(defn page [ctx String] (inner ctx))\n(defn- inner [ctx String] (p ctx))",
        )]),
        &[],
    )?
    .render("inner", Value::from("x"))
    .err()
    .ok_or("rendered a private function")?;
    assert_eq!(
        error.to_string(),
        "a.bv:2:1: inner is defined with defn-, so only its source can call it"
    );
    Ok(())
}

#[test]
fn private_functions_cannot_be_called_from_other_sources() -> Result<()> {
    let error = Program::parse(
        &sources(&[
            ("a.bv", "(defn page [ctx {}] (p (helper ctx)))"),
            ("b.bv", "(defn- helper [x {}] \"x\")"),
        ]),
        &[],
    )
    .err()
    .ok_or("called a private function of another source")?;
    assert_eq!(
        error.to_string(),
        "a.bv:1:25: helper is defined with defn- in b.bv, so only that source can call it"
    );
    let error = Program::parse(
        &sources(&[
            ("a.bv", "(defn page [ctx [String]] (ul (map ctx helper)))"),
            ("b.bv", "(defn- helper [x String] (li x))"),
        ]),
        &[],
    )
    .err()
    .ok_or("mapped a private function of another source")?;
    assert_eq!(error.kind(), ErrorKind::Name);
    assert!(error.message().contains("defn- in b.bv"), "{error}");
    Ok(())
}

#[test]
fn a_source_defines_each_name_once() -> Result<()> {
    for text in [
        "(defn- item [x String] x)\n(defn- item [x String] x)",
        "(defn item [x String] x)\n(defn- item [x String] x)",
        "(defn- item [x String] x)\n(defn item [x String] x)",
        "(defn- p [x String] x)",
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Name, "{text}");
    }
    let error = parse_error("(defn- item [x String] x)\n(defn item [x String] x)")?;
    assert_eq!(
        error.to_string(),
        "t.bv:2:1: function item is defined twice (first defined at t.bv:1:1)"
    );
    for text in [
        "(defn-item [x String] x)",
        "(defn page [ctx {}] (p defn-))",
        "(defn page [ctx {}] (p (defn- ctx)))",
        "(defn page [ctx {}] defn-.x)",
    ] {
        assert_eq!(parse_error(text)?.kind(), ErrorKind::Syntax, "{text}");
    }
    Ok(())
}

#[test]
fn functions_used_by_skips_private_functions_but_not_their_callees() -> Result<()> {
    let program = Program::parse(
        &sources(&[
            (
                "page.bv",
                "(defn page [ctx {:title String :items [String]}] (body (top ctx) (entry-list ctx.items)))\n(defn- top [ctx {:title String}] (h1 (home ctx)))",
            ),
            (
                "home.bv",
                "(defn home [ctx {:title String}] (a {:href \"/\"} ctx.title))",
            ),
            (
                "entry-list.bv",
                "(defn entry-list [items [String]] (ul (map items item)))\n(defn- item [x String] (li x))",
            ),
        ]),
        &[],
    )?;
    assert_eq!(
        program.functions_used_by("page"),
        Some(vec!["home", "entry-list", "page"])
    );
    assert_eq!(program.functions_used_by("top"), None);
    Ok(())
}

#[test]
fn functions_used_by_lists_callees_before_callers() -> Result<()> {
    let program = parse(
        r#"(defn page [ctx Post] (layout (home ctx) (if ctx.flag (item ctx) [])))
(defn layout [top Flow rest Flow] (html (body top rest)))
(defn home [ctx {:title String}] (a {:href "/"} (base ctx)))
(defn item [ctx {:items [{:name String}]}] (p (map ctx.items base)))
(defn base [x {:name String}] x.name)
(defn unused [ctx {}] ctx)"#,
    );
    // home passes a record without name to base, so the program is fixed before it is used.
    assert!(program.is_err());
    let program = parse(
        r#"(defn page [ctx Post] (layout (home ctx) (if ctx.flag (item ctx) [])))
(defn layout [top Flow rest Flow] (html (body top rest)))
(defn home [ctx {:title String}] (a {:href "/"} (base {:name ctx.title})))
(defn item [ctx {:items [{:name String}]}] (p (map ctx.items base)))
(defn base [x {:name String}] x.name)
(defn unused [ctx {}] ctx)"#,
    )?;
    assert_eq!(
        program.functions_used_by("page"),
        Some(vec!["layout", "base", "home", "item", "page"])
    );
    assert_eq!(
        program.functions_used_by("item"),
        Some(vec!["base", "item"])
    );
    assert_eq!(program.functions_used_by("base"), Some(vec!["base"]));
    assert_eq!(program.functions_used_by("missing"), None);
    Ok(())
}

#[test]
fn checking_finds_field_errors_in_every_branch_and_map() -> Result<()> {
    let text = "(defn page [ctx Post]\n  (div (if ctx.flag (p ctx.titel) []) (ul (map ctx.items item))))\n(defn item [x {:name String}] (li x.name))";
    assert_eq!(
        parse_error(text)?.to_string(),
        "t.bv:2:28: unknown field \"titel\" (fields: flag, title, items)\n  in page"
    );
    let text = "(defn page [ctx Post] (ul (map ctx.items item)))\n(defn item [x {:name String}] (li x.nam))";
    let error = parse_error(text)?;
    assert_eq!(error.kind(), ErrorKind::Field);
    assert_eq!(
        error.trace().first().map(bitview::Frame::function),
        Some("item")
    );
    Ok(())
}

#[test]
fn checking_finds_misspelled_attributes() -> Result<()> {
    let text = "(defn page [ctx Post]\n  (div (if ctx.flag (a {:herf \"/\"} ctx.title) [])))";
    let error = parse_error(text)?;
    assert_eq!(error.kind(), ErrorKind::Html);
    assert!(
        error
            .to_string()
            .starts_with("t.bv:2:22: <a> has no attribute \"herf\"; it takes href,"),
        "{error}"
    );
    Ok(())
}

#[test]
fn checking_finds_elements_where_they_cannot_go() -> Result<()> {
    for (text, message) in [
        (
            "(defn page [ctx Post] (p (if ctx.flag (div ctx.title) [])))",
            "<p> cannot contain block elements such as <p> and <div>; it takes text, phrasing elements such as <span> and <a>",
        ),
        (
            "(defn page [ctx Post] (ul (map ctx.items item)))\n(defn item [x {:name String}] (p x.name))",
            "<ul> cannot contain block elements such as <p> and <div>; it takes <li>",
        ),
        (
            "(defn page [ctx Post] (html (head (p ctx.title)) (body)))",
            "<head> cannot contain block elements such as <p> and <div>; it takes <meta>, <link>, <title>, styles, and JSON",
        ),
        (
            "(defn page [ctx Post] (div (a {:href \"/\"} (span (a {:href \"/a\"} ctx.title)))))",
            "<a> cannot contain another <a>",
        ),
        (
            "(defn page [ctx Post] (p (a {:href \"/\"} (div ctx.title))))",
            "<p> cannot contain block elements such as <p> and <div>; it takes text, phrasing elements such as <span> and <a>",
        ),
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Html, "{text}");
        assert_eq!(error.message(), message, "{text}");
    }
    // A link around blocks is a block itself, so it can go where blocks go.
    parse(
        "(defn page [ctx Post] (div (a {:href \"/\"} (div ctx.title)) (p (a {:href \"/\"} ctx.title))))",
    )?
    .check(&[("page", post_type())])?;
    Ok(())
}

#[test]
fn if_and_lists_take_the_least_common_type() -> Result<()> {
    for text in [
        r#"(defn page [ctx Post] (p (if ctx.flag (span "draft") [])))"#,
        r#"(defn page [ctx Post] (p (if ctx.flag "text" (em "html"))))"#,
        r#"(defn page [ctx Post] (p ["text" (em "html") (map ctx.items item)]))
(defn item [x {:name String}] (span x.name))"#,
        r#"(defn page [ctx Post] (p (map (pick ctx) name)))
(defn pick [ctx Post] (if ctx.flag [{:n "a"}] []))
(defn name [x {:n String}] x.n)"#,
    ] {
        parse(text)?.check(&[("page", post_type())])?;
    }
    for (text, message) in [
        (
            r#"(defn page [ctx Post] (p (if ctx.flag ctx.flag "no")))"#,
            "the two sides of if have different types: Bool and String",
        ),
        (
            "(defn page [ctx Post] (p (if ctx.flag ctx.flag [])))",
            "the two sides of if have different types: Bool and []",
        ),
        (
            r#"(defn page [ctx {}] (p (map [{:a "x"} {:b "y"}] f)))
(defn f [x {:a String}] x.a)"#,
            "the items of a list have different types: Record {a} and Record {b}",
        ),
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), ErrorKind::Type, "{text}");
        assert!(error.message().contains(message), "{text}: {error}");
    }
    Ok(())
}

#[test]
fn checking_finds_values_used_where_they_do_not_fit() -> Result<()> {
    for (text, kind) in [
        (
            "(defn page [ctx Post] (p (concat \"a\" ctx.flag)))",
            ErrorKind::Type,
        ),
        (
            "(defn page [ctx Post] (a {:href ctx.flag} \"x\"))",
            ErrorKind::Type,
        ),
        (
            "(defn page [ctx Post] (a {:onclick \"x\"} \"x\"))",
            ErrorKind::Html,
        ),
        ("(defn page [ctx Post] (p ctx.flag))", ErrorKind::Type),
        (
            "(defn page [ctx Post] (p (if ctx.title \"a\" \"b\")))",
            ErrorKind::Type,
        ),
        (
            "(defn page [ctx Post] (ul (map ctx.title f)))\n(defn f [x String] (li x))",
            ErrorKind::Type,
        ),
        (
            "(defn page [ctx Post] (p ctx.title.length))",
            ErrorKind::Type,
        ),
    ] {
        let error = parse_error(text)?;
        assert_eq!(error.kind(), kind, "{text}: {error}");
        assert!(error.span().is_some(), "{text}: {error}");
    }
    // Any function may return a record or a Bool; only an entry must return Html.
    for text in [
        "(defn page [ctx Post] ctx)",
        "(defn page [ctx Post] ctx.flag)",
    ] {
        assert_eq!(
            check_error(text, &post_type())?.kind(),
            ErrorKind::Type,
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn values_validate_against_their_exact_type() -> Result<()> {
    let ty = post_type();
    let item = |name: Value| Value::record([("name", name)]);
    let post = |items: Vec<Value>| {
        Value::record([
            ("flag", Value::from(true)),
            ("title", Value::from("T")),
            ("items", Value::from(items)),
        ])
    };
    ty.validate(&post(vec![item(Value::from("a")), item(Value::from("b"))]))?;
    ty.validate(&post(Vec::new()))?;
    for (value, message) in [
        (
            post(vec![item(Value::from("a")), item(Value::from(true))]),
            "value.items[1].name is a Bool, but the type is String",
        ),
        (
            Value::record([("flag", Value::from(true)), ("title", Value::from("T"))]),
            "value.items is missing",
        ),
        (
            Value::record([
                ("flag", Value::from(true)),
                ("title", Value::from("T")),
                ("items", Value::from(Vec::new())),
                ("extra", Value::from("x")),
            ]),
            "value.extra is not a field of the type",
        ),
        (
            Value::record([
                ("flag", Value::from(true)),
                ("title", Value::from("T")),
                ("title", Value::from(true)),
                ("items", Value::from(Vec::new())),
            ]),
            "value.title is given twice",
        ),
        (
            Value::from("text"),
            "value is a String, but the type is Record {flag, title, items}",
        ),
    ] {
        let error = ty.validate(&value).err().ok_or("validated a wrong value")?;
        assert_eq!(error.message(), message);
    }
    assert!(
        Type::Html(HtmlType::Flow)
            .validate(&Value::from("text"))
            .is_err()
    );
    Ok(())
}

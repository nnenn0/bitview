//! Renders and checks a copy of the views genbit creates for a new site, so the language is tested
//! on a program of the size and shape it is written for.

use bitview::{Html, HtmlType, Program, Source, Type, Value};
use std::{error::Error, fs, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn program() -> Result<Program> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut sources = Vec::new();
    for directory in ["views/pages", "views/components"] {
        for file in fs::read_dir(root.join(directory))? {
            let file = file?
                .file_name()
                .into_string()
                .map_err(|_| "non-UTF-8 file name")?;
            let text = fs::read_to_string(root.join(directory).join(&file))?;
            sources.push((format!("{directory}/{file}"), text));
        }
    }
    sources.sort();
    let sources = sources
        .iter()
        .map(|(name, text)| Source { name, text })
        .collect::<Vec<_>>();
    Ok(Program::parse(&sources, &types())?)
}

/// The types genbit names for the views: the parts of its pages, and the pages themselves.
fn types() -> Vec<(&'static str, Type)> {
    let site = Type::record([
        ("title", Type::String),
        ("description", Type::String),
        ("url", Type::String),
        ("og-image", Type::String),
    ]);
    let index = Type::record([
        ("url", Type::String),
        ("json-ld", Type::Html(HtmlType::Metadata)),
    ]);
    let timestamp = Type::record([("datetime", Type::String), ("date", Type::String)]);
    let tag_link = Type::record([("name", Type::String), ("url", Type::String)]);
    let tag_count = Type::record([
        ("name", Type::String),
        ("url", Type::String),
        ("count", Type::String),
    ]);
    let entry = Type::record([
        ("title", Type::String),
        ("description", Type::String),
        ("url", Type::String),
        ("created-at", timestamp.clone()),
        ("updated-at", timestamp.clone()),
        ("tags", Type::list(tag_link.clone())),
        ("draft", Type::Bool),
    ]);
    // Every page has the site and its CSS, and every page a search engine indexes has an index.
    let page = |indexed: bool, fields: Vec<(&str, Type)>| {
        let mut all = vec![
            ("site", site.clone()),
            ("style", Type::Html(HtmlType::Metadata)),
        ];
        if indexed {
            all.push(("index", index.clone()));
        }
        all.extend(fields);
        Type::record(all)
    };
    vec![
        (
            "RootPage",
            page(true, vec![("entries", Type::list(entry.clone()))]),
        ),
        (
            "ArticlePage",
            page(
                true,
                vec![
                    ("article", entry.clone()),
                    ("content", Type::Html(HtmlType::Flow)),
                ],
            ),
        ),
        (
            "TagsPage",
            page(true, vec![("tags", Type::list(tag_count.clone()))]),
        ),
        (
            "TagPage",
            page(
                true,
                vec![
                    ("tag", Type::String),
                    ("entries", Type::list(entry.clone())),
                ],
            ),
        ),
        ("NotFoundPage", page(false, Vec::new())),
        ("Site", site),
        ("Index", index),
        ("Entry", entry),
        ("Timestamp", timestamp),
        ("TagLink", tag_link),
        ("TagCount", tag_count),
    ]
}

fn page_type(name: &str) -> Result<Type> {
    types()
        .into_iter()
        .find(|(type_name, _)| *type_name == name)
        .map(|(_, ty)| ty)
        .ok_or_else(|| format!("no type {name}").into())
}

const SITE_TITLE: &str = "Blog &amp; &lt;Notes&gt;";

fn site() -> Value {
    Value::record([
        ("title", Value::from("Blog & <Notes>")),
        ("description", Value::from("Posts & more")),
        ("url", Value::from("https://example.com/")),
        (
            "og-image",
            Value::from("https://example.com/assets/site/ogp.png"),
        ),
    ])
}

fn ctx(fields: Vec<(&str, Value)>) -> Result<Value> {
    let mut all = vec![
        ("site", site()),
        ("style", Value::from(Html::style("body{color:red}")?)),
    ];
    all.extend(fields);
    Ok(Value::record(all))
}

fn indexed(url: &str, json: &str, fields: Vec<(&str, Value)>) -> Result<Value> {
    let mut all = vec![(
        "index",
        Value::record([
            ("url", Value::from(url)),
            (
                "json-ld",
                Value::from(Html::json("application/ld+json", json)?),
            ),
        ]),
    )];
    all.extend(fields);
    ctx(all)
}

fn timestamp(datetime: &str) -> Value {
    Value::record([
        ("datetime", Value::from(datetime)),
        ("date", Value::from(datetime.get(..10).unwrap_or_default())),
    ])
}

fn entry(title: &str, slug: &str, tags: &[&str], draft: bool) -> Value {
    let tags = tags
        .iter()
        .map(|name| {
            Value::record([
                ("name", Value::from(*name)),
                ("url", Value::from(format!("/tags/{name}/"))),
            ])
        })
        .collect::<Vec<_>>();
    Value::record([
        ("title", Value::from(title)),
        ("description", Value::from("Summary")),
        ("url", Value::from(format!("/entries/{slug}/"))),
        ("created-at", timestamp("2026-09-17T09:00:00+09:00")),
        ("updated-at", timestamp("2026-09-18T10:30:00+09:00")),
        ("tags", Value::from(tags)),
        ("draft", Value::from(draft)),
    ])
}

fn head(title: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html lang=\"ja\"><head>",
            "<meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<meta name=\"color-scheme\" content=\"light dark\">",
            "<link rel=\"icon\" href=\"/assets/site/favicon.png\" type=\"image/png\" sizes=\"96x96\">",
            "<link rel=\"icon\" href=\"/assets/site/favicon.svg\" type=\"image/svg+xml\">",
            "<link rel=\"alternate\" type=\"application/rss+xml\" title=\"{site}\" href=\"/feed.xml\">",
            "<title>{title}</title>",
        ),
        site = SITE_TITLE,
        title = title,
    )
}

fn seo(title: &str, description: &str, kind: &str, url: &str, json: &str) -> String {
    format!(
        concat!(
            "<meta name=\"description\" content=\"{description}\">",
            "<link rel=\"canonical\" href=\"{url}\">",
            "<meta property=\"og:title\" content=\"{title}\">",
            "<meta property=\"og:description\" content=\"{description}\">",
            "<meta property=\"og:type\" content=\"{kind}\">",
            "<meta property=\"og:url\" content=\"{url}\">",
            "<meta property=\"og:site_name\" content=\"{site}\">",
            "<meta property=\"og:image\" content=\"https://example.com/assets/site/ogp.png\">",
            "<meta property=\"og:image:alt\" content=\"{site}\">",
            "<script type=\"application/ld+json\">{json}</script>",
        ),
        title = title,
        description = description,
        kind = kind,
        url = url,
        site = SITE_TITLE,
        json = json,
    )
}

fn body(header: &str, main: &str) -> String {
    format!(
        "<style>body{{color:red}}</style></head><body><header>{header}</header><main>{main}</main></body></html>"
    )
}

const HOME_LINK: &str = "<a href=\"/\">Blog &amp; &lt;Notes&gt;</a>";

#[test]
fn article_page() -> Result<()> {
    let content = Html::element("p", Vec::new(), Html::text("Markdown body & <text>"))?;
    let ctx = indexed(
        "https://example.com/entries/hello/",
        "{\"@type\":\"BlogPosting\"}",
        vec![
            (
                "article",
                entry("Hello <World>", "hello", &["rust", "web"], true),
            ),
            ("content", Value::from(content)),
        ],
    )?;
    let page = program()?.render("page", ctx)?.to_document()?;
    let title = format!("Hello &lt;World&gt; | {SITE_TITLE}");
    let expected = head(&title)
        + &seo(
            &title,
            "Summary",
            "article",
            "https://example.com/entries/hello/",
            "{\"@type\":\"BlogPosting\"}",
        )
        + &body(
            HOME_LINK,
            concat!(
                "<article><h1>Hello &lt;World&gt;<span class=\"draft-badge\">draft</span></h1>",
                "<dl class=\"article-meta\">",
                "<dt>created_at</dt><dd><time datetime=\"2026-09-17T09:00:00+09:00\">2026-09-17</time></dd>",
                "<dt>updated_at</dt><dd><time datetime=\"2026-09-18T10:30:00+09:00\">2026-09-18</time></dd>",
                "<dt>tags</dt><dd><a href=\"/tags/rust/\">rust</a><a href=\"/tags/web/\">web</a></dd>",
                "</dl><p>Markdown body &amp; &lt;text&gt;</p></article>",
            ),
        );
    assert_eq!(page, expected);
    Ok(())
}

const ENTRY_LIST: &str = concat!(
    "<ul class=\"entry-list\">",
    "<li><a href=\"/entries/new/\">New &amp; shiny</a><span class=\"entry-meta\"><time datetime=\"2026-09-17T09:00:00+09:00\">2026-09-17</time><span class=\"draft-badge\">draft</span></span></li>",
    "<li><a href=\"/entries/old/\">Old</a><span class=\"entry-meta\"><time datetime=\"2026-09-17T09:00:00+09:00\">2026-09-17</time></span></li>",
    "</ul>",
);

fn entries() -> Value {
    Value::from(vec![
        entry("New & shiny", "new", &["rust"], true),
        entry("Old", "old", &[], false),
    ])
}

#[test]
fn home_page() -> Result<()> {
    let json = "{\"@type\":\"WebSite\"}";
    let ctx = indexed("https://example.com/", json, vec![("entries", entries())])?;
    let page = program()?.render("root", ctx)?.to_document()?;
    let expected = head(SITE_TITLE)
        + &seo(
            SITE_TITLE,
            "Posts &amp; more",
            "website",
            "https://example.com/",
            json,
        )
        + &body(
            &format!("<h1>{SITE_TITLE}</h1>"),
            &format!(
                "{ENTRY_LIST}<p class=\"home-links\"><a href=\"/tags/\">tags</a><a href=\"/feed.xml\">RSS</a></p>"
            ),
        );
    assert_eq!(page, expected);
    Ok(())
}

#[test]
fn tag_page() -> Result<()> {
    let json = "{\"@type\":\"WebSite\"}";
    let url = "https://example.com/tags/rust/";
    let ctx = indexed(
        url,
        json,
        vec![("tag", Value::from("rust")), ("entries", entries())],
    )?;
    let page = program()?.render("tag", ctx)?.to_document()?;
    let title = format!("rust | {SITE_TITLE}");
    let expected = head(&title)
        + &seo(&title, "rust の記事一覧", "website", url, json)
        + &body(
            HOME_LINK,
            &format!(
                "<h1>rust</h1>{ENTRY_LIST}<p class=\"tags-link\"><a href=\"/tags/\">tags</a></p>"
            ),
        );
    assert_eq!(page, expected);
    Ok(())
}

#[test]
fn tags_page() -> Result<()> {
    let json = "{\"@type\":\"WebSite\"}";
    let url = "https://example.com/tags/";
    let tag = |name: &str, count: &str| {
        Value::record([
            ("name", Value::from(name)),
            ("url", Value::from(format!("/tags/{name}/"))),
            ("count", Value::from(count)),
        ])
    };
    let ctx = indexed(
        url,
        json,
        vec![("tags", Value::from(vec![tag("rust", "2"), tag("c++", "1")]))],
    )?;
    let page = program()?.render("tags", ctx)?.to_document()?;
    let title = format!("tags | {SITE_TITLE}");
    let expected = head(&title)
        + &seo(&title, "記事のタグ一覧", "website", url, json)
        + &body(
            HOME_LINK,
            concat!(
                "<h1>tags</h1><ul class=\"tag-list\">",
                "<li><a href=\"/tags/rust/\">rust</a> (2)</li>",
                "<li><a href=\"/tags/c++/\">c++</a> (1)</li>",
                "</ul>",
            ),
        );
    assert_eq!(page, expected);
    Ok(())
}

#[test]
fn not_found_page() -> Result<()> {
    let page = program()?
        .render("not-found", ctx(Vec::new())?)?
        .to_document()?;
    let expected = head(&format!("404 | {SITE_TITLE}"))
        + "<meta name=\"robots\" content=\"noindex\">"
        + &body(HOME_LINK, "<h1>404</h1><p>Not Found</p>");
    assert_eq!(page, expected);
    Ok(())
}

#[test]
fn entry_functions_are_known_by_name() -> Result<()> {
    let program = program()?;
    for entry in ["root", "page", "tags", "tag", "not-found"] {
        assert!(program.has_entry(entry), "{entry}");
    }
    assert!(!program.has_entry("layout"));
    assert!(!program.has_entry("tag-link"));
    assert!(!program.has_entry("missing"));
    Ok(())
}

#[test]
fn pages_use_functions_from_the_document_to_the_page() -> Result<()> {
    let program = program()?;
    // The private seo, entry-item, tag-link, and tag-count belong to the functions of their sources.
    let base = ["document", "layout"];
    for (entry, own) in [
        (
            "page",
            &["home-link", "draft-badge", "timestamp", "page"][..],
        ),
        (
            "root",
            &["timestamp", "draft-badge", "entry-list", "root"][..],
        ),
        (
            "tag",
            &["home-link", "timestamp", "draft-badge", "entry-list", "tag"][..],
        ),
        ("tags", &["home-link", "tags"][..]),
    ] {
        let expected = base.iter().chain(own).copied().collect::<Vec<_>>();
        assert_eq!(program.functions_used_by(entry), Some(expected), "{entry}");
    }
    assert_eq!(
        program.functions_used_by("not-found"),
        Some(vec!["document", "home-link", "not-found"])
    );
    Ok(())
}

#[test]
fn every_page_takes_the_type_genbit_gives_it() -> Result<()> {
    let program = program()?;
    let pages = [
        ("root", "RootPage"),
        ("page", "ArticlePage"),
        ("tags", "TagsPage"),
        ("tag", "TagPage"),
        ("not-found", "NotFoundPage"),
    ]
    .into_iter()
    .map(|(entry, ty)| Ok((entry, page_type(ty)?)))
    .collect::<Result<Vec<_>>>()?;
    program.check(&pages)?;
    let error = program
        .check(&[("page", page_type("NotFoundPage")?)])
        .err()
        .ok_or("checked the article page with the type of the 404 page")?;
    assert_eq!(
        error.to_string(),
        "views/pages/page.bv:1:1: ctx has no field \"index\", which its type lists\n  in page"
    );
    Ok(())
}

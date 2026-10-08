//! Where HTML may go. Browsers do not reject HTML that nests elements where they cannot go, such
//! as a `<div>` in a `<p>`: they move the elements elsewhere, and the page breaks in ways that are
//! hard to trace back. Each element has a category and takes children of some categories, and the
//! same check runs on templates while they are checked and on every element that is built.

use crate::error::{Error, ErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    Text,
    Phrasing,
    Flow,
    ListItem,
    DescriptionPart,
    TableSection,
    Row,
    Cell,
    Metadata,
    Head,
    Body,
    Document,
}

impl Category {
    const ALL: [Self; 12] = [
        Self::Text,
        Self::Phrasing,
        Self::Flow,
        Self::ListItem,
        Self::DescriptionPart,
        Self::TableSection,
        Self::Row,
        Self::Cell,
        Self::Metadata,
        Self::Head,
        Self::Body,
        Self::Document,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }

    fn description(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Phrasing => "phrasing elements such as <span> and <a>",
            Self::Flow => "block elements such as <p> and <div>",
            Self::ListItem => "<li>",
            Self::DescriptionPart => "<dt> and <dd>",
            Self::TableSection => "<thead> and <tbody>",
            Self::Row => "<tr>",
            Self::Cell => "<th> and <td>",
            Self::Metadata => "<meta>, <link>, <title>, styles, and JSON",
            Self::Head => "<head>",
            Self::Body => "<body>",
            Self::Document => "<html>",
        }
    }
}

pub(crate) const PHRASING: &[Category] = &[Category::Text, Category::Phrasing];
pub(crate) const FLOW: &[Category] = &[Category::Text, Category::Phrasing, Category::Flow];

/// The categories of the top-level nodes of a fragment, and whether it contains a link anywhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Content {
    categories: u16,
    has_link: bool,
}

impl Content {
    pub(crate) const fn of(category: Category) -> Self {
        Self {
            categories: category.bit(),
            has_link: false,
        }
    }

    /// Content that a host describes only by its categories, so it may contain links.
    pub(crate) fn any_of(categories: &[Category]) -> Self {
        Self {
            categories: bits(categories),
            has_link: true,
        }
    }

    #[must_use]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self {
            categories: self.categories | other.categories,
            has_link: self.has_link || other.has_link,
        }
    }

    /// Whether every fragment with this content may stand where `other` is expected.
    pub(crate) const fn fits(self, other: Self) -> bool {
        self.categories & !other.categories == 0 && (other.has_link || !self.has_link)
    }

    fn outside(self, categories: &[Category]) -> Option<Category> {
        let allowed = bits(categories);
        Category::ALL
            .into_iter()
            .find(|category| self.categories & category.bit() & !allowed != 0)
    }

    pub(crate) fn description(self) -> String {
        let mut parts = Category::ALL
            .into_iter()
            .filter(|category| self.categories & category.bit() != 0)
            .map(Category::description)
            .collect::<Vec<_>>();
        if parts.is_empty() {
            parts.push("nothing");
        }
        parts.join(", ")
    }
}

fn bits(categories: &[Category]) -> u16 {
    categories
        .iter()
        .fold(0, |bits, category| bits | category.bit())
}

/// What an element that holds `children` is where it is placed.
pub(crate) fn place(
    name: &str,
    category: Category,
    holds: &[Category],
    children: Content,
) -> Result<Content, Error> {
    if let Some(outside) = children.outside(holds) {
        return Err(Error::new(
            ErrorKind::Html,
            format!(
                "<{name}> cannot contain {}; it takes {}",
                outside.description(),
                describe(holds)
            ),
        ));
    }
    if name == "a" {
        // A link takes the place of its contents, so it is phrasing only where they are. Browsers
        // split a link inside a link into two.
        if children.has_link {
            return Err(Error::new(
                ErrorKind::Html,
                "<a> cannot contain another <a>",
            ));
        }
        let category = if children.outside(PHRASING).is_some() {
            Category::Flow
        } else {
            Category::Phrasing
        };
        return Ok(Content {
            categories: category.bit(),
            has_link: true,
        });
    }
    Ok(Content {
        categories: category.bit(),
        has_link: children.has_link,
    })
}

fn describe(categories: &[Category]) -> String {
    if categories.is_empty() {
        "no children".to_owned()
    } else {
        categories
            .iter()
            .map(|category| category.description())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

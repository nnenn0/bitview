//! Where HTML may go. Browsers do not reject HTML that nests elements where they cannot go, such
//! as a `<div>` in a `<p>`: they move the elements elsewhere, and the page breaks in ways that are
//! hard to trace back. Each element has a category and takes children of some categories, and the
//! same check runs on templates while they are checked and on every element that is built.

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

    pub(crate) fn description(self) -> &'static str {
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

/// A set of categories, one bit each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Categories(u16);

impl Categories {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) const fn of(categories: &[Category]) -> Self {
        let mut bits = 0;
        let mut rest = categories;
        while let [first, tail @ ..] = rest {
            bits |= 1 << *first as u16;
            rest = tail;
        }
        Self(bits)
    }

    const fn one(category: Category) -> Self {
        Self::of(&[category])
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn is_within(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    fn iter(self) -> impl Iterator<Item = Category> {
        Category::ALL
            .into_iter()
            .filter(move |category| Self::one(*category).is_within(self))
    }

    pub(crate) fn description(self) -> String {
        let parts = self.iter().map(Category::description).collect::<Vec<_>>();
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// The categories of the top-level nodes of a fragment, and whether it contains a link anywhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Content {
    categories: Categories,
    has_link: bool,
}

impl Content {
    /// One node of `category` with no link in it, such as text.
    pub(crate) const fn of(category: Category) -> Self {
        Self::element(category, false)
    }

    /// One element of `category`. `has_link` is whether it is a link or contains one.
    pub(crate) const fn element(category: Category, has_link: bool) -> Self {
        Self {
            categories: Categories::one(category),
            has_link,
        }
    }

    /// Content that a host describes only by its categories, so it may contain links.
    pub(crate) const fn any_of(categories: &[Category]) -> Self {
        Self {
            categories: Categories::of(categories),
            has_link: true,
        }
    }

    #[must_use]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self {
            categories: self.categories.union(other.categories),
            has_link: self.has_link || other.has_link,
        }
    }

    /// Whether every fragment with this content may stand where `other` is expected.
    pub(crate) const fn fits(self, other: Self) -> bool {
        self.categories.is_within(other.categories) && (other.has_link || !self.has_link)
    }

    pub(crate) const fn has_link(self) -> bool {
        self.has_link
    }

    /// The first category of this content that is not in `allowed`.
    pub(crate) fn outside(self, allowed: Categories) -> Option<Category> {
        self.categories
            .iter()
            .find(|category| !Categories::one(*category).is_within(allowed))
    }

    pub(crate) fn description(self) -> String {
        self.categories.description()
    }
}

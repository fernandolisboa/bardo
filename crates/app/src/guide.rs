//! The user guide (issue #106): the pages the Guide screen shows, read
//! from the same Markdown files the repository keeps in `docs/guide/`.
//! The files are embedded at build time, so the guide works offline and
//! there is no copy to keep in sync.
//!
//! A page is front matter (`id`, `title`, `group`, and the `place` and
//! `tour` it explains, if any), a `# Title`, an introduction, then
//! sections: each `## Heading` follows an `<a id="…"></a>` line, which
//! gives the section an id that stays the same in every language (GitHub
//! and the site link to it too). Links are Markdown links: another page
//! (`page.md#section`), a section of the same page (`#section`), a place
//! in Bardo (`bardo:go/<place>`), a tour (`bardo:tour/<tour>`), or the web.

use bardo_domain::{TourId, UiLanguage};

use crate::{Destination, SettingsTab, Stage, TourPlace};

/// One page in both languages, embedded from `docs/guide/<language>/`.
pub(crate) struct PageSource {
    pub(crate) id: &'static str,
    pub(crate) en_us: &'static str,
    pub(crate) pt_br: &'static str,
}

macro_rules! page {
    ($id:literal) => {
        PageSource {
            id: $id,
            en_us: include_str!(concat!("../../../docs/guide/en-US/", $id, ".md")),
            pt_br: include_str!(concat!("../../../docs/guide/pt-BR/", $id, ".md")),
        }
    };
}

/// Every page, in the order the contents list them within their group.
pub(crate) const GUIDE_PAGES: &[PageSource] = &[
    page!("what-bardo-is"),
    page!("api-keys"),
    page!("first-video"),
    page!("niche-research"),
    page!("themes-ranking"),
    page!("performance-metrics"),
    page!("projects"),
    page!("script"),
    page!("narration"),
    page!("scenes"),
    page!("clips"),
    page!("personas"),
    page!("templates"),
    page!("editor"),
    page!("render"),
    page!("channels"),
    page!("network-accounts"),
    page!("app-credentials"),
    page!("connect-youtube"),
    page!("connect-instagram"),
    page!("connect-tiktok"),
    page!("uploading"),
    page!("exporting"),
    page!("missed-posts"),
    page!("glossary"),
    page!("shortcuts"),
];

/// Where a page sits in the contents, in this order. A group without
/// pages is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GuideGroup {
    GettingStarted,
    Strategy,
    Production,
    Editing,
    Publishing,
    Costs,
    Reference,
}

impl GuideGroup {
    pub const ALL: [GuideGroup; 7] = [
        GuideGroup::GettingStarted,
        GuideGroup::Strategy,
        GuideGroup::Production,
        GuideGroup::Editing,
        GuideGroup::Publishing,
        GuideGroup::Costs,
        GuideGroup::Reference,
    ];

    pub fn code(self) -> &'static str {
        match self {
            GuideGroup::GettingStarted => "getting-started",
            GuideGroup::Strategy => "strategy",
            GuideGroup::Production => "production",
            GuideGroup::Editing => "editing",
            GuideGroup::Publishing => "publishing",
            GuideGroup::Costs => "costs",
            GuideGroup::Reference => "reference",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.code() == code)
    }
}

/// A place in Bardo a page explains or a link opens: a screen, a stage of
/// the open video project, or a Settings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuidePlace {
    Screen(Destination),
    Stage(Stage),
    Settings(SettingsTab),
}

impl GuidePlace {
    /// Every place a page can explain: the screens (the Guide itself
    /// aside), the project stages and the Settings tabs.
    pub fn all() -> Vec<GuidePlace> {
        let screens = Destination::ALL
            .into_iter()
            .filter(|place| *place != Destination::Guide)
            .map(GuidePlace::Screen);
        let stages = Stage::ALL.into_iter().map(GuidePlace::Stage);
        let tabs = SettingsTab::ALL.into_iter().map(GuidePlace::Settings);
        screens.chain(stages).chain(tabs).collect()
    }

    /// `research`, `projects/script`, `settings/keys`.
    pub fn parse(code: &str) -> Option<Self> {
        match code.split_once('/') {
            None => Destination::from_code(code).map(GuidePlace::Screen),
            Some(("projects", stage)) => Stage::from_code(stage).map(GuidePlace::Stage),
            Some(("settings", tab)) => SettingsTab::from_code(tab).map(GuidePlace::Settings),
            Some(_) => None,
        }
    }

    pub fn code(self) -> String {
        match self {
            GuidePlace::Screen(place) => place.code().to_owned(),
            GuidePlace::Stage(stage) => format!("projects/{}", stage.code()),
            GuidePlace::Settings(tab) => format!("settings/{}", tab.code()),
        }
    }

    /// The screen that shows it.
    pub fn screen(self) -> Destination {
        match self {
            GuidePlace::Screen(place) => place,
            GuidePlace::Stage(_) => Destination::Projects,
            GuidePlace::Settings(_) => Destination::Settings,
        }
    }

    /// The place a tour of `place` runs on; `None` for the missed posts
    /// list, which is no place to go to.
    pub fn of_tour(place: TourPlace) -> Option<Self> {
        match place {
            TourPlace::Screen(screen) => Some(GuidePlace::Screen(screen)),
            TourPlace::Stage(stage) => Some(GuidePlace::Stage(stage)),
            TourPlace::Settings(tab) => Some(GuidePlace::Settings(tab)),
            TourPlace::Missed => None,
        }
    }
}

/// Where a link in a page leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideLink {
    /// A page of the guide, at one of its sections or at its top.
    Page {
        page: String,
        section: Option<String>,
    },
    Tour(TourId),
    Go(GuidePlace),
    /// A web address, opened in the browser.
    External(String),
}

impl GuideLink {
    /// What `url` leads to, read on page `from`; `None` for a link that
    /// leads nowhere Bardo knows.
    pub fn parse(url: &str, from: &str) -> Option<GuideLink> {
        let page_at = |target: &str| -> Option<GuideLink> {
            let (page, section) = match target.split_once('#') {
                Some((page, section)) => (page, Some(section)),
                None => (target, None),
            };
            let page = if page.is_empty() { from } else { page };
            let valid = |part: &str| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            };
            (valid(page) && section.is_none_or(valid)).then(|| GuideLink::Page {
                page: page.to_owned(),
                section: section.map(str::to_owned),
            })
        };
        if let Some(rest) = url.strip_prefix("bardo:") {
            let (kind, target) = rest.split_once('/')?;
            return match kind {
                "guide" => page_at(target),
                "tour" => TourId::from_code(target).map(GuideLink::Tour),
                "go" => GuidePlace::parse(target).map(GuideLink::Go),
                _ => None,
            };
        }
        if url.starts_with("https://") {
            return Some(GuideLink::External(url.to_owned()));
        }
        if let Some(section) = url.strip_prefix('#') {
            return page_at(&format!("{from}#{section}"));
        }
        // Another page, as a file next to this one.
        let (file, section) = match url.split_once('#') {
            Some((file, section)) => (file, Some(section)),
            None => (url, None),
        };
        let page = file.strip_suffix(".md")?;
        match section {
            Some(section) => page_at(&format!("{page}#{section}")),
            None => page_at(page),
        }
    }
}

/// A section of a page: what "On this page" lists and links reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideSection {
    /// The same in every language.
    pub id: String,
    pub title: String,
    /// Markdown, without its heading.
    pub body: String,
}

/// One page of the user guide, in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidePage {
    /// The file name without `.md`, the same in every language.
    pub id: String,
    pub title: String,
    pub group: GuideGroup,
    /// The screen, stage or Settings tab the page explains.
    pub place: Option<GuidePlace>,
    /// The tour of that place.
    pub tour: Option<TourId>,
    /// Markdown before the first section.
    pub intro: String,
    pub sections: Vec<GuideSection>,
}

/// Why a page's source cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuidePageError {
    #[error("the page does not start with front matter between --- lines")]
    NoFrontMatter,
    #[error("front matter line {0:?} is not `key: value`")]
    BadFrontMatter(String),
    #[error("unknown front matter field {0:?}")]
    UnknownField(String),
    #[error("front matter has no {0:?}")]
    MissingField(&'static str),
    #[error("unknown group {0:?}")]
    UnknownGroup(String),
    #[error("unknown place {0:?}")]
    UnknownPlace(String),
    #[error("unknown tour {0:?}")]
    UnknownTour(String),
    #[error("the page's # heading {0:?} is not its title")]
    TitleMismatch(String),
    #[error("the page has no # heading with its title")]
    TitleMissing,
    #[error("a ``` block is never closed")]
    UnclosedFence,
    #[error("the heading {0:?} has no <a id> line before it")]
    HeadingWithoutAnchor(String),
    #[error("the anchor {0:?} is not followed by a ## heading")]
    AnchorWithoutHeading(String),
    #[error("the section id {0:?} is used twice")]
    DuplicateSection(String),
}

/// The id of an `<a id="…"></a>` line.
fn anchor_id(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix("<a id=\"")?
        .strip_suffix("\"></a>")
        .filter(|id| !id.is_empty())
}

impl GuidePage {
    /// Reads a page's Markdown source.
    pub fn parse(source: &str) -> Result<GuidePage, GuidePageError> {
        // A Windows checkout may turn the files' line ends into CRLF, and
        // a Windows editor may start the file with a byte order mark.
        let source = source.replace("\r\n", "\n");
        let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
        let rest = source
            .strip_prefix("---\n")
            .ok_or(GuidePageError::NoFrontMatter)?;
        let (front, body) = rest
            .split_once("\n---\n")
            .ok_or(GuidePageError::NoFrontMatter)?;

        let (mut id, mut title, mut group, mut place, mut tour) = (None, None, None, None, None);
        for line in front.lines().filter(|line| !line.trim().is_empty()) {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| GuidePageError::BadFrontMatter(line.to_owned()))?;
            let value = value.trim().to_owned();
            match key.trim() {
                "id" => id = Some(value),
                "title" => title = Some(value),
                "group" => {
                    group = Some(
                        GuideGroup::from_code(&value).ok_or(GuidePageError::UnknownGroup(value))?,
                    );
                }
                "place" => {
                    place =
                        Some(GuidePlace::parse(&value).ok_or(GuidePageError::UnknownPlace(value))?);
                }
                "tour" => {
                    tour =
                        Some(TourId::from_code(&value).ok_or(GuidePageError::UnknownTour(value))?);
                }
                other => return Err(GuidePageError::UnknownField(other.to_owned())),
            }
        }
        let id = id.ok_or(GuidePageError::MissingField("id"))?;
        let title = title.ok_or(GuidePageError::MissingField("title"))?;
        let group = group.ok_or(GuidePageError::MissingField("group"))?;

        let mut intro = Vec::new();
        let mut sections: Vec<GuideSection> = Vec::new();
        let mut body_lines = Vec::new();
        let mut anchor: Option<String> = None;
        let mut fenced = false;
        let mut titled = false;
        for line in body.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            if !fenced
                && !titled
                && sections.is_empty()
                && anchor.is_none()
                && let Some(heading) = line.strip_prefix("# ")
            {
                if heading.trim() != title {
                    return Err(GuidePageError::TitleMismatch(heading.trim().to_owned()));
                }
                titled = true;
                continue;
            }
            if !fenced {
                if let Some(id) = anchor_id(line) {
                    if let Some(id) = anchor.take() {
                        return Err(GuidePageError::AnchorWithoutHeading(id));
                    }
                    anchor = Some(id.to_owned());
                    continue;
                }
                if let Some(heading) = line.strip_prefix("## ") {
                    let id = anchor
                        .take()
                        .ok_or_else(|| GuidePageError::HeadingWithoutAnchor(heading.to_owned()))?;
                    if sections.iter().any(|section| section.id == id) {
                        return Err(GuidePageError::DuplicateSection(id));
                    }
                    if let Some(last) = sections.last_mut() {
                        last.body = std::mem::take(&mut body_lines).join("\n").trim().to_owned();
                    } else {
                        intro = std::mem::take(&mut body_lines);
                    }
                    sections.push(GuideSection {
                        id,
                        title: heading.trim().to_owned(),
                        body: String::new(),
                    });
                    continue;
                }
                if anchor.is_some() && !line.trim().is_empty() {
                    return Err(GuidePageError::AnchorWithoutHeading(
                        anchor.unwrap_or_default(),
                    ));
                }
            }
            body_lines.push(line);
        }
        if let Some(id) = anchor {
            return Err(GuidePageError::AnchorWithoutHeading(id));
        }
        if fenced {
            return Err(GuidePageError::UnclosedFence);
        }
        if !titled {
            return Err(GuidePageError::TitleMissing);
        }
        match sections.last_mut() {
            Some(last) => last.body = body_lines.join("\n").trim().to_owned(),
            None => intro = body_lines,
        }
        Ok(GuidePage {
            id,
            title,
            group,
            place,
            tour,
            intro: intro.join("\n").trim().to_owned(),
            sections,
        })
    }

    pub fn section(&self, id: &str) -> Option<&GuideSection> {
        self.sections.iter().find(|section| section.id == id)
    }

    /// Every link target in the page, as written.
    pub fn links(&self) -> Vec<String> {
        std::iter::once(self.intro.as_str())
            .chain(self.sections.iter().map(|section| section.body.as_str()))
            .flat_map(links_in)
            .collect()
    }

    /// The page as plain text, for search.
    fn units(&self) -> impl Iterator<Item = (Option<&GuideSection>, &str, &str)> {
        std::iter::once((None, self.title.as_str(), self.intro.as_str())).chain(
            self.sections
                .iter()
                .map(|section| (Some(section), section.title.as_str(), section.body.as_str())),
        )
    }
}

/// The targets of the Markdown links in `markdown` (`[text](target)`).
fn links_in(markdown: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        links.push(after[..end].trim().to_owned());
        rest = &after[end..];
    }
    links
}

/// Markdown as the words a reader sees: link text without its target, no
/// emphasis, code marks, table bars or list marks.
pub fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut chars = markdown.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `](target)`: keep the text before, drop the target.
            ']' if chars.peek() == Some(&'(') => {
                for skipped in chars.by_ref() {
                    if skipped == ')' {
                        break;
                    }
                }
            }
            '[' | '*' | '`' | '|' | '#' => {}
            '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    let words: Vec<&str> = out
        .split_whitespace()
        .filter(|word| !word.chars().all(|c| c == '-' || c == ':'))
        .collect();
    words.join(" ")
}

/// Folds text for search: lower case and without accents, one char for
/// one char, so positions in the folded text are positions in the text.
pub fn fold(text: &str) -> Vec<char> {
    text.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            match lower {
                'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                'ñ' => 'n',
                'ý' | 'ÿ' => 'y',
                other => other,
            }
        })
        .collect()
}

fn find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A search result: a page, or one of its sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideHit {
    pub page: String,
    pub section: Option<String>,
    pub page_title: String,
    /// The section's title, when the hit is a section.
    pub section_title: Option<String>,
    /// The words around the first match.
    pub snippet: String,
}

/// The most results a search returns.
const MAX_HITS: usize = 40;
/// Characters of context on each side of a match in a snippet.
const SNIPPET_CONTEXT: usize = 60;

/// The user guide in one language.
#[derive(Debug, Clone)]
pub struct Guide {
    language: UiLanguage,
    pages: Vec<GuidePage>,
}

impl Guide {
    /// The embedded pages. They ship inside the binary and are checked by
    /// tests, so a page that does not parse is a build defect.
    pub fn load(language: UiLanguage) -> Self {
        let pages = GUIDE_PAGES
            .iter()
            .map(|source| {
                let text = match language {
                    UiLanguage::EnUs => source.en_us,
                    UiLanguage::PtBr => source.pt_br,
                };
                GuidePage::parse(text).unwrap_or_else(|error| {
                    panic!("docs/guide/{language}/{}.md is invalid: {error}", source.id)
                })
            })
            .collect();
        Self { language, pages }
    }

    pub fn language(&self) -> UiLanguage {
        self.language
    }

    pub fn pages(&self) -> &[GuidePage] {
        &self.pages
    }

    pub fn page(&self, id: &str) -> Option<&GuidePage> {
        self.pages.iter().find(|page| page.id == id)
    }

    /// The page the guide opens on when nothing better fits.
    pub fn first(&self) -> &GuidePage {
        self.contents()
            .first()
            .and_then(|(_, pages)| pages.first().copied())
            .expect("the guide has pages")
    }

    /// The table of contents: the groups with pages, in order, each with its
    /// pages.
    pub fn contents(&self) -> Vec<(GuideGroup, Vec<&GuidePage>)> {
        GuideGroup::ALL
            .into_iter()
            .map(|group| {
                let pages = self
                    .pages
                    .iter()
                    .filter(|page| page.group == group)
                    .collect();
                (group, pages)
            })
            .filter(|(_, pages): &(GuideGroup, Vec<&GuidePage>)| !pages.is_empty())
            .collect()
    }

    /// The page F1 opens at `place`: the one that explains it, else the
    /// one that explains its screen, else the first page.
    pub fn page_at(&self, place: Option<GuidePlace>) -> &GuidePage {
        let explaining =
            |place: GuidePlace| self.pages.iter().find(|page| page.place == Some(place));
        place
            .and_then(|place| {
                explaining(place).or_else(|| explaining(GuidePlace::Screen(place.screen())))
            })
            .unwrap_or_else(|| self.first())
    }

    /// The pages and sections whose title, headings or text hold every word
    /// of `query`, ignoring case and accents: headings first, then titles,
    /// then text, each in the contents' order.
    pub fn search(&self, query: &str) -> Vec<GuideHit> {
        let words: Vec<Vec<char>> = query.split_whitespace().map(fold).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let order: Vec<&GuidePage> = self
            .contents()
            .into_iter()
            .flat_map(|(_, pages)| pages)
            .collect();
        let mut hits: Vec<(u32, GuideHit)> = Vec::new();
        for page in order {
            let page_title = fold(&page.title);
            for (section, heading, markdown) in page.units() {
                let text = plain_text(markdown);
                let folded_heading = fold(heading);
                let folded_text = fold(&text);
                let mut score = 0;
                let mut first_in_text = None;
                // The page's title counts only for a unit that has one of
                // the words itself, so a title word alone does not list
                // every section of its page.
                let mut in_unit = false;
                for word in &words {
                    let in_title = find(&page_title, word).is_some();
                    if find(&folded_heading, word).is_some() {
                        score += 3;
                        in_unit = true;
                    } else if let Some(at) = find(&folded_text, word) {
                        score += if in_title { 2 } else { 1 };
                        first_in_text = first_in_text.or(Some(at));
                        in_unit = true;
                    } else if in_title {
                        score += 2;
                    } else {
                        score = 0;
                        break;
                    }
                }
                if score == 0 || !in_unit {
                    continue;
                }
                hits.push((
                    score,
                    GuideHit {
                        page: page.id.clone(),
                        section: section.map(|section| section.id.clone()),
                        page_title: page.title.clone(),
                        section_title: section.map(|section| section.title.clone()),
                        snippet: snippet(&text, first_in_text),
                    },
                ));
            }
        }
        // Stable: equal scores keep the contents' order.
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.0));
        hits.into_iter()
            .take(MAX_HITS)
            .map(|(_, hit)| hit)
            .collect()
    }
}

/// The text around char `at`, or the text's start, cut at words.
fn snippet(text: &str, at: Option<usize>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = at.unwrap_or(0);
    let mut start = at.saturating_sub(SNIPPET_CONTEXT);
    let mut end = (at + SNIPPET_CONTEXT * 2).min(chars.len());
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    let mut out: String = chars[start..end].iter().collect();
    out = out.trim().to_owned();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// What is wrong between the two languages' pages: a page or section
/// missing from one, or a different group, place or tour.
#[cfg(test)]
fn sync_problems(en_us: &[GuidePage], pt_br: &[GuidePage]) -> Vec<String> {
    let mut problems = Vec::new();
    for page in en_us {
        let Some(other) = pt_br.iter().find(|other| other.id == page.id) else {
            problems.push(format!("{}: missing in pt-BR", page.id));
            continue;
        };
        if (page.group, page.place, page.tour) != (other.group, other.place, other.tour) {
            problems.push(format!("{}: group, place or tour differ", page.id));
        }
        let ids = |page: &GuidePage| -> Vec<String> {
            page.sections
                .iter()
                .map(|section| section.id.clone())
                .collect()
        };
        if ids(page) != ids(other) {
            problems.push(format!(
                "{}: sections differ: {:?} and {:?}",
                page.id,
                ids(page),
                ids(other)
            ));
        }
    }
    for page in pt_br {
        if !en_us.iter().any(|other| other.id == page.id) {
            problems.push(format!("{}: missing in en-US", page.id));
        }
    }
    problems
}

/// Every link in `pages` that leads nowhere: an unknown page, section,
/// tour or place, or a web address that is not https.
#[cfg(test)]
fn link_problems(pages: &[GuidePage]) -> Vec<String> {
    let mut problems = Vec::new();
    for page in pages {
        for url in page.links() {
            let resolved = match GuideLink::parse(&url, &page.id) {
                Some(GuideLink::Page {
                    page: target,
                    section,
                }) => pages
                    .iter()
                    .find(|page| page.id == target)
                    .is_some_and(|target| {
                        section.is_none_or(|section| target.section(&section).is_some())
                    }),
                Some(GuideLink::External(url)) => url.starts_with("https://"),
                Some(GuideLink::Tour(_) | GuideLink::Go(_)) => true,
                None => false,
            };
            if !resolved {
                problems.push(format!("{}: {url}", page.id));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Text;
    use crate::Tour;

    const PAGE: &str = "---
id: sample
title: A sample
group: strategy
place: projects/script
tour: welcome
---

# A sample

The intro, with [a link](other.md#part).

<a id=\"first\"></a>
## First part

Text of the **first** part.

### A smaller heading

```
## not a section
```

<a id=\"second\"></a>
## Second part

[Research](bardo:go/research) and [the tour](bardo:tour/welcome).
";

    fn page(source: &str) -> GuidePage {
        GuidePage::parse(source).unwrap()
    }

    fn sample(id: &str, sections: &[&str], links: &str) -> GuidePage {
        GuidePage {
            id: id.to_owned(),
            title: id.to_owned(),
            group: GuideGroup::Reference,
            place: None,
            tour: None,
            intro: links.to_owned(),
            sections: sections
                .iter()
                .map(|id| GuideSection {
                    id: (*id).to_owned(),
                    title: (*id).to_owned(),
                    body: String::new(),
                })
                .collect(),
        }
    }

    fn both() -> (Guide, Guide) {
        (Guide::load(UiLanguage::EnUs), Guide::load(UiLanguage::PtBr))
    }

    #[test]
    fn a_page_reads_its_front_matter_intro_and_sections() {
        let page = page(PAGE);
        assert_eq!(page.id, "sample");
        assert_eq!(page.title, "A sample");
        assert_eq!(page.group, GuideGroup::Strategy);
        assert_eq!(page.place, Some(GuidePlace::Stage(Stage::Script)));
        assert_eq!(page.tour, Some(TourId::Welcome));
        assert_eq!(page.intro, "The intro, with [a link](other.md#part).");
        let ids: Vec<&str> = page.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["first", "second"]);
        assert_eq!(page.sections[0].title, "First part");
        assert!(
            page.sections[0]
                .body
                .starts_with("Text of the **first** part.")
        );
        assert!(
            page.sections[0].body.contains("## not a section"),
            "a fenced block keeps its lines"
        );
        assert!(page.sections[0].body.contains("### A smaller heading"));
        assert_eq!(page.section("second").unwrap().title, "Second part");
    }

    #[test]
    fn windows_line_ends_and_byte_order_mark_read_the_same() {
        assert_eq!(page(&PAGE.replace('\n', "\r\n")), page(PAGE));
        assert_eq!(page(&format!("\u{feff}{PAGE}")), page(PAGE));
    }

    #[test]
    fn a_page_lists_its_links() {
        assert_eq!(
            page(PAGE).links(),
            ["other.md#part", "bardo:go/research", "bardo:tour/welcome"]
        );
    }

    #[test]
    fn a_broken_page_says_why() {
        let with = |from: &str, to: &str| GuidePage::parse(&PAGE.replacen(from, to, 1));
        assert_eq!(
            GuidePage::parse("# No front matter"),
            Err(GuidePageError::NoFrontMatter)
        );
        assert_eq!(
            with("group: strategy\n", ""),
            Err(GuidePageError::MissingField("group"))
        );
        assert_eq!(
            with("group: strategy", "group: cooking"),
            Err(GuidePageError::UnknownGroup("cooking".into()))
        );
        assert_eq!(
            with("place: projects/script", "place: projects/bake"),
            Err(GuidePageError::UnknownPlace("projects/bake".into()))
        );
        assert_eq!(
            with("tour: welcome", "tour: nowhere"),
            Err(GuidePageError::UnknownTour("nowhere".into()))
        );
        assert_eq!(
            with("tour: welcome", "author: someone"),
            Err(GuidePageError::UnknownField("author".into()))
        );
        assert_eq!(
            with("# A sample", "# Another title"),
            Err(GuidePageError::TitleMismatch("Another title".into()))
        );
        assert_eq!(with("# A sample\n", ""), Err(GuidePageError::TitleMissing));
        assert_eq!(
            GuidePage::parse(&format!("{PAGE}\n```\nnever closed")),
            Err(GuidePageError::UnclosedFence)
        );
        assert_eq!(
            with("<a id=\"second\"></a>\n", ""),
            Err(GuidePageError::HeadingWithoutAnchor("Second part".into()))
        );
        assert_eq!(
            with("## Second part\n", ""),
            Err(GuidePageError::AnchorWithoutHeading("second".into()))
        );
        assert_eq!(
            with("id=\"second\"", "id=\"first\""),
            Err(GuidePageError::DuplicateSection("first".into()))
        );
    }

    #[test]
    fn links_lead_to_pages_sections_tours_places_and_the_web() {
        let page = |page: &str, section: Option<&str>| {
            Some(GuideLink::Page {
                page: page.into(),
                section: section.map(Into::into),
            })
        };
        assert_eq!(
            GuideLink::parse("api-keys.md", "here"),
            page("api-keys", None)
        );
        assert_eq!(
            GuideLink::parse("api-keys.md#budgets", "here"),
            page("api-keys", Some("budgets"))
        );
        assert_eq!(
            GuideLink::parse("#flow", "here"),
            page("here", Some("flow"))
        );
        assert_eq!(
            GuideLink::parse("bardo:guide/glossary#editing", "here"),
            page("glossary", Some("editing"))
        );
        assert_eq!(
            GuideLink::parse("bardo:tour/welcome", "here"),
            Some(GuideLink::Tour(TourId::Welcome))
        );
        assert_eq!(
            GuideLink::parse("bardo:go/settings/keys", "here"),
            Some(GuideLink::Go(GuidePlace::Settings(SettingsTab::Keys)))
        );
        assert_eq!(
            GuideLink::parse("bardo:go/projects/render", "here"),
            Some(GuideLink::Go(GuidePlace::Stage(Stage::Render)))
        );
        assert_eq!(
            GuideLink::parse("https://example.com/a", "here"),
            Some(GuideLink::External("https://example.com/a".into()))
        );
        for broken in [
            "bardo:go/kitchen",
            "bardo:tour/nowhere",
            "bardo:open/research",
            "../adr/0001.md",
            "file.txt",
            "Page.md",
            "javascript:alert(1)",
            "http://example.com",
        ] {
            assert_eq!(GuideLink::parse(broken, "here"), None, "{broken}");
        }
    }

    #[test]
    fn places_read_back_from_their_codes() {
        for place in GuidePlace::all() {
            assert_eq!(GuidePlace::parse(&place.code()), Some(place), "{place:?}");
        }
        assert_eq!(GuidePlace::parse("settings/kitchen"), None);
        assert_eq!(GuidePlace::parse("themes/script"), None);
        assert!(!GuidePlace::all().contains(&GuidePlace::Screen(Destination::Guide)));
    }

    #[test]
    fn plain_text_keeps_what_a_reader_sees() {
        assert_eq!(
            plain_text(
                "Save **keys** in [Settings](bardo:go/settings/keys).\n\n| A | B |\n| --- | --- |\n| `x` | y |"
            ),
            "Save keys in Settings. A B x y"
        );
    }

    #[test]
    fn folding_ignores_case_and_accents() {
        assert_eq!(fold("Narração ÉPICA"), fold("narracao epica"));
        assert_eq!(fold("Ação").len(), "Ação".chars().count());
    }

    #[test]
    fn search_ignores_accents_and_case() {
        let guide = Guide::load(UiLanguage::PtBr);
        let hits = guide.search("narracao");
        assert!(!hits.is_empty());
        assert!(
            hits.iter().any(
                |hit| hit.section_title.as_deref() == Some("Escolha um narrador")
                    || hit.snippet.to_lowercase().contains("narração")
            ),
            "{hits:?}"
        );
        assert_eq!(guide.search("NARRAÇÃO"), hits);
    }

    #[test]
    fn search_ranks_headings_before_text_and_needs_every_word() {
        let guide = Guide::load(UiLanguage::EnUs);
        let hits = guide.search("budget");
        assert_eq!(hits[0].page, "api-keys");
        assert_eq!(hits[0].section.as_deref(), Some("budgets"));
        assert!(
            hits.iter().any(|hit| hit.page == "glossary"),
            "text matches too: {hits:?}"
        );
        assert!(guide.search("budget zzzz").is_empty(), "every word");
        for hit in guide.search("keys") {
            let page = guide.page(&hit.page).unwrap();
            let unit = page
                .units()
                .into_iter()
                .find(|(section, ..)| section.map(|section| &section.id) == hit.section.as_ref())
                .unwrap();
            let unit = fold(&format!("{} {}", unit.1, plain_text(unit.2)));
            assert!(
                find(&unit, &fold("key")).is_some(),
                "a title word alone lists no section: {hit:?}"
            );
        }
        assert!(guide.search("   ").is_empty());
    }

    #[test]
    fn a_snippet_shows_the_words_around_the_match() {
        let text = "one two three four five six seven eight nine ten ".repeat(10);
        let shown = snippet(&text, Some(200));
        assert!(shown.starts_with('…') && shown.ends_with('…'), "{shown}");
        assert!(shown.chars().count() <= SNIPPET_CONTEXT * 3 + 12);
        assert_eq!(snippet("short text", None), "short text");
    }

    #[test]
    fn contents_group_the_pages_and_leave_out_empty_groups() {
        let guide = Guide::load(UiLanguage::EnUs);
        let contents = guide.contents();
        let groups: Vec<GuideGroup> = contents.iter().map(|(group, _)| *group).collect();
        assert_eq!(
            groups,
            [
                GuideGroup::GettingStarted,
                GuideGroup::Strategy,
                GuideGroup::Production,
                GuideGroup::Editing,
                GuideGroup::Publishing,
                GuideGroup::Reference
            ]
        );
        assert_eq!(guide.first().id, "what-bardo-is");
    }

    #[test]
    fn f1_opens_the_page_for_the_place_or_its_screen_or_the_first() {
        let guide = Guide::load(UiLanguage::EnUs);
        let keys = GuidePlace::Settings(SettingsTab::Keys);
        assert_eq!(guide.page_at(Some(keys)).id, "api-keys");
        assert_eq!(guide.page_at(None).id, "what-bardo-is");
        assert_eq!(
            guide.page_at(Some(GuidePlace::Stage(Stage::Script))).id,
            "script"
        );
        assert_eq!(
            guide.page_at(Some(GuidePlace::Stage(Stage::Edit))).id,
            "editor"
        );
        assert_eq!(
            guide.page_at(Some(GuidePlace::Stage(Stage::Publish))).id,
            "uploading"
        );
        assert_eq!(
            guide
                .page_at(Some(GuidePlace::Settings(SettingsTab::Networks)))
                .id,
            "app-credentials"
        );
        assert_eq!(
            guide
                .page_at(Some(GuidePlace::Settings(SettingsTab::Metrics)))
                .id,
            "what-bardo-is",
            "no page for the tab yet"
        );
        assert_eq!(
            guide
                .page_at(Some(GuidePlace::Screen(Destination::Costs)))
                .id,
            "what-bardo-is",
            "no page for the screen yet"
        );
    }

    // The sync tests: the two languages, the links, the tours and the
    // files all agree.

    #[test]
    fn every_page_parses_in_every_language_under_its_file_name() {
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            for (page, source) in guide.pages().iter().zip(GUIDE_PAGES) {
                assert_eq!(page.id, source.id, "{language}");
                assert!(!page.sections.is_empty(), "{language}: {}", page.id);
            }
        }
    }

    #[test]
    fn both_languages_have_the_same_pages_and_sections() {
        let (en_us, pt_br) = both();
        let problems = sync_problems(en_us.pages(), pt_br.pages());
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn the_sync_check_catches_a_missing_translation() {
        let en_us = [sample("a", &["one", "two"], ""), sample("b", &[], "")];
        let pt_br = [sample("a", &["one"], "")];
        let problems = sync_problems(&en_us, &pt_br);
        assert_eq!(problems.len(), 2, "{problems:?}");

        let mut moved = sample("a", &["one", "two"], "");
        moved.place = Some(GuidePlace::Screen(Destination::Costs));
        assert_eq!(sync_problems(&en_us[..1], &[moved]).len(), 1);
    }

    #[test]
    fn every_link_resolves() {
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            let problems = link_problems(guide.pages());
            assert!(problems.is_empty(), "{language}: {problems:#?}");
        }
    }

    #[test]
    fn the_link_check_catches_a_broken_link() {
        let pages = [
            sample(
                "a",
                &["one"],
                "[ok](b.md#two) [ok](#one) [ok](bardo:go/costs)",
            ),
            sample(
                "b",
                &["two"],
                "[bad](a.md#nine) [bad](c.md) [bad](http://x.org) [bad](bardo:go/x)",
            ),
        ];
        assert_eq!(
            link_problems(&pages).len(),
            4,
            "{:?}",
            link_problems(&pages)
        );
    }

    #[test]
    fn every_guide_file_is_embedded() {
        for language in UiLanguage::ALL {
            let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/guide")
                .join(language.to_string());
            let mut files: Vec<String> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".md"))
                .collect();
            files.sort();
            let mut embedded: Vec<String> = GUIDE_PAGES
                .iter()
                .map(|page| format!("{}.md", page.id))
                .collect();
            embedded.sort();
            assert_eq!(files, embedded, "{language}: add new pages to GUIDE_PAGES");
        }
    }

    #[test]
    fn no_markdown_file_points_at_the_old_network_guides() {
        // The network setup guides moved from `docs/guides/` into the user
        // guide (issue #110).
        fn markdown(folder: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    markdown(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "md") {
                    found.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(!root.join("docs/guides").exists(), "docs/guides/ is gone");
        let mut files = vec![root.join("README.md")];
        markdown(&root.join("docs"), &mut files);
        markdown(&root.join("crates"), &mut files);
        let old = ["docs/guides/", "guides/publishing-"];
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for old in old {
                assert!(!text.contains(old), "{} names {old}", file.display());
            }
        }
    }

    #[test]
    fn every_tour_step_guide_section_exists() {
        let (en_us, _) = both();
        for tour in Tour::ALL {
            for step in tour.steps {
                let Some(guide) = step.guide else {
                    continue;
                };
                let page = en_us
                    .page(guide.page)
                    .unwrap_or_else(|| panic!("{}: no page {}", step.key, guide.page));
                assert!(
                    page.section(guide.section).is_some(),
                    "{}: no section {}#{}",
                    step.key,
                    guide.page,
                    guide.section
                );
            }
        }
    }

    #[test]
    fn every_place_tour_has_the_page_of_its_place() {
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            for tour in Tour::ALL {
                let Some(place) = tour.place else {
                    continue;
                };
                // A place's page shows its first tour (the editor's has two
                // parts); "Show me" starts it. The missed posts list is no
                // place: its page names its tour alone.
                let page = guide
                    .pages()
                    .iter()
                    .find(|page| match GuidePlace::of_tour(place) {
                        Some(place) => page.place == Some(place),
                        None => page.place.is_none() && page.tour == Some(tour.id),
                    })
                    .unwrap_or_else(|| panic!("{language}: no page explains {:?}", tour.id));
                assert_eq!(
                    page.tour,
                    Tour::of(place).map(|first| first.id),
                    "{}",
                    page.id
                );
                // Every step's "Learn more" stays on the screen's page.
                for step in tour.steps {
                    assert_eq!(step.guide.map(|guide| guide.page), Some(page.id.as_str()));
                }
            }
        }
    }

    #[test]
    fn the_shortcuts_page_lists_every_shortcut() {
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            let page = guide.page("shortcuts").unwrap();
            for group in crate::SHORTCUTS {
                let Text::ShortcutGroup(code) = group.name else {
                    panic!("{:?} is not a shortcut group", group.name);
                };
                let section = page
                    .section(code)
                    .unwrap_or_else(|| panic!("{language}: no section {code}"));
                // Each table row's keys, as the first cell writes them.
                let rows: Vec<Vec<&str>> = section
                    .body
                    .lines()
                    .filter_map(|line| line.strip_prefix('|')?.split('|').next())
                    .map(|cell| {
                        cell.split([' ', ','])
                            .filter(|key| !key.is_empty() && !["or", "ou"].contains(key))
                            .collect()
                    })
                    .collect();
                for shortcut in group.shortcuts {
                    assert!(
                        rows.iter().any(|row| row.as_slice() == shortcut.keys),
                        "{language}: {code} has no row for {:?}",
                        shortcut.keys
                    );
                }
            }
        }
    }

    /// Which places have a page and a tour. It reports and does not fail
    /// yet; it turns strict once every place has both.
    #[test]
    fn coverage_report() {
        let guide = Guide::load(UiLanguage::EnUs);
        let mut lines = Vec::new();
        // The places F1 opens a page for: not the Jobs panel, a panel
        // beside the screen.
        let reached = |place: &GuidePlace| !matches!(place, GuidePlace::Screen(Destination::Jobs));
        for place in GuidePlace::all().into_iter().filter(reached) {
            let page = guide
                .pages()
                .iter()
                .find(|page| page.place == Some(place))
                .map(|page| page.id.as_str());
            let tour = guide
                .pages()
                .iter()
                .find(|page| page.place == Some(place))
                .and_then(|page| page.tour);
            lines.push(format!(
                "{:<24} page: {:<12} tour: {}",
                place.code(),
                page.unwrap_or("-"),
                tour.map_or("-", TourId::code)
            ));
        }
        println!("Guide coverage:\n{}", lines.join("\n"));
    }
}

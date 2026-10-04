//! The user guide as a website (issue #112): the sources of one mdBook
//! book per language, made from the same pages the Guide screen embeds,
//! so the app and the site never drift apart.
//!
//! Each book's sidebar is the Guide screen's contents (the groups, then
//! their pages, in the app's order). Every page starts with a link to the
//! same page in the other language. Links between pages stay Markdown
//! links, which mdBook turns into links between the HTML pages; Bardo's
//! own links become something a browser can follow: `bardo:guide/` a
//! link to the page, `bardo:go/` a link to the page that explains the
//! place, and `bardo:tour/` its text with an "in the app" note, since a
//! tour only runs in Bardo. A link that leads nowhere fails the build.
//!
//! Layout of the files, under the output folder:
//!
//! - `<language>/book.toml`, `<language>/bardo.css`,
//!   `<language>/src/SUMMARY.md` and `<language>/src/<page>.md`, one book
//!   per language, which mdBook builds into `site/<language>/`;
//! - `site/index.html`, which sends a visitor to the book in their
//!   browser's language.

use bardo_domain::UiLanguage;

use crate::guide::{link_problems, sync_problems};
use crate::{Catalog, Guide, GuideLink, GuidePage, GuidePlace, Text};

/// A file of the site's sources, relative to the output folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteFile {
    pub path: String,
    pub contents: String,
}

/// Where the site is served from, and the repository its pages are
/// edited in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteOptions {
    /// The path the site is served at, such as `/bardo` on GitHub Pages.
    /// mdBook's 404 page loads its styles from there.
    pub base_path: String,
    /// The repository's web address, for the "edit this page" link; none
    /// leaves the link out.
    pub repository: Option<String>,
}

/// The words the site adds around the pages, in one language.
struct SiteText {
    title: &'static str,
    description: &'static str,
    /// The language switch on every page of the *other* language's book.
    read_in: &'static str,
    /// After the text of a link that only works inside Bardo.
    in_app: &'static str,
}

fn site_text(language: UiLanguage) -> SiteText {
    match language {
        UiLanguage::EnUs => SiteText {
            title: "Bardo user guide",
            description: "How to use Bardo, from your first channel to a published video.",
            read_in: "In English",
            in_app: "in the app",
        },
        UiLanguage::PtBr => SiteText {
            title: "Guia do Bardo",
            description: "Como usar o Bardo, do primeiro canal ao vídeo publicado.",
            read_in: "Em português",
            in_app: "no app",
        },
    }
}

/// The sources of the site: a book per language and the landing page.
/// Fails with every problem found: a page missing in a language, a
/// section that differs, or a link that leads nowhere.
pub fn guide_site(options: &SiteOptions) -> Result<Vec<SiteFile>, Vec<String>> {
    let guides = UiLanguage::ALL.map(Guide::load);
    build(&guides, options)
}

fn build(guides: &[Guide], options: &SiteOptions) -> Result<Vec<SiteFile>, Vec<String>> {
    let mut problems = Vec::new();
    for guide in guides {
        problems.extend(
            link_problems(guide.pages())
                .into_iter()
                .map(|problem| format!("{}: broken link in {problem}", guide.language())),
        );
    }
    // en-US sets the structure; every other language follows it.
    if let Some(en_us) = guides
        .iter()
        .find(|guide| guide.language() == UiLanguage::EnUs)
    {
        for other in guides
            .iter()
            .filter(|guide| guide.language() != UiLanguage::EnUs)
        {
            problems.extend(sync_problems(en_us.pages(), other.pages()));
        }
    }

    let mut files = Vec::new();
    for guide in guides {
        let language = guide.language();
        let others: Vec<UiLanguage> = guides
            .iter()
            .map(Guide::language)
            .filter(|other| *other != language)
            .collect();
        files.push(SiteFile {
            path: format!("{language}/book.toml"),
            contents: book_toml(language, options),
        });
        files.push(SiteFile {
            path: format!("{language}/bardo.css"),
            contents: STYLE.to_owned(),
        });
        files.push(SiteFile {
            path: format!("{language}/src/SUMMARY.md"),
            contents: summary(guide),
        });
        for page in guide.pages() {
            files.push(SiteFile {
                path: format!("{language}/src/{}.md", page.id),
                contents: page_markdown(guide, page, &others),
            });
        }
    }
    files.push(SiteFile {
        path: "site/index.html".to_owned(),
        contents: landing(guides),
    });

    if problems.is_empty() {
        Ok(files)
    } else {
        Err(problems)
    }
}

fn book_toml(language: UiLanguage, options: &SiteOptions) -> String {
    let text = site_text(language);
    let quoted = |value: &str| toml::Value::String(value.to_owned()).to_string();
    let base = options.base_path.trim_end_matches('/');
    let mut toml = format!(
        "# Made by `cargo run -p bardo-app --example guide_site` from docs/guide/; do not edit.\n\
         [book]\n\
         title = {title}\n\
         description = {description}\n\
         language = {language_tag}\n\
         src = \"src\"\n\
         \n\
         [build]\n\
         build-dir = \"../site/{language}\"\n\
         create-missing = false\n\
         \n\
         [output.html]\n\
         site-url = {site_url}\n\
         default-theme = \"light\"\n\
         preferred-dark-theme = \"navy\"\n\
         no-section-label = true\n\
         additional-css = [\"bardo.css\"]\n",
        title = quoted(text.title),
        description = quoted(text.description),
        language_tag = quoted(language.tag()),
        site_url = quoted(&format!("{base}/{language}/")),
    );
    if let Some(repository) = &options.repository {
        let repository = repository.trim_end_matches('/');
        toml.push_str(&format!(
            "git-repository-url = {}\nedit-url-template = {}\n",
            quoted(repository),
            quoted(&format!(
                "{repository}/edit/main/docs/guide/{language}/{{path}}"
            )),
        ));
    }
    toml.push_str("\n[output.html.search]\nenable = true\n");
    toml
}

/// The sidebar: the Guide screen's groups, then their pages.
fn summary(guide: &Guide) -> String {
    let catalog = Catalog::load(guide.language());
    let mut summary = String::from("# Summary\n");
    for (group, pages) in guide.contents() {
        summary.push_str(&format!(
            "\n# {}\n\n",
            catalog.get(Text::GuideGroupName(group))
        ));
        for page in pages {
            summary.push_str(&format!("- [{}]({}.md)\n", page.title, page.id));
        }
    }
    summary
}

/// A page for mdBook: the language switch, the title, the introduction
/// and the sections with their anchors, with Bardo's links made into web
/// links. A link that leads nowhere stays as written; the link check
/// has already failed the build on it.
fn page_markdown(guide: &Guide, page: &GuidePage, others: &[UiLanguage]) -> String {
    let convert = |markdown: &str| {
        map_links(markdown, |label, url| {
            web_link(guide, page, label, url).unwrap_or_else(|| format!("[{label}]({url})"))
        })
    };

    let mut out = String::new();
    if !others.is_empty() {
        let links: Vec<String> = others
            .iter()
            .map(|other| {
                format!(
                    "<a href=\"../{other}/{id}.html\" hreflang=\"{other}\" lang=\"{other}\">{read_in}</a>",
                    id = page.id,
                    read_in = site_text(*other).read_in,
                )
            })
            .collect();
        out.push_str(&format!(
            "<p class=\"bardo-language\">{}</p>\n\n",
            links.join(" · ")
        ));
    }
    out.push_str(&format!("# {}\n", page.title));
    if !page.intro.is_empty() {
        out.push_str(&format!("\n{}\n", convert(&page.intro)));
    }
    for section in &page.sections {
        out.push_str(&format!("\n## {} {{#{}}}\n", section.title, section.id));
        if !section.body.is_empty() {
            out.push_str(&format!("\n{}\n", convert(&section.body)));
        }
    }
    out
}

/// What a link written in a guide page becomes on the site; `None` when
/// it leads nowhere Bardo knows.
fn web_link(guide: &Guide, page: &GuidePage, label: &str, url: &str) -> Option<String> {
    let in_app = || format!("{label} *({})*", site_text(guide.language()).in_app);
    let to_page = |target: &str, section: Option<&str>| match (target == page.id, section) {
        (true, Some(section)) => format!("[{label}](#{section})"),
        (true, None) => label.to_owned(),
        (false, Some(section)) => format!("[{label}]({target}.md#{section})"),
        (false, None) => format!("[{label}]({target}.md)"),
    };
    Some(match GuideLink::parse(url, &page.id)? {
        GuideLink::Page {
            page: target,
            section,
        } => to_page(&target, section.as_deref()),
        GuideLink::External(url) => format!("[{label}]({url})"),
        GuideLink::Go(place) => match explaining(guide, place) {
            Some(target) => to_page(&target.id, None),
            None => in_app(),
        },
        GuideLink::Tour(_) => in_app(),
    })
}

/// The page that explains `place`, else the one that explains its screen.
fn explaining(guide: &Guide, place: GuidePlace) -> Option<&GuidePage> {
    let page_of = |place: GuidePlace| guide.pages().iter().find(|page| page.place == Some(place));
    page_of(place).or_else(|| page_of(GuidePlace::Screen(place.screen())))
}

/// `markdown` with every link `[label](url)` replaced by what `link` makes
/// of its label and url. Finds links the way the guide's link check does.
fn map_links(markdown: &str, mut link: impl FnMut(&str, &str) -> String) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let before = &rest[..start];
        match before.rfind('[') {
            Some(open) => {
                out.push_str(&before[..open]);
                out.push_str(&link(&before[open + 1..], after[..end].trim()));
            }
            None => out.push_str(&rest[..start + 2 + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The site's root: sends a visitor to the book in their browser's
/// language, with a link to each book for anyone without scripts.
fn landing(guides: &[Guide]) -> String {
    let languages: Vec<UiLanguage> = guides.iter().map(Guide::language).collect();
    let fallback = languages
        .iter()
        .copied()
        .find(|language| *language == UiLanguage::EnUs)
        .or_else(|| languages.first().copied())
        .unwrap_or(UiLanguage::EnUs);
    let title = languages
        .iter()
        .map(|language| site_text(*language).title)
        .collect::<Vec<_>>()
        .join(" · ");
    let links: String = languages
        .iter()
        .map(|language| {
            format!(
                "      <li><a href=\"{language}/index.html\" hreflang=\"{language}\" lang=\"{language}\">{}</a></li>\n",
                site_text(*language).title
            )
        })
        .collect();
    let prefixes: String = languages
        .iter()
        .map(|language| {
            let tag = language.tag();
            let prefix = tag.split('-').next().unwrap_or(tag).to_lowercase();
            format!("[\"{prefix}\", \"{tag}/index.html\"], ")
        })
        .collect();
    format!(
        r#"<!doctype html>
<html lang="{fallback}">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{title}</title>
    <style>
      :root {{ color-scheme: light dark; }}
      body {{ font-family: system-ui, sans-serif; line-height: 1.5; max-width: 32rem; margin: 20vh auto; padding: 0 16px; }}
      ul {{ padding-left: 1.2rem; }}
    </style>
    <script>
      (function () {{
        var books = [{prefixes}];
        var wanted = navigator.languages || [navigator.language || ""];
        for (var i = 0; i < wanted.length; i++) {{
          for (var j = 0; j < books.length; j++) {{
            if (wanted[i].toLowerCase().indexOf(books[j][0]) === 0) {{
              location.replace(books[j][1]);
              return;
            }}
          }}
        }}
        location.replace("{fallback}/index.html");
      }})();
    </script>
  </head>
  <body>
    <h1>Bardo</h1>
    <ul>
{links}    </ul>
  </body>
</html>
"#
    )
}

const STYLE: &str = "\
/* The language switch at the top of every page. */
.bardo-language {
    float: right;
    margin: 0 0 0 1em;
    font-size: 0.9em;
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Destination, GuideGroup, GuideSection};

    fn options() -> SiteOptions {
        SiteOptions {
            base_path: "/bardo".into(),
            repository: Some("https://github.com/owner/bardo".into()),
        }
    }

    fn file<'a>(files: &'a [SiteFile], path: &str) -> &'a str {
        &files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
    }

    fn sample(id: &str, place: Option<GuidePlace>, intro: &str) -> GuidePage {
        GuidePage {
            id: id.to_owned(),
            title: format!("Title of {id}"),
            group: GuideGroup::Reference,
            place,
            tour: None,
            intro: intro.to_owned(),
            sections: vec![GuideSection {
                id: "one".into(),
                title: "One".into(),
                body: "Body.".into(),
            }],
        }
    }

    fn pair(pages: Vec<GuidePage>) -> [Guide; 2] {
        [
            Guide::from_pages(UiLanguage::EnUs, pages.clone()),
            Guide::from_pages(UiLanguage::PtBr, pages),
        ]
    }

    #[test]
    fn the_real_guide_makes_a_site_with_every_page_in_both_languages() {
        let files = guide_site(&options()).unwrap();
        for language in UiLanguage::ALL {
            let guide = Guide::load(language);
            let summary = file(&files, &format!("{language}/src/SUMMARY.md"));
            for page in guide.pages() {
                let markdown = file(&files, &format!("{language}/src/{}.md", page.id));
                assert!(!markdown.contains("bardo:"), "{language}/{}", page.id);
                assert!(
                    summary.contains(&format!("]({}.md)", page.id)),
                    "{language}: {} is not in the sidebar",
                    page.id
                );
            }
        }
        assert!(file(&files, "site/index.html").contains("\"pt-BR/index.html\""));
    }

    #[test]
    fn the_sidebar_follows_the_guide_screen_contents() {
        let guide = Guide::load(UiLanguage::PtBr);
        let summary = summary(&guide);
        let catalog = Catalog::load(UiLanguage::PtBr);
        let mut expected = Vec::new();
        for (group, pages) in guide.contents() {
            expected.push(format!("# {}", catalog.get(Text::GuideGroupName(group))));
            expected.extend(
                pages
                    .iter()
                    .map(|page| format!("- [{}]({}.md)", page.title, page.id)),
            );
        }
        let lines: Vec<&str> = summary
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .collect();
        assert_eq!(lines, expected);
        assert_eq!(lines[0], "# Primeiros passos");
    }

    #[test]
    fn every_page_links_to_itself_in_the_other_language() {
        let files = guide_site(&options()).unwrap();
        let en = file(&files, "en-US/src/costs.md");
        let pt = file(&files, "pt-BR/src/costs.md");
        assert!(en.starts_with(
            "<p class=\"bardo-language\"><a href=\"../pt-BR/costs.html\" hreflang=\"pt-BR\" lang=\"pt-BR\">Em português</a></p>"
        ));
        assert!(pt.contains("<a href=\"../en-US/costs.html\""));
        assert!(pt.contains("In English"));
    }

    #[test]
    fn bardo_links_become_web_links_or_a_note() {
        let pages = vec![
            sample(
                "costs",
                Some(GuidePlace::Screen(Destination::Costs)),
                "[Costs](bardo:go/costs), [Jobs](bardo:go/jobs), [Show me](bardo:tour/costs), \
                 [up](#one), [here](costs.md#one), [other](bardo:guide/jobs#one), \
                 [page](jobs.md), [web](https://example.com/a)",
            ),
            sample("jobs", Some(GuidePlace::Screen(Destination::Jobs)), ""),
        ];
        let [en, pt] = pair(pages);
        let markdown = page_markdown(&en, &en.pages()[0], &[]);
        assert!(markdown.contains(
            "Costs, [Jobs](jobs.md), Show me *(in the app)*, [up](#one), [here](#one), \
             [other](jobs.md#one), [page](jobs.md), [web](https://example.com/a)"
        ));
        let markdown = page_markdown(&pt, &pt.pages()[0], &[]);
        assert!(markdown.contains("Show me *(no app)*"));
    }

    #[test]
    fn a_place_without_a_page_of_its_own_links_to_its_screen_or_stays_in_the_app() {
        let pages = vec![
            sample(
                "start",
                None,
                "[Script](bardo:go/projects/script) [Costs](bardo:go/costs)",
            ),
            sample(
                "projects",
                Some(GuidePlace::Screen(Destination::Projects)),
                "",
            ),
        ];
        let [en, _] = pair(pages);
        let markdown = page_markdown(&en, &en.pages()[0], &[]);
        assert!(markdown.contains("[Script](projects.md) Costs *(in the app)*"));
    }

    #[test]
    fn sections_keep_their_anchors() {
        let [en, _] = pair(vec![sample("a", None, "Intro.")]);
        assert_eq!(
            page_markdown(&en, &en.pages()[0], &[]),
            "# Title of a\n\nIntro.\n\n## One {#one}\n\nBody.\n"
        );
    }

    #[test]
    fn a_broken_link_fails_the_build() {
        let pages = vec![
            sample(
                "a",
                None,
                "[gone](b.md) [no section](a.md#nine) [x](bardo:go/nowhere)",
            ),
            sample("b2", None, ""),
        ];
        let problems = build(&pair(pages), &options()).unwrap_err();
        assert_eq!(
            problems,
            [
                "en-US: broken link in a: b.md",
                "en-US: broken link in a: a.md#nine",
                "en-US: broken link in a: bardo:go/nowhere",
                "pt-BR: broken link in a: b.md",
                "pt-BR: broken link in a: a.md#nine",
                "pt-BR: broken link in a: bardo:go/nowhere",
            ]
        );
    }

    #[test]
    fn a_page_missing_in_one_language_fails_the_build() {
        // In the order the app lists its languages, pt-BR first.
        let guides = UiLanguage::ALL.map(|language| match language {
            UiLanguage::EnUs => Guide::from_pages(language, vec![sample("a", None, "")]),
            UiLanguage::PtBr => {
                Guide::from_pages(language, vec![sample("a", None, ""), sample("b", None, "")])
            }
        });
        let problems = build(&guides, &options()).unwrap_err();
        assert_eq!(problems, ["b: missing in en-US"]);
    }

    #[test]
    fn the_book_is_served_under_the_base_path_and_edited_on_the_repository() {
        let toml: toml::Table = book_toml(UiLanguage::PtBr, &options()).parse().unwrap();
        let html = &toml["output"]["html"];
        assert_eq!(html["site-url"].as_str(), Some("/bardo/pt-BR/"));
        assert_eq!(
            html["edit-url-template"].as_str(),
            Some("https://github.com/owner/bardo/edit/main/docs/guide/pt-BR/{path}")
        );
        assert_eq!(toml["build"]["build-dir"].as_str(), Some("../site/pt-BR"));
        assert_eq!(toml["book"]["language"].as_str(), Some("pt-BR"));

        let root = SiteOptions {
            base_path: String::new(),
            repository: None,
        };
        let toml: toml::Table = book_toml(UiLanguage::EnUs, &root).parse().unwrap();
        assert_eq!(toml["output"]["html"]["site-url"].as_str(), Some("/en-US/"));
        assert!(toml["output"]["html"].get("edit-url-template").is_none());
    }
}

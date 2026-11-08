//! `rt search`: the reviews of the person behind the token, as the instance finds them.

use std::fmt::Write;

use rt_api::{SearchHit, SearchResults};

use crate::connect::connect;
use crate::output;

#[derive(clap::Args)]
pub struct SearchArgs {
    /// What to look for: words, all of which must be there (the last also as the start of
    /// a longer word), and "quoted phrases" (quote them for the shell too: '"cache key"').
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
    /// The most hits to show, up to 50.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
    pub limit: u32,
}

pub fn run(args: SearchArgs, json: bool) -> anyhow::Result<()> {
    let client = connect()?;
    let query = args.query.join(" ");
    let results = client.search(&query, Some(args.limit))?;
    if json {
        return output::json(&results);
    }
    print!("{}", render(&results, &query, client.base()));
    Ok(())
}

/// The hits as text: each with the review it is in, where in it, the words around what
/// matched (**marked**), and its page.
fn render(results: &SearchResults, query: &str, base: &str) -> String {
    let mut out = String::new();
    if results.hits.is_empty() {
        let _ = writeln!(out, "Nothing found for \"{query}\".");
    }
    for (i, hit) in results.hits.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        hit_block(&mut out, hit, base);
    }
    if results.indexing > 0 {
        let _ = writeln!(
            out,
            "\nStill indexing {} of your reviews; search again in a moment for those.",
            results.indexing
        );
    }
    out
}

fn hit_block(out: &mut String, hit: &SearchHit, base: &str) {
    let closed = if hit.closed { ", closed" } else { "" };
    let _ = writeln!(
        out,
        "{}  {} ({}{closed}): {} {}",
        hit.session, hit.title, hit.project, hit.kind, hit.label
    );
    let mut snippet = String::new();
    for part in &hit.snippet {
        if part.mark {
            let _ = write!(snippet, "**{}**", part.text);
        } else {
            snippet.push_str(&part.text);
        }
    }
    let _ = writeln!(out, "  {}", snippet.trim());
    let _ = writeln!(out, "  {base}{}", hit.href);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_api::{HitKind, SnippetPart};

    fn part(text: &str, mark: bool) -> SnippetPart {
        SnippetPart {
            text: text.into(),
            mark,
        }
    }

    #[test]
    fn hits_say_where_they_are_and_what_matched() {
        let results = SearchResults {
            hits: vec![
                SearchHit {
                    session: "k3x9tq2m".into(),
                    title: "Offline sync".into(),
                    project: "alice/app".into(),
                    closed: true,
                    kind: HitKind::Section,
                    label: "Cache keys".into(),
                    href: "/sessions/k3x9tq2m#s-cache-keys".into(),
                    snippet: vec![
                        part("The cache is keyed by the ", false),
                        part("lockfile", true),
                        part(", so a bump invalidates it.", false),
                    ],
                },
                SearchHit {
                    session: "p7m2zz4q".into(),
                    title: "Token rotation".into(),
                    project: "alice/app".into(),
                    closed: false,
                    kind: HitKind::Draft,
                    label: "Rotate monthly?".into(),
                    href: "/sessions/p7m2zz4q#d-1".into(),
                    snippet: vec![part("… the ", false), part("lockfile", true)],
                },
            ],
            indexing: 4,
        };
        let text = render(&results, "lockf", "https://rt.example.com");
        assert_eq!(
            text,
            "k3x9tq2m  Offline sync (alice/app, closed): section Cache keys\n\
             \x20 The cache is keyed by the **lockfile**, so a bump invalidates it.\n\
             \x20 https://rt.example.com/sessions/k3x9tq2m#s-cache-keys\n\
             \n\
             p7m2zz4q  Token rotation (alice/app): draft Rotate monthly?\n\
             \x20 … the **lockfile**\n\
             \x20 https://rt.example.com/sessions/p7m2zz4q#d-1\n\
             \n\
             Still indexing 4 of your reviews; search again in a moment for those.\n"
        );
    }

    #[test]
    fn nothing_found_says_so() {
        let results = SearchResults {
            hits: vec![],
            indexing: 0,
        };
        assert_eq!(
            render(&results, "zanzibar", "https://rt.example.com"),
            "Nothing found for \"zanzibar\".\n"
        );
    }
}

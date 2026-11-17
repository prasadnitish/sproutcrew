//! A Notion page as Markdown text: its title, then its blocks in order, with nested blocks read up to two levels
//! down. Every request is a read through the read-only connector. The page is untrusted material like any web
//! page, and the limits keep a huge page from becoming a huge source.

use serde_json::Value;

/// The most blocks read from one page, and the deepest nesting followed.
pub const MAX_BLOCKS: usize = 1000;
const MAX_DEPTH: usize = 2;

/// The page id in a Notion link or a bare id: the last 32 hex digits, with or without dashes.
pub fn page_id(link: &str) -> Option<String> {
    let hex: String = link
        .split(['?', '#'])
        .next()?
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == '-')
        .filter(|c| *c != '-')
        .collect();
    // Only the trailing 32 hex digits of the address are the id; anything before belongs to the title words.
    let tail: String = link
        .split(['?', '#'])
        .next()?
        .rsplit(['/', '-'])
        .next()?
        .to_string();
    let candidate = if tail.len() == 32 && tail.chars().all(|c| c.is_ascii_hexdigit()) {
        tail
    } else if hex.len() >= 32 {
        hex[hex.len() - 32..].to_string()
    } else {
        return None;
    };
    Some(candidate.to_ascii_lowercase())
}

fn plain(rich: &Value) -> String {
    rich.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["plain_text"].as_str())
        .collect::<String>()
}

/// A page's title from its properties: the one property of type title.
pub fn title(page: &Value) -> String {
    page["properties"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .find(|p| p["type"] == "title")
        .map(|p| plain(&p["title"]))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "Untitled Notion page".into())
}

/// One block as Markdown lines, or `None` for a type that has no text here (images, embeds, databases).
fn block_line(b: &Value, depth: usize) -> Option<String> {
    let kind = b["type"].as_str()?;
    let body = &b[kind];
    let text = plain(&body["rich_text"]);
    let pad = "  ".repeat(depth);
    Some(match kind {
        "paragraph" => format!("{pad}{text}"),
        "heading_1" => format!("## {text}"),
        "heading_2" => format!("### {text}"),
        "heading_3" => format!("#### {text}"),
        "bulleted_list_item" | "toggle" => format!("{pad}- {text}"),
        "numbered_list_item" => format!("{pad}1. {text}"),
        "to_do" => format!(
            "{pad}- [{}] {text}",
            if body["checked"] == true { "x" } else { " " }
        ),
        "quote" | "callout" => format!("{pad}> {text}"),
        "code" => format!(
            "```{}\n{text}\n```",
            body["language"].as_str().unwrap_or("")
        ),
        "divider" => "---".into(),
        "child_page" => format!("{pad}- Page: {}", body["title"].as_str().unwrap_or("")),
        _ => return None,
    })
}

/// What a page read gave: Markdown text and plain notes on what was left out.
pub struct Read {
    pub title: String,
    pub markdown: String,
    pub notes: Vec<String>,
}

/// Counts what a read kept and left out.
#[derive(Default)]
struct Tally {
    count: usize,
    skipped: usize,
    cut: bool,
}

/// The Markdown lines of `parent`'s children, each block followed by its own children, two levels deep at most.
fn children(
    get: &mut dyn FnMut(&str) -> Result<Value, String>,
    parent: &str,
    depth: usize,
    t: &mut Tally,
) -> Result<Vec<String>, String> {
    let mut blocks: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let path = match &cursor {
            Some(c) => format!("/v1/blocks/{parent}/children?page_size=100&start_cursor={c}"),
            None => format!("/v1/blocks/{parent}/children?page_size=100"),
        };
        let got = get(&path)?;
        for b in got["results"].as_array().into_iter().flatten() {
            if t.count >= MAX_BLOCKS {
                t.cut = true;
                break;
            }
            t.count += 1;
            blocks.push(b.clone());
        }
        if t.cut || got["has_more"] != true {
            break;
        }
        // A cursor goes into the next path, so it must be plain.
        cursor = got["next_cursor"]
            .as_str()
            .filter(|c| c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    let mut lines = Vec::new();
    for b in &blocks {
        match block_line(b, depth) {
            Some(l) => lines.push(l),
            None => t.skipped += 1,
        }
        let id = b["id"].as_str().unwrap_or("");
        let plain_id = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if b["has_children"] == true && b["type"] != "child_page" && plain_id {
            if depth < MAX_DEPTH && !t.cut {
                lines.extend(children(get, id, depth + 1, t)?);
            } else {
                t.cut = true;
            }
        }
    }
    Ok(lines)
}

/// Reads a page through `get`, which takes a path such as `/v1/pages/<id>` and returns the JSON answer.
pub fn read(get: &mut dyn FnMut(&str) -> Result<Value, String>, id: &str) -> Result<Read, String> {
    let page = get(&format!("/v1/pages/{id}"))?;
    let title = title(&page);
    let mut t = Tally::default();
    let mut lines: Vec<String> = vec![format!("# {title}"), String::new()];
    lines.extend(children(get, id, 0, &mut t)?);
    let mut notes = Vec::new();
    if t.skipped > 0 {
        notes.push(format!(
            "{} block{} with no text (pictures, embeds or databases) {} left out",
            t.skipped,
            if t.skipped == 1 { "" } else { "s" },
            if t.skipped == 1 { "was" } else { "were" }
        ));
    }
    if t.cut {
        notes.push(format!(
            "The page is long or deeply nested, so only the first {MAX_BLOCKS} blocks and two levels of nesting were read"
        ));
    }
    Ok(Read {
        title,
        markdown: lines.join("\n").trim_end().to_string() + "\n",
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rt(t: &str) -> Value {
        json!([{"plain_text": t}])
    }

    #[test]
    fn a_page_link_or_bare_id_gives_the_page_id() {
        let id = "4158af9d5e7381b393a0fa78b39beb46";
        assert_eq!(
            page_id(&format!("https://www.notion.so/Rate-limits-{id}?pvs=4")).as_deref(),
            Some(id)
        );
        assert_eq!(
            page_id("4158af9d-5e73-81b3-93a0-fa78b39beb46").as_deref(),
            Some(id)
        );
        assert_eq!(page_id("not a page"), None);
    }

    #[test]
    fn a_page_becomes_markdown_with_nested_items_under_their_parent_and_paging_followed() {
        let mut seen = vec![];
        let mut get = |path: &str| -> Result<Value, String> {
            seen.push(path.to_string());
            Ok(match path {
                "/v1/pages/p1" => {
                    json!({"properties": {"Name": {"type": "title", "title": rt("Launch plan")}}})
                }
                "/v1/blocks/p1/children?page_size=100" => json!({"results": [
                    {"id": "b1", "type": "heading_1", "heading_1": {"rich_text": rt("Goals")}},
                    {"id": "b2aa", "type": "bulleted_list_item", "bulleted_list_item": {"rich_text": rt("Ship")}, "has_children": true},
                    {"id": "b3", "type": "image", "image": {}}
                ], "has_more": true, "next_cursor": "c2"}),
                "/v1/blocks/p1/children?page_size=100&start_cursor=c2" => json!({"results": [
                    {"id": "b4", "type": "to_do", "to_do": {"rich_text": rt("Write notes"), "checked": true}}
                ], "has_more": false}),
                "/v1/blocks/b2aa/children?page_size=100" => json!({"results": [
                    {"id": "b5", "type": "paragraph", "paragraph": {"rich_text": rt("by Friday")}}
                ], "has_more": false}),
                other => return Err(format!("unexpected {other}")),
            })
        };
        let r = read(&mut get, "p1").unwrap();
        assert_eq!(r.title, "Launch plan");
        assert_eq!(
            r.markdown,
            "# Launch plan\n\n## Goals\n- Ship\n  by Friday\n- [x] Write notes\n"
        );
        assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
        assert!(r.notes[0].contains("1 block with no text"));
    }
}

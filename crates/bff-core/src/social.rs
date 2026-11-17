//! The approved posts the social-posts workflow ends with, read out of its last step's result so they can become
//! outbox drafts. The step writes a heading for each post (`# Post 1`) and bullets for its text, hashtags, visual
//! direction, and alt text. Only the text and the hashtags are posted; the visual direction and the alt text are for
//! whoever makes the picture. Nothing here sends anything. A draft still needs a person's approval of its exact text
//! and time in the outbox.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPost {
    pub title: String,
    pub text: String,
    pub hashtags: String,
    pub visual: String,
    pub alt: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Text,
    Hashtags,
    Visual,
    Alt,
}

fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&n))
        .then(|| t[n..].strip_prefix(' ').map(str::trim))
        .flatten()
}

fn is_post_heading(h: &str) -> bool {
    let l = h.to_lowercase();
    l == "post"
        || l.strip_prefix("post").is_some_and(|r| {
            r.starts_with([' ', ':', '-']) || r.starts_with(|c: char| c.is_ascii_digit())
        })
}

/// A labelled bullet such as `- **Alt text:** words`, as the field and its first words.
fn label(line: &str) -> Option<(Field, String)> {
    let t = line
        .trim()
        .trim_start_matches(['-', '*', '•'])
        .trim_start()
        .replace("**", "");
    let (name, rest) = t.split_once(':')?;
    let field = match name.trim().to_lowercase().as_str() {
        "text" | "post text" | "post" | "copy" => Field::Text,
        "hashtags" | "hashtag" => Field::Hashtags,
        "visual direction" | "visual" | "image" => Field::Visual,
        "alt text" | "alt" => Field::Alt,
        _ => return None,
    };
    Some((field, rest.trim().to_string()))
}

fn finish(title: &str, f: [Vec<String>; 4], loose: Vec<String>) -> Option<QueuedPost> {
    let join = |v: &[String]| v.join("\n").trim().to_string();
    let mut text = join(&f[0]);
    if text.is_empty() && f.iter().all(|x| x.is_empty()) {
        text = join(&loose);
    }
    (!text.is_empty()).then(|| QueuedPost {
        title: title.to_string(),
        text,
        hashtags: join(&f[1]),
        visual: join(&f[2]),
        alt: join(&f[3]),
    })
}

/// The posts in a result, in order. A heading that is not a post, such as the team's Source warnings, ends the
/// post before it and adds none. A post with no text is left out.
pub fn parse_queue(md: &str) -> Vec<QueuedPost> {
    let (mut out, mut fence) = (Vec::new(), None::<char>);
    let mut current: Option<(String, [Vec<String>; 4], Vec<String>)> = None;
    let mut field: Option<Field> = None;
    let idx = |f: Field| match f {
        Field::Text => 0,
        Field::Hashtags => 1,
        Field::Visual => 2,
        Field::Alt => 3,
    };
    for line in md.lines() {
        // Whatever is inside a fenced block, backticks or tildes, is an example and never a post or a field.
        let t = line.trim_start();
        if let Some(c) = ['`', '~']
            .into_iter()
            .find(|c| t.starts_with(&c.to_string().repeat(3)))
        {
            match fence {
                None => fence = Some(c),
                Some(f) if f == c => fence = None,
                _ => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        {
            if let Some(h) = heading(line) {
                if let Some((title, f, loose)) = current.take() {
                    out.extend(finish(&title, f, loose));
                }
                field = None;
                if is_post_heading(h) {
                    current = Some((h.to_string(), Default::default(), vec![]));
                }
                continue;
            }
        }
        let Some((_, f, loose)) = current.as_mut() else {
            continue;
        };
        if let Some((fd, first)) = label(line) {
            field = Some(fd);
            f[idx(fd)].push(first);
        } else if !line.trim().is_empty() {
            let v = line.trim().replace("**", "");
            match field {
                Some(fd) => f[idx(fd)].push(v),
                None => loose.push(v),
            }
        }
    }
    if let Some((title, f, loose)) = current {
        out.extend(finish(&title, f, loose));
    }
    out
}

/// What is posted: the text, and the hashtags after a blank line.
pub fn draft_text(p: &QueuedPost) -> String {
    if p.hashtags.is_empty() {
        p.text.clone()
    } else {
        format!("{}\n\n{}", p.text, p.hashtags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEUE: &str = "# Posts ready to send\n\n# Post 1\n\n- **Text:** Late deliveries cost us customers. We are fixing the cause.\n- **Hashtags:** #logistics #customers\n- **Visual direction:** A truck at dawn.\n- **Alt text:** A freight truck on a highway at dawn.\n\n## Post 2: the numbers\n\n- Text: Complaints rose 42 percent in two years.\n  We want to know why.\n- Hashtags: #data\n- Visual direction: A simple bar chart.\n- Alt text: Two bars, the second taller.\n\n## Source warnings\n\n- src-4 asked for something.\n";

    #[test]
    fn each_post_heading_gives_a_post_with_its_text_hashtags_visual_and_alt() {
        let p = parse_queue(QUEUE);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].title, "Post 1");
        assert_eq!(
            p[0].text,
            "Late deliveries cost us customers. We are fixing the cause."
        );
        assert_eq!(p[0].hashtags, "#logistics #customers");
        assert_eq!(p[0].visual, "A truck at dawn.");
        assert_eq!(p[0].alt, "A freight truck on a highway at dawn.");
        assert_eq!(p[1].title, "Post 2: the numbers");
        assert_eq!(
            p[1].text,
            "Complaints rose 42 percent in two years.\nWe want to know why."
        );
    }

    #[test]
    fn the_source_warnings_section_and_other_headings_are_not_posts() {
        let p = parse_queue(QUEUE);
        assert!(p.iter().all(|x| !x.text.contains("src-4")));
        assert!(parse_queue("# Notes\n\nNothing here.\n").is_empty());
        assert!(parse_queue("").is_empty());
    }

    #[test]
    fn a_post_with_no_text_is_not_a_post() {
        let p = parse_queue("# Post 1\n\n- Hashtags: #x\n\n# Post 2\n\n- Text: Hello.\n");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text, "Hello.");
    }

    #[test]
    fn a_post_without_labels_takes_its_paragraphs_as_the_text() {
        let p = parse_queue("# Post 1\n\nJust the words of the post.\n");
        assert_eq!(p[0].text, "Just the words of the post.");
    }

    #[test]
    fn the_draft_is_the_text_and_the_hashtags_and_never_the_visual_or_the_alt_text() {
        let p = parse_queue(QUEUE);
        assert_eq!(
            draft_text(&p[0]),
            "Late deliveries cost us customers. We are fixing the cause.\n\n#logistics #customers"
        );
        assert!(!draft_text(&p[0]).contains("truck at dawn"));
        let bare = QueuedPost {
            title: "x".into(),
            text: "Only text".into(),
            hashtags: String::new(),
            visual: String::new(),
            alt: String::new(),
        };
        assert_eq!(draft_text(&bare), "Only text");
    }

    #[test]
    fn a_label_in_the_middle_of_a_sentence_is_not_a_label() {
        let p = parse_queue("# Post 1\n\n- Text: Our hashtags: are many.\n- Hashtags: #a\n");
        assert_eq!(p[0].text, "Our hashtags: are many.");
        assert_eq!(p[0].hashtags, "#a");
    }

    #[test]
    fn fenced_example_fields_cannot_become_post_content() {
        let md = "# Post 1\n- Text: Public copy.\n```\n- Hashtags: #private\n- Text: INTERNAL EXAMPLE\n```\n";
        let p = parse_queue(md);
        assert_eq!(p.len(), 1);
        assert_eq!(draft_text(&p[0]), "Public copy.");
        assert!(parse_queue("~~~\n# Post 1\n- Text: Example only.\n~~~\n").is_empty());
    }
}

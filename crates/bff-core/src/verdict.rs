//! The verdict a second check ends with. The check reads a draft and says whether it is ready. It lists problems and
//! never rewrites the piece, and the step that follows applies the fixes.
//!
//! Only some checks ask for a verdict (see [`asks_for_verdict`]). A project made before this existed keeps the guide
//! it was approved with, so its check is never read for a verdict and never shown as having none.

use serde::Serialize;

/// What a check that asks for a verdict is told to do, added to the end of its guide.
pub const VERDICT_GUIDE: &str = "Check only. Do not rewrite the piece. List each problem as a line starting with \"- \" that quotes the text and says the fix. End with a line exactly of the form `Verdict: Ready`, `Verdict: Fix first` or `Verdict: Do not send`.";

/// Why the finish step does not start after a check said not to send.
pub const DO_NOT_SEND_REASON: &str = "The second check said do not send";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ready,
    FixFirst,
    DoNotSend,
}

/// What a check said: its verdict, if it gave one, and each problem it listed before the verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub verdict: Option<Verdict>,
    pub flagged: Vec<String>,
}

/// Whether a step's guide asks for a verdict.
pub fn asks_for_verdict(guide: &str) -> bool {
    guide.contains(VERDICT_GUIDE)
}

/// The verdict a line states, when it is a verdict line: `Verdict: Ready`, with or without bold, a heading mark,
/// capitals or a closing full stop.
fn on_line(line: &str) -> Option<Verdict> {
    let plain: String = line
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect();
    let plain = plain.trim().trim_start_matches('#').trim().to_lowercase();
    let said = plain.strip_prefix("verdict:")?;
    match said
        .trim()
        .trim_end_matches(['.', '!'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .as_str()
    {
        "ready" => Some(Verdict::Ready),
        "fix first" => Some(Verdict::FixFirst),
        "do not send" => Some(Verdict::DoNotSend),
        _ => None,
    }
}

/// Reads a check's text. The last verdict line wins, so a draft verdict a model corrects at the end does not stand.
/// A check with no verdict line, such as one written to an older guide, has none.
pub fn parse(md: &str) -> Check {
    let lines: Vec<&str> = md.lines().collect();
    let Some((at, verdict)) = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, l)| on_line(l).map(|v| (i, v)))
    else {
        return Check {
            verdict: None,
            flagged: vec![],
        };
    };
    Check {
        verdict: Some(verdict),
        flagged: lines[..at]
            .iter()
            .filter_map(|l| l.trim().strip_prefix("- "))
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_verdicts_are_read_in_any_case_and_with_bold_and_a_full_stop() {
        assert_eq!(
            parse("Looks fine.\nVerdict: Ready").verdict,
            Some(Verdict::Ready)
        );
        assert_eq!(
            parse("- a problem\n**Verdict: fix first.**").verdict,
            Some(Verdict::FixFirst)
        );
        assert_eq!(
            parse("verdict: DO NOT SEND").verdict,
            Some(Verdict::DoNotSend)
        );
        assert_eq!(
            parse("## Verdict:  Fix   first  ").verdict,
            Some(Verdict::FixFirst)
        );
        assert_eq!(parse("**Verdict:** Ready").verdict, Some(Verdict::Ready));
    }

    #[test]
    fn an_old_style_ending_or_no_line_gives_no_verdict() {
        for text in [
            "End: ship",
            "Verdict: ship",
            "Verdict: fix first, then ship",
            "I would say it is ready.",
            "",
            "# Rewritten article\n\nThe bakery opens next month.",
        ] {
            let c = parse(text);
            assert_eq!(c.verdict, None, "{text}");
            assert!(c.flagged.is_empty(), "{text}");
        }
    }

    #[test]
    fn the_flagged_lines_are_the_dash_lines_before_the_verdict() {
        let c = parse(
            "Checked the draft.\n- \"Starting next month\" has no date in the brief. Say \"soon\".\n  - not a top-level line\n- The address is invented. Remove it.\n* a star line is not a flag\nVerdict: Fix first\n- after the verdict",
        );
        assert_eq!(c.verdict, Some(Verdict::FixFirst));
        assert_eq!(
            c.flagged,
            [
                "\"Starting next month\" has no date in the brief. Say \"soon\".",
                "not a top-level line",
                "The address is invented. Remove it.",
            ]
        );
    }

    #[test]
    fn the_last_verdict_line_wins() {
        assert_eq!(
            parse("Verdict: Ready\n- on reflection, a bad number\nVerdict: Do not send").verdict,
            Some(Verdict::DoNotSend)
        );
        assert_eq!(
            parse("Verdict: Fix first\nVerdict: Ready\nVerdict: maybe").verdict,
            Some(Verdict::Ready),
            "an unreadable line after a verdict does not replace it"
        );
    }

    #[test]
    fn a_guide_asks_for_a_verdict_only_when_it_holds_the_verdict_words() {
        assert!(asks_for_verdict(&format!("Check it. {VERDICT_GUIDE}")));
        assert!(!asks_for_verdict(
            "End with one verdict: ship, fix first, or do not ship."
        ));
        assert!(!asks_for_verdict(""));
    }

    #[test]
    fn a_check_is_shown_to_the_screen_with_snake_case_words() {
        let c = parse("- one\nVerdict: Do not send");
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({"verdict": "do_not_send", "flagged": ["one"]})
        );
        assert_eq!(
            serde_json::to_value(parse("x")).unwrap(),
            serde_json::json!({"verdict": null, "flagged": []})
        );
    }
}

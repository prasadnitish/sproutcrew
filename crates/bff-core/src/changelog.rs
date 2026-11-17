//! The decision log: one document that says what each checking step told the writers and what changed in
//! the work afterwards, and what the person decided or changed and what that did to the work. It is built from
//! the project's own records (the journal, every run's trace, and the stored versions of each result) with no
//! model involved, so it can be handed over as evidence of who changed what. Dates are UTC.

use crate::flow::Studio;
use crate::trace::Trace;
use serde_json::Value;
use std::collections::BTreeMap;

/// Most changed lines shown for one change, most characters of a checking step's result shown, and the
/// longest text a diff is worked out for.
const MAX_DIFF_LINES: usize = 80;
const MAX_EXCERPT: usize = 1800;
const MAX_DIFF_INPUT_LINES: usize = 1500;

/// "2026-11-02 07:15" for seconds since 1970.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// One run of a step that produced a result.
struct Run {
    trace: Trace,
    text: String,
    file: String,
}

/// The changed lines between two texts: `-` for a line only in `old`, `+` for one only in `new`. Empty when
/// they are the same. Texts too long for this are compared by line counts only, and the second value says so.
fn diff(old: &str, new: &str) -> (Vec<String>, bool) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    if a.len() > MAX_DIFF_INPUT_LINES || b.len() > MAX_DIFF_INPUT_LINES {
        return (vec![], a != b);
    }
    // Longest common subsequence by dynamic programming over the lines.
    let mut t = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || t[i + 1][j] >= t[i][j + 1]) {
            out.push(format!("- {}", a[i]));
            i += 1;
        } else {
            out.push(format!("+ {}", b[j]));
            j += 1;
        }
    }
    (out, false)
}

/// The change from one version to the next, as text for the log.
fn change_text(old: &str, new: &str) -> String {
    let (lines, too_long) = diff(old, new);
    if too_long {
        return "The text changed. It is too long to compare line by line here.".into();
    }
    if lines.is_empty() {
        return "The text is the same as the version before.".into();
    }
    let added = lines.iter().filter(|l| l.starts_with('+')).count();
    let removed = lines.len() - added;
    let mut shown: Vec<String> = lines.iter().take(MAX_DIFF_LINES).cloned().collect();
    if lines.len() > MAX_DIFF_LINES {
        shown.push(format!(
            "... {} more changed lines",
            lines.len() - MAX_DIFF_LINES
        ));
    }
    let n = |k: usize| {
        if k == 1 {
            "1 line".to_string()
        } else {
            format!("{k} lines")
        }
    };
    format!(
        "{} added, {} removed.\n\n````diff\n{}\n````",
        n(added),
        n(removed),
        shown.join("\n")
    )
}

fn excerpt(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= MAX_EXCERPT {
        return t.to_string();
    }
    let cut: String = t.chars().take(MAX_EXCERPT).collect();
    format!("{cut}\n... (the full text is in the stored result)")
}

fn quote(text: &str) -> String {
    text.trim()
        .lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every run that produced a result, by step, oldest first.
fn runs_by_step(studio: &Studio) -> Result<BTreeMap<String, Vec<Run>>, String> {
    let root = &studio.project.root;
    let artifacts = studio.project.artifacts()?;
    let mut traces: Vec<Trace> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(root.join("trace")) {
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(task) = name.strip_suffix(".json") {
                if let Some(t) = crate::trace::load(root, task)? {
                    traces.push(t);
                }
            }
        }
    }
    let number = |t: &Trace| {
        t.task
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<u64>()
            .unwrap_or(0)
    };
    traces.sort_by_key(|t| (t.at, number(t)));
    let mut out: BTreeMap<String, Vec<Run>> = BTreeMap::new();
    for t in traces {
        if t.result_sha256.is_empty() {
            continue;
        }
        let name = crate::app::artifact_name(&t.stage);
        let Some(file) = artifacts
            .iter()
            .rev()
            .find(|a| {
                a["name"].as_str() == Some(&name)
                    && a["sha256"].as_str() == Some(t.result_sha256.as_str())
            })
            .and_then(|a| a["file"].as_str().map(str::to_string))
        else {
            continue;
        };
        let text = std::fs::read_to_string(root.join("artifacts").join(&file)).unwrap_or_default();
        out.entry(t.stage.clone()).or_default().push(Run {
            trace: t,
            text,
            file,
        });
    }
    Ok(out)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

/// Builds the log for the open project.
pub fn build(studio: &Studio, now: u64) -> Result<String, String> {
    let root = &studio.project.root;
    let events = studio.journal.events()?;
    let runs = runs_by_step(studio)?;
    let plan = studio.plan();
    let title = |id: &str| {
        plan.and_then(|p| p.stages.iter().find(|x| x.id == id))
            .map(|x| format!("{} ({id})", x.title))
            .unwrap_or_else(|| id.to_string())
    };
    let mut o = String::new();
    o.push_str(&format!(
        "# Decision log\n\nBuilt {} UTC from this project's own records: the journal, what each run was given, and every stored version of each result. No model wrote this log.\n\n",
        utc(now)
    ));
    o.push_str(&format!(
        "**Brief.** {}\n\n",
        studio.project.brief()?.trim()
    ));
    if let Some(r) = crate::exercise::load(root)? {
        o.push_str(&format!(
            "**Rules for this exercise** (set {} UTC):\n\n{}\n\n",
            utc(r.at),
            quote(&r.text)
        ));
    }
    if let Some(p) = plan {
        o.push_str("**Team.**\n\n");
        for st in &p.stages {
            let who = runs
                .get(&st.id)
                .and_then(|r| r.last())
                .map(|r| format!("{} ({})", r.trace.model, r.trace.route))
                .unwrap_or_else(|| {
                    "no completed result; this does not rule out a failed attempt".into()
                });
            let side = if crate::workflows::role_info(&st.owner_role)
                .map(|r| r.side == crate::workflows::Side::Checking)
                .unwrap_or(st.owner_role == "reviewer")
            {
                "checks"
            } else {
                "writes"
            };
            o.push_str(&format!(
                "- {}: {} {side}, {who}\n",
                st.title, st.owner_role
            ));
        }
        o.push('\n');
    }
    let sources = crate::sources::SourceStore::open(root)?;
    if !sources.list().is_empty() {
        let roles = crate::roles::Roles::open(root)?;
        o.push_str("**Files given.**\n\n");
        for f in sources.list() {
            o.push_str(&format!("- {} ({})\n", f.name, roles.of(f).word()));
        }
        o.push('\n');
    }

    // ---- Part 1: the verifier
    o.push_str("## Part 1. What the checkers said and what changed after\n\n");
    let checking = |id: &str| {
        plan.and_then(|p| p.stages.iter().find(|x| x.id == id))
            .map(|x| {
                crate::workflows::role_info(&x.owner_role)
                    .map(|r| r.side == crate::workflows::Side::Checking)
                    .unwrap_or(x.owner_role == "reviewer")
            })
            .unwrap_or(false)
    };
    let mut any = false;
    for (id, list) in runs.iter().filter(|(id, _)| checking(id)) {
        for r in list {
            any = true;
            o.push_str(&format!(
                "### {} on {} UTC, {} ({})\n\n{}\n\n",
                title(id),
                utc(r.trace.at),
                r.trace.model,
                short(&r.trace.result_sha256),
                quote(&excerpt(&r.text))
            ));
            // The versions of other steps that were made with this result in front of them.
            let mut followed = false;
            for (wid, wlist) in runs.iter().filter(|(w, _)| *w != id) {
                for (n, w) in wlist.iter().enumerate() {
                    if w.trace
                        .inputs
                        .iter()
                        .any(|i| i.id == *id && i.sha256 == r.trace.result_sha256)
                    {
                        followed = true;
                        o.push_str(&format!(
                            "Then {} was written on {} UTC by {} (version {} of that step). ",
                            title(wid),
                            utc(w.trace.at),
                            w.trace.model,
                            n + 1
                        ));
                        match n.checked_sub(1).map(|p| &wlist[p]) {
                            Some(prev) => o.push_str(&format!("Compared with the version before it: {}\n\n", change_text(&prev.text, &w.text))),
                            None => o.push_str("It was the first version of that step, so there is nothing to compare it with.\n\n"),
                        }
                    }
                }
            }
            if !followed {
                o.push_str("No later step was run with this result in front of it yet.\n\n");
            }
        }
    }
    // Second opinions from a reviewer on another model, and what the person decided about each finding.
    for e in &events {
        match e.kind.as_str() {
            "review.received" => {
                any = true;
                o.push_str(&format!(
                    "### Second opinion on {} on {} UTC, by {}\n\n",
                    s(&e.payload, "name"),
                    utc(e.at),
                    s(&e.payload, "reviewer")
                ));
                match e.payload["report"].as_array() {
                    Some(f) if !f.is_empty() => {
                        for x in f {
                            o.push_str(&format!(
                                "- [{}] {} (evidence: {})\n",
                                s(x, "id"),
                                s(x, "claim"),
                                s(x, "evidence")
                            ));
                        }
                        o.push('\n');
                    }
                    _ => o.push_str("No problems found.\n\n"),
                }
            }
            "review.decided" => {
                let reason = s(&e.payload, "reason");
                o.push_str(&format!(
                    "Decision on finding {} of {} on {} UTC: **{}**{}.\n\n",
                    s(&e.payload, "finding"),
                    s(&e.payload, "name"),
                    utc(e.at),
                    s(&e.payload, "decision"),
                    if reason.is_empty() {
                        String::new()
                    } else {
                        format!(" ({reason})")
                    }
                ));
            }
            _ => {}
        }
    }
    if !any {
        o.push_str("No checking step has run yet, and no second opinion has been asked for.\n\n");
    }

    // ---- Part 2: the person
    o.push_str("## Part 2. What I decided or changed, and what it did to the work\n\n");
    let mut n_inputs = 0;
    for e in &events {
        let line = match e.kind.as_str() {
            "exercise.rules" => {
                let t = s(&e.payload, "text");
                Some(if t.is_empty() {
                    "Cleared the rules for this exercise.".to_string()
                } else {
                    format!("Set the rules for this exercise:\n\n{}", quote(&t))
                })
            }
            "note.added" => {
                let stage = s(&e.payload, "stage");
                let text = s(&e.payload, "text");
                let for_whom = if stage.is_empty() {
                    "every step".to_string()
                } else {
                    title(&stage)
                };
                Some(format!(
                    "Gave an instruction to {for_whom} (note {}):\n\n{}",
                    s(&e.payload, "id"),
                    if text.is_empty() {
                        "> (the text was not recorded; this note was made before notes were journaled)".into()
                    } else {
                        quote(&text)
                    }
                ))
            }
            "note.removed" => Some(format!("Removed note {}.", s(&e.payload, "id"))),
            "coordinator.applied" => Some(format!(
                "Applied a change the coordinator proposed: {}",
                s(&e.payload, "summary")
            )),
            "plan.approved" => Some(format!("Approved the plan (by {}).", s(&e.payload, "by"))),
            _ => None,
        };
        if let Some(l) = line {
            n_inputs += 1;
            o.push_str(&format!(
                "- {} UTC. {}\n",
                utc(e.at),
                l.replace('\n', "\n  ")
            ));
        }
    }
    if n_inputs == 0 {
        o.push_str("Nothing yet.\n");
    }
    o.push_str("\n### What each run was given from me, and what changed\n\n");
    let mut reruns = false;
    for (id, list) in &runs {
        for (n, r) in list.iter().enumerate() {
            let before: Vec<&str> = n
                .checked_sub(1)
                .map(|p| list[p].trace.notes.iter().map(|x| x.id.as_str()).collect())
                .unwrap_or_default();
            let new_notes: Vec<&crate::notes::Note> = r
                .trace
                .notes
                .iter()
                .filter(|x| !before.contains(&x.id.as_str()))
                .collect();
            let rules_changed = n
                .checked_sub(1)
                .map(|p| list[p].trace.rules != r.trace.rules)
                .unwrap_or(!r.trace.rules.is_empty());
            if n == 0 && new_notes.is_empty() && !rules_changed {
                continue;
            }
            reruns = true;
            o.push_str(&format!(
                "**{}, version {}**, {} UTC, {} ({}). ",
                title(id),
                n + 1,
                utc(r.trace.at),
                r.trace.model,
                short(&r.trace.result_sha256)
            ));
            if !new_notes.is_empty() {
                o.push_str("My instructions that were new to this run: ");
                o.push_str(
                    &new_notes
                        .iter()
                        .map(|x| format!("\"{}\"", x.text.replace('\n', " ")))
                        .collect::<Vec<_>>()
                        .join("; "),
                );
                o.push_str(". ");
            }
            if rules_changed {
                o.push_str("The rules for the exercise were different from the run before. ");
            }
            match n.checked_sub(1).map(|p| &list[p]) {
                Some(prev) => o.push_str(&format!(
                    "Change from version {}: {}\n\n",
                    n,
                    change_text(&prev.text, &r.text)
                )),
                None => o.push_str("First version.\n\n"),
            }
        }
    }
    if !reruns {
        o.push_str("No completed result has an instruction trace to compare yet. Failed or interrupted attempts may still have received instructions.\n\n");
    }

    // ---- Part 3: every version
    o.push_str("## Part 3. Every version of every result\n\n");
    if runs.is_empty() {
        o.push_str("No step has produced a result yet.\n");
    }
    for (id, list) in &runs {
        o.push_str(&format!("**{}**\n\n", title(id)));
        for (n, r) in list.iter().enumerate() {
            let same = n > 0 && list[n - 1].trace.result_sha256 == r.trace.result_sha256;
            o.push_str(&format!(
                "- Version {}: {} UTC, {} ({}), file `{}`{}\n",
                n + 1,
                utc(r.trace.at),
                r.trace.model,
                short(&r.trace.result_sha256),
                r.file,
                if same {
                    ", identical to the version before"
                } else {
                    ""
                }
            ));
        }
        o.push('\n');
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(1_793_603_700), "2026-11-02 07:15");
        assert_eq!(utc(951_868_740), "2000-02-29 23:59");
    }

    #[test]
    fn a_diff_shows_only_the_changed_lines_in_order() {
        let (d, long) = diff("a\nb\nc\nd", "a\nB\nc\nd\ne");
        assert!(!long);
        assert_eq!(d, ["- b", "+ B", "+ e"]);
        assert_eq!(diff("same\n", "same\n").0, Vec::<String>::new());
        assert_eq!(
            change_text("x", "x"),
            "The text is the same as the version before."
        );
        let t = change_text("one\ntwo", "one\nthree");
        assert!(
            t.starts_with("1 line added, 1 line removed.")
                && t.contains("- two")
                && t.contains("+ three")
        );
    }

    #[test]
    fn a_long_diff_is_cut_and_a_huge_one_is_not_compared() {
        let old = (0..200)
            .map(|i| format!("old {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = (0..200)
            .map(|i| format!("new {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let t = change_text(&old, &new);
        assert!(t.contains("... 320 more changed lines"), "{t}");
        let big = "x\n".repeat(MAX_DIFF_INPUT_LINES + 1);
        assert!(change_text(&big, "y").contains("too long to compare"));
    }

    fn project_with_two_versions() -> Studio {
        let (mut s, draft) = crate::testkit::ran(
            "changelog",
            "# Market note\n\nClaims cost 4.2 million.\nTwo regions are affected.",
        );
        let mut plan = s.plan().unwrap().clone();
        plan.stages.push(crate::plan::Stage {
            uses: None,
            id: "review".into(),
            title: "Review".into(),
            owner_role: "reviewer".into(),
            guide: String::new(),
            after: vec![],
        });
        let mut checker_seat = plan.team[0].clone();
        checker_seat.role = "reviewer".into();
        plan.team.push(checker_seat);
        s.set_plan(plan, 1_793_603_700 + 500).unwrap();
        s.approve("Nitish", 1_793_603_700 + 501).unwrap();
        let review = "review".to_string();
        let (mut checker, _) = crate::testkit::Script::new(
            "Problem: the 4.2 million figure has no source.\nVerdict: fix first",
            None,
        );
        crate::engine::run_stage(&mut s, &mut checker, &review, 1_793_603_700 + 600).unwrap();
        let note = crate::notes::Notes::open(&s.project.root)
            .unwrap()
            .add(
                &draft,
                "Cite the source for every number.",
                1_793_603_700 + 900,
            )
            .unwrap();
        s.journal
            .append(
                "note.added",
                &serde_json::json!({"id": note.id, "stage": draft, "text": note.text}),
                1_793_603_700 + 900,
            )
            .unwrap();
        crate::exercise::set(
            &s.project.root,
            "AI tools are allowed. Record the screen.",
            1_793_603_700 + 950,
        )
        .unwrap();
        s.journal
            .append(
                "exercise.rules",
                &serde_json::json!({"text": "AI tools are allowed. Record the screen."}),
                1_793_603_700 + 950,
            )
            .unwrap();
        let (mut writer, _) = crate::testkit::Script::new(
            "# Market note\n\nClaims cost 4.2 million (source: claims-report.pdf, page 3).\nTwo regions are affected.",
            None,
        );
        s.approve("Fixture owner", 1_793_603_700 + 1000).unwrap();
        crate::engine::run_stage(&mut s, &mut writer, &draft, 1_793_603_700 + 1200).unwrap();
        s
    }

    #[test]
    fn the_log_shows_what_the_checker_said_what_i_changed_and_what_changed_in_the_work() {
        let s = project_with_two_versions();
        let log = build(&s, 1_793_603_700 + 2000).unwrap();
        let part = |from: &str, to: &str| {
            let a = log
                .find(from)
                .unwrap_or_else(|| panic!("no {from} in {log}"));
            let b = log.find(to).unwrap_or(log.len());
            log[a..b].to_string()
        };
        let one = part("## Part 1.", "## Part 2.");
        let two = part("## Part 2.", "## Part 3.");
        // Part 1: the checker's words, then the writer's next version and its change.
        assert!(
            one.contains("> Problem: the 4.2 million figure has no source."),
            "{one}"
        );
        assert!(
            one.contains("was written on 2026-11-02 07:35 UTC")
                && one.contains("version 2 of that step"),
            "{one}"
        );
        assert!(
            one.contains("- Claims cost 4.2 million.")
                && one.contains("+ Claims cost 4.2 million (source: claims-report.pdf, page 3)."),
            "{one}"
        );
        // Part 2: my instruction, the rules, and the same change as its result.
        assert!(two.contains("> Cite the source for every number."), "{two}");
        assert!(
            two.contains("> AI tools are allowed. Record the screen."),
            "{two}"
        );
        assert!(
            two.contains(
                "My instructions that were new to this run: \"Cite the source for every number.\""
            ),
            "{two}"
        );
        assert!(
            two.contains("The rules for the exercise were different from the run before."),
            "{two}"
        );
        // Part 3: both versions are listed.
        assert!(log.contains("Version 1:") && log.contains("Version 2:"));
        assert!(log.contains("No model wrote this log."));
    }

    #[test]
    fn a_new_project_gives_a_log_that_says_nothing_has_happened_yet() {
        let d = std::env::temp_dir().join(format!("bff-changelog-{}-empty", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Studio::open(crate::workspace::Project::create(&d, "case", "Brief.", 1).unwrap())
            .unwrap();
        let log = build(&s, 100).unwrap();
        assert!(log.contains("No checking step has run yet"));
        assert!(log.contains("Nothing yet."));
        assert!(log.contains("No step has produced a result yet."));
    }
}

//! The four v1 playbook templates. Strategy, metrics, system design, and operating-workflow content are
//! optional sections that can be selected inside any of them, so an exercise that asks for architecture is
//! not left waiting. A template lists its required elements, and a draft is checked against the headings
//! it must have.

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playbook {
    CaseFraming,
    Proposal,
    Prototype,
    Presentation,
    Strategy,
    Metrics,
    SystemDesign,
    OperatingWorkflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Optional {
    pub name: &'static str,
    pub elements: &'static [&'static str],
}

const STRATEGY_ELEMENTS: &[&str] = &[
    "User needs",
    "Opportunities",
    "Alternatives",
    "Differentiation",
    "Prioritization",
];
const METRICS_ELEMENTS: &[&str] = &[
    "Metric tree",
    "Definitions",
    "Guardrails",
    "Experiment and decision design",
];
const SYSTEM_ELEMENTS: &[&str] = &[
    "Boundaries",
    "Components",
    "Data flow",
    "Dependencies",
    "Failure handling",
];
const OPERATING_ELEMENTS: &[&str] = &[
    "Actors",
    "Responsibilities",
    "Handoffs",
    "Exceptions",
    "Service expectations",
];

pub const OPTIONAL: &[Optional] = &[
    Optional {
        name: "Product strategy",
        elements: STRATEGY_ELEMENTS,
    },
    Optional {
        name: "Metrics and experimentation",
        elements: METRICS_ELEMENTS,
    },
    Optional {
        name: "System design",
        elements: SYSTEM_ELEMENTS,
    },
    Optional {
        name: "Operating workflow",
        elements: OPERATING_ELEMENTS,
    },
];

impl Playbook {
    pub const ALL: [Playbook; 4] = [
        Playbook::CaseFraming,
        Playbook::Proposal,
        Playbook::Prototype,
        Playbook::Presentation,
    ];

    /// All eight, with the four that were selectable sections in v1 now standing on their own.
    pub const EVERY: [Playbook; 8] = [
        Playbook::CaseFraming,
        Playbook::Proposal,
        Playbook::Prototype,
        Playbook::Presentation,
        Playbook::Strategy,
        Playbook::Metrics,
        Playbook::SystemDesign,
        Playbook::OperatingWorkflow,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Playbook::CaseFraming => "Case framing",
            Playbook::Proposal => "Proposal or PRD",
            Playbook::Prototype => "Prototype",
            Playbook::Presentation => "Interview presentation",
            Playbook::Strategy => "Product strategy",
            Playbook::Metrics => "Metrics and experimentation",
            Playbook::SystemDesign => "System design",
            Playbook::OperatingWorkflow => "Operating workflow",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Playbook::CaseFraming => "Turn an open exercise into a framed problem with stated assumptions and a rubric to meet.",
            Playbook::Proposal => "Write the recommendation as a proposal or product requirements document.",
            Playbook::Prototype => "Build the one critical journey as a static prototype with sample data.",
            Playbook::Presentation => "Prepare the narrative, options, recommendation, and likely questions for the room.",
            Playbook::Strategy => "Set out the user needs, the opportunities, the alternatives, and what to do first.",
            Playbook::Metrics => "Define the metrics, their guardrails, and how an experiment will decide.",
            Playbook::SystemDesign => "Describe the boundaries, components, data flow, dependencies, and how it fails.",
            Playbook::OperatingWorkflow => "Describe who does what, the handoffs, the exceptions, and the service expected.",
        }
    }

    pub fn required(self) -> &'static [&'static str] {
        match self {
            Playbook::CaseFraming => &[
                "Problem",
                "Audience",
                "Constraints",
                "Rubric",
                "Assumptions",
                "Questions",
                "Success criteria",
            ],
            Playbook::Proposal => &[
                "Value proposition",
                "Economics",
                "Goals",
                "Non-goals",
                "Journeys",
                "Requirements",
                "Acceptance criteria",
                "Metrics",
                "Rollout",
                "Risks",
            ],
            Playbook::Prototype => &[
                "Hypothesis",
                "Critical journey",
                "Sample data",
                "Behavior",
                "Mocked integrations",
                "Run instructions",
            ],
            Playbook::Presentation => &[
                "Executive narrative",
                "Options",
                "Recommendation",
                "Caveats",
                "Anticipated questions",
            ],
            Playbook::Strategy => STRATEGY_ELEMENTS,
            Playbook::Metrics => METRICS_ELEMENTS,
            Playbook::SystemDesign => SYSTEM_ELEMENTS,
            Playbook::OperatingWorkflow => OPERATING_ELEMENTS,
        }
    }

    pub fn from_name(n: &str) -> Option<Playbook> {
        Playbook::EVERY
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(n.trim()))
    }
}

/// The section headings for a playbook plus the optional sections selected, in order. Each optional section
/// brings its own elements as sub-headings. An unknown optional name is an error.
pub fn sections(p: Playbook, selected: &[&str]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = p.required().iter().map(|s| s.to_string()).collect();
    for name in selected {
        let o = OPTIONAL
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| {
                format!(
                    "{name:?} is not an optional section. Choose from: {}",
                    OPTIONAL
                        .iter()
                        .map(|o| o.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        if !out.iter().any(|s| s == o.name) {
            out.push(o.name.to_string());
            out.extend(o.elements.iter().map(|e| format!("{} - {e}", o.name)));
        }
    }
    Ok(out)
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The required headings (and selected optional ones) that a Markdown draft does not have. A heading
/// matches by its words, ignoring case and punctuation. An optional element may appear as its own
/// heading or as `Section - Element`.
pub fn missing_sections(
    p: Playbook,
    selected: &[&str],
    markdown: &str,
) -> Result<Vec<String>, String> {
    let headings: Vec<String> = markdown
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with('#'))
        .map(|l| norm(l.trim_start_matches('#')))
        .collect();
    let have = |want: &str| {
        let w = norm(want);
        let tail = want.rsplit_once(" - ").map(|(_, t)| norm(t));
        headings
            .iter()
            .any(|h| *h == w || tail.as_ref().is_some_and(|t| h == t))
    };
    Ok(sections(p, selected)?
        .into_iter()
        .filter(|s| !have(s))
        .collect())
}

/// Instructions for a worker: the headings to write, in order.
pub fn outline_prompt(p: Playbook, selected: &[&str]) -> Result<String, String> {
    let list = sections(p, selected)?
        .iter()
        .map(|s| format!("## {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Write a {} (playbook version {VERSION}). Purpose: {}\nUse exactly these headings, in this order, and write something under each:\n{list}", p.name(), p.purpose()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_playbooks_with_the_required_elements_from_the_plan() {
        assert_eq!(Playbook::ALL.len(), 4);
        assert_eq!(Playbook::Proposal.required().len(), 10);
        assert!(Playbook::Proposal
            .required()
            .contains(&"Acceptance criteria"));
        assert!(Playbook::CaseFraming.required().contains(&"Rubric"));
        assert!(Playbook::Prototype.required().contains(&"Run instructions"));
        assert!(Playbook::Presentation
            .required()
            .contains(&"Anticipated questions"));
        for p in Playbook::ALL {
            assert!(!p.purpose().is_empty());
            assert_eq!(Playbook::from_name(p.name()), Some(p));
        }
        assert_eq!(
            Playbook::from_name(" proposal or prd "),
            Some(Playbook::Proposal)
        );
        assert_eq!(Playbook::from_name("nope"), None);
    }

    #[test]
    fn an_architecture_exercise_selects_system_design_inside_a_template() {
        let s = sections(Playbook::Proposal, &["system design"]).unwrap();
        assert!(s.contains(&"System design".to_string()));
        assert!(s.contains(&"System design - Failure handling".to_string()));
        assert!(
            s.contains(&"Risks".to_string()),
            "the required headings stay"
        );
        let both = sections(
            Playbook::Proposal,
            &["System design", "Operating workflow", "system design"],
        )
        .unwrap();
        assert_eq!(
            both.iter().filter(|x| *x == "System design").count(),
            1,
            "no duplicates"
        );
        assert!(both.contains(&"Operating workflow - Handoffs".to_string()));
        let err = sections(Playbook::Proposal, &["Astrology"]).unwrap_err();
        assert!(err.contains("Choose from") && err.contains("Metrics and experimentation"));
    }

    #[test]
    fn a_draft_is_checked_against_its_required_headings() {
        let full: String = Playbook::CaseFraming
            .required()
            .iter()
            .map(|s| format!("## {s}\ntext\n"))
            .collect();
        assert!(missing_sections(Playbook::CaseFraming, &[], &full)
            .unwrap()
            .is_empty());
        let partial = "# Case\n## problem\nx\n## Success Criteria!\ny\n";
        let missing = missing_sections(Playbook::CaseFraming, &[], partial).unwrap();
        assert!(
            missing.contains(&"Audience".to_string()) && missing.contains(&"Rubric".to_string())
        );
        assert!(
            !missing.contains(&"Problem".to_string()),
            "case and punctuation do not matter"
        );
        assert!(!missing.contains(&"Success criteria".to_string()));
        // Body text that mentions a heading does not count as having it.
        assert!(
            missing_sections(Playbook::CaseFraming, &[], "The Audience is large.")
                .unwrap()
                .contains(&"Audience".to_string())
        );
    }

    #[test]
    fn selected_optional_sections_are_required_once_selected() {
        let base: String = Playbook::Presentation
            .required()
            .iter()
            .map(|s| format!("## {s}\n.\n"))
            .collect();
        let m = missing_sections(Playbook::Presentation, &["System design"], &base).unwrap();
        assert!(m.contains(&"System design".to_string()));
        let with: String = format!(
            "{base}## System design\n.\n## Boundaries\n.\n## System design - Components\n.\n"
        );
        let m = missing_sections(Playbook::Presentation, &["System design"], &with).unwrap();
        assert!(
            !m.contains(&"Boundaries".to_string()),
            "an element may be its own heading"
        );
        assert!(!m.contains(&"System design - Components".to_string()));
        assert!(m.contains(&"System design - Data flow".to_string()));
    }

    #[test]
    fn the_outline_prompt_lists_the_headings_in_order() {
        let p = outline_prompt(Playbook::Prototype, &[]).unwrap();
        let hyp = p.find("## Hypothesis").unwrap();
        let run = p.find("## Run instructions").unwrap();
        assert!(hyp < run);
        assert!(p.contains("version 1") && p.contains("Prototype"));
        assert!(outline_prompt(Playbook::Prototype, &["x"]).is_err());
    }

    #[test]
    fn the_four_later_playbooks_stand_on_their_own_with_the_same_elements_as_the_selectable_sections(
    ) {
        assert_eq!(Playbook::EVERY.len(), 8);
        for (pb, name) in [
            (Playbook::Strategy, "Product strategy"),
            (Playbook::Metrics, "Metrics and experimentation"),
            (Playbook::SystemDesign, "System design"),
            (Playbook::OperatingWorkflow, "Operating workflow"),
        ] {
            assert_eq!(pb.name(), name);
            assert_eq!(Playbook::from_name(name), Some(pb));
            let opt = OPTIONAL.iter().find(|o| o.name == name).unwrap();
            assert_eq!(pb.required(), opt.elements, "{name}: one list, two uses");
            assert!(!pb.purpose().is_empty());
            let full: String = pb
                .required()
                .iter()
                .map(|s| format!("## {s}\n.\n"))
                .collect();
            assert!(missing_sections(pb, &[], &full).unwrap().is_empty());
        }
        assert_eq!(Playbook::ALL.len(), 4, "v1 still ships four templates");
    }
}

//! Policy defaults (DESIGN_SPEC.md §10 Policy, §14): what every new project starts with, stored once for this Mac,
//! and how a project compares with them. Defaults only ever tighten: applying them never gives a project more than
//! it already had, and a project made before the defaults were saved keeps its own policy.
use crate::plan::Privacy;
use crate::policy::{Guard, Guardrails, Matrix, Tool};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file in the projects folder that holds the defaults.
pub const FILE: &str = "policy-defaults.json";

/// What a guarded action does by default for every writing role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// The person approves each exact action.
    Ask,
    /// The crew can never do it.
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    pub send_outside: Choice,
    pub delete_files: Choice,
    pub install_software: Choice,
    /// What the providers may keep. Compared, not applied: keeping nothing needs every step on OpenRouter, so a plan
    /// is set to it on its own page, where the steps it would refuse are shown.
    pub privacy: Privacy,
}

impl Default for Defaults {
    /// What every project has had until now, so nothing changes until the person saves defaults of their own.
    fn default() -> Self {
        Defaults {
            send_outside: Choice::Ask,
            delete_files: Choice::Ask,
            install_software: Choice::Ask,
            privacy: Privacy::Standard,
        }
    }
}

/// The defaults saved in `base`, or today's when none were saved. A file that cannot be read is treated as none,
/// which is the policy every project had before defaults existed.
pub fn load(base: &Path) -> Defaults {
    std::fs::read_to_string(base.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Saves the defaults in `base`, whole or not at all.
pub fn save(base: &Path, d: &Defaults) -> Result<(), String> {
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let tmp = base.join(format!("{FILE}.tmp"));
    let text = serde_json::to_string_pretty(d).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, base.join(FILE)).map_err(|e| e.to_string())
}

fn pairs(d: &Defaults) -> [(Tool, Choice); 3] {
    [
        (Tool::ExternalSend, d.send_outside),
        (Tool::Delete, d.delete_files),
        (Tool::DependencyInstall, d.install_software),
    ]
}

/// `matrix` with every action the defaults say Never set to Never for the writing roles. An action the defaults
/// allow to be asked keeps what the matrix had, so this never loosens anything.
pub fn apply(matrix: &Matrix, d: &Defaults) -> Result<Matrix, String> {
    let mut next = matrix.clone();
    for (tool, choice) in pairs(d) {
        if choice == Choice::Never {
            next = crate::policy::set_guardrail(&next, tool, false)?;
        }
    }
    Ok(next)
}

/// How a project's policy differs from the defaults, by field name. A field in `looser` lets the crew do more than
/// the default does; one in `stricter` lets it do less. A field in neither is the same.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Fit {
    pub looser: Vec<&'static str>,
    pub stricter: Vec<&'static str>,
}

/// Compares a project's guardrails and privacy with the defaults. Roles that differ from each other count as looser
/// when the default is Never, because at least one role may ask.
pub fn compare(g: &Guardrails, privacy: Privacy, d: &Defaults) -> Fit {
    let mut fit = Fit::default();
    let guards = [
        ("send_outside", g.send_outside, d.send_outside),
        ("delete_files", g.delete_files, d.delete_files),
        ("install_software", g.install_software, d.install_software),
    ];
    for (name, has, want) in guards {
        match (has, want) {
            (Guard::AskEachTime | Guard::VariesByRole, Choice::Never) => fit.looser.push(name),
            (Guard::Never | Guard::VariesByRole, Choice::Ask) => fit.stricter.push(name),
            _ => {}
        }
    }
    match (privacy, d.privacy) {
        (Privacy::Standard, Privacy::NoRetention) => fit.looser.push("privacy"),
        (Privacy::NoRetention, Privacy::Standard) => fit.stricter.push("privacy"),
        _ => {}
    }
    fit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{guardrails, Permission};

    fn matrix() -> Matrix {
        let row = |send, del, inst| -> std::collections::BTreeMap<Tool, Permission> {
            [
                (Tool::Read, Permission::Allow),
                (Tool::Artifact, Permission::Allow),
                (Tool::Write, Permission::Allow),
                (Tool::Shell, Permission::Deny),
                (Tool::ExternalSend, send),
                (Tool::Delete, del),
                (Tool::DependencyInstall, inst),
            ]
            .into_iter()
            .collect()
        };
        let mut m = Matrix::new();
        m.insert(
            "writer".into(),
            row(Permission::Ask, Permission::Ask, Permission::Ask),
        );
        m
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bff-defaults-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn with_nothing_saved_the_defaults_are_what_every_project_already_has() {
        let d = dir("none");
        assert_eq!(load(&d), Defaults::default());
        assert_eq!(Defaults::default().send_outside, Choice::Ask);
        assert_eq!(Defaults::default().privacy, Privacy::Standard);
        // Unreadable counts as none: the old policy, never a looser one.
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(FILE), "{not json").unwrap();
        assert_eq!(load(&d), Defaults::default());
    }

    #[test]
    fn saved_defaults_are_read_back() {
        let d = dir("saved");
        let mine = Defaults {
            install_software: Choice::Never,
            privacy: Privacy::NoRetention,
            ..Defaults::default()
        };
        save(&d, &mine).unwrap();
        assert_eq!(load(&d), mine);
        assert!(!d.join(format!("{FILE}.tmp")).exists());
    }

    #[test]
    fn applying_defaults_only_tightens() {
        let never_install = Defaults {
            install_software: Choice::Never,
            ..Defaults::default()
        };
        let m = apply(&matrix(), &never_install).unwrap();
        let g = guardrails(&m);
        assert_eq!(g.install_software, Guard::Never);
        assert_eq!(g.send_outside, Guard::AskEachTime);
        // A project that already never sends is not loosened by a default that asks.
        let strict = crate::policy::set_guardrail(&matrix(), Tool::ExternalSend, false).unwrap();
        let after = apply(&strict, &Defaults::default()).unwrap();
        assert_eq!(after, strict);
    }

    #[test]
    fn a_project_is_looser_stricter_or_the_same_field_by_field() {
        let base = guardrails(&matrix());
        assert_eq!(
            compare(&base, Privacy::Standard, &Defaults::default()),
            Fit::default()
        );
        let d = Defaults {
            send_outside: Choice::Never,
            privacy: Privacy::NoRetention,
            ..Defaults::default()
        };
        let fit = compare(&base, Privacy::Standard, &d);
        assert_eq!(fit.looser, ["send_outside", "privacy"]);
        assert!(fit.stricter.is_empty());
        let strict =
            guardrails(&crate::policy::set_guardrail(&matrix(), Tool::Delete, false).unwrap());
        let fit = compare(&strict, Privacy::NoRetention, &Defaults::default());
        assert_eq!(fit.stricter, ["delete_files", "privacy"]);
        assert!(fit.looser.is_empty());
    }

    #[test]
    fn roles_that_differ_count_as_looser_than_never() {
        let g = Guardrails {
            send_outside: Guard::VariesByRole,
            delete_files: Guard::AskEachTime,
            install_software: Guard::AskEachTime,
            run_commands: Guard::Never,
            checkers_read_only: true,
        };
        let d = Defaults {
            send_outside: Choice::Never,
            ..Defaults::default()
        };
        assert_eq!(compare(&g, Privacy::Standard, &d).looser, ["send_outside"]);
    }
}

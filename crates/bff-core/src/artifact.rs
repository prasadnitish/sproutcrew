//! Artifact ownership and versions. Only the owner changes an artifact, a change names the version it was
//! made from, and a stale base leads to reconciliation instead of an overwrite. A material change
//! invalidates the reviews of earlier versions, and a cosmetic one does not.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub n: u32,
    pub sha256: String,
    pub author: String,
    pub material: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    UnknownArtifact(String),
    /// Only the owner may change the artifact.
    NotOwner {
        owner: String,
        by: String,
    },
    /// The change was made from an older version. The author must reconcile against `head`.
    StaleBase {
        head: String,
        base: String,
    },
    AlreadyExists(String),
    /// A review named a hash that is not any version of the artifact.
    UnknownVersion(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReviewState {
    NoReview,
    /// The review covers the current content, or only cosmetic changes since.
    Valid,
    /// A material change landed after the reviewed version.
    Invalidated {
        reviewed: u32,
        head: u32,
    },
}

struct Artifact {
    owner: String,
    versions: Vec<Version>,
    /// reviewer role to the version number they reviewed
    reviews: BTreeMap<String, u32>,
}

#[derive(Default)]
pub struct Registry {
    items: BTreeMap<String, Artifact>,
}

impl Registry {
    fn get(&self, name: &str) -> Result<&Artifact, Refusal> {
        self.items
            .get(name)
            .ok_or_else(|| Refusal::UnknownArtifact(name.to_string()))
    }

    pub fn create(&mut self, name: &str, owner: &str, sha256: &str) -> Result<u32, Refusal> {
        if self.items.contains_key(name) {
            return Err(Refusal::AlreadyExists(name.to_string()));
        }
        self.items.insert(
            name.to_string(),
            Artifact {
                owner: owner.to_string(),
                versions: vec![Version {
                    n: 1,
                    sha256: sha256.to_string(),
                    author: owner.to_string(),
                    material: true,
                }],
                reviews: BTreeMap::new(),
            },
        );
        Ok(1)
    }

    pub fn names(&self) -> Vec<String> {
        self.items.keys().cloned().collect()
    }

    /// Every reviewer who has reviewed `name`, with the state of that review now.
    pub fn reviewers(&self, name: &str) -> Result<Vec<(String, ReviewState)>, Refusal> {
        let art = self.get(name)?;
        art.reviews
            .keys()
            .map(|r| Ok((r.clone(), self.review_state(name, r)?)))
            .collect()
    }

    pub fn head(&self, name: &str) -> Result<&Version, Refusal> {
        Ok(self
            .get(name)?
            .versions
            .last()
            .expect("an artifact has a version"))
    }

    pub fn owner(&self, name: &str) -> Result<&str, Refusal> {
        Ok(&self.get(name)?.owner)
    }

    /// Whether `by` could change the artifact from the version with hash `base` right now. Nothing changes.
    pub fn can_change(&self, name: &str, by: &str, base: &str) -> Result<(), Refusal> {
        let art = self.get(name)?;
        if art.owner != by {
            return Err(Refusal::NotOwner {
                owner: art.owner.clone(),
                by: by.to_string(),
            });
        }
        let head = art.versions.last().expect("an artifact has a version");
        if head.sha256 != base {
            return Err(Refusal::StaleBase {
                head: head.sha256.clone(),
                base: base.to_string(),
            });
        }
        Ok(())
    }

    /// A change by `by`, made from the version with hash `base`. Ownership is checked before the base, so a
    /// non-owner learns nothing about the current head.
    pub fn change(
        &mut self,
        name: &str,
        by: &str,
        base: &str,
        new_sha256: &str,
        material: bool,
    ) -> Result<u32, Refusal> {
        self.can_change(name, by, base)?;
        let art = self.items.get_mut(name).expect("checked by can_change");
        let head = art.versions.last().expect("an artifact has a version");
        let n = head.n + 1;
        art.versions.push(Version {
            n,
            sha256: new_sha256.to_string(),
            author: by.to_string(),
            material,
        });
        Ok(n)
    }

    /// Records that `reviewer` reviewed the version with hash `reviewed_sha256`. A review of an older version
    /// is kept against that version and reads as invalidated if material changes followed.
    pub fn record_review(
        &mut self,
        name: &str,
        reviewer: &str,
        reviewed_sha256: &str,
    ) -> Result<(), Refusal> {
        let art = self
            .items
            .get_mut(name)
            .ok_or_else(|| Refusal::UnknownArtifact(name.to_string()))?;
        let n = art
            .versions
            .iter()
            .find(|v| v.sha256 == reviewed_sha256)
            .map(|v| v.n)
            .ok_or_else(|| Refusal::UnknownVersion(reviewed_sha256.to_string()))?;
        art.reviews.insert(reviewer.to_string(), n);
        Ok(())
    }

    pub fn review_state(&self, name: &str, reviewer: &str) -> Result<ReviewState, Refusal> {
        let art = self.get(name)?;
        let Some(&reviewed) = art.reviews.get(reviewer) else {
            return Ok(ReviewState::NoReview);
        };
        let head = art.versions.last().expect("an artifact has a version").n;
        let voided = art.versions.iter().any(|v| v.n > reviewed && v.material);
        Ok(if voided {
            ReviewState::Invalidated { reviewed, head }
        } else {
            ReviewState::Valid
        })
    }
}

/// Whether a change alters the content or only its spacing, case, or punctuation. A cosmetic change does not
/// void a review.
pub fn is_material(old: &str, new: &str) -> bool {
    // Tokens split on whitespace, lowercased, with punctuation trimmed from their ends. Punctuation inside
    // a token stays, so 3.14 and 314 differ.
    fn tokens(t: &str) -> Vec<String> {
        t.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    }
    tokens(old) != tokens(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Registry {
        let mut r = Registry::default();
        r.create("plan", "writer", "h1").unwrap();
        r
    }

    #[test]
    fn creating_an_artifact_gives_version_one_and_a_second_create_is_refused() {
        let mut r = Registry::default();
        assert_eq!(r.create("plan", "writer", "h1").unwrap(), 1);
        assert_eq!(r.head("plan").unwrap().sha256, "h1");
        assert_eq!(r.owner("plan").unwrap(), "writer");
        assert_eq!(
            r.create("plan", "other", "h9"),
            Err(Refusal::AlreadyExists("plan".into()))
        );
        assert!(matches!(r.head("nope"), Err(Refusal::UnknownArtifact(_))));
    }

    #[test]
    fn a_non_owner_cannot_change_the_artifact_whichever_provider_it_runs_on() {
        // Roles stand in for agents on either route. Ownership is by role, not by provider.
        for intruder in ["reviewer-on-claude", "reviewer-on-codex", "writer2"] {
            let mut r = reg();
            let err = r.change("plan", intruder, "h1", "h2", true).unwrap_err();
            assert_eq!(
                err,
                Refusal::NotOwner {
                    owner: "writer".into(),
                    by: intruder.into()
                }
            );
            assert_eq!(
                r.head("plan").unwrap().sha256,
                "h1",
                "the artifact must be untouched"
            );
            assert_eq!(r.head("plan").unwrap().n, 1);
        }
    }

    #[test]
    fn the_owner_changes_it_from_the_current_version() {
        let mut r = reg();
        assert_eq!(r.change("plan", "writer", "h1", "h2", true).unwrap(), 2);
        let h = r.head("plan").unwrap();
        assert_eq!(
            (h.n, h.sha256.as_str(), h.author.as_str()),
            (2, "h2", "writer")
        );
    }

    #[test]
    fn a_stale_base_is_refused_with_the_head_to_reconcile_against_and_nothing_is_overwritten() {
        let mut r = reg();
        r.change("plan", "writer", "h1", "h2", true).unwrap();
        let err = r.change("plan", "writer", "h1", "h3", true).unwrap_err();
        assert_eq!(
            err,
            Refusal::StaleBase {
                head: "h2".into(),
                base: "h1".into()
            }
        );
        assert_eq!(r.head("plan").unwrap().sha256, "h2");
        // After reconciling, the same author succeeds from the new head.
        assert!(r.change("plan", "writer", "h2", "h3", true).is_ok());
    }

    #[test]
    fn a_material_change_invalidates_the_review_and_a_cosmetic_one_does_not() {
        let mut r = reg();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::NoReview
        );
        r.record_review("plan", "critic", "h1").unwrap();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::Valid
        );
        r.change("plan", "writer", "h1", "h2", false).unwrap();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::Valid,
            "typo fixes do not void a review"
        );
        r.change("plan", "writer", "h2", "h3", true).unwrap();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::Invalidated {
                reviewed: 1,
                head: 3
            }
        );
        r.record_review("plan", "critic", "h3").unwrap();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::Valid
        );
    }

    #[test]
    fn a_review_names_the_exact_version_and_an_unknown_hash_is_refused() {
        let mut r = reg();
        r.change("plan", "writer", "h1", "h2", true).unwrap();
        // Reviewing the old version is allowed, and it reads as invalidated at once.
        r.record_review("plan", "critic", "h1").unwrap();
        assert_eq!(
            r.review_state("plan", "critic").unwrap(),
            ReviewState::Invalidated {
                reviewed: 1,
                head: 2
            }
        );
        assert!(r.record_review("plan", "critic", "not-a-version").is_err());
    }

    #[test]
    fn reviews_are_kept_per_reviewer() {
        let mut r = reg();
        r.record_review("plan", "a", "h1").unwrap();
        r.change("plan", "writer", "h1", "h2", true).unwrap();
        r.record_review("plan", "b", "h2").unwrap();
        assert!(matches!(
            r.review_state("plan", "a").unwrap(),
            ReviewState::Invalidated { .. }
        ));
        assert_eq!(r.review_state("plan", "b").unwrap(), ReviewState::Valid);
    }

    #[test]
    fn spacing_case_and_punctuation_are_cosmetic_and_words_numbers_and_lines_are_material() {
        assert!(!is_material("The total is 20.", "the  total is 20"));
        assert!(!is_material("a\nb", "a b"), "line wrapping is cosmetic");
        assert!(!is_material("Hello, world!", "hello world"));
        assert!(
            is_material("The total is 20.", "The total is 25."),
            "a changed number"
        );
        assert!(is_material("one two", "one two three"), "an added word");
        assert!(
            is_material("one two", "two one"),
            "reordering changes the meaning"
        );
        assert!(is_material("", "text"));
        assert!(!is_material("", "  \n"), "empty either way");
    }

    #[test]
    fn can_change_reports_what_change_would_refuse_and_changes_nothing() {
        let mut r = reg();
        assert_eq!(r.can_change("plan", "writer", "h1"), Ok(()));
        assert_eq!(
            r.can_change("plan", "critic", "h1"),
            Err(Refusal::NotOwner {
                owner: "writer".into(),
                by: "critic".into()
            })
        );
        assert_eq!(
            r.can_change("plan", "writer", "old"),
            Err(Refusal::StaleBase {
                head: "h1".into(),
                base: "old".into()
            })
        );
        assert!(matches!(
            r.can_change("none", "writer", "h1"),
            Err(Refusal::UnknownArtifact(_))
        ));
        assert_eq!(r.head("plan").unwrap().n, 1, "nothing changed");
        // It agrees with change() in both directions.
        assert!(r.change("plan", "critic", "h1", "h2", true).is_err());
        assert!(r.change("plan", "writer", "h1", "h2", true).is_ok());
    }

    #[test]
    fn names_and_reviewers_are_listed_in_order() {
        let mut r = reg();
        r.create("alpha", "w", "a1").unwrap();
        assert_eq!(r.names(), vec!["alpha".to_string(), "plan".to_string()]);
        assert!(r.reviewers("plan").unwrap().is_empty());
        r.record_review("plan", "zed", "h1").unwrap();
        r.record_review("plan", "amy", "h1").unwrap();
        r.change("plan", "writer", "h1", "h2", true).unwrap();
        r.record_review("plan", "amy", "h2").unwrap();
        assert_eq!(
            r.reviewers("plan").unwrap(),
            vec![
                ("amy".to_string(), ReviewState::Valid),
                (
                    "zed".to_string(),
                    ReviewState::Invalidated {
                        reviewed: 1,
                        head: 2
                    }
                )
            ]
        );
        assert!(r.reviewers("none").is_err());
    }
}

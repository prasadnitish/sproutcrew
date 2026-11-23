use bff_core::{attention::Tracker, decision_inbox::Item, policy::Tier};
fn item(id: &str, tier: Tier) -> Item {
    Item {
        revision: "scope-a".into(),
        step_title: String::new(),
        project: "Private client".into(),
        stage: "draft".into(),
        id: id.into(),
        tier,
        kind: "action".into(),
        title: "Secret research and destination".into(),
        anchor: "decision".into(),
    }
}
#[test]
fn foreground_is_quiet_background_groups_new_decisions_and_never_repeats() {
    let mut t = Tracker::default();
    let a = item("1", Tier::Reversible);
    assert!(t.observe(std::slice::from_ref(&a), true, 10).is_none());
    assert!(t.observe(std::slice::from_ref(&a), false, 11).is_none());
    let b = item("2", Tier::Destructive);
    let items = [a, b];
    let n = t.observe(&items, false, 12).unwrap();
    assert_eq!(n.count, 1);
    assert_eq!(n.tier, Tier::Destructive);
    assert!(!n.body().contains("Private"));
    assert!(!n.body().contains("Secret"));
    t.attempted(&items, 12, true);
    assert!(t.observe(&items, false, 100).is_none());
    assert!(t.observe(&[], false, 101).is_none());
    assert!(t.observe(&items, false, 102).is_some());
}
#[test]
fn cooldown_retains_unseen_work_and_failures_can_retry_without_a_storm() {
    let mut t = Tracker::default();
    let a = item("a", Tier::Consequential);
    assert!(t.observe(std::slice::from_ref(&a), false, 0).is_some());
    t.attempted(std::slice::from_ref(&a), 0, false);
    assert!(t.observe(std::slice::from_ref(&a), false, 29).is_none());
    assert!(t.observe(std::slice::from_ref(&a), false, 30).is_some());
    t.attempted(std::slice::from_ref(&a), 30, true);
    let mut changed = a;
    changed.revision = "scope-b".into();
    assert!(t.observe(&[changed.clone()], false, 31).is_none());
    assert!(t.observe(&[changed], false, 60).is_some());
}

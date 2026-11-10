use super::*;

#[test]
fn lazy_decay_renormalizes_without_reviving_old_evidence() {
    let mut model = NormalizedCochangeModel::default();
    let t0 = 1_000_000;
    let half_life = EXPERIENCE_DECAY_HALF_LIFE_MS;
    let anchors = BTreeSet::from(["src/a.rs".to_owned()]);

    model.decay_to(t0);
    model.learn(&["src/a.rs".into(), "src/old.rs".into()], "full", 2);
    model.decay_to(t0 + 39 * half_life);
    assert!(model.scale > 1e-12);
    let support_before = model.path_support["src/a.rs"] * model.scale;
    let pair_before = model.cochange["src/a.rs"]["src/old.rs"] * model.scale;

    model.decay_to(t0 + 40 * half_life);
    assert_eq!(model.scale, 1.0);
    let expected = 3.0 * 2.0_f64.powi(-40);
    assert_eq!(model.path_support["src/a.rs"], expected);
    assert_eq!(model.path_support["src/old.rs"], support_before * 0.5);
    assert_eq!(model.cochange["src/a.rs"]["src/old.rs"], pair_before * 0.5);
    assert_eq!(model.pair_score("src/a.rs", "src/old.rs"), 0);
    assert!(model.ranked_scores(&anchors).is_empty());

    model.learn(&["src/a.rs".into(), "src/new.rs".into()], "full", 2);
    assert_eq!(
        model.ranked_scores(&anchors),
        vec![("src/new.rs".to_owned(), 500)]
    );
}

#[test]
fn lazy_decay_underflow_retains_finite_state_and_learns_fresh_evidence() {
    let mut model = NormalizedCochangeModel::default();
    let t0 = 1_000_000;
    let half_life = EXPERIENCE_DECAY_HALF_LIFE_MS;
    let anchors = BTreeSet::from(["src/a.rs".to_owned()]);

    model.decay_to(t0);
    model.learn(&["src/a.rs".into(), "src/old.rs".into()], "full", 2);
    model.decay_to(t0 + 1_075 * half_life);
    assert_eq!(model.scale, 1.0);
    assert!(model
        .path_support
        .values()
        .all(|weight| weight.is_finite() && *weight == 0.0));
    assert!(model
        .cochange
        .values()
        .flat_map(|candidates| candidates.values())
        .all(|weight| weight.is_finite() && *weight == 0.0));
    assert!(model.ranked_scores(&anchors).is_empty());

    model.learn(&["src/a.rs".into(), "src/new.rs".into()], "full", 2);
    assert_eq!(
        model.ranked_scores(&anchors),
        vec![("src/new.rs".to_owned(), 500)]
    );
    assert_eq!(model.pair_score("src/a.rs", "src/old.rs"), 0);
    model.decay_to(t0 + 1_076 * half_life);
    assert_eq!(model.pair_score("src/a.rs", "src/new.rs"), 333);
}

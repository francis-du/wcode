use crate::harness::harness_retrieval::{
    retain_repo_candidates_with_task_evidence, RepoMapCandidate,
};
use std::collections::BTreeSet;

fn candidate(index: usize) -> RepoMapCandidate {
    RepoMapCandidate {
        id: index.to_string(),
        path: format!("src/node_{index}.rs"),
        name: format!("node_{index}"),
        qualified_name: format!("node_{index}"),
        kind: "function".into(),
        relevance: 0.0,
        direct: false,
        exact_direct: false,
        design_path: false,
        query_hits: 0,
        experience_weight: 0,
        degree: 99,
        rank: 0.0,
    }
}

#[test]
fn engineering_fitness_challenge_graph_membership_is_order_independent() {
    let topology = [vec![1], vec![0, 2], vec![1], vec![4], vec![3], vec![]];
    for seed in ["direct", "query", "design", "experience"] {
        for rotation in 0..6 {
            for reverse in [false, true] {
                let mut order: Vec<_> = (0..6).collect();
                order.rotate_left(rotation);
                if reverse {
                    order.reverse();
                }
                let mut indices = [0; 6];
                for (position, &logical) in order.iter().enumerate() {
                    indices[logical] = position;
                }
                let mut candidates: Vec<_> = order.iter().map(|&i| candidate(i)).collect();
                let root = &mut candidates[indices[0]];
                match seed {
                    "direct" => root.direct = true,
                    "query" => root.query_hits = 1,
                    "design" => root.design_path = true,
                    "experience" => root.experience_weight = 1,
                    _ => unreachable!(),
                }
                let neighbors: Vec<Vec<_>> = order
                    .iter()
                    .map(|&i| {
                        topology[i]
                            .iter()
                            .map(|&j| indices[j])
                            .chain([indices[i]])
                            .collect()
                    })
                    .collect();
                retain_repo_candidates_with_task_evidence(&mut candidates, &neighbors);
                let kept: BTreeSet<_> = candidates.iter().map(|c| c.id.as_str()).collect();
                assert_eq!(
                    kept,
                    ["0", "1", "2"].into_iter().collect(),
                    "{seed} {order:?}"
                );
            }
        }
    }
}

#[test]
fn engineering_fitness_challenge_unanchored_exploration_stays_available() {
    let mut candidates: Vec<_> = (0..3).map(candidate).collect();
    retain_repo_candidates_with_task_evidence(&mut candidates, &[vec![1], vec![0], vec![]]);
    assert_eq!(candidates.len(), 3, "no anchor must not mean no repository");
}

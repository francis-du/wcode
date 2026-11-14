use super::*;

#[derive(Default)]
pub(super) struct EvaluationCounters {
    pub(super) records_with_prediction: usize,
    pub(super) prediction_cases: usize,
    pub(super) hit_cases: usize,
    pub(super) predictions: usize,
    pub(super) true_positives: usize,
}

pub(super) struct NormalizedCochangeModel {
    pub(super) last_timestamp_ms: Option<u64>,
    pub(super) path_support: BTreeMap<String, f64>,
    pub(super) cochange: BTreeMap<String, BTreeMap<String, f64>>,
    /// Accumulated decay factor. Stored weights are pre-scaled (divided by this
    /// factor) so advancing time is one multiply instead of a pass over every
    /// entry; ratios cancel it, and `pair_score` restores it where the value is
    /// compared against a fixed prior.
    pub(super) scale: f64,
}

impl Default for NormalizedCochangeModel {
    fn default() -> Self {
        Self {
            last_timestamp_ms: None,
            path_support: BTreeMap::new(),
            cochange: BTreeMap::new(),
            scale: 1.0,
        }
    }
}

impl NormalizedCochangeModel {
    pub(super) fn decay_to(&mut self, timestamp_ms: u64) {
        let Some(previous) = self.last_timestamp_ms else {
            self.last_timestamp_ms = Some(timestamp_ms);
            return;
        };
        if timestamp_ms <= previous {
            return;
        }
        let age = timestamp_ms.saturating_sub(previous) as f64;
        let factor = (-(age / EXPERIENCE_DECAY_HALF_LIFE_MS as f64)).exp2();
        self.scale *= factor;
        self.last_timestamp_ms = Some(timestamp_ms);
        if self.scale < 1e-12 {
            // Fold the accumulated decay into the stored weights so the
            // invariant `true weight = stored * scale` still holds once scale
            // resets to one. Dividing instead would multiply every true weight
            // by 1/scale^2 -- reviving stale evidence by orders of magnitude --
            // and an underflowed scale would turn it into infinities.
            for support in self.path_support.values_mut() {
                *support *= self.scale;
            }
            for candidates in self.cochange.values_mut() {
                for weight in candidates.values_mut() {
                    *weight *= self.scale;
                }
            }
            self.scale = 1.0;
        }
    }

    pub(super) fn learn(&mut self, paths: &[String], level: &str, original_path_count: usize) {
        if paths.is_empty() {
            return;
        }
        let support_weight = level_weight(level) / self.scale;
        for path in paths {
            *self.path_support.entry(path.clone()).or_default() += support_weight;
        }
        if paths.len() < 2 {
            return;
        }
        let breadth = original_path_count.saturating_sub(1).max(1) as f64;
        let pair_weight = support_weight / breadth;
        for anchor in paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in paths {
                if candidate != anchor {
                    *candidates.entry(candidate.clone()).or_default() += pair_weight;
                }
            }
        }
    }

    pub(super) fn learn_trajectory(
        &mut self,
        context_paths: &[String],
        changed_paths: &[String],
        level: &str,
        intent_factor: f64,
    ) {
        if context_paths.is_empty() || changed_paths.is_empty() || intent_factor <= 0.0 {
            return;
        }
        let support_weight =
            level_weight(level) * CONTEXT_TRAJECTORY_WEIGHT * intent_factor / self.scale;
        for path in context_paths {
            *self.path_support.entry(path.clone()).or_default() += support_weight;
        }
        let breadth = changed_paths.len().max(1) as f64;
        let edge_weight = support_weight / breadth;
        for anchor in context_paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in changed_paths {
                if candidate != anchor {
                    *candidates.entry(candidate.clone()).or_default() += edge_weight;
                }
            }
        }
    }

    pub(super) fn ranked_scores(&self, anchors: &BTreeSet<String>) -> Vec<(String, u16)> {
        let mut candidates = BTreeSet::<String>::new();
        for anchor in anchors {
            if let Some(related) = self.cochange.get(anchor) {
                candidates.extend(
                    related
                        .keys()
                        .filter(|candidate| !anchors.contains(*candidate))
                        .cloned(),
                );
            }
        }
        let mut ranked = candidates
            .into_iter()
            .filter_map(|candidate| {
                let combined = anchors
                    .iter()
                    .map(|anchor| self.pair_score(anchor, &candidate) as u32)
                    .sum::<u32>()
                    .min(EXPERIENCE_SCORE_SCALE as u32) as u16;
                (combined > 0).then_some((candidate, combined))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_path, left_score), (right_path, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_path.cmp(right_path))
        });
        ranked
    }

    pub(super) fn pair_score(&self, anchor: &str, candidate: &str) -> u16 {
        let Some(cochange) = self
            .cochange
            .get(anchor)
            .and_then(|candidates| candidates.get(candidate))
            .copied()
        else {
            return 0;
        };
        let Some(anchor_support) = self.path_support.get(anchor).copied() else {
            return 0;
        };
        let Some(candidate_support) = self.path_support.get(candidate).copied() else {
            return 0;
        };
        let denominator = anchor_support + candidate_support;
        if cochange <= 0.0 || denominator <= 0.0 {
            return 0;
        }
        // Weighted Dice penalizes globally popular files, while confidence
        // shrinkage prevents one accidental co-change from looking certain.
        // Pair weight is already discounted by the original change-set breadth.
        let association = (2.0 * cochange / denominator).clamp(0.0, 1.0);
        // The association ratio cancels `scale`, but confidence is compared
        // against a fixed prior, so it needs the true (unscaled) weight.
        let true_cochange = cochange * self.scale;
        let confidence = true_cochange / (true_cochange + EXPERIENCE_CONFIDENCE_PRIOR);
        (association * confidence * EXPERIENCE_SCORE_SCALE)
            .round()
            .clamp(0.0, EXPERIENCE_SCORE_SCALE) as u16
    }
}

#[derive(Default)]
pub(super) struct RawCountModel {
    pub(super) cochange: BTreeMap<String, BTreeMap<String, u32>>,
}

impl RawCountModel {
    pub(super) fn learn(&mut self, paths: &[String], level: &str) {
        if paths.len() < 2 {
            return;
        }
        let weight = if level == "full" { 3 } else { 1 };
        for anchor in paths {
            let candidates = self.cochange.entry(anchor.clone()).or_default();
            for candidate in paths {
                if candidate != anchor {
                    let entry = candidates.entry(candidate.clone()).or_default();
                    *entry = entry.saturating_add(weight);
                }
            }
        }
    }

    pub(super) fn ranked_paths(&self, anchor: &str, limit: usize) -> Vec<String> {
        let Some(candidates) = self.cochange.get(anchor) else {
            return Vec::new();
        };
        let mut ranked = candidates.iter().collect::<Vec<_>>();
        ranked.sort_by(|(left_path, left_weight), (right_path, right_weight)| {
            right_weight
                .cmp(left_weight)
                .then_with(|| left_path.cmp(right_path))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(path, _)| path.clone())
            .collect()
    }
}

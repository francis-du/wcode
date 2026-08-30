use super::*;

impl SoftwareIntelligenceRuntime {
    pub fn evidence_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        subject: Option<&str>,
        limit: usize,
    ) -> Result<EvidenceStatus> {
        let mut records = evidence_store::load(workspace)?
            .into_iter()
            .map(|evidence| (evidence.id.clone(), evidence))
            .collect::<BTreeMap<_, _>>();
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        for stored in state
            .evidence
            .iter()
            .filter(|stored| stored.workspace == workspace_id)
        {
            records.insert(stored.evidence.id.clone(), stored.evidence.clone());
        }
        let mut matching = records
            .into_values()
            .filter(|evidence| subject.is_none_or(|subject| evidence.subject == subject))
            .collect::<Vec<_>>();
        matching.sort_by_key(|evidence| evidence.timestamp_ms);
        let total = matching.len();
        let passed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Pass)
            .count();
        let failed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Fail)
            .count();
        let inconclusive = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Inconclusive)
            .count();
        let disagreed = matching
            .iter()
            .filter(|evidence| evidence.result == EvidenceResult::Disagree)
            .count();
        let deterministic = matching
            .iter()
            .filter(|evidence| evidence.confidence == Confidence::Deterministic)
            .count();
        let limit = limit.clamp(1, 500);
        let evidence = matching.into_iter().rev().take(limit).collect::<Vec<_>>();
        Ok(EvidenceStatus {
            workspace: workspace_id.to_owned(),
            total,
            passed,
            failed,
            inconclusive,
            disagreed,
            deterministic,
            truncated: total > evidence.len(),
            evidence,
        })
    }

    pub fn semantic_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<SemanticStatusView> {
        let mut facts = semantic_store::load(workspace)?;
        facts.sort_by_key(|fact| fact.timestamp_ms);
        let total = facts.len();
        let candidates = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Candidate)
            .count();
        let confirmed = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Confirmed)
            .count();
        let retired = facts
            .iter()
            .filter(|fact| fact.status == SemanticStatus::Retired)
            .count();
        let limit = limit.clamp(1, 500);
        let facts = facts.into_iter().rev().take(limit).collect::<Vec<_>>();
        Ok(SemanticStatusView {
            workspace: workspace_id.to_owned(),
            total,
            candidates,
            confirmed,
            retired,
            truncated: total > facts.len(),
            facts,
        })
    }

    pub fn semantic_query(
        &self,
        workspace: &Workspace,
        query: &str,
        requested_scopes: &[String],
        include_candidates: bool,
        limit: usize,
    ) -> Result<Vec<SemanticMatch>> {
        Ok(semantic::query_scoped(
            semantic_store::load(workspace)?,
            query,
            requested_scopes,
            include_candidates,
            limit,
        ))
    }

    pub fn semantic_record_candidate(
        &self,
        workspace: &Workspace,
        input: SemanticCandidateInput,
    ) -> Result<SemanticFact> {
        if input.origin == crate::semantic::SemanticOrigin::Provider
            && input
                .provider
                .as_ref()
                .is_none_or(|provider| provider.trim().is_empty())
        {
            return Err(anyhow!(
                "provider semantic candidates require provider provenance"
            ));
        }
        let fact = SemanticFact::candidate(self.next_id("SEM"), input)?;
        semantic_store::persist(workspace, &fact)?;
        Ok(fact)
    }

    pub fn semantic_confirm(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        let fact = semantic_store::load_one(workspace, fact_id)?
            .ok_or_else(|| anyhow!("semantic fact does not exist"))?;
        if fact.status == SemanticStatus::Retired {
            return Err(anyhow!(
                "retired semantic facts cannot be confirmed; record a new candidate"
            ));
        }
        if fact.status == SemanticStatus::Confirmed {
            return Ok(fact);
        }
        let confirmed = fact.confirm(attested_by.trim().to_owned())?;
        semantic_store::persist(workspace, &confirmed)?;
        Ok(confirmed)
    }

    pub fn semantic_retire(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        let fact = semantic_store::load_one(workspace, fact_id)?
            .ok_or_else(|| anyhow!("semantic fact does not exist"))?;
        if fact.status == SemanticStatus::Retired {
            return Ok(fact);
        }
        let retired = fact.retire(attested_by.trim().to_owned())?;
        semantic_store::persist(workspace, &retired)?;
        Ok(retired)
    }
}

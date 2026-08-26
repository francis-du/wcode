use super::*;

pub(super) fn design_context_items(
    state: &design::DesignState,
    requirements: &[String],
    components: &[String],
    constraints: &[String],
    acceptance: &[String],
    decisions: &[String],
    limit: usize,
) -> Vec<DesignContextItem> {
    let mut items = Vec::new();
    for id in requirements {
        let Some(requirement) = state.requirements.get(id) else {
            continue;
        };
        items.push(DesignContextItem {
            id: requirement.id.clone(),
            kind: "requirement".into(),
            title: requirement.title.clone(),
            summary: requirement.intent.clone(),
            relations: BTreeMap::from([
                ("implemented_by".into(), requirement.implemented_by.clone()),
                ("acceptance".into(), requirement.acceptance.clone()),
                ("constraints".into(), requirement.constraints.clone()),
            ]),
        });
        if items.len() >= limit {
            return items;
        }
    }
    for id in components {
        let Some(component) = state.components.get(id) else {
            continue;
        };
        let implementation = component
            .implementation
            .iter()
            .map(|reference| match reference {
                CodeRef::File { path } => path.clone(),
                CodeRef::Symbol { path, symbol } => format!("{path}::{symbol}"),
            })
            .collect::<Vec<_>>();
        items.push(DesignContextItem {
            id: component.id.clone(),
            kind: "component".into(),
            title: component.name.clone(),
            summary: component.responsibilities.join(" "),
            relations: BTreeMap::from([
                ("depends_on".into(), component.depends_on.clone()),
                ("constraints".into(), component.constraints.clone()),
                ("implementation".into(), implementation),
            ]),
        });
        if items.len() >= limit {
            return items;
        }
    }
    for id in constraints {
        let Some(constraint) = state.constraints.get(id) else {
            continue;
        };
        items.push(DesignContextItem {
            id: constraint.id.clone(),
            kind: "constraint".into(),
            title: constraint.title.clone(),
            summary: constraint.statement.clone(),
            relations: BTreeMap::from([("applies_to".into(), constraint.applies_to.clone())]),
        });
        if items.len() >= limit {
            return items;
        }
    }
    for id in acceptance {
        let Some(criterion) = state.acceptance.get(id) else {
            continue;
        };
        let verification = criterion
            .verification
            .iter()
            .map(|reference| match reference {
                VerificationRef::Test { path, symbol } => format!("{path}::{symbol}"),
                VerificationRef::Check { id } => format!("check:{id}"),
            })
            .collect::<Vec<_>>();
        items.push(DesignContextItem {
            id: criterion.id.clone(),
            kind: "acceptance_criterion".into(),
            title: criterion.title.clone(),
            summary: criterion.statement.clone(),
            relations: BTreeMap::from([("verification".into(), verification)]),
        });
        if items.len() >= limit {
            return items;
        }
    }
    for id in decisions {
        let Some(decision) = state.decisions.get(id) else {
            continue;
        };
        items.push(DesignContextItem {
            id: decision.id.clone(),
            kind: "decision".into(),
            title: decision.title.clone(),
            summary: if decision.rationale.trim().is_empty() {
                decision.decision.clone()
            } else {
                format!("{} Rationale: {}", decision.decision, decision.rationale)
            },
            relations: BTreeMap::from([("affects".into(), decision.affects.clone())]),
        });
        if items.len() >= limit {
            return items;
        }
    }
    items
}

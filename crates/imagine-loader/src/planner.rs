//! Dependency ordering — the last loader pipeline stage.
//!
//! NeoForge orders mods by their declared dependencies: a mod loads after
//! every mod it depends on (`ordering="AFTER"` on the *dependency* entry
//! means this mod comes after the target), and before mods that declare
//! `BEFORE` against it. Missing required dependencies and dependency cycles
//! are fatal: reported into `plan.errors` so the runtime refuses to enable
//! the offending mods — matching NeoForge's "refuse to boot" behavior.

use crate::mods_toml::{DepOrdering, DepType};
use crate::{DiscoveredMod, LoadPlan};
use std::collections::BTreeMap;

/// Build the load order for `mods` (which must already be de-duplicated).
///
/// Algorithm: DFS topological sort over explicit edges
/// (`mod --AFTER--> dependency`) plus inverted edges for `BEFORE`
/// declarations. Ties break alphabetically by modid, so the resulting order
/// is deterministic run-to-run (CI-comparable).
pub fn order(mods: Vec<DiscoveredMod>, plan: &mut LoadPlan) {
    let known: std::collections::BTreeSet<String> =
        mods.iter().map(|m| m.mod_id.clone()).collect();

    // Fold dependency problems first.
    for m in &mods {
        for d in &m.dependencies {
            if known.contains(&d.mod_id) {
                continue;
            }
            let msg = format!(
                "mod '{}' declares dependency on '{}' which is not installed",
                m.mod_id, d.mod_id
            );
            match d.dep_type {
                DepType::Required => plan.errors.push(msg),
                _ => plan.warnings.push(msg),
            }
        }
    }

    // Edges from AFTER dependencies and BEFORE declarations.
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in &mods {
        edges.entry(m.mod_id.clone()).or_default();
    }
    for m in &mods {
        for d in &m.dependencies {
            if !known.contains(&d.mod_id) {
                continue;
            }
            match d.ordering {
                DepOrdering::After => edges
                    .entry(m.mod_id.clone())
                    .or_default()
                    .push(d.mod_id.clone()),
                DepOrdering::Before => edges
                    .entry(d.mod_id.clone())
                    .or_default()
                    .push(m.mod_id.clone()),
                DepOrdering::None => {}
            }
        }
    }

    // Deterministic visit order.
    let mut ids: Vec<String> = mods.iter().map(|m| m.mod_id.clone()).collect();
    ids.sort();

    let mut state: BTreeMap<String, u8> = BTreeMap::new(); // 0 new, 1 in-stack, 2 done
    let mut ordered: Vec<String> = Vec::with_capacity(ids.len());
    let mut stack: Vec<String> = Vec::new();

    fn visit(
        id: &str,
        edges: &BTreeMap<String, Vec<String>>,
        state: &mut BTreeMap<String, u8>,
        stack: &mut Vec<String>,
        out: &mut Vec<String>,
        plan: &mut LoadPlan,
    ) {
        match state.get(id).copied().unwrap_or(0) {
            2 => return,
            1 => {
                let cycle_start = stack.iter().position(|s| s == id).unwrap_or(0);
                let cycle: Vec<String> = stack[cycle_start..]
                    .iter()
                    .chain(std::iter::once(&id.to_string()))
                    .cloned()
                    .collect();
                plan.errors.push(format!("dependency cycle detected: {}", cycle.join(" -> ")));
                return;
            }
            _ => {}
        }
        state.insert(id.to_string(), 1);
        stack.push(id.to_string());
        if let Some(targets) = edges.get(id) {
            let mut targets: Vec<String> = targets.clone();
            targets.sort();
            for t in targets {
                visit(&t, edges, state, stack, out, plan);
            }
        }
        stack.pop();
        state.insert(id.to_string(), 2);
        out.push(id.to_string());
    }

    for id in &ids {
        visit(id, &edges, &mut state, &mut stack, &mut ordered, plan);
    }

    let by_id: BTreeMap<String, DiscoveredMod> =
        mods.into_iter().map(|m| (m.mod_id.clone(), m)).collect();
    plan.mods = ordered
        .into_iter()
        .filter_map(|id| by_id.get(&id).cloned())
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods_toml::Dependency;
    use crate::VersionRange;
    use crate::ModSource;

    fn mod_with_deps(id: &str, deps: &[(&str, DepType, DepOrdering)]) -> DiscoveredMod {
        DiscoveredMod {
            mod_id: id.to_string(),
            version: "1.0.0".into(),
            license: "MIT".into(),
            source: ModSource::Root(std::path::PathBuf::from("/dev/null")),
            display_name: None,
            dependencies: deps
                .iter()
                .map(|(t, ty, ord)| Dependency {
                    mod_id: t.to_string(),
                    dep_type: *ty,
                    range: VersionRange::any(),
                    ordering: *ord,
                    side: crate::mods_toml::Side::Both,
                })
                .collect(),
        }
    }

    fn just(id: &str) -> DiscoveredMod {
        mod_with_deps(id, &[])
    }

    #[test]
    fn dependency_loads_before_dependent() {
        let mods = vec![
            mod_with_deps("alpha", &[("beta", DepType::Required, DepOrdering::After)]),
            just("beta"),
        ];
        let mut plan = LoadPlan::default();
        order(mods, &mut plan);
        let ids = plan.mod_ids();
        assert_eq!(ids, vec!["beta", "alpha"]);
        assert!(plan.is_ok());
    }

    #[test]
    fn before_reverses_order() {
        // alpha declares BEFORE gamma -> alpha loads before gamma
        let mods = vec![
            just("gamma"),
            mod_with_deps("alpha", &[("gamma", DepType::Required, DepOrdering::Before)]),
        ];
        let mut plan = LoadPlan::default();
        order(mods, &mut plan);
        assert_eq!(plan.mod_ids(), vec!["alpha", "gamma"]);
    }

    #[test]
    fn cycle_is_fatal() {
        let mods = vec![
            mod_with_deps("a", &[("b", DepType::Required, DepOrdering::After)]),
            mod_with_deps("b", &[("a", DepType::Required, DepOrdering::After)]),
        ];
        let mut plan = LoadPlan::default();
        order(mods, &mut plan);
        assert!(!plan.is_ok());
        assert!(plan.errors[0].contains("cycle"));
    }

    #[test]
    fn missing_required_reported() {
        let mods = vec![mod_with_deps("alpha", &[("ghost", DepType::Required, DepOrdering::After)])];
        let mut plan = LoadPlan::default();
        order(mods, &mut plan);
        assert!(!plan.is_ok());
        assert!(plan.errors[0].contains("'ghost' which is not installed"));
    }

    #[test]
    fn deterministic_order_for_independent_mods() {
        let mods = vec![just("zeta"), just("alpha"), just("mid")];
        let mut plan = LoadPlan::default();
        order(mods, &mut plan);
        assert_eq!(plan.mod_ids(), vec!["alpha", "mid", "zeta"]);
    }
}

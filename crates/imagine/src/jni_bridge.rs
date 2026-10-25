//! Java-side enforcement of the load plan.
//!
//! M0 scope: logging + plan summary through the SDK logger. The real
//! enforcement (child-first classloader for `mods/*.jar`, event-bus wiring,
//! registry injection through Paper's internals) lands in M1/M2 — see
//! `ARCHITECTURE.md` for the full boot contract. Kept in its own module so
//! the JNI surface grows in one auditable place.

use imagine_loader::LoadPlan;

pub fn apply_plan(plan: &LoadPlan) {
    for m in &plan.mods {
        cplug_sdk::log::info(&format!(
            "imagine: mod '{}' {} ({}) <- {}",
            m.mod_id,
            m.version,
            m.license,
            m.source
        ));
    }
    for w in &plan.warnings {
        cplug_sdk::log::info(&format!("imagine: warning: {w}"));
    }
    for e in &plan.errors {
        cplug_sdk::log::info(&format!("imagine: error: {e}"));
    }
    let _ = plan.is_ok();
}

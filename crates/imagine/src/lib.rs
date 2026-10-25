//! imagine — the CRUSSTY module that boots a mini NeoForge 1.21.1 runtime
//! *inside* a Paper 26.3 kernel.
//!
//! Contract with the platform (see CRUSSTY `cplug-abi`):
//! the runtime calls `cplugin_init(api, vm, options)` on the JVMTI OnLoad
//! thread, **before** the kernel boots. Everything heavy is deferred:
//! class lookups run on background threads, server work runs on the main
//! thread via the SDK scheduler, and mods are only scanned once the kernel
//! exposes its working directory.
//!
//! Boot sequence (M0):
//! 1. `cplugin_init` — wire SDK, register class-load watchers
//! 2. kernel comes up → `on_kernel_ready`
//! 3. main thread → scan `mods/`, build `LoadPlan`, log the outcome
//! 4. `pipeline ready` analog: `imagine: plan ready (N mods, 0 errors)`
//!
//! Never panic: every mod-driven path is guarded. A broken mod must degrade
//! to a log line, not a dead kernel.

use cplug_abi::{CPluginApi, JavaVmPtr};
use std::ffi::c_char;
use std::sync::atomic::{AtomicUsize, Ordering};

mod jni_bridge;

/// Mods directory, relative to the kernel working directory (Paper root).
pub const MODS_DIR: &str = "mods";

static STATE: AtomicUsize = AtomicUsize::new(0);
const STATE_INIT: usize = 1;
const STATE_KERNEL_READY: usize = 2;
const STATE_PLAN_READY: usize = 3;

/// The single required export (cplug-abi contract).
///
/// # Safety
/// `api` must be a valid CPluginApi owned by the agent, `vm` the live
/// JavaVM pointer, `options` a NUL-terminated string for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cplugin_init(
    api: *const CPluginApi,
    vm: JavaVmPtr,
    _options: *const c_char,
) -> i32 {
    eprintln!("[imagine] cplugin_init: mini-NeoForge runtime for Paper 26.3 (mods target: 1.21.1)");
    cplug_sdk::init(api, vm);
    STATE.store(STATE_INIT, Ordering::Relaxed);

    // Watch the kernel server class to know exactly when Paper is up.
    // (Paper remaps to net.minecraft.server.MinecraftServer in the jar.)
    cplug_sdk::hooks::register("net/minecraft/server/MinecraftServer", |_name| {
        eprintln!("[imagine] kernel class observed: MinecraftServer loading");
    });

    cplug_sdk::on_kernel_ready("org.bukkit.Bukkit", || {
        STATE.store(STATE_KERNEL_READY, Ordering::Relaxed);
        eprintln!("[imagine] kernel ready — scheduling mod scan");
        cplug_sdk::run_on_main_thread(|_env| {
            match build_and_apply_plan() {
                Ok(plan) => {
                    STATE.store(STATE_PLAN_READY, Ordering::Relaxed);
                    eprintln!(
                        "[imagine]: plan ready ({} mod(s), {} error(s)){}",
                        plan.mods.len(),
                        plan.errors.len(),
                        if plan.errors.is_empty() {
                            String::new()
                        } else {
                            format!(" — {:?}", plan.errors)
                        }
                    );
                }
                Err(e) => eprintln!("[imagine] mod scan failed: {e}"),
            }
        });
    });

    0
}

/// Build the load plan for `mods/`. Falls back to the process CWD when the
/// kernel does not expose its directory (e.g. unit-test runs).
fn build_and_apply_plan() -> std::io::Result<imagine_loader::LoadPlan> {
    let mods_dir = std::path::Path::new(MODS_DIR);
    let plan = imagine_loader::build_plan(mods_dir)?;
    jni_bridge::apply_plan(&plan);
    Ok(plan)
}

/// Exposed for integration harnesses: last known lifecycle stage.
pub fn stage() -> &'static str {
    match STATE.load(Ordering::Relaxed) {
        STATE_INIT => "init",
        STATE_KERNEL_READY => "kernel-ready",
        STATE_PLAN_READY => "plan-ready",
        _ => "cold",
    }
}

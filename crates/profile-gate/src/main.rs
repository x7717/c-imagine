//! CI profile gate (ARCHITECTURE §8): builds 50 synthetic mod jars on disk,
//! runs the discovery pipeline, enforces time + RSS budgets.
//!
//! Local use: `cargo run --release -p profile-gate`
//! Exit code 0 = within budget; 1 = budget violation (fails the PR).

use std::io::Write as _;
use std::time::Instant;

const MOD_COUNT: usize = 50;
const PLAN_BUDGET_MS: u128 = 500;
const RSS_BUDGET_KB: u64 = 64 * 1024; // 64 MB

fn main() {
    let dir = std::env::temp_dir().join("imagine-profile-gate");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let t_build = Instant::now();
    for i in 0..MOD_COUNT {
        let toml = format!(
            "modLoader=\"javafml\"\nloaderVersion=\"[4,)\"\nlicense=\"MIT\"\n[[mods]]\nmodId=\"synth{i}\"\nversion=\"1.0.{i}\"\n"
        );
        let path = dir.join(format!("synth{i}.jar"));
        let f = std::fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        w.start_file(
            "META-INF/neoforge.mods.toml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w.write_all(toml.as_bytes()).unwrap();
        w.finish().unwrap();
    }
    let build_ms = t_build.elapsed().as_millis();

    let before = rss_kb();
    let t_plan = Instant::now();
    let plan = imagine_loader::build_plan(&dir).unwrap();
    let plan_ms = t_plan.elapsed().as_millis();
    let after = rss_kb();

    println!("+-----------------------------+-----------+");
    println!("| metric                      | value     |");
    println!("+-----------------------------+-----------+");
    println!("| mods discovered             | {:>9} |", plan.mods.len());
    println!("| jar build ({MOD_COUNT}, cold)        | {build_ms:>7} ms |");
    println!("| plan time                   | {plan_ms:>7} ms |");
    println!("| rss delta                   | {:>6} KB |", after.saturating_sub(before));
    println!("+-----------------------------+-----------+");

    let mut failed = false;
    if plan_ms > PLAN_BUDGET_MS {
        eprintln!("::error::budget: plan {plan_ms} ms > {PLAN_BUDGET_MS} ms");
        failed = true;
    }
    if after.saturating_sub(before) > RSS_BUDGET_KB {
        eprintln!("::error::budget: rss delta {} KB > {RSS_BUDGET_KB} KB", after.saturating_sub(before));
        failed = true;
    }
    if plan.mods.len() != MOD_COUNT {
        eprintln!("::error::expected {MOD_COUNT} mods, got {}", plan.mods.len());
        failed = true;
    }
    if plan.errors.is_empty() {
        println!("plan errors: none");
    } else {
        for e in &plan.errors {
            eprintln!("plan error: {e}");
        }
        failed = true;
    }
    std::process::exit(if failed { 1 } else { 0 });
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0)
}

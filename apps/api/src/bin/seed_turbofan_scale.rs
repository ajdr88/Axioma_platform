//! Generates and seeds the synthetic ~1,000,000-element `Turbofan-Scale` fixture (roadmap:
//! P1.4, T-P1.4-06, NFR-PERF-06) — a standalone binary, not wired into `ensure_seeded`
//! (`apps/api/src/main.rs`), since auto-seeding this on every fresh dev-environment startup
//! would be slow and wasteful; this is explicitly invoked when the fixture is actually needed.
//!
//! Reuses `apps/api/src/store`'s store types directly (self-contained, no dependency on
//! `main.rs`'s other modules) via `#[path]`, the same way `main.rs` uses them.
//!
//! **Fixture shape** (the docs only say "five subsystems recursively elaborated to part level" —
//! no depth/branching/ratio pinned down, a real design gap, not something inferred beyond what's
//! written): each of the 5 `Turbofan-Ref` subsystems gets its own breadth-first-expanded subtree
//! (branching factor 10, `Structure` kind throughout) until it reaches 200,000 descendant
//! elements (5 x 200,000 = 1,000,000, plus the `Engine` root + 5 subsystems + one
//! `REQ-THRUST-SCALE` requirement). ~1,200 evenly-sampled part-level elements (240 per subsystem)
//! get a `Satisfy` edge to `REQ-THRUST-SCALE`, mirroring T-P1.3-01's own literal setup ("~1,200
//! downstream elements"). No Postgres bodies/positions — nothing this fixture is built to measure
//! needs them, and generating 1M bodies would be pure waste.
//!
//! Writes go through `Neo4jStore::bulk_upsert_elements`/`bulk_create_edges` — the ordinary
//! one-`MERGE`-per-element/edge path (`upsert_element`/`create_edge`) is architecturally the
//! wrong shape at this scale, and (confirmed directly) its label-less `MATCH` can't use the new
//! per-label index either, making it doubly wrong at real scale — see `bulk_create_edges`'s own
//! doc comment. Deliberately bypasses `record_commit`/`build_snapshot` entirely (see
//! `apps/api/src/traceability.rs`'s doc comment) — routing a million-element seed through the
//! snapshot-per-commit versioning path would itself trigger the exact bottleneck this fixture
//! exists to measure.

// This binary only exercises a subset of `store`'s full API (`main.rs`'s HTTP handlers use the
// rest — `PostgresStore`, `ObjectStore`, most of `VersioningStore`) — allowed here rather than
// pared down, since the whole point is reusing the exact same store module `main.rs` does.
#[allow(dead_code, unused_imports)]
#[path = "../store/mod.rs"]
mod store;

use std::time::Instant;

use store::versioning::DEFAULT_REGION;
use store::{Neo4jStore, VersioningStore};
use sysml_core::{EdgeKind, Element, ElementId, NodeKind, Origin};

const PROJECT_NAME: &str = "Turbofan Scale";
const SUBSYSTEMS: [(&str, &str); 5] = [
    ("FanLpCompression", "Fan & LP Compression"),
    ("CoreHpCompressor", "Core (HP) Compressor"),
    ("Combustor", "Combustor"),
    ("TurbineHpLp", "Turbine (HP & LP)"),
    ("ControlFadecEec", "Control (FADEC/EEC)"),
];
const TARGET_PER_SUBSYSTEM: usize = 200_000;
const BRANCH_FACTOR: usize = 10;
const SATISFY_SAMPLES_PER_SUBSYSTEM: usize = 240;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let neo4j = Neo4jStore::connect(
        &env_or("NEO4J_URI", "bolt://localhost:7687"),
        &env_or("NEO4J_USER", "neo4j"),
        &env_or("NEO4J_PASSWORD", "axioma-dev"),
    )
    .await?;
    let versioning = VersioningStore::connect(&env_or(
        "DATABASE_URL",
        "postgres://axioma:axioma-dev@localhost:5433/axioma",
    ))
    .await?;

    let existing = versioning
        .list_projects()
        .await?
        .into_iter()
        .find(|p| p.name == PROJECT_NAME);
    let project = match existing {
        Some(project) => {
            println!(
                "project {PROJECT_NAME:?} already exists (id={}) — skipping generation. \
                 Delete it first if you want a fresh seed.",
                project.id
            );
            return Ok(());
        }
        None => {
            versioning
                .create_project(PROJECT_NAME, DEFAULT_REGION)
                .await?
        }
    };
    println!("created project {PROJECT_NAME:?} (id={})", project.id);

    let overall_start = Instant::now();

    let engine = Element {
        id: "Engine".to_string(),
        kind: NodeKind::Structure,
        name: "Engine".to_string(),
        active: true,
        origin: Origin::Human,
    };
    neo4j.upsert_element(&project.id, &engine).await?;

    let req_thrust = Element {
        id: "REQ-THRUST-SCALE".to_string(),
        kind: NodeKind::Requirement,
        name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
        active: true,
        origin: Origin::Human,
    };
    neo4j.upsert_element(&project.id, &req_thrust).await?;

    let mut total_elements = 2; // Engine + REQ-THRUST-SCALE
    let mut total_edges = 0;
    let mut total_satisfy = 0;

    for (subsystem_id, subsystem_name) in SUBSYSTEMS {
        let subsystem_start = Instant::now();

        let subsystem_element = Element {
            id: subsystem_id.to_string(),
            kind: NodeKind::Structure,
            name: subsystem_name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j
            .upsert_element(&project.id, &subsystem_element)
            .await?;
        // Not `create_edge` — its cycle check re-fetches *every* existing Contains edge in the
        // project (already ~800k by the last subsystem), and its label-less MATCH hits the same
        // unindexed-scan cost `bulk_create_edges`'s own doc comment describes. One-pair "bulk"
        // call sidesteps both.
        neo4j
            .bulk_create_edges(
                &project.id,
                EdgeKind::Contains,
                "Structure",
                "Structure",
                &[("Engine".to_string(), subsystem_id.to_string())],
            )
            .await?;

        let (elements, edges, satisfy_sample_ids) =
            generate_subtree(subsystem_id, TARGET_PER_SUBSYSTEM, BRANCH_FACTOR);

        neo4j.bulk_upsert_elements(&project.id, &elements).await?;
        neo4j
            .bulk_create_edges(
                &project.id,
                EdgeKind::Contains,
                "Structure",
                "Structure",
                &edges,
            )
            .await?;

        // Bulk here too, not a `create_edge` loop — the exact same unindexed-scan cost
        // `bulk_create_edges`'s own doc comment describes applies per call regardless of how few
        // calls there are, once the graph is already at real scale (confirmed directly: this
        // phase was unusably slow as a `create_edge` loop once >100k elements already existed).
        let satisfy_pairs: Vec<_> = satisfy_sample_ids
            .iter()
            .map(|part_id| (part_id.clone(), "REQ-THRUST-SCALE".to_string()))
            .collect();
        neo4j
            .bulk_create_edges(
                &project.id,
                EdgeKind::Satisfy,
                "Structure",
                "Requirement",
                &satisfy_pairs,
            )
            .await?;

        total_elements += 1 + elements.len();
        total_edges += 1 + edges.len();
        total_satisfy += satisfy_sample_ids.len();

        println!(
            "{subsystem_id}: {} elements, {} Contains edges, {} Satisfy edges in {:.1}s",
            elements.len(),
            edges.len(),
            satisfy_sample_ids.len(),
            subsystem_start.elapsed().as_secs_f64()
        );
    }

    println!(
        "done: {total_elements} elements, {total_edges} Contains edges, {total_satisfy} Satisfy \
         edges in {:.1}s total",
        overall_start.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Breadth-first-expands `subsystem_id`'s subtree until it reaches exactly `target_count`
/// descendant elements, each subsequent element's id/name prefixed by `subsystem_id` (so a
/// per-subsystem-local counter starting at 0 can never collide with another subsystem's ids —
/// no global counter needed). Returns the generated elements, their `(parent, child)` Contains
/// pairs, and an evenly-sampled subset of ids (`SATISFY_SAMPLES_PER_SUBSYSTEM` of them) for the
/// caller to link to `REQ-THRUST-SCALE`.
fn generate_subtree(
    subsystem_id: &str,
    target_count: usize,
    branch: usize,
) -> (Vec<Element>, Vec<(ElementId, ElementId)>, Vec<ElementId>) {
    let mut elements = Vec::with_capacity(target_count);
    let mut edges = Vec::with_capacity(target_count);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(subsystem_id.to_string());
    let mut counter: u64 = 0;

    while elements.len() < target_count {
        let Some(parent) = queue.pop_front() else {
            break; // unreachable given branch > 1, but no infinite loop if it ever were 0
        };
        for _ in 0..branch {
            if elements.len() >= target_count {
                break;
            }
            counter += 1;
            let child_id = format!("{subsystem_id}-part-{counter}");
            elements.push(Element {
                id: child_id.clone(),
                kind: NodeKind::Structure,
                name: format!("{subsystem_id} Part {counter}"),
                active: true,
                origin: Origin::Human,
            });
            edges.push((parent.clone(), child_id.clone()));
            queue.push_back(child_id);
        }
    }

    let sample_stride = (elements.len() / SATISFY_SAMPLES_PER_SUBSYSTEM).max(1);
    let satisfy_sample_ids = elements
        .iter()
        .step_by(sample_stride)
        .take(SATISFY_SAMPLES_PER_SUBSYSTEM)
        .map(|e| e.id.clone())
        .collect();

    (elements, edges, satisfy_sample_ids)
}

//! Phase 0 benchmark: 15 towers x 80 apartments = 1200 linked apartment models.
//! Geometry is synthetic triangle soup, so this measures data-structure cost
//! (memory, index build, culling, LOD batching), not real model quality.
//!
//! cargo run --release -p openarch-links --example masterplan

use openarch_links::*;
use std::time::Instant;

fn mib(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn main() {
    let full_tris: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60_000);
    let mut w = World::default();
    let t = Instant::now();
    let defs: Vec<u32> = [("1 bed", 55.0, 4, 3), ("2 bed", 80.0, 6, 5), ("2 bed + study", 92.0, 7, 6), ("3 bed", 110.0, 8, 7)]
        .iter()
        .enumerate()
        .map(|(i, (n, area, doors, windows))| {
            w.add_def(synthetic_definition(
                n,
                [10.0, 8.0, 3.0],
                full_tris,
                Summary { gross_area_m2: *area, doors: *doors, windows: *windows },
                100 + i as u64,
            ))
        })
        .collect();
    let gen = t.elapsed();

    for tower in 0..15usize {
        let (tx, ty) = ((tower % 5) as f32 * 90.0, (tower / 5) as f32 * 90.0);
        for level in 0..10usize {
            for slot in 0..8usize {
                w.place(Placement {
                    def: defs[(tower + level + slot) % 4],
                    pos: [tx + (slot % 4) as f32 * 11.0, ty + (slot / 4) as f32 * 9.0, level as f32 * 3.2],
                    yaw: if slot / 4 == 1 { std::f32::consts::PI } else { 0.0 },
                    mirrored: slot % 2 == 1,
                });
            }
        }
    }
    println!("placements: {}   definitions: {}   full-detail triangles per apartment: {}", w.placements.len(), w.defs.len(), full_tris);
    println!("generate 4 definitions: {:.0} ms", gen.as_secs_f64() * 1000.0);

    let t = Instant::now();
    w.build_index();
    println!("build spatial index (1200 placements): {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);

    let m = w.memory_report();
    println!("\nmemory (geometry + placements + index)");
    println!("  shared definitions : {:8.1} MiB", mib(m.definitions_bytes));
    println!("  1200 placements    : {:8.3} MiB", mib(m.placements_bytes));
    println!("  spatial index      : {:8.3} MiB", mib(m.index_bytes));
    println!("  if every copy were loaded separately: {:.0} MiB  ({:.0}x more)", mib(m.unshared_geometry_bytes),
        m.unshared_geometry_bytes as f64 / (m.definitions_bytes + m.placements_bytes + m.index_bytes) as f64);

    let t = Instant::now();
    let tot = w.total_summary();
    println!("\nmasterplan schedule totals from cached summaries ({:.3} ms): {:.0} m2, {} doors, {} windows",
        t.elapsed().as_secs_f64() * 1000.0, tot.gross_area_m2, tot.doors, tot.windows);

    let everything_full: usize = w.placements.iter().map(|p| w.defs[p.def as usize].lods[2].tri_count()).sum();
    println!("\nviews (triangles if everything drew at full detail: {:.1} M)", everything_full as f64 / 1e6);
    let views: [(&str, Aabb, [f32; 3]); 4] = [
        ("whole site from above", Aabb { min: [-50.0, -50.0, -5.0], max: [450.0, 250.0, 40.0] }, [200.0, 90.0, 600.0]),
        ("one tower, 80 m away", Aabb { min: [-10.0, -10.0, -5.0], max: [60.0, 40.0, 40.0] }, [25.0, -80.0, 20.0]),
        ("walking through one level", Aabb { min: [-5.0, -5.0, 6.0], max: [50.0, 20.0, 10.0] }, [20.0, 5.0, 8.0]),
        ("inside one apartment", Aabb { min: [-1.0, -1.0, 0.0], max: [12.0, 10.0, 4.0] }, [4.0, 4.0, 1.6]),
    ];
    for (name, region, eye) in views {
        let t = Instant::now();
        let b = w.visible_batches(&region, eye, LodRule::default());
        let dt = t.elapsed();
        let tris = w.triangles_in(&b);
        println!("  {:<28} {:4} instances, {:2} draw calls, {:7.2} M tris, cull+LOD {:.3} ms",
            name, b.instance_count(), b.draw_calls(), tris as f64 / 1e6, dt.as_secs_f64() * 1000.0);
    }
}

//! Instanced linked models (Phase 0 spike).
//!
//! A linked model is a shared *definition* (geometry at several levels of
//! detail plus cached schedule summaries) and many cheap *placements*. Ten
//! thousand identical apartments cost one mesh set plus ten thousand small
//! structs, and a schedule total over all of them is `sum(summary * count)`
//! rather than a walk through every element.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    pub fn empty() -> Self {
        Aabb { min: [f32::MAX; 3], max: [f32::MIN; 3] }
    }

    pub fn grow(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        let mut r = *self;
        r.grow(o.min);
        r.grow(o.max);
        r
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && self.max[i] >= o.min[i])
    }

    pub fn centre(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    pub fn diagonal(&self) -> f32 {
        let d = [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn bytes(&self) -> usize {
        self.positions.len() * 12 + self.indices.len() * 4
    }

    /// 12-triangle box.
    pub fn box_mesh(b: &Aabb) -> Mesh {
        let (a, c) = (b.min, b.max);
        let positions = vec![
            [a[0], a[1], a[2]],
            [c[0], a[1], a[2]],
            [c[0], c[1], a[2]],
            [a[0], c[1], a[2]],
            [a[0], a[1], c[2]],
            [c[0], a[1], c[2]],
            [c[0], c[1], c[2]],
            [a[0], c[1], c[2]],
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3,
            4, 7,
        ];
        Mesh { positions, indices }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lod {
    Envelope = 0,
    Simplified = 1,
    Full = 2,
}

/// Per-definition numbers that schedules need, computed once.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub gross_area_m2: f64,
    pub doors: u32,
    pub windows: u32,
}

impl Summary {
    pub fn times(&self, n: u32) -> Summary {
        Summary {
            gross_area_m2: self.gross_area_m2 * n as f64,
            doors: self.doors * n,
            windows: self.windows * n,
        }
    }

    pub fn add(&self, o: &Summary) -> Summary {
        Summary {
            gross_area_m2: self.gross_area_m2 + o.gross_area_m2,
            doors: self.doors + o.doors,
            windows: self.windows + o.windows,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    pub aabb: Aabb,
    /// Indexed by `Lod as usize`.
    pub lods: [Mesh; 3],
    pub summary: Summary,
}

/// Where one copy of a definition sits. 24 bytes.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub def: u32,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub mirrored: bool,
}

impl Placement {
    /// World-space bounds: mirror in X, rotate about Z, then translate.
    pub fn world_aabb(&self, local: &Aabb) -> Aabb {
        let (s, c) = self.yaw.sin_cos();
        let mut out = Aabb::empty();
        for &x in &[local.min[0], local.max[0]] {
            for &y in &[local.min[1], local.max[1]] {
                let x = if self.mirrored { -x } else { x };
                let wx = x * c - y * s + self.pos[0];
                let wy = x * s + y * c + self.pos[1];
                out.grow([wx, wy, local.min[2] + self.pos[2]]);
                out.grow([wx, wy, local.max[2] + self.pos[2]]);
            }
        }
        out
    }
}

struct BvhNode {
    bounds: Aabb,
    /// Leaf: range into `order`. Inner: left = idx+1, right = `right`.
    start: u32,
    count: u32,
    right: u32,
}

pub struct Bvh {
    nodes: Vec<BvhNode>,
    order: Vec<u32>,
}

const LEAF_SIZE: usize = 8;

impl Bvh {
    pub fn build(bounds: &[Aabb]) -> Bvh {
        let mut order: Vec<u32> = (0..bounds.len() as u32).collect();
        let mut nodes = Vec::with_capacity(bounds.len() / 2 + 1);
        if !bounds.is_empty() {
            Self::build_rec(bounds, &mut order, 0, bounds.len(), &mut nodes);
        }
        Bvh { nodes, order }
    }

    fn build_rec(bounds: &[Aabb], order: &mut [u32], lo: usize, hi: usize, nodes: &mut Vec<BvhNode>) -> usize {
        let mut b = Aabb::empty();
        for &i in &order[lo..hi] {
            b = b.union(&bounds[i as usize]);
        }
        let idx = nodes.len();
        nodes.push(BvhNode { bounds: b, start: lo as u32, count: (hi - lo) as u32, right: 0 });
        if hi - lo <= LEAF_SIZE {
            return idx;
        }
        let ext = [b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]];
        let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
            0
        } else if ext[1] >= ext[2] {
            1
        } else {
            2
        };
        let mid = (lo + hi) / 2;
        order[lo..hi].select_nth_unstable_by(mid - lo, |&a, &c| {
            let ca = bounds[a as usize].centre()[axis];
            let cc = bounds[c as usize].centre()[axis];
            ca.partial_cmp(&cc).unwrap()
        });
        Self::build_rec(bounds, order, lo, mid, nodes);
        let right = Self::build_rec(bounds, order, mid, hi, nodes);
        nodes[idx].count = 0;
        nodes[idx].right = right as u32;
        idx
    }

    pub fn query(&self, bounds: &[Aabb], region: &Aabb, out: &mut Vec<u32>) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if !n.bounds.intersects(region) {
                continue;
            }
            if n.count > 0 {
                for &p in &self.order[n.start as usize..(n.start + n.count) as usize] {
                    if bounds[p as usize].intersects(region) {
                        out.push(p);
                    }
                }
            } else {
                stack.push(i + 1);
                stack.push(n.right as usize);
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.nodes.len() * std::mem::size_of::<BvhNode>() + self.order.len() * 4
    }
}

/// What one frame would draw: instances grouped by (definition, LOD).
#[derive(Debug, Default)]
pub struct Batches {
    pub groups: HashMap<(u32, Lod), Vec<u32>>,
}

impl Batches {
    pub fn instance_count(&self) -> usize {
        self.groups.values().map(|v| v.len()).sum()
    }

    pub fn draw_calls(&self) -> usize {
        self.groups.len()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LodRule {
    /// Switch to Simplified when (diagonal / distance) rises above this.
    pub simplified_above: f32,
    /// Switch to Full when (diagonal / distance) rises above this.
    pub full_above: f32,
}

impl Default for LodRule {
    fn default() -> Self {
        LodRule { simplified_above: 0.05, full_above: 0.5 }
    }
}

#[derive(Default)]
pub struct World {
    pub defs: Vec<Definition>,
    pub placements: Vec<Placement>,
    bounds: Vec<Aabb>,
    bvh: Option<Bvh>,
}

#[derive(Debug)]
pub struct MemoryReport {
    pub definitions_bytes: usize,
    pub placements_bytes: usize,
    pub index_bytes: usize,
    /// What a model that loads every copy in full (no sharing) would hold for geometry.
    pub unshared_geometry_bytes: usize,
}

impl World {
    pub fn add_def(&mut self, d: Definition) -> u32 {
        self.defs.push(d);
        (self.defs.len() - 1) as u32
    }

    pub fn place(&mut self, p: Placement) {
        self.placements.push(p);
        self.bvh = None;
    }

    pub fn build_index(&mut self) {
        self.bounds = self
            .placements
            .iter()
            .map(|p| p.world_aabb(&self.defs[p.def as usize].aabb))
            .collect();
        self.bvh = Some(Bvh::build(&self.bounds));
    }

    pub fn query(&self, region: &Aabb) -> Vec<u32> {
        let mut out = Vec::new();
        self.bvh.as_ref().expect("build_index first").query(&self.bounds, region, &mut out);
        out
    }

    pub fn choose_lod(&self, p: &Placement, eye: [f32; 3], rule: LodRule) -> Lod {
        let def = &self.defs[p.def as usize];
        let c = p.world_aabb(&def.aabb).centre();
        let d = ((c[0] - eye[0]).powi(2) + (c[1] - eye[1]).powi(2) + (c[2] - eye[2]).powi(2)).sqrt().max(0.001);
        let size = def.aabb.diagonal() / d;
        if size >= rule.full_above {
            Lod::Full
        } else if size >= rule.simplified_above {
            Lod::Simplified
        } else {
            Lod::Envelope
        }
    }

    /// Everything a view would draw: cull to `region`, pick a LOD per
    /// placement, group into instanced draw batches.
    pub fn visible_batches(&self, region: &Aabb, eye: [f32; 3], rule: LodRule) -> Batches {
        let mut b = Batches::default();
        for i in self.query(region) {
            let p = &self.placements[i as usize];
            let lod = self.choose_lod(p, eye, rule);
            b.groups.entry((p.def, lod)).or_default().push(i);
        }
        b
    }

    pub fn triangles_in(&self, b: &Batches) -> usize {
        b.groups
            .iter()
            .map(|((d, lod), v)| self.defs[*d as usize].lods[*lod as usize].tri_count() * v.len())
            .sum()
    }

    /// Schedule total across every placement, from cached per-definition summaries.
    pub fn total_summary(&self) -> Summary {
        let mut counts = vec![0u32; self.defs.len()];
        for p in &self.placements {
            counts[p.def as usize] += 1;
        }
        self.defs
            .iter()
            .zip(counts)
            .fold(Summary::default(), |acc, (d, n)| acc.add(&d.summary.times(n)))
    }

    pub fn memory_report(&self) -> MemoryReport {
        let definitions_bytes: usize =
            self.defs.iter().map(|d| d.lods.iter().map(|m| m.bytes()).sum::<usize>()).sum();
        let placements_bytes = self.placements.len() * std::mem::size_of::<Placement>();
        let index_bytes = self.bvh.as_ref().map_or(0, |b| b.bytes()) + self.bounds.len() * std::mem::size_of::<Aabb>();
        let unshared_geometry_bytes = self
            .placements
            .iter()
            .map(|p| self.defs[p.def as usize].lods.iter().map(|m| m.bytes()).sum::<usize>())
            .sum();
        MemoryReport { definitions_bytes, placements_bytes, index_bytes, unshared_geometry_bytes }
    }
}

/// Deterministic synthetic geometry for benchmarks: a triangle soup inside
/// `bounds` with roughly `tris` triangles. Not real building geometry.
pub fn synthetic_mesh(bounds: &Aabb, tris: usize, seed: u64) -> Mesh {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut next = move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 33) as f32) / (u32::MAX >> 1) as f32
    };
    let mut positions = Vec::with_capacity(tris * 3);
    let mut indices = Vec::with_capacity(tris * 3);
    for t in 0..tris {
        for _ in 0..3 {
            positions.push([
                bounds.min[0] + next() * (bounds.max[0] - bounds.min[0]),
                bounds.min[1] + next() * (bounds.max[1] - bounds.min[1]),
                bounds.min[2] + next() * (bounds.max[2] - bounds.min[2]),
            ]);
        }
        let b = (t * 3) as u32;
        indices.extend_from_slice(&[b, b + 1, b + 2]);
    }
    Mesh { positions, indices }
}

/// Build a definition with full / simplified / envelope meshes.
pub fn synthetic_definition(name: &str, size: [f32; 3], full_tris: usize, summary: Summary, seed: u64) -> Definition {
    let aabb = Aabb { min: [0.0; 3], max: size };
    Definition {
        name: name.to_string(),
        aabb,
        lods: [
            Mesh::box_mesh(&aabb),
            synthetic_mesh(&aabb, (full_tris / 50).max(12), seed + 1),
            synthetic_mesh(&aabb, full_tris, seed),
        ],
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apt(area: f64) -> Definition {
        synthetic_definition("apt", [10.0, 8.0, 3.0], 2000, Summary { gross_area_m2: area, doors: 6, windows: 5 }, 1)
    }

    fn grid_world(n: usize) -> World {
        let mut w = World::default();
        let d = w.add_def(apt(80.0));
        for i in 0..n {
            w.place(Placement {
                def: d,
                pos: [(i % 40) as f32 * 12.0, (i / 40) as f32 * 10.0, 0.0],
                yaw: 0.0,
                mirrored: i % 2 == 0,
            });
        }
        w.build_index();
        w
    }

    #[test]
    fn totals_come_from_definition_summaries() {
        let mut w = World::default();
        let a = w.add_def(apt(80.0));
        let b = w.add_def(apt(55.0));
        for i in 0..3 {
            w.place(Placement { def: a, pos: [i as f32 * 20.0, 0.0, 0.0], yaw: 0.0, mirrored: false });
        }
        for i in 0..2 {
            w.place(Placement { def: b, pos: [i as f32 * 20.0, 30.0, 0.0], yaw: 0.0, mirrored: false });
        }
        let t = w.total_summary();
        assert_eq!(t.gross_area_m2, 3.0 * 80.0 + 2.0 * 55.0);
        assert_eq!(t.doors, 30);
        assert_eq!(t.windows, 25);
    }

    #[test]
    fn mirrored_and_rotated_bounds() {
        let local = Aabb { min: [0.0, 0.0, 0.0], max: [10.0, 4.0, 3.0] };
        let m = Placement { def: 0, pos: [100.0, 0.0, 0.0], yaw: 0.0, mirrored: true };
        let b = m.world_aabb(&local);
        assert_eq!((b.min[0], b.max[0]), (90.0, 100.0));
        let r = Placement { def: 0, pos: [0.0, 0.0, 0.0], yaw: std::f32::consts::FRAC_PI_2, mirrored: false };
        let b = r.world_aabb(&local);
        assert!((b.min[0] + 4.0).abs() < 1e-4 && b.max[0].abs() < 1e-4);
        assert!((b.max[1] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn bvh_matches_brute_force() {
        let w = grid_world(1200);
        for region in [
            Aabb { min: [0.0, 0.0, -1.0], max: [50.0, 40.0, 10.0] },
            Aabb { min: [200.0, 100.0, -1.0], max: [260.0, 160.0, 10.0] },
            Aabb { min: [-500.0, -500.0, -1.0], max: [-400.0, -400.0, 10.0] },
        ] {
            let mut fast = w.query(&region);
            fast.sort();
            let slow: Vec<u32> = (0..w.placements.len() as u32)
                .filter(|&i| w.bounds[i as usize].intersects(&region))
                .collect();
            assert_eq!(fast, slow);
        }
    }

    #[test]
    fn lod_gets_coarser_with_distance() {
        let w = grid_world(1);
        let p = &w.placements[0];
        let rule = LodRule::default();
        assert_eq!(w.choose_lod(p, [5.0, 4.0, 10.0], rule), Lod::Full);
        assert_eq!(w.choose_lod(p, [5.0, 4.0, 100.0], rule), Lod::Simplified);
        assert_eq!(w.choose_lod(p, [5.0, 4.0, 2000.0], rule), Lod::Envelope);
    }

    #[test]
    fn sharing_saves_memory() {
        let w = grid_world(1200);
        let m = w.memory_report();
        assert!(m.unshared_geometry_bytes > 100 * (m.definitions_bytes + m.placements_bytes));
    }

    #[test]
    fn batches_group_by_definition_and_lod() {
        let w = grid_world(1200);
        let all = Aabb { min: [-1e4; 3], max: [1e4; 3] };
        let b = w.visible_batches(&all, [200.0, 150.0, 300.0], LodRule::default());
        assert_eq!(b.instance_count(), 1200);
        assert!(b.draw_calls() <= 3);
    }
}

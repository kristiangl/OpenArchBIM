use super::*;

fn all() -> Vec<GeneratorParams> {
    let mut v = vec![];
    for a in [Archetype::Tile, Archetype::WoodPlank, Archetype::WoodSolid, Archetype::Masonry, Archetype::Metal] {
        for l in Layout::for_archetype(a) {
            v.push(GeneratorParams { layout: *l, ..GeneratorParams::preset(a) });
        }
    }
    v
}

/// Height at a domain point (mm, wrapped), read from the map.
fn height_at(g: &Generated, x: f64, y: f64) -> f64 {
    let (pw, ph) = (f64::from(g.px[0]), f64::from(g.px[1]));
    let c = ((x.rem_euclid(g.width_mm) / g.width_mm) * pw).floor().min(pw - 1.0) as usize;
    let r = (ph - 1.0 - ((y.rem_euclid(g.height_mm) / g.height_mm) * ph).floor().min(ph - 1.0)) as usize;
    f64::from(g.height[r * g.px[0] as usize + c]) / 65535.0 * g.depth_mm
}

fn area(q: &[[f64; 2]]) -> f64 {
    (0..q.len()).map(|i| { let (a, b) = (q[i], q[(i + 1) % q.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f64>() / 2.0
}

#[test]
fn every_layout_covers_its_domain_and_tiles() {
    for p in all() {
        let g = generate(&p, 256, false).unwrap_or_else(|e| panic!("{:?}: {e}", p.layout));
        let n = (g.px[0] * g.px[1]) as usize;
        assert_eq!(g.albedo.len(), n * 3);
        assert_eq!(g.height.len(), n);
        assert!(g.width_mm > 0.0 && g.height_mm > 0.0);
        let pl = plan(&p).unwrap();
        let covered: f64 = pl.units.iter().filter(|u| u.kind != Kind::Over).map(|u| area(&u.poly).abs()).sum();
        let want = g.width_mm * g.height_mm;
        assert!((covered - want).abs() < 1e-6 * want + 1.0, "{:?}: units cover {covered} of {want}", p.layout);
        // the maps wrap: the last and first columns differ about as much as neighbouring columns do
        let gh = &g;
        let col = |c: usize| (0..gh.px[1] as usize).map(move |r| gh.height[r * gh.px[0] as usize + c]);
        let diff = |a: usize, b: usize| col(a).zip(col(b)).filter(|(x, y)| (i32::from(*x) - i32::from(*y)).abs() > 20000).count();
        let last = g.px[0] as usize - 1;
        assert!(diff(last, 0) <= diff(last - 1, last) + diff(0, 1) + g.px[1] as usize / 8, "{:?} seams at its edge", p.layout);
    }
}

#[test]
fn herringbone_basketweave_chevron_and_versailles_cells_do_not_overlap() {
    for l in [Layout::Herringbone, Layout::Basketweave, Layout::Chevron, Layout::Versailles] {
        let p = GeneratorParams { layout: l, unit_length: 297.0, unit_height: 147.0, joint_width: 3.0, ..GeneratorParams::preset(Archetype::Tile) };
        let pl = plan(&p).unwrap();
        let inside = |u: &Unit, x: f64, y: f64| -> bool {
            let q = &u.poly;
            let s = area(q).signum();
            (0..q.len()).all(|i| { let (a, b) = (q[i], q[(i + 1) % q.len()]); ((b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])) * s > 1e-6 })
        };
        for i in 0..40 {
            for j in 0..40 {
                let (x, y) = ((f64::from(i) + 0.37) / 40.0 * pl.w, (f64::from(j) + 0.61) / 40.0 * pl.h);
                let hits: usize = pl.units.iter().map(|u| {
                    let mut c = 0;
                    for kx in -3..=3 { for ky in -3..=3 { if inside(u, x + f64::from(kx) * pl.w, y + f64::from(ky) * pl.h) { c += 1; } } }
                    c
                }).sum();
                assert_eq!(hits, 1, "{l:?} at ({x:.1}, {y:.1})");
            }
        }
    }
}

#[test]
fn hatch_lines_fall_on_the_joints() {
    for p in all() {
        if matches!(p.layout, Layout::Seamless | Layout::Perforated | Layout::Corrugated | Layout::Trapezoidal | Layout::StandingSeam | Layout::BoardAndBatten | Layout::Glulam) { continue; }
        let p = GeneratorParams { joint_width: p.joint_width.max(6.0), joint_depth: p.joint_depth.max(3.0), bevel: 0.0, pitting: 0.0, finish: Finish::Matte, mortar_profile: MortarProfile::Raked, ..p };
        let g = generate(&p, 2048, false).unwrap();
        assert!(!g.hatch.is_empty(), "{:?} has no hatch", p.layout);
        for l in &g.hatch {
            let (s, c) = l.angle_deg.to_radians().sin_cos();
            let period: f64 = l.dashes.iter().sum();
            let dash = l.dashes.first().copied().unwrap_or(f64::INFINITY);
            for n in -3..=3i32 {
                let n = f64::from(n);
                let base = [l.origin[0] - s * l.spacing * n, l.origin[1] + c * l.spacing * n];
                let start = l.shift * n;
                for k in 0..6 {
                    let along = if period > 0.0 { start + period * f64::from(k / 3) + dash * (0.2 + 0.3 * f64::from(k % 3)) } else { start + 37.0 * f64::from(k) };
                    let (x, y) = (base[0] + c * along, base[1] + s * along);
                    let h = height_at(&g, x, y);
                    assert!(h < g.depth_mm - 1.0, "{:?}: hatch line at {:.1} deg passes over a face at ({x:.1}, {y:.1}), height {h:.2} of {:.2}", p.layout, l.angle_deg, g.depth_mm);
                }
            }
        }
    }
}

#[test]
fn running_bond_repeats_after_the_lcm_of_its_offset() {
    let p = GeneratorParams { layout: Layout::RunningBond, unit_length: 230.0, unit_height: 76.0, joint_width: 10.0, offset: 80.0, offset_percent: false, ..GeneratorParams::preset(Archetype::Masonry) };
    assert_eq!(domain(&p).unwrap(), (240.0, 258.0));
    let half = GeneratorParams { offset: 50.0, offset_percent: true, ..p.clone() };
    assert_eq!(domain(&half).unwrap(), (240.0, 172.0));
    let stack = GeneratorParams { offset: 0.0, ..p };
    assert_eq!(domain(&stack).unwrap(), (240.0, 86.0));
}

#[test]
fn colour_modifiers_and_base_colour() {
    let p = GeneratorParams { primary_color: [200, 100, 50], ..GeneratorParams::preset(Archetype::Masonry) };
    assert_eq!(p.base_color(), [200, 100, 50]);
    let c = GeneratorParams { saturation: 0.0, ..p.clone() }.base_color();
    assert!(c[0] == c[1] && c[1] == c[2]);
    assert_eq!(GeneratorParams { brightness: 50.0, ..p.clone() }.base_color(), [100, 50, 25]);
    let t = GeneratorParams { hue_shift: 120.0, ..p }.base_color();
    assert!(t[1] > t[0] && t[1] > t[2], "{t:?}");
}

#[test]
fn same_parameters_same_maps_and_pngs_write() {
    let p = GeneratorParams::preset(Archetype::WoodPlank);
    let a = generate(&p, 128, false).unwrap();
    let b = generate(&p, 128, false).unwrap();
    assert_eq!(a.albedo, b.albedo);
    let other = generate(&GeneratorParams { seed: 9, ..p }, 128, false).unwrap();
    assert_ne!(a.albedo, other.albedo);
    for bytes in [a.albedo_png().unwrap(), a.normal_png().unwrap(), a.height_png().unwrap(), a.roughness_png().unwrap(), a.metalness_png().unwrap(), a.surface_png().unwrap()] {
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }
    assert_eq!(generate(&GeneratorParams::preset(Archetype::Masonry), 256, true).unwrap().px, [256, 256]);
}

#[test]
fn metal_is_metallic_unless_painted() {
    let m = GeneratorParams { finish: Finish::MillFinish, ..GeneratorParams::preset(Archetype::Metal) };
    assert!(generate(&m, 64, false).unwrap().metalness.iter().all(|v| *v == 255));
    assert!(generate(&GeneratorParams::preset(Archetype::Metal), 64, false).unwrap().metalness.iter().all(|v| *v == 0));
    let g = generate(&GeneratorParams { layout: Layout::StandingSeam, ..m }, 256, false).unwrap();
    assert_eq!(g.depth_mm, 38.0);
    assert_eq!(g.hatch.len(), 1);
    assert_eq!(g.hatch[0].spacing, 450.0);
}

#[test]
fn random_end_joints_keep_the_least_stagger() {
    let p = GeneratorParams { layout: Layout::RunningBond, random_stagger: true, min_stagger: 300.0, ..GeneratorParams::preset(Archetype::WoodPlank) };
    let xs = |p: &GeneratorParams| -> Vec<f64> { let pl = plan(p).unwrap(); pl.units.iter().map(|u| u.poly[0][0].rem_euclid(pl.w)).collect() };
    let a = xs(&p);
    let cw = plan(&p).unwrap().w;
    assert_eq!(a.len(), 8);
    for k in 0..8 {
        let d = (a[k] - a[(k + 1) % 8]).rem_euclid(cw);
        assert!(d.min(cw - d) >= 300.0, "courses {k} and {} are {:.0} apart: {a:?}", (k + 1) % 8, d.min(cw - d));
    }
    assert_ne!(a, xs(&GeneratorParams { seed: 7, ..p.clone() }), "another seed, other joints");
    // off, the regular half-board offset (two courses); masonry keeps its bond
    assert_eq!(xs(&GeneratorParams { random_stagger: false, ..p.clone() }).len(), 2);
    assert_eq!(xs(&GeneratorParams { random_stagger: true, layout: Layout::RunningBond, ..GeneratorParams::preset(Archetype::Masonry) }).len(), 2);
}

#[test]
fn maps_write_as_tiff_and_the_height_as_exr() {
    let g = generate(&GeneratorParams { layout: Layout::RunningBond, ..GeneratorParams::preset(Archetype::Masonry) }, 64, false).unwrap();
    let [w, h] = g.px;
    let u16le = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32le = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    // a TIFF's directory: size, samples, bits, and the pixels where the strip says
    for (map, samples, bits) in [("albedo", 3u32, 8u32), ("height", 1, 16), ("roughness", 1, 8)] {
        let t = g.map_file(map, "tiff").unwrap();
        assert_eq!(&t[..4], b"II*\0");
        let ifd = u32le(&t, 4) as usize;
        let n = u16le(&t, ifd) as usize;
        let tag = |id: u16| (0..n).map(|k| ifd + 2 + 12 * k).find(|&e| u16le(&t, e) == id).map(|e| (u32le(&t, e + 4), u32le(&t, e + 8))).unwrap();
        assert_eq!(tag(256).1, w);
        assert_eq!(tag(257).1, h);
        assert_eq!(tag(277).1, samples);
        let (count, v) = tag(258);
        assert_eq!(if count == 1 { v } else { u32::from(u16le(&t, v as usize)) }, bits);
        let (off, len) = (tag(273).1 as usize, tag(279).1 as usize);
        assert_eq!(len, (w * h * samples * bits / 8) as usize);
        assert_eq!(off + len, t.len());
        if map == "height" {
            assert_eq!(u16le(&t, off + 2 * 5), g.height[5]);
        }
    }
    // the EXR: magic, version, a header ending in an empty name, one offset a row, rows of floats
    let e = g.map_file("height", "exr").unwrap();
    assert_eq!(u32le(&e, 0), 20000630);
    assert_eq!(u32le(&e, 4), 2);
    let rows = (h * (8 + 4 * w)) as usize;
    let header_end = e.len() - rows - 8 * h as usize;
    assert_eq!(e[header_end - 1], 0);
    let first = u64::from_le_bytes(e[header_end..header_end + 8].try_into().unwrap()) as usize;
    assert_eq!(first, header_end + 8 * h as usize);
    assert_eq!(u32le(&e, first), 0);
    assert_eq!(u32le(&e, first + 4), w * 4);
    let v = f32::from_le_bytes(e[first + 8 + 4 * 5..first + 8 + 4 * 6].try_into().unwrap());
    assert!((v - f32::from(g.height[5]) / 65535.0).abs() < 1e-6);
    assert!(g.map_file("albedo", "exr").is_err());
}

#[test]
fn versailles_has_its_four_sizes_and_a_hatch_on_every_joint() {
    let p = GeneratorParams { layout: Layout::Versailles, unit_height: 200.0, joint_width: 3.0, ..GeneratorParams::preset(Archetype::Tile) };
    let pl = plan(&p).unwrap();
    assert_eq!((pl.w, pl.h), (1218.0, 1218.0));
    let mut sizes: Vec<(i64, i64)> = pl.units.iter().map(|u| {
        let w = (u.poly[2][0] - u.poly[0][0]) / 203.0;
        let h = (u.poly[2][1] - u.poly[0][1]) / 203.0;
        (w.round().min(h.round()) as i64, w.round().max(h.round()) as i64)
    }).collect();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![(1, 1), (1, 1), (1, 1), (1, 1), (1, 2), (1, 2), (2, 2), (2, 2), (2, 2), (2, 2), (2, 3), (2, 3)]);
    // each joint line has one run per repeat: six lines each way, each its own family
    let g = generate(&p, 512, false).unwrap();
    assert_eq!(g.hatch.len(), 12, "{:?}", g.hatch);
    let along: f64 = g.hatch.iter().map(|l| l.dashes.first().copied().unwrap_or(0.0)).sum();
    // joint runs, modules: across 4 + 4 + 4 + 2 + 3 + 3 and up 3 + 4 + 2 + 3 + 4 + 4
    assert!((along - 40.0 * 203.0).abs() < 1.0, "joint length {along}");
}

use super::*;

/// A small survey in metres: two layers, a contour at 12.5, a line, a circle, an arc, a text, and a block (a line
/// on layer 0 and a "by block" circle) placed twice, once turned and scaled.
pub(crate) const SURVEY: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n\
0\nSECTION\n2\nTABLES\n\
0\nTABLE\n2\nLTYPE\n70\n1\n0\nLTYPE\n2\nDASHED\n70\n0\n3\nDashed\n72\n65\n73\n2\n40\n0.75\n49\n0.5\n49\n-0.25\n0\nENDTAB\n\
0\nTABLE\n2\nLAYER\n70\n3\n\
0\nLAYER\n2\n0\n70\n0\n62\n7\n6\nCONTINUOUS\n\
0\nLAYER\n2\nCONTOURS\n70\n0\n62\n3\n6\nDASHED\n\
0\nLAYER\n2\nHIDDEN-STUFF\n70\n0\n62\n-1\n6\nCONTINUOUS\n\
0\nENDTAB\n0\nENDSEC\n\
0\nSECTION\n2\nBLOCKS\n\
0\nBLOCK\n8\n0\n2\nPEG\n70\n0\n10\n0.0\n20\n0.0\n30\n0.0\n3\nPEG\n\
0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n1.0\n21\n0.0\n\
0\nCIRCLE\n8\n0\n62\n0\n10\n0.0\n20\n0.0\n40\n0.5\n\
0\nENDBLK\n8\n0\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n\
0\nLWPOLYLINE\n8\nCONTOURS\n90\n3\n70\n0\n38\n12.5\n10\n0.0\n20\n0.0\n10\n10.0\n20\n0.0\n10\n10.0\n20\n5.0\n\
0\nLINE\n8\n0\n62\n1\n10\n0.0\n20\n0.0\n30\n1.0\n11\n0.0\n21\n20.0\n31\n2.0\n\
0\nCIRCLE\n8\n0\n10\n5.0\n20\n5.0\n40\n2.0\n\
0\nARC\n8\n0\n10\n0.0\n20\n0.0\n40\n3.0\n50\n0.0\n51\n90.0\n\
0\nTEXT\n8\nHIDDEN-STUFF\n10\n1.0\n20\n2.0\n40\n0.25\n1\nRL 12.50\n\
0\nINSERT\n8\nCONTOURS\n62\n5\n2\nPEG\n10\n100.0\n20\n100.0\n\
0\nINSERT\n8\n0\n2\nPEG\n10\n0.0\n20\n50.0\n41\n2.0\n42\n2.0\n50\n90.0\n\
0\nENDSEC\n0\nEOF\n";

fn check(d: &Drawing) {
    assert_eq!((d.unit.as_str(), d.unit_mm), ("m", 1000.0));
    let layer = |n: &str| d.layers.iter().find(|l| l.name == n).unwrap_or_else(|| panic!("layer {n}"));
    assert_eq!(layer("CONTOURS").color, [0, 255, 0]);
    assert!(layer("CONTOURS").on && !layer("HIDDEN-STUFF").on, "a negative colour is a layer turned off");
    let contours = d.layer_index("contours").unwrap();
    let zero = d.layer_index("0").unwrap();

    // the contour keeps its height
    let contour = d.shapes.iter().find_map(|s| match &s.kind { Kind::Poly { pts, z, closed } if pts.len() == 3 => Some((s, pts, z, closed)), _ => None }).expect("contour");
    assert_eq!((contour.0.layer, contour.2.as_slice(), *contour.3), (contours, &[12.5][..], false));
    // the red line with a height at each end
    assert!(d.shapes.iter().any(|s| s.color == Some([255, 0, 0]) && matches!(&s.kind, Kind::Poly { z, .. } if z == &vec![1.0, 2.0])));
    assert!(d.shapes.iter().any(|s| matches!(&s.kind, Kind::Circle { c, r, .. } if *c == [5.0, 5.0] && *r == 2.0)));
    assert!(d.shapes.iter().any(|s| matches!(&s.kind, Kind::Arc { r, a0, a1, .. } if *r == 3.0 && a0.abs() < 1e-9 && (a1 - TAU / 4.0).abs() < 1e-9)));
    assert!(d.shapes.iter().any(|s| matches!(&s.kind, Kind::Text { text, h, .. } if text == "RL 12.50" && *h == 0.25)));

    // the first peg: its layer 0 line goes to the reference's layer, its "by block" circle takes the reference's blue
    assert!(d.shapes.iter().any(|s| s.layer == contours && s.color.is_none() && matches!(&s.kind, Kind::Poly { pts, .. } if pts[0] == [100.0, 100.0] && pts[1] == [101.0, 100.0])));
    assert!(d.shapes.iter().any(|s| s.layer == contours && s.color == Some([0, 0, 255]) && matches!(&s.kind, Kind::Circle { c, r, .. } if *c == [100.0, 100.0] && *r == 0.5)));
    // the second, turned a quarter and doubled: the line runs up from (0, 50) to (0, 52), the circle's radius is 1
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    assert!(d.shapes.iter().any(|s| s.layer == zero && matches!(&s.kind, Kind::Poly { pts, .. } if near(pts[0], [0.0, 50.0]) && near(pts[1], [0.0, 52.0]))));
    assert!(d.shapes.iter().any(|s| matches!(&s.kind, Kind::Circle { c, r, .. } if near(*c, [0.0, 50.0]) && (*r - 1.0).abs() < 1e-9)));

    let e = d.extents.unwrap();
    assert!(e[0] <= -3.0 + 1e-9 && e[2] >= 101.0 - 1e-9, "{e:?}");
    assert!(d.left_out.is_empty(), "{:?}", d.left_out);
}

#[test]
fn a_dxf_is_read_with_its_layers_blocks_and_heights() {
    let d = read(SURVEY.as_bytes()).unwrap();
    assert_eq!(d.format, "DXF");
    check(&d);
    let dash = &d.layers.iter().find(|l| l.name == "CONTOURS").unwrap().dash;
    assert_eq!(dash, &vec![0.5, 0.25]);
}

#[test]
fn a_dwg_is_read_the_same() {
    let dwg = dwg_from_dxf(SURVEY).unwrap();
    assert!(is_dwg(&dwg) && !is_dwg(SURVEY.as_bytes()));
    let d = read(&dwg).unwrap();
    assert_eq!(d.format, "DWG");
    check(&d);
}

#[test]
fn bulges_become_arcs_and_a_stretched_block_keeps_its_shape() {
    // a half circle from (0,0) to (2,0), bulging below the chord (bulge 1 is a half circle to the left of travel... to the right here)
    let pts = bulged(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false);
    assert!(pts.len() > 10);
    let low = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    assert!((low + 1.0).abs() < 1e-3, "a positive bulge goes anticlockwise, so below a chord drawn left to right: {low}");
    assert!(pts.iter().all(|p| ((p.0 - 1.0).hypot(p.1) - 1.0).abs() < 1e-9));

    // a block stretched one way: its circle is no longer a circle, so it comes as a closed run of points
    let dxf = SURVEY.replace("41\n2.0\n42\n2.0", "41\n3.0\n42\n1.0");
    let d = read(dxf.as_bytes()).unwrap();
    assert!(d.shapes.iter().any(|s| matches!(&s.kind, Kind::Poly { pts, closed: true, .. } if pts.len() > 50 && pts.iter().all(|p| p[0].abs() <= 0.5 + 1e-9 && (p[1] - 50.0).abs() <= 1.5 + 1e-9))));
}

#[test]
fn what_is_not_a_drawing_says_so() {
    assert!(read(b"hello").is_err() || read(b"hello").unwrap().shapes.is_empty());
    assert!(read(b"AC1032\0\0\0garbage").is_err());
}

//! More DXF entities (IOP-15): WIPEOUT, SOLID and TRACE, MESH, LEADER, the reasons for the
//! entities that are not imported, and the islands of a hatch.

use kadakas_core::geom::point_in_polygon_closed;
use kadakas_core::mesh::triangulate;

use super::tilted::Plane;
use super::*;

/// The white of the paper: a wipeout and the island of a hatch cover what lies below.
pub(super) const PAPER: Color = Color([255, 255, 255, 255]);

/// A WIPEOUT as a closed polygon with a white fill and no line. The path is the one that the
/// ezdxf reader finds: the boundary is in pixels of an image with its origin at the top left
/// corner, and the image has the pixel vectors `u` and `v`.
pub(super) fn wipeout(e: &Entity, xf: &Xf) -> Result<Item, Skip> {
    let origin = DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0));
    let u = DVec2::new(e.fd(11, 0.0), e.fd(21, 0.0));
    let v = DVec2::new(e.fd(12, 0.0), e.fd(22, 0.0));
    let (w, h) = (e.fd(13, 1.0), e.fd(23, 1.0));
    if u.length_squared() == 0.0 && v.length_squared() == 0.0 {
        return Err("wipeout without size");
    }
    let mut path: Vec<DVec2> = Vec::new();
    let mut pending: Option<f64> = None;
    for (code, value) in &e.items {
        let Some(x) = parse::parse_f64(value) else {
            continue;
        };
        match code {
            14 => pending = Some(x),
            24 => {
                if let Some(px) = pending.take() {
                    path.push(DVec2::new(px, x));
                }
            }
            _ => {}
        }
    }
    let clipping = e.int(280).unwrap_or(1) != 0;
    if !clipping || path.len() < 2 {
        path = vec![
            DVec2::new(-0.5, -0.5),
            DVec2::new(w - 0.5, -0.5),
            DVec2::new(w - 0.5, h - 0.5),
            DVec2::new(-0.5, h - 0.5),
        ];
    } else if path.len() == 2 {
        let (a, b) = (path[0], path[1]);
        path = vec![a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)];
    }
    let start = origin + u * 0.5 - v * 0.5;
    let mut pts: Vec<DVec2> = path
        .iter()
        .map(|p| xf.apply(start + u * p.x + v * (h - p.y)))
        .collect();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 3 {
        return Err("wipeout without area");
    }
    let mut item = Item::new(Shape::Polyline {
        points: corners(&pts),
        closed: true,
    });
    item.fill = Some(PAPER);
    item.pen = Some(Color::NONE);
    item.data.insert("wipeout".into(), true.into());
    Ok(item)
}

/// SOLID and TRACE: a filled triangle or quadrilateral. The corners come in the order 1, 2, 4, 3.
pub(super) fn solid(e: &Entity, xf: &Xf, plane: &Plane) -> Result<Item, Skip> {
    // The four corners are at one elevation (group code 30).
    let z = e.fd(30, 0.0);
    let corner = |k: i32| DVec3::new(e.fd(10 + k, 0.0), e.fd(20 + k, 0.0), z);
    let (p1, p2, p3, p4) = (corner(0), corner(1), corner(2), corner(3));
    let mut ring = vec![p1, p2];
    if p4 != p3 && e.f(13).is_some() {
        ring.push(p4);
    }
    ring.push(p3);
    ring.dedup();
    if ring.first() == ring.last() && ring.len() > 1 {
        ring.pop();
    }
    if ring.len() < 3 {
        return Err("solid with fewer than 3 corners");
    }
    match plane {
        Plane::Flat { flip } => {
            let pts: Vec<DVec2> = ring
                .iter()
                .map(|p| xf.apply(flipx(p.truncate(), *flip)))
                .collect();
            let mut item = Item::new(Shape::Polyline {
                points: corners(&pts),
                closed: true,
            });
            item.fill_pen = true;
            Ok(item)
        }
        Plane::Tilted(ocs) => {
            let points = ring
                .iter()
                .map(|p| {
                    let w = ocs.world(*p);
                    xf.apply(w.truncate()).extend(xf.z(w.z))
                })
                .collect();
            Ok(Item::new(Shape::Polyline3d {
                points,
                closed: true,
            }))
        }
    }
}

/// The numbers of group codes `x`, `x + 10` and `x + 20` as points, in the order of the entity.
fn points_of(e: &Entity, x: i32) -> Vec<DVec3> {
    let mut pts: Vec<DVec3> = Vec::new();
    for (code, value) in &e.items {
        let Some(v) = parse::parse_f64(value) else {
            continue;
        };
        if *code == x {
            pts.push(DVec3::new(v, 0.0, 0.0));
        } else if *code == x + 10 {
            if let Some(p) = pts.last_mut() {
                p.y = v;
            }
        } else if *code == x + 20
            && let Some(p) = pts.last_mut()
        {
            p.z = v;
        }
    }
    pts
}

/// LEADER: the vertices (in world coordinates) as an open polyline. The arrowhead and the text
/// are not part of it: the text is an entity of its own.
pub(super) fn leader(e: &Entity, xf: &Xf) -> Result<Item, Skip> {
    let pts = points_of(e, 10);
    if pts.len() < 2 {
        return Err("leader with fewer than 2 vertices");
    }
    if pts.iter().any(|p| p.z != 0.0) {
        let points = pts
            .iter()
            .map(|p| xf.apply(p.truncate()).extend(xf.z(p.z)))
            .collect();
        return Ok(Item::new(Shape::Polyline3d {
            points,
            closed: false,
        }));
    }
    let pts: Vec<DVec2> = pts.iter().map(|p| xf.apply(p.truncate())).collect();
    Ok(Item::new(Shape::Polyline {
        points: corners(&pts),
        closed: false,
    }))
}

/// The most vertices and face entries of one MESH entity.
const MAX_MESH: usize = 4_000_000;

/// MESH (a subdivision mesh of release 2010): the vertices, and the faces cut into triangles. The
/// second value tells whether the mesh has a subdivision level above 0 (only the control mesh is
/// imported).
pub(super) fn mesh(e: &Entity, xf: &Xf) -> Result<(Mesh3d, bool), Skip> {
    let at92 = e
        .items
        .iter()
        .position(|(c, _)| *c == 92)
        .ok_or("mesh without vertices")?;
    let at93 = e.items[at92..]
        .iter()
        .position(|(c, _)| *c == 93)
        .map(|p| p + at92)
        .ok_or("mesh without faces")?;
    let want = e.items[at92].1.trim().parse::<usize>().unwrap_or(0);
    if want == 0 || want > MAX_MESH {
        return Err("mesh with a wrong vertex count");
    }
    let head = Entity {
        kind: String::new(),
        items: e.items[at92 + 1..at93].to_vec(),
        children: vec![],
    };
    let raw = points_of(&head, 10);
    if raw.len() != want {
        return Err("mesh with a wrong vertex count");
    }
    let vertices: Vec<DVec3> = raw
        .iter()
        .map(|p| xf.apply(p.truncate()).extend(xf.z(p.z)))
        .collect();
    let size = e.items[at93].1.trim().parse::<usize>().unwrap_or(0);
    if size == 0 || size > MAX_MESH * 5 {
        return Err("mesh with a wrong face list");
    }
    // A negative or unreadable number makes the list wrong.
    let ints: Vec<usize> = e.items[at93 + 1..]
        .iter()
        .filter(|(c, _)| *c == 90)
        .take(size)
        .map_while(|(_, v)| {
            v.trim()
                .parse::<i64>()
                .ok()
                .and_then(|n| usize::try_from(n).ok())
        })
        .collect();
    if ints.len() != size {
        return Err("mesh with a wrong face list");
    }
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut i = 0;
    while i < ints.len() {
        let k = ints[i];
        let end = i
            .checked_add(1)
            .and_then(|n| n.checked_add(k))
            .ok_or("mesh with a wrong face list")?;
        let ids = ints.get(i + 1..end).ok_or("mesh with a wrong face list")?;
        i = end;
        if ids.iter().any(|v| *v >= vertices.len()) {
            return Err("mesh face with a vertex that does not exist");
        }
        if k < 3 {
            continue;
        }
        let ring: Vec<u32> = ids.iter().map(|v| *v as u32).collect();
        triangles.extend(cut_face(&vertices, &ring));
        if triangles.len() > MAX_MESH {
            return Err("mesh with too many triangles");
        }
    }
    if triangles.is_empty() {
        return Err("mesh without faces");
    }
    let subdivided = e.int(91).unwrap_or(0) > 0;
    Ok((
        Mesh3d {
            name: String::new(),
            vertices,
            triangles,
            color: None,
            exact: None,
            exact_unusable: None,
        },
        subdivided,
    ))
}

/// Triangles of one face. A triangle is one, a quadrilateral two, and a face with more corners
/// is cut in its plane (a concave face is cut by ears, and a fan when that fails).
fn cut_face(vertices: &[DVec3], ring: &[u32]) -> Vec<[u32; 3]> {
    if ring.len() == 3 {
        return vec![[ring[0], ring[1], ring[2]]];
    }
    let p = |i: u32| vertices[i as usize];
    // The plane of the face: drop the axis along which the normal is largest.
    let mut n = DVec3::ZERO;
    for k in 0..ring.len() {
        let (a, b) = (p(ring[k]), p(ring[(k + 1) % ring.len()]));
        n += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let flat: Vec<DVec2> = ring
        .iter()
        .map(|i| {
            let q = p(*i);
            let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
            if az >= ax && az >= ay {
                DVec2::new(q.x, q.y)
            } else if ax >= ay {
                DVec2::new(q.y, q.z)
            } else {
                DVec2::new(q.z, q.x)
            }
        })
        .collect();
    let ears = triangulate(&flat);
    if ears.len() == ring.len() - 2 {
        return ears
            .into_iter()
            .map(|t| [ring[t[0]], ring[t[1]], ring[t[2]]])
            .collect();
    }
    (1..ring.len() - 1)
        .map(|k| [ring[0], ring[k], ring[k + 1]])
        .collect()
}

/// Why an entity kind is not imported, for the kinds that deserve more than "unsupported".
pub(super) fn reason(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "3DSOLID" | "BODY" | "REGION" => {
            "ACIS solid: Kadakas does not read ACIS data, so the solid is not imported"
        }
        "SURFACE" | "EXTRUDEDSURFACE" | "LOFTEDSURFACE" | "REVOLVEDSURFACE" | "SWEPTSURFACE"
        | "PLANESURFACE" | "NURBSURFACE" => {
            "ACIS surface: Kadakas does not read ACIS data, so the surface is not imported"
        }
        "MLEADER" | "MULTILEADER" => {
            "multileader: its lines and text are one entity that is not read (explode it in the CAD program)"
        }
        "XLINE" | "RAY" => "infinite line (XLINE or RAY) has no finite shape",
        "OLE2FRAME" | "OLEFRAME" => "OLE object is not imported",
        "PDFUNDERLAY" | "DGNUNDERLAY" | "DWFUNDERLAY" | "UNDERLAY" => {
            "underlay reference (a PDF, DGN or DWF file) is not imported"
        }
        "MLINE" => "multiline is not imported (explode it in the CAD program)",
        "TOLERANCE" => "tolerance frame is not imported",
        "SHAPE" => "shape from an SHX file is not imported",
        "LIGHT" | "SUN" | "CAMERA" => "light, sun and camera are not geometry",
        "HELIX" => "helix is not imported",
        "ACAD_PROXY_ENTITY" => "proxy entity (a custom object without its program) is not imported",
        "ATTDEF" => {
            "attribute definition: the visible text is the ATTRIB of the INSERT (a constant definition shows its value)"
        }
        "VIEWPORT" => "viewport in model space is not geometry",
        _ => return None,
    })
}

/// Why an IMAGE is not imported, with the file that its IMAGEDEF names.
pub(super) fn image_reason(e: &Entity, dxf: &Dxf) -> String {
    let file = e
        .get(340)
        .and_then(crate::dxf::hash::normal_handle)
        .and_then(|h| {
            dxf.objects
                .iter()
                .find(|o| o.kind == "IMAGEDEF" && o.handle().as_deref() == Some(h.as_str()))
        })
        .and_then(|d| d.get(1))
        .filter(|f| !f.trim().is_empty());
    match file {
        Some(f) => format!("raster image '{f}' is not imported: the picture is not read"),
        None => "raster image is not imported: the picture is not read".into(),
    }
}

/// The larger side of the box around the polygons (0 when there is no point).
pub(super) fn extent(loops: &[Vec<DVec2>]) -> f64 {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in loops.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if lo.x <= hi.x && lo.is_finite() && hi.is_finite() {
        (hi.x - lo.x).max(hi.y - lo.y)
    } else {
        0.0
    }
}

/// The area of a polygon, without a sign.
pub(super) fn area(pts: &[DVec2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        / 2.0
}

/// The distance from a point to the outline of a polygon.
fn distance_to_outline(p: DVec2, poly: &[DVec2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let ab = b - a;
            let len2 = ab.length_squared();
            let t = if len2 < 1e-300 {
                0.0
            } else {
                ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
            };
            (p - (a + ab * t)).length()
        })
        .fold(f64::MAX, f64::min)
}

/// Which loops of a hatch are islands (holes). A loop is an island when an odd number of the
/// other loops contain it (the normal style of a hatch). Only a loop with a larger area can
/// contain another one. A loop is inside another one when most of the points that are spread
/// along it are inside by more than `margin`, so two loops that touch along a boundary are not
/// taken for nested ones. `points` are the points of each loop (curves cut into chords within
/// `margin`).
pub(super) fn islands(points: &[Vec<DVec2>], margin: f64) -> Vec<bool> {
    let areas: Vec<f64> = points.iter().map(|p| area(p)).collect();
    points
        .iter()
        .enumerate()
        .map(|(i, own)| {
            if own.is_empty() {
                return false;
            }
            let step = own.len().div_ceil(8).max(1);
            let samples: Vec<DVec2> = own.iter().step_by(step).copied().collect();
            let depth = points
                .iter()
                .enumerate()
                .filter(|(j, other)| {
                    *j != i && other.len() >= 3 && areas[*j] > areas[i] && {
                        let inside = samples
                            .iter()
                            .filter(|p| {
                                point_in_polygon_closed(**p, other)
                                    && distance_to_outline(**p, other) > margin
                            })
                            .count();
                        inside * 4 >= samples.len() * 3
                    }
                })
                .count();
            depth % 2 == 1
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(kind: &str, items: &[(i32, &str)]) -> Entity {
        Entity {
            kind: kind.into(),
            items: items.iter().map(|(c, v)| (*c, v.to_string())).collect(),
            children: vec![],
        }
    }

    #[test]
    fn islands_follow_the_nesting() {
        let sq = |a: f64, b: f64| {
            vec![
                DVec2::new(a, a),
                DVec2::new(b, a),
                DVec2::new(b, b),
                DVec2::new(a, b),
            ]
        };
        assert_eq!(
            islands(&[sq(0.0, 10.0), sq(2.0, 8.0)], 0.01),
            vec![false, true]
        );
        assert_eq!(
            islands(&[sq(0.0, 10.0), sq(2.0, 8.0), sq(4.0, 6.0)], 0.01),
            vec![false, true, false]
        );
        assert_eq!(
            islands(&[sq(0.0, 1.0), sq(5.0, 6.0)], 0.01),
            vec![false, false]
        );
        // Two squares that share an edge are side by side, not nested.
        assert_eq!(
            islands(
                &[
                    sq(0.0, 10.0),
                    vec![
                        DVec2::new(10.0, 2.0),
                        DVec2::new(14.0, 2.0),
                        DVec2::new(14.0, 8.0),
                        DVec2::new(10.0, 8.0)
                    ]
                ],
                0.01
            ),
            vec![false, false]
        );
        assert_eq!(islands(&[], 0.01), Vec::<bool>::new());
        assert_eq!(islands(&[vec![]], 0.01), vec![false]);
    }

    #[test]
    fn reasons_name_the_family() {
        assert!(reason("3DSOLID").unwrap().contains("ACIS"));
        assert!(reason("BODY").unwrap().contains("ACIS"));
        assert!(reason("LOFTEDSURFACE").unwrap().contains("surface"));
        assert!(reason("MULTILEADER").unwrap().contains("multileader"));
        assert!(reason("XLINE").is_some() && reason("RAY").is_some());
        assert!(reason("LINE").is_none());
    }

    #[test]
    fn a_face_with_many_corners_is_cut_in_its_plane() {
        let v: Vec<DVec3> = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 4.0, 0.0),
            DVec3::new(2.0, 1.0, 0.0),
            DVec3::new(0.0, 4.0, 0.0),
        ];
        let t = cut_face(&v, &[0, 1, 2, 3, 4]);
        assert_eq!(t.len(), 3);
        // The concave notch at vertex 3 is not covered by a triangle across it.
        let area: f64 = t
            .iter()
            .map(|t| {
                let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
                ((b - a).cross(c - a).length()) / 2.0
            })
            .sum();
        assert!((area - 10.0).abs() < 1e-9, "{area}");
        assert_eq!(cut_face(&v, &[0, 1, 2]), vec![[0, 1, 2]]);
        assert_eq!(cut_face(&v, &[0, 1, 2, 3]).len(), 2);
    }

    #[test]
    fn a_mesh_entity_gives_triangles() {
        let e = ent(
            "MESH",
            &[
                (71, "2"),
                (72, "0"),
                (91, "0"),
                (92, "4"),
                (10, "0"),
                (20, "0"),
                (30, "0"),
                (10, "10"),
                (20, "0"),
                (30, "0"),
                (10, "10"),
                (20, "10"),
                (30, "0"),
                (10, "0"),
                (20, "10"),
                (30, "5"),
                (93, "5"),
                (90, "4"),
                (90, "0"),
                (90, "1"),
                (90, "2"),
                (90, "3"),
                (94, "0"),
                (95, "0"),
                (90, "0"),
            ],
        );
        let (m, sub) = mesh(&e, &Xf::scale(2.0)).unwrap();
        assert!(!sub);
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.triangles.len(), 2);
        assert_eq!(m.vertices[1], DVec3::new(20.0, 0.0, 0.0));
        assert_eq!(m.vertices[3].z, 10.0);
        // Damaged lists are refused.
        let mut bad = e.clone();
        bad.items.retain(|(c, v)| !(*c == 92 && v == "4"));
        bad.items.insert(3, (92, "9".into()));
        assert!(mesh(&bad, &Xf::scale(1.0)).is_err());
        let mut bad = e.clone();
        bad.items.iter_mut().find(|(c, _)| *c == 93).unwrap().1 = "900".into();
        assert!(mesh(&bad, &Xf::scale(1.0)).is_err());
        let mut bad = e.clone();
        bad.items
            .iter_mut()
            .filter(|(c, _)| *c == 90)
            .nth(2)
            .unwrap()
            .1 = "77".into();
        assert!(mesh(&bad, &Xf::scale(1.0)).is_err());
        assert!(mesh(&ent("MESH", &[]), &Xf::scale(1.0)).is_err());
        let mut sub = e;
        sub.items.iter_mut().find(|(c, _)| *c == 91).unwrap().1 = "2".into();
        assert!(mesh(&sub, &Xf::scale(1.0)).unwrap().1);
    }

    #[test]
    fn a_wipeout_follows_the_ezdxf_masking_area() {
        // The masking area (10, 20) to (30, 50) that ezdxf makes: insert = lower left, u = (20, 0),
        // v = (0, 30), image size 1 x 1, the boundary in -0.5 to 0.5.
        let e = ent(
            "WIPEOUT",
            &[
                (10, "10"),
                (20, "20"),
                (30, "0"),
                (11, "20"),
                (21, "0"),
                (31, "0"),
                (12, "0"),
                (22, "30"),
                (32, "0"),
                (13, "1"),
                (23, "1"),
                (71, "1"),
                (91, "2"),
                (14, "-0.5"),
                (24, "0.5"),
                (14, "0.5"),
                (24, "-0.5"),
            ],
        );
        let it = wipeout(&e, &Xf::scale(1.0)).unwrap();
        let Shape::Polyline { points, closed } = &it.shape else {
            panic!()
        };
        assert!(*closed);
        let mut xs: Vec<(f64, f64)> = points.iter().map(|p| (p.p.x, p.p.y)).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            xs,
            vec![(10.0, 20.0), (10.0, 50.0), (30.0, 20.0), (30.0, 50.0)]
        );
        assert_eq!(it.fill, Some(PAPER));
        assert_eq!(it.pen, Some(Color::NONE));
        assert_eq!(it.data["wipeout"], true);
        assert!(wipeout(&ent("WIPEOUT", &[]), &Xf::scale(1.0)).is_err());
    }

    #[test]
    fn solids_are_drawn_in_the_order_1_2_4_3() {
        let e = ent(
            "SOLID",
            &[
                (10, "0"),
                (20, "0"),
                (11, "10"),
                (21, "0"),
                (12, "0"),
                (22, "10"),
                (13, "10"),
                (23, "10"),
            ],
        );
        let it = solid(&e, &Xf::scale(1.0), &Plane::Flat { flip: false }).unwrap();
        let Shape::Polyline { points, .. } = &it.shape else {
            panic!()
        };
        let p: Vec<(f64, f64)> = points.iter().map(|v| (v.p.x, v.p.y)).collect();
        assert_eq!(p, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert!(it.fill_pen);
        // A triangle repeats its third corner.
        let tri = ent(
            "SOLID",
            &[
                (10, "0"),
                (20, "0"),
                (11, "5"),
                (21, "0"),
                (12, "0"),
                (22, "5"),
                (13, "0"),
                (23, "5"),
            ],
        );
        let it = solid(&tri, &Xf::scale(1.0), &Plane::Flat { flip: false }).unwrap();
        assert!(matches!(&it.shape, Shape::Polyline { points, .. } if points.len() == 3));
        assert!(
            solid(
                &ent("SOLID", &[]),
                &Xf::scale(1.0),
                &Plane::Flat { flip: false }
            )
            .is_err()
        );
    }

    #[test]
    fn a_leader_is_an_open_polyline() {
        let e = ent(
            "LEADER",
            &[
                (76, "3"),
                (10, "0"),
                (20, "0"),
                (30, "0"),
                (10, "5"),
                (20, "5"),
                (30, "0"),
                (10, "9"),
                (20, "5"),
                (30, "0"),
            ],
        );
        let it = leader(&e, &Xf::scale(10.0)).unwrap();
        assert!(
            matches!(&it.shape, Shape::Polyline { points, closed: false } if points.len() == 3 && points[2].p.x == 90.0)
        );
        assert!(leader(&ent("LEADER", &[(10, "0"), (20, "0")]), &Xf::scale(1.0)).is_err());
        let z = ent(
            "LEADER",
            &[
                (10, "0"),
                (20, "0"),
                (30, "1"),
                (10, "5"),
                (20, "5"),
                (30, "2"),
            ],
        );
        assert!(matches!(
            leader(&z, &Xf::scale(1.0)).unwrap().shape,
            Shape::Polyline3d { .. }
        ));
    }

    #[test]
    fn an_image_names_its_file() {
        let mut dxf = Dxf::default();
        let mut def = ent("IMAGEDEF", &[(5, "2A"), (1, "photos\\site.png")]);
        def.items.push((0, String::new()));
        dxf.objects.push(def);
        let img = ent("IMAGE", &[(340, "2A")]);
        assert!(image_reason(&img, &dxf).contains("'photos\\site.png'"));
        assert!(!image_reason(&ent("IMAGE", &[(340, "FF")]), &dxf).contains('\''));
    }
}

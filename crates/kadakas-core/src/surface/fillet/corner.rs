//! The ends of the strips at a vertex of three faces, and the corner pieces (see the module
//! `fillet` for the three cases).

use glam::DMat3;

use super::*;

fn refuse(v: VertexId, why: String) -> FilletError {
    FilletError::Corner { vertex: v, why }
}

/// Plans the corner at `v`. `sel[i]` is the strip of the edge `{v, next[i]}` when it is chosen.
pub(super) fn corner(
    b: &mut Builder<'_>,
    strips: &mut [Strip],
    v: VertexId,
    at: &Around,
    sel: &[Option<usize>],
    profile: &Profile,
) -> Result<(), FilletError> {
    let chosen: Vec<usize> = (0..3).filter(|i| sel[*i].is_some()).collect();
    let kinds: BTreeSet<bool> = chosen
        .iter()
        .map(|i| strips[sel[*i].expect("chosen")].convex)
        .collect();
    if kinds.len() > 1 {
        let names: Vec<String> = chosen
            .iter()
            .map(|i| strips[sel[*i].expect("chosen")].edge.to_string())
            .collect();
        return Err(refuse(
            v,
            format!(
                "it joins convex and concave edges to round or cut (edges {}); leave out the \
                 concave or the convex ones",
                names.join(", ")
            ),
        ));
    }
    match chosen.as_slice() {
        [i] => one(b, strips, v, at, *i, sel[*i].expect("chosen")),
        [x, y] => {
            // The edge that stays sharp is the third one.
            let i = (0..3).find(|k| k != x && k != y).expect("three edges");
            two(
                b,
                strips,
                v,
                at,
                i,
                sel[(i + 2) % 3].expect("chosen"),
                sel[(i + 1) % 3].expect("chosen"),
            )
        }
        _ => match profile {
            Profile::Round { radius, .. } => three_round(b, strips, v, at, sel, *radius),
            Profile::Flat { .. } => three_flat(b, strips, v, at, sel),
        },
    }
}

/// One chosen edge `{v, next[i]}`: its strip runs to the plane of the third face.
fn one(
    b: &mut Builder<'_>,
    strips: &mut [Strip],
    v: VertexId,
    at: &Around,
    i: usize,
    si: usize,
) -> Result<(), FilletError> {
    let s = &strips[si];
    let n = s.n();
    let (fi, fi1, g) = (at.faces[i], at.faces[(i + 1) % 3], at.faces[(i + 2) % 3]);
    let gp = b.shape.faces[&g].plane;
    let mut ts = Vec::with_capacity(n + 1);
    for j in 0..=n {
        ts.push(s.t_on(j, &gp).ok_or_else(|| {
            refuse(
                v,
                format!("edge {} runs along the plane of face {g}", s.edge),
            )
        })?);
    }
    let pos: Vec<DVec3> = (0..=n).map(|j| s.point(j, ts[j])).collect();
    let side_i = s
        .side_of(fi)
        .expect("the edge lies between faces i and i + 1");
    let pts: Vec<Pt> = pos.iter().map(|p| b.add(*p)).collect();
    let (pt_i, pt_i1) = if side_i == 0 {
        (pts[0], pts[n])
    } else {
        (pts[n], pts[0])
    };
    b.sub(fi, v, vec![pt_i]);
    b.sub(fi1, v, vec![pt_i1]);
    // The third face runs in from the side of face i + 1 and out to the side of face i.
    let mut seq = pts.clone();
    if side_i == 0 {
        seq.reverse();
    }
    b.sub(g, v, seq);
    b.trims.push(Trim {
        far: at.next[(i + 1) % 3],
        old: v,
        to: pt_i1,
    });
    b.trims.push(Trim {
        far: at.next[(i + 2) % 3],
        old: v,
        to: pt_i,
    });
    let e = strips[si].end_of(v);
    strips[si].ends[e] = Some(End {
        between: vec![Vec::new(); n],
        pts,
        t: ts,
    });
    Ok(())
}

/// Two chosen edges: `a` between faces `i + 2` (shared) and `i`, `b` between faces `i + 1` and
/// `i + 2`. The edge `{v, next[i]}` stays sharp. Both strips end on one plane through that edge.
#[allow(clippy::too_many_arguments)]
fn two(
    b: &mut Builder<'_>,
    strips: &mut [Strip],
    v: VertexId,
    at: &Around,
    i: usize,
    sa: usize,
    sb: usize,
) -> Result<(), FilletError> {
    let (fp, fq, fs) = (at.faces[i], at.faces[(i + 1) % 3], at.faces[(i + 2) % 3]);
    let vp = b.shape.points[&v];
    let far = at.next[i];
    let udir = (b.shape.points[&far] - vp).normalize();
    let fsp = b.shape.faces[&fs].plane;
    let (a, c) = (&strips[sa], &strips[sb]);
    let n = a.n();
    let (Some(xa), Some(xc)) = (a.side_of(fs), c.side_of(fs)) else {
        unreachable!("both edges lie on the shared face");
    };
    let (ma, mc) = (a.inward[xa], c.inward[xc]);
    let (e1, e2) = fsp.basis();
    let det = ma.dot(e1) * mc.dot(e2) - ma.dot(e2) * mc.dot(e1);
    // The plane of the corner: through the sharp edge and the corner of the two rails, or (when
    // that plane leaves a thin wedge on the sharp edge, see `Builder::bisect`) the plane that
    // halves the angle of the material at the sharp edge.
    let bisect = b.bisect.contains(&v);
    let into = |s: &Strip| -> DVec3 {
        let other = if s.p == v { s.at + s.d * s.len } else { s.at };
        let x = other - vp;
        (x - udir * x.dot(udir)).normalize_or_zero()
    };
    let normal = if bisect {
        udir.cross(into(a) + into(c))
    } else if det.abs() > 1e-9 {
        // The corner of the two rails in the shared face.
        let mu1 = (a.s * mc.dot(e2) - ma.dot(e2) * c.s) / det;
        let mu2 = (ma.dot(e1) * c.s - mc.dot(e1) * a.s) / det;
        let cs = vp + e1 * mu1 + e2 * mu2;
        udir.cross(cs - vp)
    } else {
        // The two edges continue each other: they need the same setback.
        if (a.s - c.s).abs() > b.tol {
            return Err(refuse(
                v,
                format!(
                    "edges {} and {} continue each other but meet face {fs} at different angles",
                    a.edge, c.edge
                ),
            ));
        }
        udir.cross(b.shape.faces[&fp].plane.normal)
    };
    let pi = Plane::new(vp, normal).ok_or_else(|| {
        refuse(
            v,
            format!(
                "the rails of edges {} and {} meet on the sharp edge",
                a.edge, c.edge
            ),
        )
    })?;
    let end_on = |s: &Strip| -> Result<Vec<f64>, FilletError> {
        (0..=n)
            .map(|j| {
                s.t_on(j, &pi).ok_or_else(|| {
                    refuse(
                        v,
                        format!("edge {} runs along the plane of the corner", s.edge),
                    )
                })
            })
            .collect()
    };
    let (ta, tc) = (end_on(a)?, end_on(c)?);
    // Both ends ordered from the shared face.
    let from_shared = |x: usize, k: usize| if x == 0 { k } else { n - k };
    let pa_pos: Vec<DVec3> = (0..=n)
        .map(|k| {
            let j = from_shared(xa, k);
            a.point(j, ta[j])
        })
        .collect();
    let pc_pos: Vec<DVec3> = (0..=n)
        .map(|k| {
            let j = from_shared(xc, k);
            c.point(j, tc[j])
        })
        .collect();
    if !bisect && pa_pos[0].distance(pc_pos[0]) > b.tol {
        return Err(refuse(
            v,
            format!(
                "the rails of edges {} and {} do not meet in face {fs}",
                a.edge, c.edge
            ),
        ));
    }
    // Points closer than this are joined: five length tolerances, at most a quarter of a segment.
    let shortest = a.prof[0]
        .distance(a.prof[1])
        .min(c.prof[0].distance(c.prof[1]));
    let join = (5.0 * b.tol).min(0.25 * shortest).max(b.tol);
    let whole = 10.0 * b.tol;
    let ends = join_ends(b, v, &pi, &pa_pos, &pc_pos, join, whole)?;
    let (pa, pc) = (ends.a.clone(), ends.c.clone());
    // The faces. The shared face runs in along the rail of `c` and out along the rail of `a`.
    if pa[0] == pc[0] {
        b.sub(fs, v, vec![pa[0]]);
    } else {
        b.sub(fs, v, vec![pc[0], pa[0]]);
    }
    let (ua, uc) = (pa[n], pc[n]);
    if ua == uc {
        b.sub(fp, v, vec![ua]);
        b.sub(fq, v, vec![ua]);
        b.trims.push(Trim {
            far,
            old: v,
            to: ua,
        });
    } else {
        let (da, dc) = ((pa_pos[n] - vp).dot(udir), (pc_pos[n] - vp).dot(udir));
        if dc > da {
            b.sub(fp, v, vec![ua, uc]);
            b.sub(fq, v, vec![uc]);
            b.trims.push(Trim {
                far,
                old: v,
                to: uc,
            });
        } else {
            b.sub(fp, v, vec![ua]);
            b.sub(fq, v, vec![ua, uc]);
            b.trims.push(Trim {
                far,
                old: v,
                to: ua,
            });
        }
    }
    let looks = vec![fs, fp, fq];
    for e in ends.shared_edges {
        b.blend.insert(e);
    }
    for poly in ends.pieces {
        b.to_orient.push(b.corners.len());
        b.corners.push(NewFace {
            points: poly,
            normal: pi.normal,
            role: Role::Corner { vertex: v },
            looks: looks.clone(),
        });
    }
    // The ends in the order of each profile, with the points between them.
    let profile_end = |x: usize, pts: &[Pt], between: &[Vec<Pt>]| -> End {
        let p: Vec<Pt> = (0..=n).map(|j| pts[from_shared(x, j)]).collect();
        let w: Vec<Vec<Pt>> = (0..n)
            .map(|j| {
                if x == 0 {
                    between[j].clone()
                } else {
                    between[n - 1 - j].iter().rev().copied().collect()
                }
            })
            .collect();
        End {
            pts: p,
            t: Vec::new(),
            between: w,
        }
    };
    let mut end_a = profile_end(xa, &pa, &ends.a_between);
    let mut end_c = profile_end(xc, &pc, &ends.c_between);
    end_a.t = ta;
    end_c.t = tc;
    let (ea, ec) = (strips[sa].end_of(v), strips[sb].end_of(v));
    strips[sa].ends[ea] = Some(end_a);
    strips[sb].ends[ec] = Some(end_c);
    Ok(())
}

/// The two ends of a corner of two chosen edges, joined in their plane.
struct Joined {
    /// The points of the two ends, from the shared face. A point of `c` within the length
    /// tolerance of the point of `a` with the same index is that point.
    a: Vec<Pt>,
    c: Vec<Pt>,
    /// For each side of an end, the points put into it: where the two ends cross, and a corner of
    /// one end that lies on a side of the other.
    a_between: Vec<Vec<Pt>>,
    c_between: Vec<Vec<Pt>>,
    /// Sides that both ends share (a miter).
    shared_edges: Vec<(Pt, Pt)>,
    /// The flat pieces between the ends, each a loop of points (side found later).
    pieces: Vec<Vec<Pt>>,
}

/// Joins the two ends `pa` and `pc` (points in the plane `pi`, from the shared face to the sharp
/// edge). The construction:
///
/// 0. When every point of `c` lies within `whole` of the point of `a` with the same index, the
///    ends become one (the points of `a`).
/// 1. A point of `c` within five length tolerances of the point of `a` with the same index becomes
///    that point (the first points always, as both lie on the corner of the rails).
/// 2. A corner of one end (not the first) within five length tolerances of a side of the other is
///    put into that side.
/// 3. Where a side of one end crosses a side of the other, the crossing point is put into both.
/// 4. The points that both ends hold split them into runs. A run where both ends have no point
///    between is a shared side. Any other run is a flat piece: one end out, the other back.
///
/// A strip whose side gets a point that is not on it is cut into triangles later, so every face
/// stays flat. When the shared points come in another order on the two ends, the corner is
/// refused.
fn join_ends(
    b: &mut Builder<'_>,
    v: VertexId,
    pi: &Plane,
    pa: &[DVec3],
    pc: &[DVec3],
    tol: f64,
    whole: f64,
) -> Result<Joined, FilletError> {
    let n = pa.len() - 1;
    let a: Vec<Pt> = pa.iter().map(|p| b.add(*p)).collect();
    // Ends that lie close everywhere (a small size, or angles that differ a little) become one:
    // the points of `c` are the points of `a`, and the end row of `c` takes the difference. A
    // flat piece between them would be thinner than the tolerances allow.
    let gap = (0..=n).map(|k| pa[k].distance(pc[k])).fold(0.0, f64::max);
    if gap <= whole {
        let shared_edges = (0..n).map(|k| pair(a[k], a[k + 1])).collect();
        return Ok(Joined {
            c: a.clone(),
            a,
            a_between: vec![Vec::new(); n],
            c_between: vec![Vec::new(); n],
            shared_edges,
            pieces: Vec::new(),
        });
    }
    // 1. The points of `c` that are points of `a`.
    let mut c: Vec<Pt> = Vec::with_capacity(n + 1);
    let mut cpos: Vec<DVec3> = Vec::with_capacity(n + 1);
    let mut used: BTreeSet<usize> = BTreeSet::new();
    for k in 0..=n {
        let same = (pa[k].distance(pc[k]) <= tol).then_some(k);
        let other = |used: &BTreeSet<usize>| {
            (1..n)
                .filter(|i| !used.contains(i) && pa[*i].distance(pc[k]) <= tol)
                .min_by(|x, y| pa[*x].distance(pc[k]).total_cmp(&pa[*y].distance(pc[k])))
        };
        let same = same.filter(|i| !used.contains(i));
        // The last points lie on the sharp edge: they join only each other.
        match same.or_else(|| if k > 0 && k < n { other(&used) } else { None }) {
            Some(i) => {
                used.insert(i);
                c.push(a[i]);
                cpos.push(pa[i]);
            }
            None => {
                c.push(b.add(pc[k]));
                cpos.push(pc[k]);
            }
        }
    }
    // The chains: (point, position), the corners of each end first.
    let mut ca: Vec<(Pt, DVec3)> = a.iter().copied().zip(pa.iter().copied()).collect();
    let mut cc: Vec<(Pt, DVec3)> = c.iter().copied().zip(cpos.iter().copied()).collect();
    // 2. A corner of one end on a side of the other goes into that side.
    let put_on = |to: &mut Vec<(Pt, DVec3)>, from: &[(Pt, DVec3)]| {
        for &(p, x) in from.iter().skip(1) {
            if to.iter().any(|q| q.0 == p) {
                continue;
            }
            let at = (0..to.len() - 1).find(|j| {
                let (d, t) = geom::point_segment(x, to[*j].1, to[*j + 1].1);
                d <= tol && t > 1e-9 && t < 1.0 - 1e-9
            });
            if let Some(j) = at {
                to.insert(j + 1, (p, x));
            }
        }
    };
    let snapshot_a = ca.clone();
    put_on(&mut cc, &snapshot_a);
    let snapshot_c = cc.clone();
    put_on(&mut ca, &snapshot_c);
    // 3. Where the chains cross, the crossing goes into both.
    let flat = |p: DVec3| pi.project(p);
    let mut cross_a: Vec<(usize, f64, Pt, DVec3)> = Vec::new();
    let mut cross_c: Vec<(usize, f64, Pt, DVec3)> = Vec::new();
    for i in 0..ca.len() - 1 {
        for j in 0..cc.len() - 1 {
            let (s0, s1, r0, r1) = (ca[i], ca[i + 1], cc[j], cc[j + 1]);
            if s0.0 == r0.0 || s0.0 == r1.0 || s1.0 == r0.0 || s1.0 == r1.0 {
                continue;
            }
            let (p, q) = (flat(s0.1), flat(s1.1));
            let (r, w) = (flat(r0.1), flat(r1.1));
            let (d1, d2) = (q - p, w - r);
            let den = d1.perp_dot(d2);
            if den.abs() <= 1e-12 * d1.length() * d2.length() {
                continue;
            }
            let t = (r - p).perp_dot(d2) / den;
            let u = (r - p).perp_dot(d1) / den;
            if !(t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9) {
                continue;
            }
            let x = s0.1 + (s1.1 - s0.1) * t;
            let pt = b.add(x);
            cross_a.push((i, t, pt, x));
            cross_c.push((j, u, pt, x));
        }
    }
    let insert = |chain: &mut Vec<(Pt, DVec3)>, mut crossings: Vec<(usize, f64, Pt, DVec3)>| {
        crossings.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.total_cmp(&x.1)));
        for (i, _, p, x) in crossings {
            chain.insert(i + 1, (p, x));
        }
    };
    insert(&mut ca, cross_a);
    insert(&mut cc, cross_c);
    // The points between the corners of each end.
    let between = |chain: &[(Pt, DVec3)], ends: &[Pt]| -> Vec<Vec<Pt>> {
        let mut out = vec![Vec::new(); n];
        let mut k = 0;
        for &(p, _) in chain.iter().skip(1) {
            if k < n && p == ends[k + 1] {
                k += 1;
            } else if k < n {
                out[k].push(p);
            }
        }
        out
    };
    let a_between = between(&ca, &a);
    let c_between = between(&cc, &c);
    let ca: Vec<Pt> = ca.into_iter().map(|x| x.0).collect();
    let cc: Vec<Pt> = cc.into_iter().map(|x| x.0).collect();
    let in_c: BTreeSet<Pt> = cc.iter().copied().collect();
    let shared_a: Vec<usize> = (0..ca.len()).filter(|i| in_c.contains(&ca[*i])).collect();
    let pos_c: BTreeMap<Pt, usize> = cc.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    let shared_c: Vec<usize> = shared_a.iter().map(|i| pos_c[&ca[*i]]).collect();
    if shared_c.windows(2).any(|w| w[0] >= w[1]) {
        return Err(refuse(
            v,
            "the ends of the two rounds cross each other in an order that gives no flat piece"
                .into(),
        ));
    }
    let mut shared_edges = Vec::new();
    let mut pieces = Vec::new();
    // The runs between the anchors: the start of both ends, each shared point, the ends of both.
    // A run is one end out and the other back; the closing sides at the start and at the end lie
    // in the shared face and on the sharp edge.
    let (la, lc) = (ca.len() - 1, cc.len() - 1);
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    if shared_a.first() != Some(&0) || shared_c.first() != Some(&0) {
        anchors.push((0, 0));
    }
    anchors.extend(shared_a.iter().copied().zip(shared_c.iter().copied()));
    if anchors.last() != Some(&(la, lc)) {
        anchors.push((la, lc));
    }
    for w in anchors.windows(2) {
        let ((i0, j0), (i1, j1)) = (w[0], w[1]);
        if i1 == i0 + 1 && j1 == j0 + 1 && ca[i0] == cc[j0] && ca[i1] == cc[j1] {
            shared_edges.push(pair(ca[i0], ca[i1]));
            continue;
        }
        let mut poly: Vec<Pt> = cc[j0..=j1].to_vec();
        poly.extend(ca[i0..=i1].iter().rev().copied());
        poly.dedup();
        while poly.len() > 1 && poly.first() == poly.last() {
            poly.pop();
        }
        if poly.len() >= 3 {
            pieces.push(poly);
        }
    }
    Ok(Joined {
        a,
        c,
        a_between,
        c_between,
        shared_edges,
        pieces,
    })
}

/// Three rounded edges: the strips end at the cross sections through the centre of the ball that
/// touches the three planes, and triangles on the ball close the corner.
fn three_round(
    b: &mut Builder<'_>,
    strips: &mut [Strip],
    v: VertexId,
    at: &Around,
    sel: &[Option<usize>],
    r: f64,
) -> Result<(), FilletError> {
    let convex = strips[sel[0].expect("three chosen")].convex;
    let sign = if convex { -1.0 } else { 1.0 };
    let planes: Vec<Plane> = at.faces.iter().map(|f| b.shape.faces[f].plane).collect();
    let mtx = DMat3::from_cols(planes[0].normal, planes[1].normal, planes[2].normal).transpose();
    if mtx.determinant().abs() < 1e-9 {
        return Err(refuse(
            v,
            "the three faces there are nearly parallel to one line".into(),
        ));
    }
    let rhs = DVec3::new(
        planes[0].offset + sign * r,
        planes[1].offset + sign * r,
        planes[2].offset + sign * r,
    );
    let x = mtx.inverse() * rhs;
    let mut face_pt: BTreeMap<FaceId, Pt> = BTreeMap::new();
    let mut side_pts: Vec<Vec<Pt>> = Vec::with_capacity(3);
    for chosen in sel.iter().take(3) {
        let si = chosen.expect("three chosen");
        let s = &strips[si];
        let n = s.n();
        let t = (x - s.at).dot(s.d);
        let mut pts = Vec::with_capacity(n + 1);
        for j in 0..=n {
            let p = s.point(j, t);
            let pt = if j == 0 || j == n {
                let f = s.faces[usize::from(j == n)];
                match face_pt.get(&f) {
                    Some(q) => {
                        if b.pos(*q).distance(p) > b.tol {
                            return Err(refuse(v, format!("the rounds do not meet in face {f}")));
                        }
                        *q
                    }
                    None => {
                        let q = b.add(p);
                        face_pt.insert(f, q);
                        q
                    }
                }
            } else {
                b.add(p)
            };
            pts.push(pt);
        }
        side_pts.push(pts.clone());
        let e = strips[si].end_of(v);
        strips[si].ends[e] = Some(End {
            pts,
            t: vec![t; n + 1],
            between: vec![Vec::new(); n],
        });
    }
    // A side from the corner in face `from`: the end of strip `k` in that order.
    let side = |k: usize, from: FaceId| -> Vec<Pt> {
        let s = &strips[sel[k].expect("three chosen")];
        let mut p = side_pts[k].clone();
        if s.faces[0] != from {
            p.reverse();
        }
        p
    };
    let (f0, f1) = (at.faces[0], at.faces[1]);
    // Edge 0 lies between faces 0 and 1, edge 1 between 1 and 2, edge 2 between 2 and 0.
    let ab = side(0, f0);
    let ac = side(2, f0);
    let bc = side(1, f1);
    let n = ab.len() - 1;
    let mut rows: Vec<Vec<Pt>> = Vec::with_capacity(n + 1);
    rows.push(vec![ab[0]]);
    for i in 1..n {
        let (pb, pc) = (b.pos(ab[i]), b.pos(ac[i]));
        let (db, dc) = ((pb - x).normalize(), (pc - x).normalize());
        let mut row = vec![ab[i]];
        for j in 1..i {
            let p = x + slerp(db, dc, j as f64 / i as f64) * r;
            row.push(b.add(p));
        }
        row.push(ac[i]);
        rows.push(row);
    }
    rows.push(bc);
    // A ball corner of a small radius with many segments has triangles below the tolerance.
    for i in 0..n {
        for j in 0..=i {
            let (p, q) = (b.pos(rows[i][j]), b.pos(rows[i + 1][j]));
            let w = b.pos(rows[i + 1][j + 1]);
            if p.distance(q) <= b.tol || q.distance(w) <= b.tol {
                let s = &strips[sel[0].expect("three chosen")];
                return Err(FilletError::TooSmall {
                    noun: "radius",
                    asked: r,
                    segments: n as u32,
                    edge: s.edge,
                });
            }
        }
    }
    let looks = at.faces.clone();
    let tri = |b: &mut Builder<'_>, p: [Pt; 3]| {
        let pos: Vec<DVec3> = p.iter().map(|q| b.pos(*q)).collect();
        let mut normal = geom::newell(&pos).normalize_or_zero();
        let mid = (pos[0] + pos[1] + pos[2]) / 3.0;
        let mut pts = p.to_vec();
        if normal.dot(mid - x) * -sign < 0.0 {
            pts.reverse();
            normal = -normal;
        }
        for k in 0..3 {
            b.blend.insert(pair(pts[k], pts[(k + 1) % 3]));
        }
        b.corners.push(NewFace {
            points: pts,
            normal,
            role: Role::Corner { vertex: v },
            looks: looks.clone(),
        });
    };
    for i in 0..n {
        for j in 0..=i {
            tri(b, [rows[i][j], rows[i + 1][j], rows[i + 1][j + 1]]);
        }
        for j in 0..i {
            tri(b, [rows[i][j], rows[i + 1][j + 1], rows[i][j + 1]]);
        }
    }
    for (f, p) in face_pt {
        b.sub(f, v, vec![p]);
    }
    Ok(())
}

/// Three cut edges: each face gets the corner of its two rails, and a triangle closes the corner.
fn three_flat(
    b: &mut Builder<'_>,
    strips: &mut [Strip],
    v: VertexId,
    at: &Around,
    sel: &[Option<usize>],
) -> Result<(), FilletError> {
    let vp = b.shape.points[&v];
    let mut face_pt: BTreeMap<FaceId, Pt> = BTreeMap::new();
    for i in 0..3 {
        let f = at.faces[i];
        let plane = b.shape.faces[&f].plane;
        // Face i lies between edge i - 1 and edge i.
        let (s1, s2) = (
            &strips[sel[(i + 2) % 3].expect("three chosen")],
            &strips[sel[i].expect("three chosen")],
        );
        let (Some(x1), Some(x2)) = (s1.side_of(f), s2.side_of(f)) else {
            unreachable!("both edges lie on the face");
        };
        let (m1, m2) = (s1.inward[x1], s2.inward[x2]);
        let (e1, e2) = plane.basis();
        let det = m1.dot(e1) * m2.dot(e2) - m1.dot(e2) * m2.dot(e1);
        if det.abs() < 1e-9 {
            return Err(refuse(
                v,
                format!(
                    "edges {} and {} run in one line in face {f}",
                    s1.edge, s2.edge
                ),
            ));
        }
        let mu1 = (s1.s * m2.dot(e2) - m1.dot(e2) * s2.s) / det;
        let mu2 = (m1.dot(e1) * s2.s - m2.dot(e1) * s1.s) / det;
        let p = vp + e1 * mu1 + e2 * mu2;
        let pt = b.add(p);
        face_pt.insert(f, pt);
        b.sub(f, v, vec![pt]);
    }
    for chosen in sel.iter().take(3) {
        let si = chosen.expect("three chosen");
        let s = &strips[si];
        let pts = vec![face_pt[&s.faces[0]], face_pt[&s.faces[1]]];
        let t: Vec<f64> = pts.iter().map(|p| (b.pos(*p) - s.at).dot(s.d)).collect();
        let e = s.end_of(v);
        strips[si].ends[e] = Some(End {
            pts,
            t,
            between: vec![Vec::new()],
        });
    }
    let pts: Vec<Pt> = at.faces.iter().map(|f| face_pt[f]).collect();
    let pos: Vec<DVec3> = pts.iter().map(|p| b.pos(*p)).collect();
    b.to_orient.push(b.corners.len());
    b.corners.push(NewFace {
        points: pts,
        normal: geom::newell(&pos).normalize_or_zero(),
        role: Role::Corner { vertex: v },
        looks: at.faces.clone(),
    });
    Ok(())
}

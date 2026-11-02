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
    strips[si].ends[e] = Some(End { pts, t: ts });
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
    let normal = if det.abs() > 1e-9 {
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
    if pa_pos[0].distance(pc_pos[0]) > b.tol {
        return Err(refuse(
            v,
            format!(
                "the rails of edges {} and {} do not meet in face {fs}",
                a.edge, c.edge
            ),
        ));
    }
    let pinch: Vec<bool> = (0..=n)
        .map(|k| k == 0 || pa_pos[k].distance(pc_pos[k]) <= b.tol)
        .collect();
    let pa: Vec<Pt> = pa_pos.iter().map(|p| b.add(*p)).collect();
    let pc: Vec<Pt> = (0..=n)
        .map(|k| if pinch[k] { pa[k] } else { b.add(pc_pos[k]) })
        .collect();
    // The faces.
    b.sub(fs, v, vec![pa[0]]);
    let (ua, uc) = (pa[n], pc[n]);
    if pinch[n] {
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
    // Flat pieces in the plane where the two ends part.
    let looks = vec![fs, fp, fq];
    let mut k = 0;
    while k < n {
        if pinch[k + 1] {
            b.blend.insert(pair(pa[k], pa[k + 1]));
            k += 1;
            continue;
        }
        let mut j = k + 1;
        while j <= n && !pinch[j] {
            j += 1;
        }
        let mut poly = vec![pa[k]];
        if j <= n {
            poly.extend_from_slice(&pc[k + 1..j]);
            poly.push(pa[j]);
            poly.extend(pa[k + 1..j].iter().rev());
        } else {
            poly.extend_from_slice(&pc[k + 1..=n]);
            poly.extend(pa[k + 1..=n].iter().rev());
        }
        b.to_orient.push(b.corners.len());
        b.corners.push(NewFace {
            points: poly,
            normal: pi.normal,
            role: Role::Corner { vertex: v },
            looks: looks.clone(),
        });
        k = j;
    }
    // The ends in the order of each profile.
    let ends_a: Vec<Pt> = (0..=n).map(|j| pa[from_shared(xa, j)]).collect();
    let ends_c: Vec<Pt> = (0..=n).map(|j| pc[from_shared(xc, j)]).collect();
    let (ea, ec) = (strips[sa].end_of(v), strips[sb].end_of(v));
    strips[sa].ends[ea] = Some(End { pts: ends_a, t: ta });
    strips[sb].ends[ec] = Some(End { pts: ends_c, t: tc });
    Ok(())
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
    for k in 0..3 {
        let si = sel[k].expect("three chosen");
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
    for k in 0..3 {
        let si = sel[k].expect("three chosen");
        let s = &strips[si];
        let pts = vec![face_pt[&s.faces[0]], face_pt[&s.faces[1]]];
        let t: Vec<f64> = pts.iter().map(|p| (b.pos(*p) - s.at).dot(s.d)).collect();
        let e = s.end_of(v);
        strips[si].ends[e] = Some(End { pts, t });
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

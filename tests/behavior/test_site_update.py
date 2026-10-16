"""Scenario: a surveyor sends a new revision of the topographic survey (IOP-23). The loci and 3D
polylines of the first revision went into the site model (`site.create` moves them in). The
update must change the sources inside the model, so the terrain and the cut and fill follow."""

from pathlib import Path

FIXTURES = Path(__file__).parent / "fixtures"
REV1 = FIXTURES / "survey_rev1.dxf"
REV2 = FIXTURES / "survey_rev2.dxf"
# The second file says it is another drawing (its own GUID): the person confirms the lineage.
DRAWING = "guid:{6D6B37CD-4A03-47AD-B02E-199CB5C271A1}"


def make_site(cad):
    """Import revision 1 and make the site model from its 3D loci and 3D polylines. A pad cuts and
    fills part of it, so that the cut and fill depend on the terrain."""
    cad.result("import.file", {"path": str(REV1), "options": {"layer_mode": "classes"}})
    ids = cad.result("objects.query", {"criteria": 'Kind = "locus3d" OR Kind = "polyline3d"'})["ids"]
    assert ids, "the survey has 3D sources"
    site = cad.result("site.create", {"source_ids": ids})["id"]
    shape = cad.result("objects.get", {"id": site})["shape"]
    xs = [p[0] for p in shape["points"]] + [p[0] for l in shape["lines"] for p in l["points"]]
    ys = [p[1] for p in shape["points"]] + [p[1] for l in shape["lines"] for p in l["points"]]
    cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
    cad.result("pad.add", {
        "boundary": [[cx - 15000, cy - 15000], [cx + 15000, cy - 15000], [cx + 15000, cy + 15000], [cx - 15000, cy + 15000]],
        "elevation": 36000,
        "layer": cad.result("objects.get", {"id": site})["layer"],  # a pad acts on the models of its layer
    })
    return site


def counts(cad, site):
    shape = cad.result("objects.get", {"id": site})["shape"]
    return len(shape["points"]), len(shape["lines"])


def test_the_sources_keep_the_tags_of_the_objects_they_came_from(cad):
    site = make_site(cad)
    data = cad.result("objects.get", {"id": site})["data"]["import_sources"]
    shape = cad.result("objects.get", {"id": site})["shape"]
    assert len(data["points"]) == len(shape["points"])
    assert len(data["lines"]) == len(shape["lines"])
    assert all(r["tag"]["kind"] in ("POINT", "POLYLINE", "LWPOLYLINE") for r in data["points"] + data["lines"])
    layers = {r["dxf_layer"] for r in data["points"] + data["lines"]}
    assert "SPOT_LEVELS" in layers and any(l.startswith("CONTOURS") for l in layers)
    # A copy of the site model is new work: it has no tags.
    copy = cad.result("execute", {"command": {"op": "copy_objects", "ids": [site], "delta": [0, 1000]}})
    assert "import_sources" not in cad.result("objects.get", {"id": copy["created"][0]}).get("data", {})


def test_an_update_changes_the_sources_in_the_model_and_the_cut_and_fill_follow(cad):
    site = make_site(cad)
    points0, lines0 = counts(cad, site)
    before = cad.result("site.cut_fill", {"id": site})
    up = cad.result("import.update", {"path": str(REV2), "drawing": DRAWING})
    assert up["counts"]["added"] >= 2, up["counts"]  # two new spot levels
    assert up["counts"]["changed"] >= 1, up["counts"]  # the re-surveyed contour
    assert up["counts"]["conflicts"] == 0
    sources = [c for c in up["changes"] if c["entity"] in ("POINT", "POLYLINE")]
    assert sources, "changes of sources are listed"
    listed = {i for c in sources for i in c["object_ids"]}
    assert listed <= {site}, "the changes name the site model, not objects that do not exist"
    cad.result("import.apply", {"update_id": up["update_id"], "accept": "all"})
    # No loose loci or 3D polylines: the new sources are inside the model.
    loose = cad.result("objects.query", {"criteria": 'Kind = "locus3d" OR Kind = "polyline3d"'})["ids"]
    assert loose == []
    points1, lines1 = counts(cad, site)
    assert points1 == points0 + 2 and lines1 == lines0
    after = cad.result("site.cut_fill", {"id": site})
    assert after["cut_m3"] != before["cut_m3"] or after["fill_m3"] != before["fill_m3"], (before, after)
    # The tags follow: the revision of the changed sources is the new one.
    data = cad.result("objects.get", {"id": site})["data"]["import_sources"]
    revisions = {r["tag"]["revision"] for r in data["points"] + data["lines"]}
    assert len(revisions) == 2, "the sources that changed have the new revision, the others the old one"
    # The same update again finds nothing to do.
    again = cad.result("import.update", {"path": str(REV2), "drawing": DRAWING, "dry_run": True})
    assert again["counts"]["added"] == 0 and again["counts"]["changed"] == 0 and again["counts"]["removed"] == 0
    # Undo brings the model back in one step.
    assert cad.result("undo")["done"]
    assert counts(cad, site) == (points0, lines0)


def test_a_source_that_the_person_edited_is_a_conflict(cad):
    site = make_site(cad)
    shape = cad.result("objects.get", {"id": site})["shape"]
    target = next(i for i, l in enumerate(shape["lines"]) if abs(l["points"][0][2] - 35000) < 1)
    original = shape["lines"][target]["points"]
    shape["lines"][target]["points"] = [[p[0] + 300, p[1], p[2]] for p in original]
    cad.result("execute", {"command": {"op": "modify_object", "id": site, "shape": shape}})
    up = cad.result("import.update", {"path": str(REV2), "drawing": DRAWING})
    assert up["counts"]["conflicts"] == 1, up["counts"]
    assert up["conflicts"], "the conflict is listed"
    # The default keeps the edit.
    cad.result("import.apply", {"update_id": up["update_id"], "accept": "all"})
    kept = cad.result("objects.get", {"id": site})["shape"]["lines"][target]["points"]
    assert kept[0][0] == original[0][0] + 300
    # The new spot levels came in anyway.
    assert counts(cad, site)[0] > len(shape["points"])
    # Asking for the overwrite replaces the edited source (a new update of the same file, with the
    # revision of the first one as the old side, is not needed: undo, then update again).
    assert cad.result("undo")["done"]
    up = cad.result("import.update", {"path": str(REV2), "drawing": DRAWING})
    cad.result("import.apply", {"update_id": up["update_id"], "accept": "all", "conflicts": "overwrite"})
    over = cad.result("objects.get", {"id": site})["shape"]["lines"][target]["points"]
    assert over[0][0] != original[0][0] + 300


def test_site_update_replaces_a_source_instead_of_adding_one_next_to_it(cad):
    site = make_site(cad)
    p0, l0 = counts(cad, site)
    new = cad.result("execute", {"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 2000, 3000]}}})["created"][0]
    r = cad.result("site.update", {"id": site, "replace": [{"point": 0, "with": new}]})
    assert (r["points"], r["lines"]) == (p0, l0)
    shape = cad.result("objects.get", {"id": site})["shape"]
    assert shape["points"][0] == [1000, 2000, 3000]
    assert cad.result("objects.query", {"criteria": 'Kind = "locus3d"'})["ids"] == []
    # A line cannot replace a point, and an index must exist.
    line = cad.result("execute", {"command": {"op": "add_object", "shape": {"kind": "polyline3d", "points": [[0, 0, 0], [1, 1, 1]], "closed": False}}})["created"][0]
    assert "cannot replace a point" in cad.error("site.update", {"id": site, "replace": [{"point": 0, "with": line}]})["message"]
    assert "out of range" in cad.error("site.update", {"id": site, "replace": [{"point": 99999, "with": new}]})["message"]
    assert "one of them" in cad.error("site.update", {"id": site, "replace": [{"with": line}]})["message"]

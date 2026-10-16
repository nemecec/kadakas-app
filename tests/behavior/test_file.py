"""Scenario: a designer saves a drawing, closes the tool, and opens the drawing again in a fresh process."""

import json

from helpers import plant_cmd, read_doc, rect_cmd, uid


def test_save_then_open_in_a_fresh_process_round_trips(make_serve, tmp_path):
    path = tmp_path / "garden.kad"
    first = make_serve()
    site = first.execute({"op": "add_layer", "name": "Site", "elevation": 150})[0]
    first.execute({"op": "set_active", "layer": site})
    first.execute(rect_cmd(0, 0, 3000, 1200, **{"class": uid(0)}))
    first.execute({"op": "add_class", "name": "Paths"})
    first.execute(plant_cmd(500, 500, species="Ilex aquifolium"))
    saved = first.result("file.save", {"path": str(path)})
    assert saved == {"ok": True, "format": "container"}
    expected = first.doc()

    second = make_serve()
    assert second.result("file.open", {"path": str(path)}) == {"ok": True, "format": "container"}
    assert second.doc() == expected


def test_saved_file_is_the_document_json(make_serve, tmp_path):
    path = tmp_path / "doc.kad"
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.result("file.save", {"path": str(path)})
    on_disk = read_doc(path)
    assert on_disk == cad.doc()
    assert on_disk["format_version"] == 3


def test_serve_with_a_file_argument_opens_it_at_start(make_serve, tmp_path):
    path = tmp_path / "start.kad"
    writer = make_serve()
    writer.execute(plant_cmd(0, 0, species="Betula pendula"))
    writer.result("file.save", {"path": str(path)})

    reader = make_serve(path)
    assert reader.result("report.plants")["rows"] == [{"species": "Betula pendula", "count": 1}]


def test_opening_a_file_clears_undo_history(make_serve, tmp_path):
    path = tmp_path / "d.kad"
    writer = make_serve()
    writer.execute(rect_cmd(0, 0, 1, 1))
    writer.result("file.save", {"path": str(path)})

    cad = make_serve()
    cad.execute(rect_cmd(5, 5, 6, 6))
    cad.result("file.open", {"path": str(path)})
    assert cad.result("document.summary")["can_undo"] is False
    assert cad.result("undo") == {"done": False}


def test_document_with_newer_format_version_is_refused_and_session_is_kept(make_serve, tmp_path):
    future = tmp_path / "future.kad"
    doc = make_serve().doc()
    doc["format_version"] = 999
    future.write_text(json.dumps(doc))

    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 1, 1))
    before = cad.doc()
    error = cad.error("file.open", {"path": str(future)})
    assert error["code"] == 3
    assert "999" in error["message"]
    assert cad.doc() == before, "a refused file must not replace the open document"


def test_document_load_with_newer_format_version_is_refused(cad):
    doc = cad.doc()
    doc["format_version"] = 999
    error = cad.error("document.load", {"document": doc})
    assert error["code"] == 1
    assert "newer" in error["message"]


def test_opening_a_missing_file_is_an_application_error(cad, tmp_path):
    error = cad.error("file.open", {"path": str(tmp_path / "missing.kad")})
    assert error["code"] == 4


def test_opening_a_file_that_is_not_a_document_is_an_application_error(cad, tmp_path):
    junk = tmp_path / "junk.kad"
    junk.write_text("[1, 2, 3]")
    error = cad.error("file.open", {"path": str(junk)})
    assert error["code"] == 3


def test_saving_without_a_path_is_invalid_params(cad):
    assert cad.error("file.save", {})["code"] == -32602


def test_saving_into_a_missing_directory_is_an_application_error(cad, tmp_path):
    error = cad.error("file.save", {"path": str(tmp_path / "no" / "such" / "dir.kad")})
    assert error["code"] == 4
    assert not (tmp_path / "no").exists()


def test_a_file_over_the_session_limit_is_refused_and_the_error_says_how_to_raise_it(make_serve, tmp_path):
    big = tmp_path / "big.kad"
    with open(big, "wb") as f:
        f.truncate(2 * 1024 * 1024 + 1)
    limited = make_serve("--max-document-mib", 2)
    err = limited.error("file.open", {"path": str(big)})
    assert "limit" in err["message"] and "--max-document-mib" in err["message"]
    # The default limit is 512 MiB: the same file is read, and it is not a document.
    default = make_serve()
    err = default.error("file.open", {"path": str(big)})
    assert "limit" not in err["message"]

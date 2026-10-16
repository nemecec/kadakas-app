"""Scenario: the file is a ZIP container (DOC-20) or one readable JSON file (DOC-22); both load the
same document. The program also reads hostile containers without trouble, and `kadakas convert`
goes from one form to the other."""

import base64
import hashlib
import json
import struct
import zipfile
import zlib

from helpers import plant_cmd, read_doc, rect_cmd, run_cli


def png(width, height, seed=0):
    """A PNG file with noisy pixels, so that it does not compress and its text is long."""
    rows = bytearray()
    state = 12345 + seed
    for _ in range(height):
        rows.append(0)
        for _ in range(width * 3):
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            rows.append((state >> 16) & 255)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(rows), 0))
        + chunk(b"IEND", b"")
    )


def with_picture(cad, name="Field maple", seed=0):
    """A plant style with a picture (an image resource, which a container keeps as a blob).
    Returns the id of the style and what the program says about its picture."""
    sid = cad.result("plant_style.create", {"name": name, "category": "tree", "spread": 7000})["id"]
    b64 = base64.b64encode(png(40, 40, seed)).decode()
    cad.result("plant_style.set_picture", {"id": sid, "base64": b64, "name": "maple.png"})
    return sid, picture_of(cad, sid)


def picture_of(cad, sid):
    got = cad.result("plant_style.get_picture", {"id": sid, "bytes": True})
    return got["picture"]["sha256"], got["base64"]


def test_the_default_file_is_a_zip_with_a_manifest_that_any_zip_tool_reads(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    with_picture(cad)
    path = tmp_path / "plan.kad"
    assert cad.result("file.save", {"path": str(path)}) == {"ok": True, "format": "container"}
    assert path.read_bytes().startswith(b"PK\x03\x04")
    with zipfile.ZipFile(path) as z:
        assert z.testzip() is None
        names = z.namelist()
        assert "document.json" in names and "manifest.json" in names
        manifest = json.loads(z.read("manifest.json"))
        assert manifest["format"] == "kadakas-container" and manifest["format_version"] == 1
        listed = {e["name"]: e for e in manifest["entries"]}
        # Every entry but the manifest is listed with its size and SHA-256.
        assert set(listed) == set(names) - {"manifest.json"}
        for name, e in listed.items():
            data = z.read(name)
            assert e["size"] == len(data)
            assert e["sha256"] == hashlib.sha256(data).hexdigest()
        # The picture is a blob named by the hash of its text, stored (not compressed) once.
        blobs = [n for n in names if n.startswith("blobs/")]
        assert len(blobs) == 1
        assert blobs[0] == "blobs/" + hashlib.sha256(z.read(blobs[0])).hexdigest()
        assert z.getinfo(blobs[0]).compress_type == zipfile.ZIP_STORED
        assert z.getinfo("document.json").compress_type == zipfile.ZIP_DEFLATED
        # The drawing holds a reference and not the picture.
        document = z.read("document.json").decode()
        assert '"$blob"' in document
        assert z.read(blobs[0]).decode()[:300] not in document


def test_the_two_forms_load_the_same_document(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    cad.execute(plant_cmd(100, 100, species="Betula pendula"))
    sid, picture = with_picture(cad)
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    k = tmp_path / "a.kad"
    j = tmp_path / "a.json"
    assert cad.result("file.save", {"path": str(k)})["format"] == "container"
    # The extension decides the form, and `format` overrides it.
    assert cad.result("file.save", {"path": str(j)}) == {"ok": True, "format": "json"}
    forced = tmp_path / "forced.kad"
    assert cad.result("file.save", {"path": str(forced), "format": "json"})["format"] == "json"
    assert forced.read_bytes().lstrip().startswith(b"{")
    assert cad.result("file.save", {"path": str(tmp_path / "x.json"), "format": "container"})["format"] == "container"
    text = j.read_text()
    assert text.startswith("{") and '"history"' in text and "$blob" not in text
    assert json.loads(text)["format_version"] == 3

    docs = []
    for f in (k, j, forced):
        other = make_serve()
        r = other.result("file.open", {"path": str(f)})
        assert r["ok"] is True and "notes" not in r
        assert r["format"] == ("container" if f == k else "json")
        assert picture_of(other, sid) == picture
        assert [v["name"] for v in other.result("version.list")["versions"]] == ["One"]
        docs.append(other.doc())
    # (The saved drawing holds the stacking keys of the versions, so it is not byte for byte the
    # live document.)
    assert docs[0] == docs[1] == docs[2]


def test_a_bad_format_is_a_parameter_error(cad, tmp_path):
    for bad in ("zip2", 5, ""):
        err = cad.error("file.save", {"path": str(tmp_path / "x.kad"), "format": bad})
        assert err["code"] == -32602, bad
    assert not (tmp_path / "x.kad").exists()


def test_convert_goes_both_ways_and_keeps_pictures_and_versions(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    with_picture(cad)
    cad.result("version.create", {"name": "One"})
    cad.execute(rect_cmd(20, 0, 30, 10))
    a = tmp_path / "a.kad"
    cad.result("file.save", {"path": str(a)})
    j = tmp_path / "a.json"
    p = run_cli("convert", str(a), str(j))
    assert p.returncode == 0, p.stderr
    assert json.loads(p.stdout)["format"] == "json"
    assert j.read_bytes().lstrip().startswith(b"{")
    b = tmp_path / "b.kad"
    p = run_cli("convert", str(j), str(b))
    assert p.returncode == 0, p.stderr
    assert b.read_bytes().startswith(b"PK")
    # container -> json -> container: the same document and the same history.
    assert read_doc(a) == read_doc(b) == json.loads(j.read_text())
    other = make_serve(b)
    assert other.doc() == make_serve(a).doc()
    assert [v["name"] for v in other.result("version.list")["versions"]] == ["One"]
    # The input is never changed.
    before = a.read_bytes()
    run_cli("convert", str(a), str(tmp_path / "c.json"))
    assert a.read_bytes() == before
    # The flag decides the form.
    p = run_cli("convert", str(a), str(tmp_path / "d.kad"), "--format", "json")
    assert p.returncode == 0 and (tmp_path / "d.kad").read_bytes().startswith(b"{")
    p = run_cli("convert", str(a), str(tmp_path / "e.kad"), "--format", "nope")
    assert p.returncode == 1 and "--format" in p.stderr
    p = run_cli("convert", str(tmp_path / "missing.kad"), str(tmp_path / "f.json"))
    assert p.returncode == 1 and "missing.kad" in p.stderr


def test_a_second_save_over_an_opened_file_keeps_its_pictures(make_serve, tmp_path):
    cad = make_serve()
    sid, picture = with_picture(cad)
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    work = make_serve(path)
    work.execute(rect_cmd(0, 0, 5, 5))
    work.result("file.save", {"path": str(path)})
    work.execute(rect_cmd(6, 0, 11, 5))
    work.result("file.save", {"path": str(path)})
    # The picture was copied from file to file without being decoded, and is still there.
    assert (tmp_path / "plan.kad.bak").exists()
    assert picture_of(work, sid) == picture
    again = make_serve(path)
    assert picture_of(again, sid) == picture
    assert again.result("document.summary")["object_count"] == 2
    with zipfile.ZipFile(path) as z:
        assert len([n for n in z.namelist() if n.startswith("blobs/")]) == 1


def manifest_of(entries):
    return json.dumps(
        {
            "format": "kadakas-container",
            "format_version": 1,
            "entries": [
                {"name": n, "size": len(d), "sha256": hashlib.sha256(d).hexdigest()} for n, d in entries.items()
            ],
        }
    ).encode()


def write_zip(path, entries, manifest=True, method=zipfile.ZIP_DEFLATED):
    with zipfile.ZipFile(path, "w", method) as z:
        for name, data in entries.items():
            z.writestr(name, data)
        if manifest:
            z.writestr("manifest.json", manifest_of(entries))


def test_hostile_containers_are_errors_that_name_the_problem(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 1, 1))
    good = tmp_path / "good.kad"
    cad.result("file.save", {"path": str(good)})
    document = zipfile.ZipFile(good).read("document.json")
    before = cad.doc()

    def refused(name, needle):
        err = cad.error("file.open", {"path": str(tmp_path / name)})
        assert err["code"] == 3, (name, err)
        assert needle in err["message"], (name, err["message"])
        assert name in err["message"], "the message names the file"
        assert cad.doc() == before, "a refused file must not replace the open document"

    # Cut short: the end of the central directory is gone.
    raw = good.read_bytes()
    (tmp_path / "cut.kad").write_bytes(raw[: len(raw) // 2])
    refused("cut.kad", "central directory")
    # A bit flipped in the middle of the drawing.
    flipped = bytearray(raw)
    flipped[80] ^= 0xFF
    (tmp_path / "flipped.kad").write_bytes(bytes(flipped))
    err = cad.error("file.open", {"path": str(tmp_path / "flipped.kad")})
    assert err["code"] == 3 and "document.json" in err["message"]
    # No manifest.
    write_zip(tmp_path / "nomanifest.kad", {"document.json": document}, manifest=False)
    refused("nomanifest.kad", "manifest")
    # An entry that the manifest does not list.
    entries = {"document.json": document}
    with zipfile.ZipFile(tmp_path / "extra.kad", "w") as z:
        z.writestr("document.json", document)
        z.writestr("blobs/extra", b"x")
        z.writestr("manifest.json", manifest_of(entries))
    refused("extra.kad", "blobs/extra")
    # A name that leaves the container.
    for bad in ("../evil", "/abs", "a/../b", "a\\b"):
        e = {"document.json": document, bad: b"x"}
        with zipfile.ZipFile(tmp_path / "names.kad", "w") as z:
            for n, d in e.items():
                z.writestr(zipfile.ZipInfo(n), d)
            z.writestr("manifest.json", manifest_of(e))
        err = cad.error("file.open", {"path": str(tmp_path / "names.kad")})
        assert err["code"] == 3, bad
    # The same name twice.
    with zipfile.ZipFile(tmp_path / "twice.kad", "w") as z:
        z.writestr("document.json", document)
        import warnings

        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            z.writestr("document.json", document)
        z.writestr("manifest.json", manifest_of({"document.json": document}))
    refused("twice.kad", "twice")
    # A manifest that says another hash.
    bad_manifest = json.loads(manifest_of({"document.json": document}))
    bad_manifest["entries"][0]["sha256"] = "0" * 64
    with zipfile.ZipFile(tmp_path / "hash.kad", "w") as z:
        z.writestr("document.json", document)
        z.writestr("manifest.json", json.dumps(bad_manifest))
    refused("hash.kad", "SHA-256")
    # A newer container version says so.
    newer = json.loads(manifest_of({"document.json": document}))
    newer["format_version"] = 99
    with zipfile.ZipFile(tmp_path / "newer.kad", "w") as z:
        z.writestr("document.json", document)
        z.writestr("manifest.json", json.dumps(newer))
    refused("newer.kad", "update Kadakas")
    # A compression bomb: 300 MB of zeros in a small file. It is refused before it is decoded.
    zeros = b"\0" * (300 * 1024 * 1024)
    write_zip(tmp_path / "bomb.kad", {"document.json": zeros})
    assert (tmp_path / "bomb.kad").stat().st_size < 5 * 1024 * 1024
    refused("bomb.kad", "compression bomb")
    # A file that is neither a container nor JSON.
    (tmp_path / "junk.kad").write_bytes(b"\x89PNG not a document")
    refused("junk.kad", "not a Kadakas file")
    # The good file still opens, and nothing was written next to the files.
    assert cad.result("file.open", {"path": str(good)})["ok"] is True
    assert not [p for p in tmp_path.iterdir() if p.name.endswith(".tmp")]


def test_a_file_that_changes_on_disk_after_open_is_found_and_never_read_wrong(make_serve, tmp_path):
    cad = make_serve()
    with_picture(cad)
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    work = make_serve(path)
    # Another program writes another document over the file.
    other = make_serve()
    other.execute(rect_cmd(0, 0, 3, 3))
    import time

    time.sleep(0.05)
    other.result("file.save", {"path": str(path)})
    # The old file is the .bak of that save and has the picture: take it away, so that no file has it.
    (tmp_path / "plan.kad.bak").unlink()
    changed = path.read_bytes()
    err = work.error("file.save", {"path": str(path)})
    assert err["code"] == 5 and "changed on disk" in err["message"], err
    assert path.read_bytes() == changed, "the other program's file is left alone"
    # A save to another path needs the picture of the first file, which is gone: refused as well.
    err = work.error("file.save", {"path": str(tmp_path / "copy.kad")})
    assert err["code"] == 5
    assert not (tmp_path / "copy.kad").exists()
    # The plain form of the same session is refused for the same reason.
    assert work.error("file.save", {"path": str(tmp_path / "c.json")})["code"] == 5


def test_a_file_that_is_replaced_but_has_the_old_file_as_bak_is_found_by_content(make_serve, tmp_path):
    cad = make_serve()
    with_picture(cad)
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    work = make_serve(path)
    other = make_serve()
    other.execute(rect_cmd(0, 0, 3, 3))
    import time

    time.sleep(0.05)
    # Another program saves over the file: the old file is the .bak next to it, and has the picture.
    other.result("file.save", {"path": str(path)})
    assert (tmp_path / "plan.kad.bak").exists()
    work.execute(rect_cmd(50, 50, 60, 60))
    # The file at the path is another program's file: replacing it needs the person.
    err = work.error("file.save", {"path": str(path)})
    assert err["code"] == 5 and err["data"]["reason"] == "file_changed", err
    assert err["data"]["ask_the_person"] is True, err
    for name in ("copy.kad", "plain.json"):
        assert work.result("file.save", {"path": str(tmp_path / name)})["ok"] is True, name
    check = make_serve(tmp_path / "copy.kad")
    assert check.doc()["objects"], "the document is there"
    sid = [r for r in check.doc()["resources"] if r["body"]["type"] == "plant_style"][0]["id"]
    assert picture_of(check, sid)[1], "the picture is there"


def test_parts_that_are_gone_are_saved_only_with_the_consent_of_the_person(make_serve, tmp_path):
    cad = make_serve()
    with_picture(cad)
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    work = make_serve(path)
    work.execute(rect_cmd(50, 50, 60, 60))
    other = make_serve()
    other.execute(rect_cmd(0, 0, 3, 3))
    import time

    time.sleep(0.05)
    other.result("file.save", {"path": str(path)})
    (tmp_path / "plan.kad.bak").unlink()
    # The picture is nowhere: the save asks, and writes nothing.
    err = work.error("file.save", {"path": str(tmp_path / "a.kad")})
    assert err["code"] == 5, err
    assert err["data"]["reason"] == "parts_missing" and err["data"]["ask_the_person"] is True, err
    assert not (tmp_path / "a.kad").exists()
    # The consent is bound to the refusal: `true` is not enough, and neither is a token of another one.
    for bad in (True, "0" * 64):
        refused = work.error("file.save", {"path": str(tmp_path / "a.kad"), "allow_missing": bad})
        assert refused["code"] in (-32602, 5), refused
        assert not (tmp_path / "a.kad").exists()
    # With the consent of the person the rest is saved, and the reply has notes.
    done = work.result(
        "file.save", {"path": str(tmp_path / "a.kad"), "allow_missing": err["data"]["consent_token"]}
    )
    assert done["ok"] is True and "placeholder" in json.dumps(done["notes"]), done
    again = work.result("file.save", {"path": str(tmp_path / "b.kad")})
    assert "notes" not in again, "the next save is whole"


def test_zip64_overlap_and_local_header_faults_are_refused(make_serve, tmp_path):
    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 1, 1))
    good = tmp_path / "good.kad"
    cad.result("file.save", {"path": str(good)})
    document = zipfile.ZipFile(good).read("document.json")
    before = cad.doc()

    def opens(name):
        return cad.result("file.open", {"path": str(tmp_path / name)})["ok"] is True

    def refused(name, needle):
        err = cad.error("file.open", {"path": str(tmp_path / name)})
        assert err["code"] == 3 and needle in err["message"], (name, err)
        assert cad.doc() == before

    # A container that another program wrote in the ZIP64 form (sizes in 64 bit fields) opens.
    with zipfile.ZipFile(tmp_path / "zip64.kad", "w", zipfile.ZIP_DEFLATED) as z:
        with z.open("document.json", "w", force_zip64=True) as f:
            f.write(document)
        z.writestr("manifest.json", manifest_of({"document.json": document}))
    assert opens("zip64.kad")
    # Two entries that start at the same place overlap.
    entries = {"document.json": document, "blobs/x": b"x" * 100}
    write_zip(tmp_path / "overlap.kad", entries, method=zipfile.ZIP_STORED)
    raw = bytearray((tmp_path / "overlap.kad").read_bytes())
    sig = b"PK\x01\x02"
    heads = []
    at = raw.find(sig)
    while at != -1:
        heads.append(at)
        at = raw.find(sig, at + 4)
    raw[heads[1] + 42 : heads[1] + 46] = raw[heads[0] + 42 : heads[0] + 46]
    (tmp_path / "overlap.kad").write_bytes(bytes(raw))
    refused("overlap.kad", "overlaps")
    # A stored entry whose local header has another CRC-32 than the directory.
    write_zip(tmp_path / "localcrc.kad", {"document.json": document}, method=zipfile.ZIP_STORED)
    raw = bytearray((tmp_path / "localcrc.kad").read_bytes())
    raw[14] ^= 0x55
    (tmp_path / "localcrc.kad").write_bytes(bytes(raw))
    refused("localcrc.kad", "document.json")
    # An entry that the local header says is encrypted.
    write_zip(tmp_path / "encrypted.kad", {"document.json": document}, method=zipfile.ZIP_STORED)
    raw = bytearray((tmp_path / "encrypted.kad").read_bytes())
    raw[6] |= 1
    (tmp_path / "encrypted.kad").write_bytes(bytes(raw))
    refused("encrypted.kad", "encrypted")


def test_other_zip_tools_read_the_container(make_serve, tmp_path):
    import shutil
    import subprocess

    cad = make_serve()
    cad.execute(rect_cmd(0, 0, 10, 10))
    with_picture(cad)
    cad.result("version.create", {"name": "first"})
    path = tmp_path / "plan.kad"
    cad.result("file.save", {"path": str(path)})
    names = zipfile.ZipFile(path).namelist()
    ran = []
    if shutil.which("unzip"):
        out = subprocess.run(["unzip", "-t", str(path)], capture_output=True, text=True)
        assert out.returncode == 0 and "No errors detected" in out.stdout, out.stdout + out.stderr
        ran.append("unzip -t")
    if shutil.which("bsdtar"):
        out = subprocess.run(["bsdtar", "-tf", str(path)], capture_output=True, text=True)
        assert out.returncode == 0 and set(out.stdout.split()) == set(names), out.stderr
        ran.append("bsdtar -tf")
    if shutil.which("ditto"):
        dest = tmp_path / "ditto"
        out = subprocess.run(["ditto", "-x", "-k", str(path), str(dest)], capture_output=True, text=True)
        assert out.returncode == 0, out.stderr
        with zipfile.ZipFile(path) as z:
            for n in names:
                assert (dest / n).read_bytes() == z.read(n), n
        ran.append("ditto -x -k")
    java = shutil.which("java")
    if java:
        # The stream reader of Java refuses a stored entry with a data descriptor.
        src = tmp_path / "Check.java"
        src.write_text(
            "import java.io.*; import java.util.zip.*;\n"
            "public class Check { public static void main(String[] a) throws Exception {\n"
            "  try (ZipInputStream z = new ZipInputStream(new FileInputStream(a[0]))) {\n"
            "    ZipEntry e; while ((e = z.getNextEntry()) != null) {\n"
            "      byte[] b = z.readAllBytes(); System.out.println(e.getName() + \" \" + b.length); } } } }\n"
        )
        out = subprocess.run([java, str(src), str(path)], capture_output=True, text=True, cwd=tmp_path)
        assert out.returncode == 0, out.stderr
        with zipfile.ZipFile(path) as z:
            sizes = {line.rsplit(" ", 1)[0]: int(line.rsplit(" ", 1)[1]) for line in out.stdout.splitlines()}
            assert sizes == {n: z.getinfo(n).file_size for n in names}
        ran.append("java ZipInputStream")
    if not ran:
        import pytest

        pytest.skip("no unzip, bsdtar, ditto or java on this computer")

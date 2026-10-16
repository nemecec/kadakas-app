"""Checks the XML files of a BCF archive against the buildingSMART XSD files.

Run with `uv run --no-project --with lxml python -I bcf_check.py <archive.bcfzip> <schemas folder>`.
The schema folder has `2.1/` and `3.0/` (see docs/attribution.md for their licence). The archive
names its version in `bcf.version`. Prints one JSON line: {"version", "files": {name: [errors]}}.
The exit code is 0 when the script ran, whatever the result: the test reads the answer."""

import json
import sys
import zipfile
from pathlib import Path

from lxml import etree


def schema(folder: Path, name: str):
    return etree.XMLSchema(etree.parse(str(folder / name)))


def main() -> None:
    archive, schemas = Path(sys.argv[1]), Path(sys.argv[2])
    parser = etree.XMLParser(resolve_entities=False, no_network=True)
    out = {"version": None, "files": {}}
    with zipfile.ZipFile(archive) as z:
        names = z.namelist()
        version_doc = etree.fromstring(z.read("bcf.version"), parser)
        version = version_doc.get("VersionId")
        out["version"] = version
        folder = schemas / version
        by_kind = {
            "markup.bcf": schema(folder, "markup.xsd"),
            ".bcfv": schema(folder, "visinfo.xsd"),
            "bcf.version": schema(folder, "version.xsd"),
        }
        if (folder / "extensions.xsd").exists():
            by_kind["bcf.extensions"] = schema(folder, "extensions.xsd")
        if (folder / "project.xsd").exists():
            by_kind["project"] = schema(folder, "project.xsd")
        for name in names:
            base = name.rsplit("/", 1)[-1]
            if base in by_kind:
                xsd = by_kind[base]
            elif base.endswith(".bcfv"):
                xsd = by_kind[".bcfv"]
            elif base in ("project.bcfp", "bcf.project"):
                xsd = by_kind["project"]
            else:
                continue
            doc = etree.fromstring(z.read(name), parser)
            xsd.validate(doc)
            out["files"][name] = [f"{e.line}: {e.message}" for e in xsd.error_log]
    print(json.dumps(out))


main()

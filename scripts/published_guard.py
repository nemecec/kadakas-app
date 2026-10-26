#!/usr/bin/env python3
"""The record of the highest version ever published (replay guard).

The launcher refuses a manifest that is older than the newest it has seen. So a version number
that was published once must never be reused or lowered, even if its release is deleted. The
sign job of update-manifest.yml keeps this record as the asset `highest-published.json` of the
release `update-manifests`: {"stable": "0.1.0", "beta": "0.1.0"}. It only goes up. Deleting a
release does not touch it.

    published_guard.py check FILE VERSION [--channel stable|beta] [--allow-equal]
        exit 1 when VERSION is not higher than the record (of the channel, or of any channel).
        --allow-equal accepts the same version (a new signature of the latest release).
        A missing file means nothing was published: exit 0.
    published_guard.py order VERSION TAG...
        exit 1 when VERSION is not newer than a tag. A stable VERSION is compared with the stable
        tags only (a hotfix 0.1.1 may follow a beta 0.2.0-beta.1). A pre-release is compared with
        all tags. Tags that are no versions (and -dev) are skipped.
    published_guard.py record FILE VERSION CHANNEL...
        sets the record of each channel to the higher of the old one and VERSION.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from version_cmp import cmp, problem  # noqa: E402


def load(path):
    if not os.path.exists(path):
        return {}
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    for channel, version in data.items():
        why = problem(version)
        if channel not in ("stable", "beta") or why:
            sys.exit("bad record %s: %s %r" % (path, channel, version))
    return data


def highest(data, channel):
    versions = [data[channel]] if channel else list(data.values())
    top = None
    for v in versions:
        if top is None or cmp(v, top) > 0:
            top = v
    return top


def main(argv):
    if len(argv) >= 4 and argv[1] == "check":
        path, version, rest = argv[2], argv[3], argv[4:]
        why = problem(version)
        if why:
            sys.exit("bad version %r: %s" % (version, why))
        allow_equal = "--allow-equal" in rest
        channel = rest[rest.index("--channel") + 1] if "--channel" in rest else None
        data = load(path)
        if channel and channel not in data:
            return 0
        top = highest(data, channel)
        if top is None:
            return 0
        c = cmp(version, top)
        if c > 0 or (c == 0 and allow_equal):
            return 0
        print(
            "%s is not higher than %s, the highest version ever published%s. A published version "
            "number is never reused or lowered: release a HIGHER version."
            % (version, top, " on the %s channel" % channel if channel else ""),
            file=sys.stderr,
        )
        return 1
    if len(argv) >= 3 and argv[1] == "order":
        version, tags = argv[2], argv[3:]
        why = problem(version)
        if why:
            sys.exit("bad version %r: %s" % (version, why))
        stable = "-" not in version
        for tag in tags:
            v = tag[1:] if tag.startswith("v") else tag
            if problem(v) or (stable and "-" in v):
                continue
            if cmp(version, v) <= 0:
                print("%s is not newer than the existing tag %s." % (version, tag), file=sys.stderr)
                return 1
        return 0
    if len(argv) >= 5 and argv[1] == "record":
        path, version, channels = argv[2], argv[3], argv[4:]
        why = problem(version)
        if why or any(c not in ("stable", "beta") for c in channels):
            sys.exit("bad input: %s" % (why or channels))
        data = load(path)
        for c in channels:
            if c not in data or cmp(version, data[c]) > 0:
                data[c] = version
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, sort_keys=True)
            f.write("\n")
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv))

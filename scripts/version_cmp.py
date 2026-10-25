#!/usr/bin/env python3
"""Compares two Kadakas versions like kadakas_update::Version: prints -1, 0 or 1.

    scripts/version_cmp.py A B      (-1 when A < B)

A release is newer than its pre-releases. Pre-release parts compare by dot-separated
identifiers: numbers by value, a number before text, text by bytes, fewer identifiers first.
"""
import re
import sys

PATTERN = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9a-z]+(?:[.-][0-9a-z]+)*))?$")


def parse(text):
    m = PATTERN.match(text)
    if not m:
        sys.exit(f"bad version: {text}")
    return tuple(int(m.group(i)) for i in (1, 2, 3)), m.group(4)


def ident(i):
    return (0, int(i), "") if i.isdigit() else (1, 0, i)


def key(text):
    core, pre = parse(text)
    if pre is None:
        return core, (1,)
    return core, (0, [ident(i) for i in pre.split(".")])


def cmp(a, b):
    (ca, pa), (cb, pb) = key(a), key(b)
    if ca != cb:
        return -1 if ca < cb else 1
    if pa[0] != pb[0]:
        return -1 if pa[0] < pb[0] else 1
    if pa[0] == 1:
        return 0
    return (pa[1] > pb[1]) - (pa[1] < pb[1])


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: version_cmp.py A B")
    print(cmp(sys.argv[1], sys.argv[2]))

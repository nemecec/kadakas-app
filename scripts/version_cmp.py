#!/usr/bin/env python3
"""Checks and compares Kadakas versions with the rules of kadakas_update::Version.

    scripts/version_cmp.py valid V      exit 0 when V is a valid version, else 1 with a reason
    scripts/version_cmp.py A B          prints -1, 0 or 1 (-1 when A < B)

A valid version is major.minor.patch with an optional pre-release part: identifiers of lower-case
letters, digits and inner hyphens, separated by dots. No leading zeros in the numbers or in the
numeric identifiers. At most 40 characters. A release is newer than its pre-releases. Identifiers
compare as numbers when both are numbers, a number before text, text by bytes, fewer identifiers
first.
"""
import re
import sys

MAX_LEN = 40
PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9a-z]+(?:[.-][0-9a-z]+)*))?")


def problem(text):
    """Returns the reason why text is not a version, or None."""
    if not text or len(text) > MAX_LEN:
        return "empty or longer than %d characters" % MAX_LEN
    m = PATTERN.fullmatch(text)
    if not m:
        return "not major.minor.patch[-pre-release] in lower case"
    if m.group(4) and any(i.isdigit() and len(i) > 1 and i[0] == "0" for i in m.group(4).split(".")):
        return "leading zero in a number of the pre-release part"
    return None


def parse(text):
    why = problem(text)
    if why:
        sys.exit("bad version %r: %s" % (text, why))
    m = PATTERN.fullmatch(text)
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
    if len(sys.argv) == 3 and sys.argv[1] == "valid":
        why = problem(sys.argv[2])
        if why:
            print("bad version %r: %s" % (sys.argv[2], why), file=sys.stderr)
            sys.exit(1)
        sys.exit(0)
    if len(sys.argv) != 3:
        sys.exit("usage: version_cmp.py valid V | version_cmp.py A B")
    print(cmp(sys.argv[1], sys.argv[2]))

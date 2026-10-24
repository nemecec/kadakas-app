"""Writes the table of standard hatch patterns that the DXF import and export use (IOP-27).

The numbers are the metric table of ezdxf (MIT licence, https://github.com/mozman/ezdxf, module
ezdxf.tools.pattern), which lists the public AutoCAD pattern names. Run:

    uv run --with ezdxf python -I tests/data/make_std_patterns.py > crates/kadakas-io/src/dxf/hatch/std_patterns.txt

One pattern is a line `NAME`, then one line for each definition line:
`angle base_x base_y offset_x offset_y dash,dash,...` in mm at scale 1. Offsets are in the world
frame, as in the group codes 45 and 46 of a HATCH.
"""
from ezdxf.tools import pattern

SKIP_PREFIXES = ("ACAD_ISO", "ISO", "JIS_", "V_", "BRICK_", "CONC_")
SKIP = {"SOLID"}
# A pattern keeps at most 128 lines on import. One pattern of ezdxf has more (CONCRETE3).
MAX_LINES = 128


def num(v):
    s = f"{v:.6f}".rstrip("0").rstrip(".")
    return "0" if s in ("", "-0") else s


def main():
    table = pattern.load(measurement=1)
    for name in sorted(table):
        if name in SKIP or name.startswith(SKIP_PREFIXES) or len(table[name]) > MAX_LINES:
            continue
        print(name)
        for angle, base, offset, dashes in table[name]:
            row = [num(angle), num(base[0]), num(base[1]), num(offset[0]), num(offset[1])]
            row.append(",".join(num(d) for d in dashes) if dashes else "-")
            print(" ".join(row))


main()

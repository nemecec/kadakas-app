#!/usr/bin/env python3
"""Makes docs/icons.html: a static grid of all icons (inline SVG, light and dark).

    python3 scripts/icons_gallery.py
"""
import html
import os
import re

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
ICONS = os.path.join(ROOT, "assets", "icons")


def load(sub):
    d = os.path.join(ICONS, sub)
    out = []
    for f in sorted(os.listdir(d)):
        if f.endswith(".svg"):
            s = open(os.path.join(d, f), encoding="utf-8").read()
            
            s = re.sub(r'\s(width|height|class)="[^"]*"', "", s, count=3)
            out.append((f[:-4], s.strip()))
    return out


def section(title, items):
    cells = "".join(
        f'<figure>{svg}<figcaption>{html.escape(name)}</figcaption></figure>'
        for name, svg in items
    )
    return f"<h2>{title} ({len(items)})</h2><div class=grid>{cells}</div>"


page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cadektor icons</title>
<style>
:root {{ color-scheme: light dark; --fg: #1f2328; --bg: #fff; --mute: #656d76; }}
@media (prefers-color-scheme: dark) {{ :root {{ --fg: #e6edf3; --bg: #0d1117; --mute: #8b949e; }} }}
body {{ margin: 0 auto; max-width: 62rem; padding: 1rem 16px; font: 15px/1.5 system-ui, sans-serif; color: var(--fg); background: var(--bg); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); gap: 12px; }}
figure {{ margin: 0; text-align: center; }}
figure svg {{ width: 48px; height: 48px; color: var(--fg); }}
figcaption {{ font-size: 12px; color: var(--mute); overflow-wrap: anywhere; }}
</style></head><body>
<h1>Cadektor icons</h1>
<p>Made by <code>scripts/icons_gallery.py</code>. Rules: <code>docs/icons.md</code>.
Our icons use the same style as Tabler Icons (MIT).</p>
{section("Cadektor icons (own drawings)", load("cadektor"))}
{section("Tabler Icons (MIT, generic actions)", load("tabler"))}
</body></html>
"""
open(os.path.join(ROOT, "docs", "icons.html"), "w", encoding="utf-8").write(page)
print("wrote docs/icons.html")

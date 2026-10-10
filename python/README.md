# cadektor (Python)

Python API for Cadektor. It talks to a Cadektor session over JSON-RPC. The package has no
dependencies.

```
uv pip install -e python      # or: pip install -e python
```

```python
import cadektor

with cadektor.open() as doc:          # starts `cadektor serve`
    doc.add.line([0, 0], [5000, 0])
    print(doc.summary()["objects_by_kind"])
```

Inside a script that the Cadektor app starts, `cadektor.connect()` joins the live document.
See `docs/scripting.md` in the repository for the full guide.

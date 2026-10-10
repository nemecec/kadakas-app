# kadakas (Python)

Python API for Kadakas. It talks to a Kadakas session over JSON-RPC. The package has no
dependencies.

```
uv pip install -e python      # or: pip install -e python
```

```python
import kadakas

with kadakas.open() as doc:          # starts `kadakas serve`
    doc.add.line([0, 0], [5000, 0])
    print(doc.summary()["objects_by_kind"])
```

Inside a script that the Kadakas app starts, `kadakas.connect()` joins the live document.
See `docs/scripting.md` in the repository for the full guide.

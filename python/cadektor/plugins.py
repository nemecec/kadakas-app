"""Plug-in objects and scripts that run next to the app.

A plug-in object has a definition (name, shape type, parameters, a Python script) and any number
of objects placed from it. The script defines ``reset(params, geometry)`` and returns shapes.
See ``docs/scripting.md``.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Sequence

from .errors import CadektorError

if TYPE_CHECKING:  # pragma: no cover
    from .document import Document, Point

JsonDict = dict[str, Any]


class Param:
    """Builders for the parameter entries of a definition."""

    @staticmethod
    def _make(name: str, ty: str, default: Any, label: str | None, **extra: Any) -> JsonDict:
        p: JsonDict = {"name": name, "type": ty}
        if default is not None:
            p["default"] = default
        if label:
            p["label"] = label
        p.update({k: v for k, v in extra.items() if v is not None})
        return p

    @classmethod
    def number(cls, name: str, default: float = 0, *, min: float | None = None,
               max: float | None = None, label: str | None = None) -> JsonDict:
        return cls._make(name, "number", default, label, min=min, max=max)

    @classmethod
    def length(cls, name: str, default: float = 0, *, min: float | None = None,
               max: float | None = None, label: str | None = None) -> JsonDict:
        """A length in mm."""
        return cls._make(name, "length", default, label, min=min, max=max)

    @classmethod
    def text(cls, name: str, default: str = "", *, label: str | None = None) -> JsonDict:
        return cls._make(name, "text", default, label)

    @classmethod
    def bool(cls, name: str, default: bool = False, *, label: str | None = None) -> JsonDict:
        return cls._make(name, "bool", default, label)

    @classmethod
    def choice(cls, name: str, options: Sequence[str], default: str | None = None, *,
               label: str | None = None) -> JsonDict:
        return cls._make(name, "choice", default or options[0], label, options=list(options))

    @classmethod
    def color(cls, name: str, default: str = "#000000", *, label: str | None = None) -> JsonDict:
        return cls._make(name, "color", default, label)

    @classmethod
    def record(cls, name: str, fields: Sequence[JsonDict], *, label: str | None = None) -> JsonDict:
        """A group of fields. The value is a dict with one entry per field."""
        return cls._make(name, "record", None, label, fields=list(fields))


class PluginObject:
    """A plug-in object in the document (a group of generated shapes)."""

    def __init__(self, doc: "Document", id: int) -> None:
        self._doc, self.id = doc, id

    def info(self) -> JsonDict:
        return self._doc.api.plugin_object(id=self.id)

    @property
    def params(self) -> JsonDict:
        return self.info()["params"]

    @property
    def error(self) -> str | None:
        """The error of the last reset, or ``None`` if it worked."""
        return self.info()["error"]

    @property
    def children(self) -> list[int]:
        return self.info()["children"]

    def set_params(self, **values: Any) -> JsonDict:
        """Change parameters and regenerate. If the script fails, the old shapes stay and
        ``result["ok"]`` is false with ``result["error"]``."""
        return self._doc.api.plugin_set_params(id=self.id, params=values)

    def reset(self) -> JsonDict:
        """Run the script again with the same parameters."""
        return self._doc.api.plugin_reset(id=self.id)["results"][0]

    def __repr__(self) -> str:
        return f"PluginObject({self.id})"


class Plugins:
    """``doc.plugins``: definitions and objects."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def examples(self) -> list[JsonDict]:
        """The plug-in objects that ship with the app."""
        return self._doc.api.plugin_examples()

    def install_example(self, name: str) -> int:
        """Add a shipped plug-in object (``"Bench"``, ``"Stepping stones"``). Returns the definition ID."""
        return self._doc.api.plugin_install_example(name=name)["id"]

    def define(self, name: str, shape_type: str, script: str, params: Sequence[JsonDict] = (),
               description: str = "", timeout_ms: int | None = None) -> int:
        """Create a definition. Returns its ID. Nothing runs yet."""
        extra: JsonDict = {"timeout_ms": timeout_ms} if timeout_ms else {}
        return self._doc.api.plugin_define(name=name, shape_type=shape_type, script=script,
                                           params=list(params), description=description, **extra)["id"]

    def update(self, definition: int | str, *, reset: bool = False, **fields: Any) -> JsonDict:
        """Change a definition (``script``, ``params``, ``new_name``, ...). ``reset=True`` also
        regenerates its objects, in the same undo step."""
        key = "id" if isinstance(definition, int) else "name"
        return self._doc.api.plugin_update(**{key: definition}, reset=reset, **fields)

    def delete(self, definition: int | str) -> None:
        key = "id" if isinstance(definition, int) else "name"
        self._doc.api.plugin_delete(**{key: definition})

    def list(self) -> list[JsonDict]:
        return self._doc.api.plugin_list()

    def get(self, definition: int | str) -> JsonDict:
        key = "id" if isinstance(definition, int) else "name"
        return self._doc.api.plugin_get(**{key: definition})

    def place(self, definition: int | str, *, at: "Point | None" = None, rotation: float = 0.0,
              a: "Point | None" = None, b: "Point | None" = None,
              min: "Point | None" = None, max: "Point | None" = None,
              points: Sequence["Point"] | None = None, closed: bool = False,
              params: JsonDict | None = None, **options: Any) -> PluginObject:
        """Place an object. Give the geometry for the shape type of the definition:
        point: ``at`` (and ``rotation``); linear: ``a`` and ``b``; rectangle: ``min`` and ``max``;
        path: ``points`` (and ``closed``)."""
        if self._doc.in_undo_group:
            raise CadektorError(1, "plugins.place cannot join an undo group (it is already one undo step)")
        geometry: JsonDict = {}
        if at is not None:
            geometry.update(at=list(at), rotation=rotation)
        if a is not None and b is not None:
            geometry.update(a=list(a), b=list(b))
        if min is not None and max is not None:
            geometry.update(min=list(min), max=list(max))
        if points is not None:
            geometry.update(points=[list(p) for p in points], closed=closed)
        res = self._doc.api.plugin_place(definition=definition, params=params or {}, **geometry, **options)
        return PluginObject(self._doc, res["id"])

    def object(self, id: int) -> PluginObject:
        """The plug-in object with this object ID."""
        self._doc.api.plugin_object(id=id)
        return PluginObject(self._doc, id)


class Run:
    """A script that runs next to the app."""

    def __init__(self, doc: "Document", id: int) -> None:
        self._doc, self.id = doc, id
        self._next = 0

    def status(self) -> JsonDict:
        res = self._doc.api.script_status(run=self.id, since=self._next)
        self._next = res["next"]
        return res

    def wait(self, timeout: float = 60.0) -> JsonDict:
        """Wait for the end. Returns the last status, with all console lines in ``lines``."""
        import time

        lines: list[JsonDict] = []
        end = time.monotonic() + timeout
        while True:
            res = self.status()
            lines += res["lines"]
            if res["status"] != "running":
                return {**res, "lines": lines}
            if time.monotonic() > end:
                raise TimeoutError(f"script run {self.id} is still running")
            time.sleep(0.05)

    def stop(self) -> bool:
        return self._doc.api.script_stop(run=self.id)["stopped"]


class Scripts:
    """``doc.scripts``: run Python scripts next to the app. A request from a program (not a user)
    needs a host that allows it (``--allow-agent-scripts``), and a running app."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def info(self) -> JsonDict:
        return self._doc.api.script_info()

    def run(self, path: str | None = None, *, source: str | None = None, args: Sequence[str] = ()) -> Run:
        params: JsonDict = {"args": list(args)}
        if path is not None:
            params["path"] = str(path)
        if source is not None:
            params["source"] = source
        return Run(self._doc, self._doc.api.script_run(**params)["run"])

    def list(self) -> list[JsonDict]:
        return self._doc.api.script_list()

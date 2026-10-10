"""The high-level API: documents, layers, classes, objects, selection, undo groups and reports.

Everything here is a thin layer over JSON-RPC. For a call that has no wrapper, use
``doc.api.<method>(**params)`` (generated from ``describe``) or ``doc.call("method", **params)``.
Lengths are in millimeters, angles in radians, and the Y axis points up.
"""

from __future__ import annotations

import base64
import json
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable, Iterable, Iterator, Sequence, Union

from ._api import Api
from ._rpc import StdioTransport, TcpTransport, Transport, resolve_endpoint
from .errors import CadektorError

Point = Sequence[float]
JsonDict = dict[str, Any]


class Pending:
    """A new object inside an ``undo_group``. Its ID is known when the group ends."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc
        self._id: int | None = None

    @property
    def id(self) -> int:
        if self._id is None:
            raise CadektorError(1, "the ID is known when the undo group ends")
        return self._id

    def object(self) -> "Obj":
        """The object, read from the session. Only after the undo group ended."""
        return self._doc.get(self.id)

    def __repr__(self) -> str:
        return f"Pending({self._id})" if self._id is not None else "Pending(?)"


ObjRef = Union["Obj", Pending, int]


def _id_of(ref: ObjRef) -> int:
    return ref if isinstance(ref, int) else ref.id


class Layer:
    """A layer. Change it with ``doc.set_layer`` or the methods here."""

    def __init__(self, doc: "Document", data: JsonDict) -> None:
        self._doc, self.data = doc, data

    id = property(lambda self: self.data["id"])
    name = property(lambda self: self.data["name"])
    elevation = property(lambda self: self.data["elevation"])
    visible = property(lambda self: self.data["visible"])
    locked = property(lambda self: self.data["locked"])

    def objects(self, **filters: Any) -> list["Obj"]:
        return self._doc.objects(layer=self, **filters)

    def activate(self) -> None:
        """New objects go on this layer."""
        self._doc.execute({"op": "set_active", "layer": self.id})

    def modify(self, **fields: Any) -> None:
        """Change ``name``, ``visible``, ``locked``, ``elevation`` or ``scale``."""
        self._doc.execute({"op": "modify_layer", "id": self.id, **fields})
        self.data.update(fields)

    def __repr__(self) -> str:
        return f"Layer({self.id}, {self.name!r})"


class Class:
    """A class (a named set of graphic attributes)."""

    def __init__(self, doc: "Document", data: JsonDict) -> None:
        self._doc, self.data = doc, data

    id = property(lambda self: self.data["id"])
    name = property(lambda self: self.data["name"])
    visible = property(lambda self: self.data["visible"])
    attrs = property(lambda self: self.data.get("attrs", {}))

    def activate(self) -> None:
        self._doc.execute({"op": "set_active", "class": self.id})

    def modify(self, **fields: Any) -> None:
        """Change ``name``, ``visible`` or ``attrs`` (pen, fill, line_weight)."""
        self._doc.execute({"op": "modify_class", "id": self.id, **fields})
        self.data.update(fields)

    def __repr__(self) -> str:
        return f"Class({self.id}, {self.name!r})"


class Obj:
    """An object of the document: a snapshot with its ID, kind, layer, class and shape fields.

    Shape fields are attributes: ``circle.radius``. Methods that change the object send a command
    and refresh the snapshot.
    """

    def __init__(self, doc: "Document", data: JsonDict) -> None:
        self._doc, self.data = doc, data

    id = property(lambda self: self.data["id"])
    kind = property(lambda self: self.data["shape"]["kind"])
    shape = property(lambda self: self.data["shape"])
    layer_id = property(lambda self: self.data["layer"])
    class_id = property(lambda self: self.data["class"])
    attrs = property(lambda self: self.data.get("attrs", {}))
    record = property(lambda self: self.data.get("data", {}))
    parent = property(lambda self: self.data.get("parent"))

    @property
    def layer(self) -> Layer:
        return self._doc.layer(self.layer_id)

    @property
    def class_(self) -> Class:
        return self._doc.class_(self.class_id)

    def __getattr__(self, name: str) -> Any:
        shape = self.__dict__.get("data", {}).get("shape", {})
        if name in shape:
            return shape[name]
        raise AttributeError(f"{self.kind} has no field {name!r}")

    def __getitem__(self, key: str) -> Any:
        return self.shape[key]

    def refresh(self) -> "Obj":
        self.data = self._doc.api.objects_get(id=self.id)
        return self

    def modify(self, *, layer: Layer | int | None = None, class_: Class | int | None = None,
               attrs: JsonDict | None = None, **shape_fields: Any) -> "Obj":
        """Change shape fields (``radius=300``), the layer, the class or the attributes."""
        command: JsonDict = {"op": "modify_object", "id": self.id}
        if shape_fields:
            command["shape"] = {**self.shape, **shape_fields}
        if layer is not None:
            command["layer"] = layer if isinstance(layer, int) else layer.id
        if class_ is not None:
            command["class"] = class_ if isinstance(class_, int) else class_.id
        if attrs is not None:
            command["attrs"] = {**self.attrs, **attrs}
        self._doc.execute(command)
        return self.refresh() if not self._doc.in_undo_group else self

    def move(self, dx: float, dy: float) -> None:
        self._doc.move([self], dx, dy)

    def delete(self) -> None:
        self._doc.delete([self])

    def set_record(self, **values: Any) -> None:
        """Set keys in the free-form data of the object."""
        self._doc.execute({"op": "set_data", "id": self.id, "set": values})

    def __repr__(self) -> str:
        return f"Obj({self.id}, {self.kind})"


class Adder:
    """``doc.add``: draws shapes. Each method returns an ``Obj`` (a ``Pending`` in an undo group)."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def shape(self, kind: str, *, layer: Layer | int | None = None, class_: Class | int | None = None,
              attrs: JsonDict | None = None, **fields: Any) -> Any:
        """Add any shape kind (see ``describe``): ``doc.add.shape("circle", center=[0, 0], radius=5)``."""
        command: JsonDict = {"op": "add_object", "shape": {"kind": kind, **fields}}
        if layer is not None:
            command["layer"] = layer if isinstance(layer, int) else layer.id
        if class_ is not None:
            command["class"] = class_ if isinstance(class_, int) else class_.id
        if attrs:
            command["attrs"] = attrs
        return self._doc._add_object(command)

    def line(self, a: Point, b: Point, **kw: Any) -> Any:
        return self.shape("line", a=list(a), b=list(b), **kw)

    def rect(self, min: Point, max: Point, **kw: Any) -> Any:
        return self.shape("rect", min=list(min), max=list(max), **kw)

    def circle(self, center: Point, radius: float, **kw: Any) -> Any:
        return self.shape("circle", center=list(center), radius=radius, **kw)

    def arc(self, center: Point, radius: float, start: float, end: float, **kw: Any) -> Any:
        return self.shape("arc", center=list(center), radius=radius, start=start, end=end, **kw)

    def polyline(self, points: Iterable[Any], closed: bool = False, **kw: Any) -> Any:
        """Points are ``[x, y]`` or vertices with curves (see ``describe.vertex_kinds``)."""
        return self.shape("polyline", points=[list(p) if not isinstance(p, dict) else p for p in points],
                          closed=closed, **kw)

    def text(self, at: Point, text: str, height: float = 250.0, **kw: Any) -> Any:
        return self.shape("text", at=list(at), text=text, height=height, **kw)


class Plants:
    """``doc.plants``: plant styles, placing and the schedule."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def place(self, mode: str = "single", **params: Any) -> list[int]:
        """Place plants in one undo step. ``doc.plants.place(at=[0, 0], style="Oak")`` is one plant;
        other modes are ``path``, ``grid``, ``triangular`` and ``fill`` (see the API text of
        ``plants.place``). Returns the IDs of the new plants."""
        if self._doc.in_undo_group:
            raise CadektorError(1, "plants.place cannot join an undo group: call it outside the group")
        return self._doc.api.plants_place(mode=mode, **params)["created"]

    def styles(self) -> list[JsonDict]:
        return self._doc.api.plant_style_list()

    def create_style(self, name: str, **fields: Any) -> Any:
        return self._doc.api.plant_style_create(name=name, **fields)

    def schedule(self) -> JsonDict:
        """Count of plants by species and style, with prices."""
        return self._doc.api.report_plant_schedule()


class Selection:
    """``doc.selection``: the selected objects of the session (the app shows them)."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def ids(self) -> list[int]:
        return self._doc.api.selection_get()["ids"]

    def objects(self) -> list[Obj]:
        return [self._doc.get(i) for i in self.ids()]

    def set(self, items: Iterable[ObjRef]) -> None:
        self._doc.api.selection_set(ids=[_id_of(i) for i in items])

    def clear(self) -> None:
        self.set([])

    def __len__(self) -> int:
        return len(self.ids())

    def __iter__(self) -> Iterator[Obj]:
        return iter(self.objects())


class Reports:
    """``doc.report``: the reports of the session."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def plants(self) -> JsonDict:
        return self._doc.api.report_plants()

    def plant_schedule(self, csv: bool = False) -> JsonDict:
        return self._doc.api.report_plant_schedule(**({"format": "csv"} if csv else {}))

    def areas(self) -> JsonDict:
        return self._doc.api.report_areas()

    def hardscape(self) -> JsonDict:
        return self._doc.api.report_hardscape()


class Worksheets:
    """``doc.worksheets``: spreadsheets with formulas and database rows from the drawing."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def create(self, name: str, cells: dict[str, Any] | None = None, **params: Any) -> int:
        """Create a worksheet. ``cells`` maps addresses to text, numbers or ``"=formulas"``."""
        if cells is not None:
            params["cells"] = cells
        return self._doc.api.worksheet_create(name=name, **params)["id"]

    def list(self) -> Any:
        return self._doc.api.worksheet_list()

    def set_cell(self, sheet: int | str, address: str, value: Any) -> None:
        self._doc.api.worksheet_set_cell(id=sheet, address=address, input=value)

    def evaluate(self, sheet: int | str) -> JsonDict:
        """The calculated values: ``result["cells"]["B2"]``."""
        return self._doc.api.worksheet_evaluate(id=sheet)

    def csv(self, sheet: int | str) -> str:
        return self._doc.api.worksheet_export(id=sheet, format="csv")["csv"]

    def place(self, sheet: int | str, at: Point, **params: Any) -> int:
        return self._doc.api.worksheet_place(id=sheet, at=list(at), **params)["id"]


class Document:
    """A session with a document. Make one with ``cadektor.open()`` or ``cadektor.connect()``."""

    def __init__(self, transport: Transport) -> None:
        self._transport = transport
        self.api = Api(self.call_raw)
        self.add = Adder(self)
        self.plants = Plants(self)
        self.selection = Selection(self)
        self.report = Reports(self)
        self.worksheets = Worksheets(self)
        from .plugins import Plugins, Scripts  # late: plugins import this module

        self.plugins = Plugins(self)
        self.scripts = Scripts(self)
        self._queue: list[tuple[JsonDict, list[Pending]]] | None = None

    # -- calls ---------------------------------------------------------------------------

    def call_raw(self, method: str, params: JsonDict | None = None) -> Any:
        return self._transport.request(method, params)

    def call(self, method: str, **params: Any) -> Any:
        """Call any JSON-RPC method by its name: ``doc.call("report.areas")``."""
        return self.call_raw(method, params or None)

    def describe(self) -> JsonDict:
        """The self-description of the session: methods, units and examples."""
        return self.api.describe()

    def close(self) -> None:
        self._transport.close()

    def __enter__(self) -> "Document":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    # -- commands and undo ----------------------------------------------------------------

    @property
    def in_undo_group(self) -> bool:
        return self._queue is not None

    def execute(self, command: JsonDict) -> list[int]:
        """Run one command (see ``describe``: ``command_examples``) as one undo step. Inside an
        ``undo_group`` the command waits, and the list is empty."""
        if self._queue is not None:
            self._queue.append((command, []))
            return []
        return self.api.execute(command=command)["created"]

    def _add_object(self, command: JsonDict) -> Any:
        if self._queue is not None:
            pending = Pending(self)
            self._queue.append((command, [pending]))
            return pending
        created = self.api.execute(command=command)["created"]
        return self.get(created[0])

    @contextmanager
    def undo_group(self, name: str | None = None) -> Iterator[None]:
        """Everything in the ``with`` block is one undo step (one ``batch`` command).

        Drawing calls, ``move``, ``delete``, ``modify`` and ``execute`` wait until the block ends.
        New objects are ``Pending`` until then. Calls that are not commands (such as
        ``plants.place``) cannot join. If an error happens in the block, nothing is sent.
        ``name`` is for your own notes in logs."""
        if self._queue is not None:  # nested: join the outer group
            yield
            return
        self._queue = []
        try:
            yield
            queue, self._queue = self._queue, None
            if not queue:
                return
            created = self.api.execute(command={"op": "batch", "commands": [c for c, _ in queue]})["created"]
            ids = iter(created)
            for _, pendings in queue:
                for p in pendings:
                    p._id = next(ids)
        finally:
            self._queue = None

    def undo(self) -> bool:
        return self.api.undo()["done"]

    def redo(self) -> bool:
        return self.api.redo()["done"]

    # -- layers and classes -----------------------------------------------------------------

    def summary(self) -> JsonDict:
        return self.api.document_summary()

    @property
    def layers(self) -> list[Layer]:
        return [Layer(self, d) for d in self.summary()["layers"]]

    @property
    def classes(self) -> list[Class]:
        return [Class(self, d) for d in self.summary()["classes"]]

    def layer(self, ref: str | int | Layer) -> Layer:
        """A layer by ID or name."""
        for lay in self.layers:
            if lay.id == ref or lay.name == ref or (isinstance(ref, Layer) and ref.id == lay.id):
                return lay
        raise KeyError(f"there is no layer {ref!r}")

    def class_(self, ref: str | int | Class) -> Class:
        for cls in self.classes:
            if cls.id == ref or cls.name == ref or (isinstance(ref, Class) and ref.id == cls.id):
                return cls
        raise KeyError(f"there is no class {ref!r}")

    @property
    def active_layer(self) -> Layer:
        return self.layer(self.summary()["active_layer"])

    def add_layer(self, name: str, elevation: float = 0.0, scale: float | None = None) -> Layer:
        cmd: JsonDict = {"op": "add_layer", "name": name, "elevation": elevation}
        if scale is not None:
            cmd["scale"] = scale
        if self.in_undo_group:
            raise CadektorError(1, "create layers outside the undo group")
        (new_id,) = self.execute(cmd)
        return self.layer(new_id)

    def add_class(self, name: str, **attrs: Any) -> Class:
        if self.in_undo_group:
            raise CadektorError(1, "create classes outside the undo group")
        (new_id,) = self.execute({"op": "add_class", "name": name, "attrs": attrs})
        return self.class_(new_id)

    # -- objects and queries ------------------------------------------------------------------

    def get(self, id: int) -> Obj:
        return Obj(self, self.api.objects_get(id=id))

    def objects(self, layer: Layer | str | int | None = None, class_: Class | str | int | None = None,
                kind: str | None = None, where: Callable[[Obj], bool] | None = None) -> list[Obj]:
        """Query objects. ``layer`` and ``class_`` take an object, an ID or a name. ``where`` is a
        function that gets each ``Obj``: ``doc.objects(kind="circle", where=lambda o: o.radius > 100)``."""
        params: JsonDict = {}
        if layer is not None:
            params["layer"] = layer if isinstance(layer, int) else self.layer(layer).id
        if class_ is not None:
            params["class"] = class_ if isinstance(class_, int) else self.class_(class_).id
        if kind is not None:
            params["kind"] = kind
        found = [Obj(self, d) for d in self.api.objects_list(**params)]
        return [o for o in found if where is None or where(o)]

    query = objects

    def move(self, items: Iterable[ObjRef], dx: float, dy: float) -> None:
        self.execute({"op": "move_objects", "ids": [_id_of(i) for i in items], "delta": [dx, dy]})

    def delete(self, items: Iterable[ObjRef]) -> None:
        self.execute({"op": "delete_objects", "ids": [_id_of(i) for i in items]})

    def group(self, items: Iterable[ObjRef], name: str | None = None) -> int:
        """Group objects. Returns the ID of the group."""
        return self.api.group_create(ids=[_id_of(i) for i in items], **({"name": name} if name else {}))["id"]

    # -- files, import, export, rendering -------------------------------------------------------

    def save(self, path: str | Path) -> None:
        self.call("file.save", path=str(path))

    def open(self, path: str | Path) -> None:
        self.call("file.open", path=str(path))

    def import_file(self, path: str | Path, **options: Any) -> JsonDict:
        """Import DXF, Shapefile or GeoJSON as one undo step."""
        return self.api.import_file(path=str(path), **({"options": options} if options else {}))

    def export_file(self, path: str | Path, **options: Any) -> JsonDict:
        """Export to DXF, GeoJSON (or IFC when the path ends in ``.ifc``)."""
        return self.api.export_file(path=str(path), **({"options": options} if options else {}))

    def render_png(self, path: str | Path | None = None, **params: Any) -> bytes:
        """Render the 3D model (``view``, ``width``, ``height``, ``projection``, ``mode``).
        Returns the PNG bytes, and writes them to ``path`` if given."""
        data = base64.b64decode(self.api.render_png(**params)["png_base64"])
        if path is not None:
            Path(path).write_bytes(data)
        return data

    def render_svg(self, path: str | Path | None = None, width: int = 800) -> str:
        """Render the 2D drawing as SVG text. Writes it to ``path`` if given."""
        svg = self.api.render_svg(width=width)["svg"]
        if path is not None:
            Path(path).write_text(svg, encoding="utf-8")
        return svg

    def to_json(self) -> JsonDict:
        """The whole document as JSON."""
        return self.api.document_get()

    def __repr__(self) -> str:
        return f"Document({json.dumps(self.summary().get('objects_by_kind', {}))})"


def open(path: str | Path | None = None, *, binary: str | Path | None = None,  # noqa: A001
         allow_dir: Sequence[str | Path] = (), allow_agent_scripts: bool = False) -> Document:
    """Start ``cadektor serve`` and return a ``Document`` for its session.

    ``path`` is a document file to open at the start. ``allow_dir`` lists the folders that the
    session may read and write (default: the current folder). The process ends with ``close()``."""
    args: list[str] = []
    for d in allow_dir:
        args += ["--allow-dir", str(d)]
    if allow_agent_scripts:
        args.append("--allow-agent-scripts")
    if path is not None:
        args.append(str(path))
    return Document(StdioTransport(args, binary))


def connect(addr: str | None = None, token: str | None = None) -> Document:
    """Connect to a running Cadektor app and see its live document.

    Without arguments the address and token come from ``CADEKTOR_ADDR`` and ``CADEKTOR_TOKEN``
    (the app sets them for scripts that it starts) or from the runtime file of the app."""
    host, port, tok = resolve_endpoint(addr, token)
    return Document(TcpTransport(host, port, tok))

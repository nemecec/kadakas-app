"""The high-level API: documents, layers, classes, objects, selection, undo groups and reports.

Everything here is a thin layer over JSON-RPC. For a call that has no wrapper, use
``doc.api.<method>(**params)`` (generated from ``describe``) or ``doc.call("method", **params)``.
Lengths are in millimeters, angles in radians, and the Y axis points up.
"""

from __future__ import annotations

import base64
import json
import re
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable, Iterable, Iterator, Sequence, Union

from ._api import Api
from ._rpc import StdioTransport, TcpTransport, Transport, resolve_endpoint
from .errors import KadakasError

Point = Sequence[float]
JsonDict = dict[str, Any]
Id = str
"""The ID of an object, layer, class or resource: a UUID text such as
``0190f1c2-7a3b-7c1d-8e2f-123456789abc``. IDs are unique in the world, so offline copies of a
document never create the same ID."""


class Pending:
    """A new object inside an ``undo_group``. Its ID is known when the group ends."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc
        self._id: Id | None = None

    @property
    def id(self) -> Id:
        if self._id is None:
            raise KadakasError(1, "the ID is known when the undo group ends")
        return self._id

    def object(self) -> "Obj":
        """The object, read from the session. Only after the undo group ended."""
        return self._doc.get(self.id)

    def __repr__(self) -> str:
        return f"Pending({self._id})" if self._id is not None else "Pending(?)"


ObjRef = Union["Obj", Pending, Id]


_UUID = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")


def is_id(text: object) -> bool:
    """True if ``text`` has the form of an ID (a UUID text). A name that looks like one is an ID."""
    return isinstance(text, str) and _UUID.match(text) is not None


def _id_of(ref: ObjRef) -> Id:
    return ref if isinstance(ref, str) else ref.id


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

    def modify(self, *, layer: Layer | Id | None = None, class_: Class | Id | None = None,
               attrs: JsonDict | None = None, **shape_fields: Any) -> "Obj":
        """Change shape fields (``radius=300``), the layer, the class or the attributes."""
        command: JsonDict = {"op": "modify_object", "id": self.id}
        if shape_fields:
            command["shape"] = {**self.shape, **shape_fields}
        if layer is not None:
            command["layer"] = layer if isinstance(layer, str) else layer.id
        if class_ is not None:
            command["class"] = class_ if isinstance(class_, str) else class_.id
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

    def shape(self, kind: str, *, layer: Layer | Id | None = None, class_: Class | Id | None = None,
              attrs: JsonDict | None = None, **fields: Any) -> Any:
        """Add any shape kind (see ``describe``): ``doc.add.shape("circle", center=[0, 0], radius=5)``."""
        command: JsonDict = {"op": "add_object", "shape": {"kind": kind, **fields}}
        if layer is not None:
            command["layer"] = layer if isinstance(layer, str) else layer.id
        if class_ is not None:
            command["class"] = class_ if isinstance(class_, str) else class_.id
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

    def place(self, mode: str = "single", **params: Any) -> list[Id]:
        """Place plants in one undo step. ``doc.plants.place(at=[0, 0], style="Oak")`` is one plant;
        other modes are ``path``, ``grid``, ``triangular`` and ``fill`` (see the API text of
        ``plants.place``). Returns the IDs of the new plants."""
        if self._doc.in_undo_group:
            raise KadakasError(1, "plants.place cannot join an undo group: call it outside the group")
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

    def ids(self) -> list[Id]:
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

    def create(self, name: str, cells: dict[str, Any] | None = None, **params: Any) -> Id:
        """Create a worksheet. ``cells`` maps addresses to text, numbers or ``"=formulas"``."""
        if cells is not None:
            params["cells"] = cells
        return self._doc.api.worksheet_create(name=name, **params)["id"]

    def list(self) -> Any:
        return self._doc.api.worksheet_list()

    def set_cell(self, sheet: Id | str, address: str, value: Any) -> None:
        self._doc.api.worksheet_set_cell(id=sheet, address=address, input=value)

    def evaluate(self, sheet: Id | str) -> JsonDict:
        """The calculated values: ``result["cells"]["B2"]``."""
        return self._doc.api.worksheet_evaluate(id=sheet)

    def csv(self, sheet: Id | str) -> str:
        return self._doc.api.worksheet_export(id=sheet, format="csv")["csv"]

    def place(self, sheet: Id | str, at: Point, **params: Any) -> Id:
        return self._doc.api.worksheet_place(id=sheet, at=list(at), **params)["id"]


class Events:
    """The change events of the document (``doc.events``).

    After ``subscribe()`` the program sends a message for each change: ``{"method":
    "events.change", "params": {revision, op, origin, added, changed, removed, document}}``.
    ``added``, ``changed`` and ``removed`` list the IDs of ``objects``, ``layers``, ``classes`` and
    ``resources``. ``origin`` is ``user`` (the person at the app), ``agent`` (any program that calls the API, a Python script too) or ``script`` (a plug-in object that its script made again). A message ``events.reset`` says
    that the whole document was replaced: read it again. Over stdio the messages come before the
    reply of the call that made the change, so ``take()`` has them when the call returns. A
    connection to the app also gets the changes that the user makes, at any time: use ``wait()``.
    """

    def __init__(self, doc: "Document") -> None:
        self._doc = doc
        self._callbacks: list[Any] = []

    def subscribe(self, kinds: list[str] | None = None, callback: Any = None) -> int:
        """Start the events. ``kinds`` limits them to some of ``objects``, ``layers``, ``classes``,
        ``resources`` and ``document``. ``callback(message)`` is called for each message, in the
        thread that reads it. Returns the revision of the document now."""
        if callback is not None:
            add = getattr(self._doc._transport, "add_listener", None)
            if add is None:
                raise NotImplementedError("this transport has no events")
            add(callback)
            self._callbacks.append(callback)
        params = {"kinds": kinds} if kinds is not None else None
        return self._doc.call_raw("events.subscribe", params)["revision"]

    def unsubscribe(self) -> bool:
        """Stop the events. Returns false if there was no subscription."""
        remove = getattr(self._doc._transport, "remove_listener", None)
        while self._callbacks:
            callback = self._callbacks.pop()
            if remove is not None:
                remove(callback)
        return self._doc.call_raw("events.unsubscribe")["done"]

    def take(self) -> list[JsonDict]:
        """The messages that came since the last call, oldest first."""
        drain = getattr(self._doc._transport, "drain_notifications", None)
        return drain() if drain else []

    def wait(self, timeout: float | None = None) -> list[JsonDict]:
        """Like ``take()``, and waits up to ``timeout`` seconds when there is nothing (TCP only)."""
        wait = getattr(self._doc._transport, "wait_notifications", None)
        return wait(timeout) if wait else []

    def poll(self, since_revision: int, kinds: list[str] | None = None) -> JsonDict:
        """The change sets after a revision: ``{revision, truncated, changes}``. For a client that
        cannot receive messages. If ``truncated`` is true, read the document again."""
        params: JsonDict = {"since_revision": since_revision}
        if kinds is not None:
            params["kinds"] = kinds
        return self._doc.call_raw("events.poll", params)


class Collab:
    """Shared editing with other people and agents (``doc.collab``).

    ``connect`` joins a document on a collaboration server. After that every call of the normal API
    (``doc.add``, ``execute``) works on the shared document. Other users' changes reach the document
    when you call ``sync()`` (or ``presence()`` or ``undo()``). While connected, ``doc.undo()``
    refuses: use ``doc.collab.undo()``, which takes back only your own edits.

    The token stays in memory; it is never written to a file. See ``docs/collaboration.md``."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def connect(self, url: str | None = None, token: str | None = None, doc: str | None = None, *,
                name: str | None = None, user: str | None = None, store: str | Path | None = None,
                wait: bool | float = True, **params: Any) -> JsonDict:
        """Join a shared document and return the status.

        ``url`` is the server (``ws://127.0.0.1:8787``, or ``wss://host/ws/<document>``), ``doc``
        the document ID, ``token`` the token that the server admin made for you. All three may be
        left out when the program has defaults (``KADAKAS_COLLAB_URL``, ``KADAKAS_COLLAB_TOKEN``).
        A server that is not on this computer must be allowed with ``kadakas.open(allow_collab_url=...)``
        and must use ``wss://``. ``store`` is a path without extension for the offline log.

        The shared document replaces the open one, and edits made before the client is online
        would be lost. So by default this call syncs until the client is online (``wait=True``:
        10 seconds; give a number for another limit; ``False``: do not wait). If the server is
        down, the call raises ``KadakasError``."""
        given = {"url": url, "token": token, "doc": doc, "name": name, "user": user,
                 "store": str(store) if store is not None else None}
        p: JsonDict = {k: v for k, v in given.items() if v is not None}
        p.update(params)
        status = self._doc.api.collab_connect(**p)
        if wait is not False:
            limit = 10.0 if wait is True else float(wait)
            status = self.wait_online(limit)
        return status

    def wait_online(self, timeout: float = 10.0) -> JsonDict:
        """Sync until the client is online. Raises ``TimeoutError`` otherwise."""
        end = time.monotonic() + timeout
        while True:
            status = self.sync()["status"]
            if status["state"] == "online":
                return status
            if status["state"] == "failed" or time.monotonic() > end:
                raise TimeoutError(f"the collaboration client is {status['state']}: "
                                   f"{status.get('last_error') or 'no answer from the server'}")
            time.sleep(0.01)

    def status(self) -> JsonDict:
        return self._doc.api.collab_status()

    def sync(self, **params: Any) -> JsonDict:
        """Send your changes and take the changes of others (see ``collab.sync``)."""
        return self._doc.api.collab_sync(**params)

    def sync_until(self, done: Callable[[], bool], timeout: float = 10.0, interval: float = 0.01) -> None:
        """Sync until ``done()`` is true. Raises ``TimeoutError`` after ``timeout`` seconds."""
        end = time.monotonic() + timeout
        while True:
            self.sync()
            if done():
                return
            if time.monotonic() > end:
                raise TimeoutError("the condition was not true in time")
            time.sleep(interval)

    def settle(self, timeout: float = 10.0) -> JsonDict:
        """Sync until all your edits are confirmed by the server. Returns the status."""
        end = time.monotonic() + timeout
        while True:
            status = self.sync()["status"]
            if status["state"] == "online" and status["pending"] == 0:
                return status
            if time.monotonic() > end:
                raise TimeoutError("your edits were not confirmed in time")
            time.sleep(0.01)

    def presence(self, **params: Any) -> JsonDict:
        """Set ``cursor``, ``selection`` or ``viewport`` and list the others (``peers``)."""
        return self._doc.api.collab_presence(**params)

    def undo(self) -> JsonDict:
        """Take back your own last edit; the work of others stays."""
        return self._doc.api.collab_undo()

    def redo(self) -> JsonDict:
        return self._doc.api.collab_redo()

    def disconnect(self) -> JsonDict:
        """Close the connection. Edits go on offline and join the shared document on ``connect``."""
        return self._doc.api.collab_disconnect()


class Versions:
    """Named versions of the document and the diff between them (``doc.versions``).

    A version is a saved state of the whole document. The file keeps the drawing in full and each
    version as a list of property changes, so many versions stay small. No version is ever deleted
    by itself: when a limit of the session is reached, ``create`` fails and a person chooses.
    Versions belong to the file: collaboration does not share them. ``create``, ``restore`` and
    ``delete`` are one undo step each."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def create(self, name: str, note: str | None = None, *, author: str | None = None,
               **params: Any) -> JsonDict:
        """Save the document as a version. Returns ``{version, count, bytes}``.

        If the version would go over a limit of the session, nothing is saved and nothing is
        deleted: the call raises ``KadakasError`` (code 6) and ``data`` lists the versions. Show the
        list to the person and ask which versions to delete (``delete``), or whether to raise the
        limit. Do not delete versions by yourself."""
        given = {"name": name, "note": note, "author": author}
        p: JsonDict = {k: v for k, v in given.items() if v is not None}
        p.update(params)
        return self._doc.api.version_create(**p)

    def list(self) -> JsonDict:
        """The versions, oldest first, with the limits and ``current_is_newest``."""
        return self._doc.api.version_list()

    def get(self, id: int, *, document: bool = False) -> JsonDict:
        """One version. With ``document=True`` also the document as it was (read only)."""
        if document:
            return self._doc.api.version_get(id=id, document=True)
        return self._doc.api.version_get(id=id)

    def restore(self, id: int, **params: Any) -> JsonDict:
        """Make the document equal to a version. The state before is saved as a version first."""
        return self._doc.api.version_restore(id=id, **params)

    def delete(self, id: int | Sequence[int]) -> JsonDict:
        """Delete a version, or several in one undo step. Only the versions that a person chose:
        never delete versions to make room by yourself (see ``create``). Returns
        ``{deleted: [{id, name, ...}], count}``."""
        if isinstance(id, int):
            return self._doc.api.version_delete(id=id)
        return self._doc.api.version_delete(ids=list(id))

    def repair(self, *, confirm: bool = False) -> JsonDict:
        """Keep only the part of a history that could be read. A history that could not be read
        completely is read only; this call loses the versions that could not be read, so call it
        only when the person has said so (``confirm=True``)."""
        return self._doc.api.version_repair(confirm=confirm)

    def diff(self, from_: int | str, to: int | str = "current", *, kinds: Sequence[str] | None = None,
             limit: int | None = None) -> JsonDict:
        """What differs between two versions, or between a version and the current document
        (``"current"``): things added, removed and changed by ID, with the changed property paths
        and the old and new values. ``kinds`` limits the answer to some of ``objects``, ``layers``,
        ``classes``, ``resources`` and ``document``; ``limit`` is the most things to list."""
        p: JsonDict = {"from": from_, "to": to}
        if kinds is not None:
            p["kinds"] = list(kinds)
        if limit is not None:
            p["limit"] = limit
        return self._doc.api.version_diff(**p)

    @staticmethod
    def compare_option(from_: int | str, to: int | str = "current", *, mode: str | None = None,
                       hide_unchanged: bool | None = None, unchanged: str | None = None,
                       palette: str | None = None, dark: bool | None = None,
                       ghosts: bool | None = None, previous: bool | None = None) -> JsonDict:
        """The ``compare`` option of ``render.png``, ``render.svg`` and ``sheet.render``: the
        picture shows ``to`` with the changes since ``from`` (a version ID or ``"current"``).
        Added objects are green, changed ones orange, and removed ones are red ghosts drawn from
        the old state. ``mode`` is ``"overlay"`` (default) or ``"side_by_side"``;
        ``hide_unchanged`` leaves out what did not change (or ``unchanged`` is ``"show"``,
        ``"dim"`` or ``"hide"``); ``palette`` is ``"standard"`` or ``"color_blind"``."""
        given = {"mode": mode, "hide_unchanged": hide_unchanged, "unchanged": unchanged,
                 "palette": palette, "dark": dark, "ghosts": ghosts, "previous": previous}
        option: JsonDict = {"from": from_, "to": to}
        option.update({k: v for k, v in given.items() if v is not None})
        return option

    def render_png(self, from_: int | str, to: int | str = "current",
                   path: str | Path | None = None, *, mode: str | None = None,
                   hide_unchanged: bool | None = None, palette: str | None = None,
                   **params: Any) -> bytes:
        """A PNG of the 3D model with the changes since ``from_`` tinted (see ``compare_option``).
        ``view="top"`` is the 3D top view, drawn from above, not the 2D plan (use ``render_svg``
        for the plan). Other keyword arguments go to ``render.png`` (``view``, ``width``,
        ``height``, ``projection``). ``mode`` here is the compare mode, not the render mode.
        Returns the PNG bytes, and writes them to ``path`` if given."""
        option = self.compare_option(from_, to, mode=mode, hide_unchanged=hide_unchanged,
                                     palette=palette)
        return self._doc.render_png(path, compare=option, **params)

    def render_svg(self, from_: int | str, to: int | str = "current",
                   path: str | Path | None = None, *, width: int = 800, mode: str | None = None,
                   hide_unchanged: bool | None = None, palette: str | None = None) -> str:
        """The plan as SVG with the changes since ``from`` tinted, a legend with the counts and
        a mark (+, ~, -) on each changed object (see ``compare_option``)."""
        option = self.compare_option(from_, to, mode=mode, hide_unchanged=hide_unchanged,
                                     palette=palette)
        return self._doc.render_svg(path, width, compare=option)


class Issues:
    """Review issues of the document (``doc.issues``): the BCF data model with a pin, viewpoints
    and a comment thread (REV-04, REV-07).

    An issue made through this class has the origin ``agent``. Every call is one undo step.
    Deleting the work of somebody else needs the consent of a person: ``delete`` raises
    ``KadakasError`` (reason ``confirm_needed``) until the person has said yes and you pass
    ``confirm=True``. An import of a BCF file that meets an issue with the same GUID and other
    content raises ``KadakasError`` (reason ``bcf_conflict``): ask the person, then pass
    ``on_conflict``. See ``describe()["review"]["issues"]``."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def create(self, title: str, **params: Any) -> JsonDict:
        """Make an issue. Useful params: ``description``, ``type``, ``priority``, ``assignee``,
        ``labels``, ``due`` (``YYYY-MM-DD``), ``objects`` (IDs), ``point`` (``[x, y, z]`` in mm),
        ``viewpoint`` (``{view, selection, snapshot}``) and ``capture=True`` (a viewpoint of the
        current view with a snapshot). Returns ``{id, viewpoints, issue}``."""
        return self._doc.api.issue_create(title=title, **params)

    def list(self, **params: Any) -> JsonDict:
        """The issues. Filters: ``status`` (a name or a list), ``open_only``, ``assignee``,
        ``label``, ``object``, ``text``, ``origin``; ``sort`` and ``limit``. Returns
        ``{issues, count, total, facets}``."""
        return self._doc.api.issue_list(**params)

    def get(self, id: str, *, snapshots: bool = False) -> JsonDict:
        """One issue with its viewpoints and comments. ``snapshots=True`` adds the pictures as
        base64 text."""
        if snapshots:
            return self._doc.api.issue_get(id=id, snapshots=True)
        return self._doc.api.issue_get(id=id)

    def update(self, id: str, **params: Any) -> JsonDict:
        """Change fields: ``title``, ``description``, ``type``, ``status``, ``priority``,
        ``assignee`` (``None`` clears), ``labels`` or ``add_labels`` and ``remove_labels``,
        ``due``, ``objects`` or ``add_objects`` and ``remove_objects``, ``point``."""
        return self._doc.api.issue_update(id=id, **params)

    def add_viewpoint(self, id: str, **params: Any) -> JsonDict:
        """Add a viewpoint: ``view`` (default: the current view), ``selection``, ``name``,
        ``snapshot`` (``True`` or the params of ``render.png``). Remove one with
        ``update(id, remove_viewpoints=[...])``."""
        return self._doc.api.issue_add_viewpoint(id=id, **params)

    def comment(self, id: str, text: str, **params: Any) -> JsonDict:
        """Add a comment to the thread. ``viewpoint`` names a viewpoint of the issue."""
        return self._doc.api.issue_comment(id=id, text=text, **params)

    def resolve(self, id: str, comment: str | None = None, *, status: str | None = None) -> JsonDict:
        """Set the status to ``resolved`` (default) or ``closed``, with an optional comment."""
        given = {"comment": comment, "status": status}
        return self._doc.api.issue_resolve(
            id=id, **{k: v for k, v in given.items() if v is not None})

    def delete(self, id: str | Sequence[str], *, confirm: bool = False,
               purge_snapshots: bool = False, author: str | None = None) -> JsonDict:
        """Delete an issue, or several in one undo step. The work of somebody else (another
        origin or another ``author``) needs ``confirm=True``, which you may pass only after the
        person said yes. ``purge_snapshots=True`` also removes the pictures that no other issue
        uses, in the same undo step."""
        p: JsonDict = {"id": id} if isinstance(id, str) else {"ids": list(id)}
        if confirm:
            p["confirm"] = True
        if purge_snapshots:
            p["purge_snapshots"] = True
        if author is not None:
            p["author"] = author
        return self._doc.api.issue_delete(**p)

    def export_bcf(self, path: str | Path | None = None, *, version: str = "3.0",
                   **params: Any) -> JsonDict:
        """Write a BCF file (``.bcfzip``). Without ``path`` the answer has the file as
        ``base64``. Filters as in ``list``, or ``ids``."""
        p: JsonDict = {"version": version}
        if path is not None:
            p["path"] = str(path)
        p.update(params)
        return self._doc.api.issue_export_bcf(**p)

    def import_bcf(self, path: str | Path | None = None, *, base64: str | None = None,
                   on_conflict: str | None = None, **params: Any) -> JsonDict:
        """Read a BCF file and add its issues. ``on_conflict`` is ``skip``, ``replace`` or
        ``keep_both``; leave it out to be asked (a conflict raises ``KadakasError``). Items of
        the file that do not fit (a comment, a snapshot, a camera) are also asked about: the
        call raises ``KadakasError`` (reason ``bcf_unfit``) with the list, and only after the
        person agreed may you pass ``skip_items=True``. ``dry_run=True`` shows the plan.
        ``author`` is the author of new issues that the file gives none."""
        p: JsonDict = {}
        if path is not None:
            p["path"] = str(path)
        if base64 is not None:
            p["base64"] = base64
        if on_conflict is not None:
            p["on_conflict"] = on_conflict
        p.update(params)
        return self._doc.api.issue_import_bcf(**p)


class Feedback:
    """Bug reports and feature requests for the Kadakas team (``doc.feedback``).

    File a feature request when a task needs a feature that Kadakas lacks, and a bug report when a
    method fails in a way it must not. Reports go to a local outbox. A report with attachments (a
    model excerpt, a picture of the view, the recent calls) gets the status ``needs_review``: it
    never leaves the computer until a human approves it in Help > Feedback or with
    ``kadakas feedback review``. Nothing in this class can approve or send it."""

    def __init__(self, doc: "Document") -> None:
        self._doc = doc

    def submit(self, kind: str, title: str, body: str = "", *, wanted: str | None = None,
               tried: str | None = None, method: str | None = None, error: str | None = None,
               severity: str | None = None, attach: JsonDict | bool | None = None,
               **params: Any) -> JsonDict:
        """File a report: ``kind`` is ``bug``, ``feature_request`` or ``question``. Returns
        ``{id, merged, count, status, attachments}``. Equal reports merge into a counter.
        ``attach`` changes what is attached: ``{"log": False}`` removes one part, ``False`` all,
        ``{"objects": [id]}`` or ``{"selection": True}`` names objects."""
        given = {"wanted": wanted, "tried": tried, "method": method, "error": error,
                 "severity": severity, "attach": attach}
        p: JsonDict = {"kind": kind, "title": title, "body": body}
        p.update({k: v for k, v in given.items() if v is not None})
        p.update(params)
        return self._doc.api.feedback_submit(**p)

    def bug(self, title: str, body: str = "", **params: Any) -> JsonDict:
        """File a bug report (see ``submit``)."""
        return self.submit("bug", title, body, **params)

    def feature_request(self, title: str, body: str = "", **params: Any) -> JsonDict:
        """File a feature request (see ``submit``)."""
        return self.submit("feature_request", title, body, **params)

    def list(self, **params: Any) -> JsonDict:
        """The reports in the outbox: ``{reports, needs_review, sending}``."""
        return self._doc.api.feedback_list(**params)

    def get(self, id: str) -> JsonDict:
        """One report with the kinds and sizes of its attachments."""
        return self._doc.api.feedback_get(id=id)

    def withdraw(self, id: str) -> JsonDict:
        """Take back a report that was not sent."""
        return self._doc.api.feedback_withdraw(id=id)


class Document:
    """A session with a document. Make one with ``kadakas.open()`` or ``kadakas.connect()``."""

    def __init__(self, transport: Transport) -> None:
        self._transport = transport
        self.api = Api(self.call_raw)
        self.add = Adder(self)
        self.plants = Plants(self)
        self.selection = Selection(self)
        self.report = Reports(self)
        self.worksheets = Worksheets(self)
        self.events = Events(self)
        self.collab = Collab(self)
        self.versions = Versions(self)
        self.issues = Issues(self)
        self.feedback = Feedback(self)
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

    def execute(self, command: JsonDict) -> list[Id]:
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

    def layer(self, ref: str | Layer) -> Layer:
        """A layer by ID or name."""
        for lay in self.layers:
            if lay.id == ref or lay.name == ref or (isinstance(ref, Layer) and ref.id == lay.id):
                return lay
        raise KeyError(f"there is no layer {ref!r}")

    def class_(self, ref: str | Class) -> Class:
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
            raise KadakasError(1, "create layers outside the undo group")
        (new_id,) = self.execute(cmd)
        return self.layer(new_id)

    def add_class(self, name: str, **attrs: Any) -> Class:
        if self.in_undo_group:
            raise KadakasError(1, "create classes outside the undo group")
        (new_id,) = self.execute({"op": "add_class", "name": name, "attrs": attrs})
        return self.class_(new_id)

    # -- objects and queries ------------------------------------------------------------------

    def get(self, id: Id) -> Obj:
        return Obj(self, self.api.objects_get(id=id))

    def objects(self, layer: Layer | str | None = None, class_: Class | str | None = None,
                kind: str | None = None, where: Callable[[Obj], bool] | None = None) -> list[Obj]:
        """Query objects. ``layer`` and ``class_`` take an object, an ID or a name. ``where`` is a
        function that gets each ``Obj``: ``doc.objects(kind="circle", where=lambda o: o.radius > 100)``."""
        params: JsonDict = {}
        if layer is not None:
            params["layer"] = self.layer(layer).id
        if class_ is not None:
            params["class"] = self.class_(class_).id
        if kind is not None:
            params["kind"] = kind
        found = [Obj(self, d) for d in self.api.objects_list(**params)]
        return [o for o in found if where is None or where(o)]

    query = objects

    def move(self, items: Iterable[ObjRef], dx: float, dy: float) -> None:
        self.execute({"op": "move_objects", "ids": [_id_of(i) for i in items], "delta": [dx, dy]})

    def delete(self, items: Iterable[ObjRef]) -> None:
        self.execute({"op": "delete_objects", "ids": [_id_of(i) for i in items]})

    def group(self, items: Iterable[ObjRef], name: str | None = None) -> Id:
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

    def update_from_dxf(self, path: str | Path, *, accept: str | list[str] | None = None,
                        reject: list[str] | None = None, conflicts: str | None = None,
                        **params: Any) -> JsonDict:
        """Update an imported DXF or DWG drawing from a new revision (``import.update``, IOP-23).
        ``params`` are those of ``import.update`` (``drawing``, ``old``, ``layers``, ``tolerance``,
        ``limit``, ``options``). Without ``accept`` the document is not changed: the answer lists the
        changes (``counts``, ``changes``, ``conflicts``) and ``update_id``. With ``accept`` (``"all"``
        or a list of change IDs) the changes are applied at once as one undo step with a version
        before and after (``import.apply``); the answer is that of ``import.apply`` plus the
        ``update`` that was applied. Changes that concern objects the person edited are conflicts:
        ``"all"`` skips them unless ``conflicts="overwrite"``."""
        update = self.api.import_update(path=str(path), **params)
        if accept is None:
            return update
        apply: JsonDict = {"update_id": update["update_id"], "accept": accept}
        if reject:
            apply["reject"] = reject
        if conflicts:
            apply["conflicts"] = conflicts
        applied = self.api.import_apply(**apply)
        applied["update"] = update
        return applied

    def compare_drawings(self, base: str | Path, new: str | Path, *,
                         render: JsonDict | None = None, **options: Any) -> JsonDict:
        """Compare two DXF or DWG files without importing them (``dxf.compare``, IOP-24). The answer
        has ``counts`` and ``changes``. ``render`` ({"format": "png" | "svg" | "pdf",
        "revision_clouds": bool, ...}) adds ``image`` with the overlay picture; use
        ``save_compare_image`` to write it. ``options`` are ``tolerance``, ``layers``, ``limit``."""
        params: JsonDict = {"base": str(base), "new": str(new)}
        if options:
            params["options"] = options
        if render is not None:
            params["render"] = render
        return self.api.dxf_compare(**params)

    @staticmethod
    def save_compare_image(result: JsonDict, path: str | Path) -> None:
        """Write the overlay picture of ``compare_drawings`` to ``path``."""
        image = result["image"]
        if "svg" in image:
            Path(path).write_text(image["svg"], encoding="utf-8")
        else:
            key = "png_base64" if "png_base64" in image else "pdf_base64"
            Path(path).write_bytes(base64.b64decode(image[key]))

    def export_file(self, path: str | Path, **options: Any) -> JsonDict:
        """Export to DXF, GeoJSON (or IFC when the path ends in ``.ifc``)."""
        return self.api.export_file(path=str(path), **({"options": options} if options else {}))

    def render_png(self, path: str | Path | None = None, **params: Any) -> bytes:
        """Render the 3D model (``view``, ``width``, ``height``, ``projection``, ``mode``).
        ``compare`` tints what changed since a version (see ``doc.versions.compare_option``).
        Returns the PNG bytes, and writes them to ``path`` if given."""
        data = base64.b64decode(self.api.render_png(**params)["png_base64"])
        if path is not None:
            Path(path).write_bytes(data)
        return data

    def render_svg(self, path: str | Path | None = None, width: int = 800, *,
                   compare: JsonDict | None = None) -> str:
        """Render the 2D drawing as SVG text. Writes it to ``path`` if given. ``compare`` is the
        option of ``doc.versions.compare_option``: the plan with the changes since a version."""
        params: JsonDict = {"width": width}
        if compare is not None:
            params["compare"] = compare
        svg = self.api.render_svg(**params)["svg"]
        if path is not None:
            Path(path).write_text(svg, encoding="utf-8")
        return svg

    def to_json(self) -> JsonDict:
        """The whole document as JSON."""
        return self.api.document_get()

    def __repr__(self) -> str:
        return f"Document({json.dumps(self.summary().get('objects_by_kind', {}))})"


def open(path: str | Path | None = None, *, binary: str | Path | None = None,  # noqa: A001
         allow_dir: Sequence[str | Path] = (), allow_agent_scripts: bool = False,
         allow_converters: bool = False, allow_url: Sequence[str] = (),
         allow_collab_url: Sequence[str] = (), collab_url: str | None = None, collab_token: str | None = None) -> Document:
    """Start ``kadakas serve`` and return a ``Document`` for its session.

    ``path`` is a document file to open at the start. ``allow_dir`` lists the folders that the
    session may read and write (default: the current folder). ``allow_url`` lists the servers for map
    services. ``allow_collab_url`` lists the collaboration servers that are not on this computer
    (they must use ``wss://``); ``collab_url`` is allowed without an entry.
    ``collab_url`` and ``collab_token`` are the defaults of ``doc.collab.connect``; the token goes
    to the program in its environment, not on the command line. The process ends with ``close()``."""
    args: list[str] = []
    env: dict[str, str] = {}
    for u in allow_url:
        args += ["--allow-url", u]
    for u in allow_collab_url:
        args += ["--allow-collab-url", u]
    if collab_url:
        env["KADAKAS_COLLAB_URL"] = collab_url
    if collab_token:
        env["KADAKAS_COLLAB_TOKEN"] = collab_token
    for d in allow_dir:
        args += ["--allow-dir", str(d)]
    if allow_agent_scripts:
        args.append("--allow-agent-scripts")
    if allow_converters:
        args.append("--allow-converters")
    if path is not None:
        args.append(str(path))
    return Document(StdioTransport(args, binary, env or None))


def connect(addr: str | None = None, token: str | None = None, *,
            timeout: float | None = 120.0) -> Document:
    """Connect to a running Kadakas app and see its live document.

    Without arguments the address and token come from ``KADAKAS_ADDR`` and ``KADAKAS_TOKEN``
    (the app sets them for scripts that it starts) or from the runtime file of the app.
    ``timeout`` is the time in seconds to wait for one answer (``None``: for ever). A slow call
    raises ``RequestTimeout``, and the connection stays usable."""
    host, port, tok = resolve_endpoint(addr, token)
    return Document(TcpTransport(host, port, tok, timeout))

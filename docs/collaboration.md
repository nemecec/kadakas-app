# Collaboration protocol (PLAT-07) and presence (PLAT-08)

This document describes how Kadakas clients share one document. It is written so that the
collaboration server, which lives in a separate repository (`kadakas-server`), can implement
the server side. The client side is the crate `kadakas-collab`. The server may depend on that
crate for the merge rules and the checks, or it may follow this text and write its own code.

Protocol version: **1**.

## 1. The idea

- A shared document is a **log of transactions**. The server puts them in one order and gives
  each a **sequence number**. The first has 1.
- Every client applies the same transactions in the same order with the same rules. So all
  clients end with the same shared state. There is no other merge step.
- A client shows its own edits at once (**optimistic apply**) and sends them to the server. When
  the order of the server differs, the client replays its waiting edits on the new base
  (**rebase**).
- The server does not change operations. It checks them, orders them and sends them on.
- Presence (cursors, selections, viewports) is separate. It has no sequence number and is never
  stored.

The server keeps for each shared document: the **state** (a replica, section 3), the **log** (the
transactions, at least the recent ones), the **name of the log** (`log_id`), and for each client
three small numbers (section 4.4).

The server in `crates/kadakas-collab/src/testkit.rs` (`Sequencer`) shows all of this in a few
hundred lines. It is for tests only. It has no network, accounts, permissions or storage.

## 2. Who writes: user and client

- A **user** is a person. The server knows the user from the sign-in.
- A **client** is one running program. It makes a UUID for itself, the **client ID**, and keeps it
  in its offline journal. The client ID is the **writer** of its transactions.

One user can work on two devices or in two sessions. Each has its own client ID, so their
transaction numbers never meet. A program that lost its journal starts with a new client ID. The
user is only information: it is in the entries (`user`) so that others can show who did what.

The pair (client ID, `op_id`) names a transaction. `op_id` counts up from 1 for each client ID and
is **strictly increasing**. A client never uses a number twice.

## 3. Operations and the merge rules

### 3.1 Things and properties

An operation works on one **thing**. A thing has a kind (`object`, `layer`, `class`, `resource` or
`document`) and an ID. IDs are UUIDs (version 7 for new things, so two clients never make the same
ID). The `document` kind has exactly one thing, with the nil ID. It holds the settings of the
document.

A thing has **properties**. A property is the smallest unit that can win or lose a conflict. It is
a top-level field of the JSON form of the thing, cut deeper in these places so that users who
change different parts both keep their change:

| Thing | Properties |
|---|---|
| object | `shape`, `layer`, `class`, `parent`, `order`, `attrs.<field>` (for example `attrs.pen`, `attrs.fill`), `data.<key>` (record data), and any other top-level field. A site model is cut further: `shape` (without the points and lines), `shape.points.<n>` (chunks of 256 points) and `shape.lines.<n>` (one line each). |
| class | `name`, `visible`, `attrs.<field>` |
| layer | `name`, `kind`, `visible`, `locked`, `elevation`, `scale`, `order` |
| resource | `name`, `body` |
| document | `georef`, `drawing.<field>` for each field of the drawing data, cut once more when the field is an object (`drawing.project.name`), and `drawing.sheets.<layer id>` for the page settings of each sheet layer |

The property `id` does not exist. It is the identity. Names have 1 to 256 characters.

The **shape** is one property, except for a site model (above). Moving one point of a site model sends
one chunk of 256 points, not all points. Two users who move points in different chunks both keep
their change; in the same chunk, the last writer wins. Inserting or removing a point shifts the
later chunks, which are sent again.

**Limit.** The chunks are cut by *position*, because a point has no ID in the document. If one user
inserts or removes a point while another user moves a point of the same site model, the chunks no
longer fit together: a moved point can land on a neighbour, or a point can appear twice or be lost.
The result is the same on every client (it converges), but it is not what either user meant. Points
that only move, or that are only added at the end, are safe. A later version needs stable point IDs
in the document to remove this limit. A resource body is one property too.

The picture of an **underlay** is not sent in the operation. The client sends it once as a blob
(section 4.7) and the shape holds a reference. A move or a new opacity of the underlay sends a small
operation.

### 3.2 The five operations

In JSON, the field `op` names the operation.

| `op` | Fields | Meaning |
|---|---|---|
| `create` | `target`, `props` | Make a thing with all its properties. |
| `delete` | `target` | Delete a thing. |
| `restore` | `target`, `deleted_by` | Bring back a deleted thing, if the deletion was written by `deleted_by`. |
| `set` | `target`, `prop`, `value` | Set one property. |
| `unset` | `target`, `prop` | Remove one property. |

`target` is `{"kind": "object", "id": "<uuid>"}`. A **writer** is `{"client": "<uuid>", "op": n}`.
Example:

```json
{"op": "create", "target": {"kind": "object", "id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"},
 "props": {"layer": "00000000-0000-0000-0000-000000000001",
           "class": "00000000-0000-0000-0000-000000000000",
           "shape": {"kind": "rect", "min": [0, 0], "max": [5000, 3000]}}}
```

A **transaction** is a list of operations from one user action. The operations come from the
**result** of a command, not from the command: the client compares the document with the
**shadow** (the state of the document at the last sync) for the things that the change set of the
command names (PLAT-02). This works for every command, also for scripts. A change with many things
is cut into several transactions (at most 5 000 operations and 24 MiB each, creates of layers,
classes and resources first).

### 3.3 The rules

Apply the operations of a transaction in order, with the writer of the transaction. The replica
holds, for each thing: who created it, who deleted it (if it is deleted), and for each property its
value, its last writer and whether it was removed. Deleted things stay as tombstones with their
properties.

- **create**: if the ID exists or ever existed (also deleted), drop the operation. Otherwise make
  the thing.
- **delete**: drop it if the thing is protected (the `document` thing, the first layer
  `00000000-0000-0000-0000-000000000001`, the class "None" `00000000-0000-0000-0000-000000000000`),
  unknown or already deleted. Otherwise mark the thing as deleted by the writer.
- **restore**: drop it if the thing is unknown, not deleted, or deleted by another writer than
  `deleted_by`. Otherwise it is alive again, with the properties it had.
- **set** and **unset**: drop them if the thing is unknown, deleted, or the property is `id`.
  Otherwise write the value (or the removal) with the writer.

To "drop" means the operation has no effect, and the transaction is still ordered. A dropped
operation is not an error: every replica drops the same operations.

What this gives:

- **Last writer wins per property.** The later transaction in server order wins.
- **Delete wins over edit.** Edits after a delete are dropped, also edits that the author made
  before they saw the delete.
- **Duplicate IDs are refused.** UUIDv7 makes this impossible by accident. The rule still holds, so
  a faulty client cannot overwrite a thing.
- **Layers, classes and resources** are things like objects. A reference can break when one user
  deletes a layer, class or resource while another uses it. The replica keeps the reference as it
  is. The client repairs it when it builds the document (section 3.4).
- **Not shared:** the active layer and class, the selection, the view. (The stacking order is
  shared, section 3.3.1.)
- **Plug-in objects** converge when two users regenerate them at once (section 3.3.2). Plug-in
  objects of the older script kind (Python) still make new IDs at each run, and two runs at the same
  time double the geometry until someone runs it again.

### 3.3.1 The stacking order

The z-order of objects and the order of layers are shared. Every object and every layer has the
property `order`: a text key. The lists are sorted by the key (compared as text), and by the ID for
equal keys. Between two keys there is always a third (*fractional indexing*, in the form of the
`fractional-indexing` library of Rocicorp; the code is our own, in `kadakas_collab::order`).

A key is an integer part and a fraction, written with the digits `0-9A-Za-z`. The integer part
starts with a letter that gives its length (`a` is 2 characters, `b` 3, up to `z` with 27; `Z` is 2,
`Y` 3, down to `A` with 27), so appending counts up and the key grows with the *logarithm* of the
number of appends: 1000 appends give a key of 3 characters, and so do 1000 prepends (4 at most). The
fraction never ends with `0`, and the first key is `a0`. **A key is valid** if it has only the
digits `0-9A-Za-z`, at most 1024 characters, a head letter with as many digits as it says, no `0` at
the end of the fraction, and is not the smallest integer part alone. A server must refuse an
operation that gives a key that is not valid (`validate_entity` does it: `invalid_txn`). A client
treats an invalid key as no key.

- When the user changes the order locally (`arrange` to front, back, forward or backward,
  `move_layer`, or a new object), the client looks at the list of each touched layer, keeps the
  longest run of objects whose keys already increase, and gives new keys between the neighbours to the
  others. One object that moves needs one new key. The keys are `set` operations like any other.
- Two users who insert at the same place at the same time may make the same key. Both objects stay,
  the ID decides which is first, and the client of the next change gives one of them a new key.
- The key of an object is in the field `order` of the object in the document (in `extra`), so a saved
  file keeps it. The client writes the keys into its document without a command or a change set
  (`Editor::stamp_order_keys`).
- Undo of a creation deletes the object, and its key with it. Undo of an `arrange` puts the old key
  back (when no other client changed it).
- A thing that has no key (made by an older client) sorts first, by ID, until a client gives it a
  key.

### 3.3.2 Things that a program makes again and again

The shapes of a plug-in object are made again each time the object is regenerated. They get IDs that
follow from the ID of the object, the number of the regeneration (`generation`) and their place in
the list (`Uid::derive`, UUID version 8). The data of the plug-in object (parameters, generation and
the list of shapes) is one property of the object, so the last writer wins on all three together.
Two users who regenerate the same object at the same time make the same IDs.

To make this work, `create` has a flag `merge` (false when it is left out). A create with `merge`
of a thing that exists and is alive writes the properties like `set` operations (last writer wins)
instead of being refused. A deleted thing stays deleted. The client sets the flag for objects with a
derived ID, and for nothing else. **The server must refuse `merge` on anything else** (`invalid_txn`:
the flag is only for objects with a derived ID, version 8; `validate_ops` does it). A server must accept
a `merge` create of a living thing, and also of a thing that is deleted: the merge then drops that
operation (the thing stays deleted), and the rest of the transaction is ordered as usual. It must
refuse every other create of an ID that exists or ever existed.

**Generations.** The IDs of a generation that was undone are deleted things, and their IDs stay
taken. So a client picks as the next generation the first number whose first shape ID is not in the
shared state, deleted things included. (Place, regenerate, undo, regenerate again then makes
generation 3, not 2.) If two clients see different deleted things at that moment, they can pick
different numbers and both sets of shapes stay until the next regeneration.

With different parameters at the same time, the last writer in server order wins: its parameters, its
generation, and its properties of every shape. A shape that the loser made at an index the winner did
not make stays as a leftover with a derived ID of the same generation; the next regeneration of the
object removes it.

### 3.4 From the replica to a document

The merge rules do not look at links between things. Two edits that are fine alone can make a state
that no document can have: two groups that become each other's parent, an object that moves to
another layer than its group, two resources with the same name, a viewport whose sheet layer was
deleted. If the client could not build a document from such a state, it would be stuck for good. So
building the document (`kadakas_collab::materialize`) **never fails**. It repairs with rules that
use only the content of the replica and IDs, so every client makes the same document:

- An object whose layer or class is gone goes to the first layer or the "None" class. A reference
  to a missing resource (fill pattern, line type) is dropped. A viewport shows only layers that
  exist.
- An object that is only allowed on paper (viewport, title block) goes to the sheet layer with the
  smallest ID, or to quarantine if there is none. An object that is not allowed on paper goes to the
  first layer.
- A group parent is cleared when it is gone, is not a group, or is on another layer than its child.
  A cycle of groups is broken: the object with the smallest ID in the cycle loses its parent. A chain
  that is too deep loses a parent in the same way.
- A cycle of resources (symbols, tiles, line types that use each other) is broken: the resource with
  the smallest ID goes to quarantine.
- An empty resource name becomes `Resource <last 8 characters of the ID>`. When two resources of one
  type have the same name (without case), the smaller ID keeps it, and the others get
  ` (<last 8 characters of the ID>)` added.
- The lists of objects and layers are sorted by their keys (section 3.3.1).
- **Quarantine:** a thing that does not read as its type, or fails `validate_entity`, is left out
  and reported. An object whose picture has not arrived yet (section 4.7) is left out too, and
  appears when the picture comes. It stays in the replica, so nothing is lost: a later good edit brings it back.

The repairs are not edits. They are not sent. The local shadow is the document that was built, so a
repair never comes back as a change.

## 4. Messages

Messages are JSON objects with a `type` field. Send one message per frame (a WebSocket text frame, or
one line). The transport is not part of this protocol. Use TLS in any real deployment.

### 4.1 Client to server

**`hello`**: the first message.

```json
{"type": "hello", "protocol": 1, "user": "<uuid>", "client": "<uuid>", "name": "Ann",
 "token": "…", "last_seq": 17, "log_id": "log-1"}
```

`last_seq` is the last sequence number that the client has, and `log_id` the log it comes from. Both
are left out if the client has no state yet.

**`submit`**: a transaction. `{"type": "submit", "op_id": 5, "ops": [ … ]}`

**`presence`**: what this client shows to the others.

```json
{"type": "presence", "state": {"cursor": [1200.0, 800.0], "selection": ["<uuid>"],
 "viewport": {"center": [0, 0], "scale": 2.0, "size": [1280, 800]}}}
```

All fields of `state` are optional. Numbers must be finite. `scale` is above 0. A selection has at
most 1000 IDs.

**`blob_put`**: `{"type": "blob_put", "hash": "<sha256 hex>", "data": "<text>"}`. A blob (section
4.7). The client sends it before the transaction that refers to it.

**`blob_get`**: `{"type": "blob_get", "hash": "<sha256 hex>"}`. Asks for a blob.

**`bye`**: the client leaves.

### 4.2 Server to client

**`welcome`**: the answer to a good `hello`.

```json
{"type": "welcome", "protocol": 1, "log_id": "log-1", "head_seq": 20, "your_last_op": 5,
 "peers": [{"client": "<uuid>", "user": "<uuid>", "name": "Bo", "state": {…}}]}
```

`head_seq` is the last sequence number of the log. `your_last_op` is the highest `op_id` of this
**client ID** that the server has ordered (0 if none). `peers` are the other clients online with
their latest presence.

Directly after `welcome`, the server sends **one** of these:

- **`entries`**: `{"type": "entries", "entries": [Entry, …]}`. All log entries after the client's
  `last_seq`, in order. It can be empty. Use it only when the client sent a `last_seq` and a
  `log_id` that is the log's own name, `last_seq` is not above `head_seq`, and the log still holds
  the entries after it.
- **`snapshot`**: `{"type": "snapshot", "log_id": "log-1", "seq": 20, "state": <snapshot>}`. Use it
  in every other case: the client has no state, has another `log_id`, is ahead of the server, or is
  too far behind.

A client that has no state must never get `entries`. It treats that as a violation and stops.

An **Entry** is `{"seq": 18, "author": "<client uuid>", "user": "<user uuid>", "op_id": 3,
"ops": [ … ]}`.

**`ack`**: `{"type": "ack", "op_id": 5, "seq": 21}`. Sent to the author of a transaction.

**`broadcast`**: `{"type": "broadcast", "entry": Entry}`. Sent to every other connected client.

**`presence`**: `{"type": "presence", "client": "<uuid>", "user": "<uuid>", "name": "Bo",
"state": {…}}`. Another client's presence. The server also sends it when a client joins, with an
empty `state`, so the others learn who is online.

**`presence_leave`**: `{"type": "presence_leave", "client": "<uuid>"}`.

**`blob`**: `{"type": "blob", "hash": "…", "data": "…"}`, the answer to `blob_get`. **`blob_missing`**:
`{"type": "blob_missing", "hash": "…"}`.

**`error`**: `{"type": "error", "code": "…", "message": "…", "op_id": 5}`. `op_id` is left out if
the error is not about one transaction.

### 4.3 The snapshot

A replica as JSON. Things are in a sorted list (by kind in the order `object`, `layer`, `class`,
`resource`, `document`, then by ID):

```json
{"things": [{"kind": "object", "id": "…", "created_by": {"client": "…", "op": 1},
             "deleted_by": {"client": "…", "op": 4},
             "props": {"shape": {"value": {…}, "writer": {"client": "…", "op": 2}},
                       "name": {"value": null, "writer": {…}, "removed": true}}}]}
```

`deleted_by` and `removed` are left out when they do not apply. The baseline writer, for the first
content of a document, is the nil client with number 0. Two replicas are equal when their snapshots,
as canonical JSON text, are equal.

### 4.4 The rules of the server

1. **Before `hello`** the server accepts nothing else (`bad_message`). A second `hello` is a
   `bad_message`.
2. **Protocol version.** If `protocol` differs, send `error` with `unsupported_protocol` and close.
3. **Sign-in and identity.** If the token is not good, send `error` with `auth_failed` and close.
   The server **binds `user` to the identity of the token**: a `hello` whose `user` is not the user
   of the token is `auth_failed`. The `user` in entries and presence always comes from the sign-in,
   never from a message. The client ID may be any UUID that this user uses; a server must also
   limit a client ID to one connection at a time (a second connection with the same client ID
   replaces the first, or is refused).
4. **Ordering.** For a good `submit`: give the next sequence number, apply the operations to the
   server replica with the writer (client ID, `op_id`), append the entry to the log, send `ack` to
   the author and `broadcast` to all other connected clients.
5. **No gaps.** Every connected client must receive every sequence number exactly once and in order:
   as `ack` (its own), `broadcast` (the others) or inside `entries`. The `seq` of an `ack` is the
   number after the last one that the client received. A client treats a gap as a broken connection
   and signs in again.
6. **Per-writer numbers.** For each client ID the server keeps only three things: the highest
   `op_id` it ordered (the high-water mark), the sequence number of that transaction, and a hash of
   its content (use a strong hash, such as SHA-256, of the JSON text). It does not keep a set of all
   numbers. For a `submit`:
   - `op_id` equal to the high-water mark and the same hash: answer with `ack` and the old sequence
     number. Do not order it again. (A client sends again after a lost connection.)
   - `op_id` equal with another hash, or below the mark: `error` with `op_id_reused`.
   - `op_id` above the mark: check and order it.
7. **Per-connection FIFO.** The server handles the submits of one connection in the order they
   come. If it refuses a submit with a **retry** error (`rate_limited`, `internal`), it must refuse
   every later submit of that connection with the same code, until the refused number comes again.
   Otherwise a later transaction could be ordered before the refused one, and edits that depend on
   it would be lost.
8. **Refuse before ordering** (send `error` with the `op_id`; do not use a sequence number):
   - `invalid_txn`: the transaction is malformed or breaks a limit or a value check (below);
   - `duplicate_uid`: a `create` without `merge` uses an ID that exists or ever existed in the
     replica, also twice in one transaction. (A `create` with `merge` of an ID that exists or is
     deleted is not an error: section 3.3.2.)
   - `rate_limited`: the client sends too much (for example more than the server's limit of
     transactions per second or bytes per minute). It is temporary.
9. **Presence.** Check the numbers (`bad_message` if they are not valid). Send at most one update per
   50 ms per client to the others. An update that comes faster is **held, not dropped**: the server
   keeps the latest one and sends it when the 50 ms have passed (the trailing edge), so the last
   position of a cursor always arrives. Do not store presence in the log, the snapshot or the
   document. Keep the latest state of each connected client to fill `peers` in `welcome`.
10. **Leaving.** On `bye` or when the connection ends, send `presence_leave` to the others.
11. **Log size.** The server may drop old log entries. A client that needs them gets a `snapshot`.
12. **Re-serializing.** The server may re-serialize the operations. Each value must stay equal as JSON
    (keys, strings, and numbers as numbers, with no loss of precision; `kadakas-collab` parses with
    `float_roundtrip`). Clients compare numbers as numbers.

### 4.5 Limits and value checks

The server must refuse what breaks these limits with `invalid_txn` (or close the connection if the
frame is too large to read). The crate `kadakas-collab` has the constants and the checks
(`op::validate_ops`, `validate::validate_entity`).

| Limit | Value |
|---|---|
| operations in a transaction | 20 000 |
| property name | 1 to 256 characters, not `id` |
| one property value | 32 MiB of JSON |
| one transaction | 48 MiB of JSON |
| one frame (not a snapshot) | 64 MiB |
| one snapshot frame | 1 GiB |
| nesting of a value (arrays and objects) | 64 levels |

Reading JSON must stop at a depth limit (the Rust `serde_json` stops at 128). A value must have only
finite numbers.

Value checks. Before the server orders a transaction, it checks each thing that the transaction
touches, as the core checks a command: apply the operations to a copy of the properties of the
touched things and run `validate_entity` on the result. The check reads the properties as the type
of the thing, requires finite numbers in range, and runs the checks of the file format for shapes
(arcs, thickness, viewports and so on). A thing that does not pass makes the transaction
`invalid_txn`. An edit of a thing that is not there or is deleted is not an error: the merge drops
it. This check is the first defence; the quarantine of section 3.4 is the second one.

A client that cannot read a frame from the server, or gets a value that is too deep, does not try
again in a loop. It stops with a fatal error (`bad_message`) and tells the host. Going back to the
server would bring the same frame again.

### 4.6 Errors

Every error code has a class. The client acts on the class:

| Code | Class | The client |
|---|---|---|
| `unsupported_protocol`, `auth_failed` | fatal | stops; the host must connect again on purpose |
| `bad_message`, `invalid_txn`, `duplicate_uid` | reject | drops the transaction and its undo step; its edit leaves the view |
| `rate_limited`, `internal` | retry | keeps the transaction and sends it and all later ones again after 250 ms, then 500 ms, 1 s and so on up to 30 s |
| any other code (the client reads it as `unknown`) | retry when the error has an `op_id`; otherwise the client reports it and goes on | as for a retry error, or reports it |
| `op_id_reused` | renumber | goes offline; at the next sign-in the waiting transactions get new numbers above everything the server knows |

### 4.7 Blobs

The picture of an underlay (the base64 text of a PNG, 4 KiB or more) is a *blob*. The client
replaces it in the operation by `{"$blob": "<sha256 hex>"}` (the SHA-256 of the text), keeps the text,
and sends `blob_put` first. The server checks that the hash is the SHA-256 of `data` (`bad_message`
if not) and keeps the blob; one blob is at most 32 MiB. Before it orders a transaction, the server
puts the pictures back to check the objects (section 4.5), so a transaction that refers to a blob the
server does not have is `invalid_txn`. Messages of one connection keep their order, so the blob
arrives first.

A client that finds a reference to a blob it does not have (after a snapshot or an entry) sends
`blob_get` once, and leaves the object out of its document until `blob` arrives. On `blob_missing` it
does not ask again on this connection (the list is cleared at the next sign-in). After a sign-in the client sends `blob_put` again for every picture that a
waiting transaction refers to. The checkpoint and the journal of the client hold the pictures they
need. The same picture has the same hash, so it is sent once.

**Keeping blobs.** The rule for version 1: a server keeps a blob while any property of any thing in
the replica refers to it, deleted things included (an undo can bring them back). A blob that nothing
refers to may be dropped. The client does not rely on it: it sends a picture again (`blob_put`) on
every new connection for every picture a waiting transaction refers to, and for every picture an
undo or redo step brings back, also on the same connection. A `blob_put` that fails is sent again
after the next server error. The sizes: a blob is at most 32 MiB, so a `blob_put` fits in a frame
(64 MiB). The checkpoint of a client keeps the pictures that its confirmed state (deleted things
too), its waiting transactions and its undo and redo steps refer to, and drops the others.

### 4.8 The rules of the client

- `hello` first. Then wait for `welcome` and for `entries` or `snapshot`. In that time the client
  edits only locally.
- On `welcome`: if `your_last_op` is above the client's counter, the journal is behind (it was lost
  or old). The client sets its counter to `your_last_op` and gives its waiting transactions new
  numbers above. Otherwise it drops waiting transactions up to `your_last_op`: the server has them.
- If `log_id` differs from the client's state, or `head_seq` is below the client's sequence number,
  the server went back (a restore from a backup, another log). The client waits for the
  `snapshot` and then **throws away its queue and undo steps, compares the snapshot with the state
  it had, and sends again what it had and the snapshot lacks** (acknowledged work that the server
  lost). `entries` in this case is a violation.
- On `entries` or `snapshot`: the client updates its confirmed state, then replays its waiting
  transactions on it and removes the operations that have no effect now. Then it sends the rest
  with `submit`, in order.
- Live: on `broadcast`, apply to the confirmed state and replay the waiting transactions on top. On
  `ack`, move the transaction into the confirmed state.
- A sequence number that is not the next one means a gap. The client goes offline and connects again
  with its `last_seq`.

## 5. Optimistic apply and rebase

The client keeps two replicas: **confirmed** (the server state at the sequence number the client
has) and the **view** (confirmed plus the waiting transactions). The view is what the user sees.

- A local edit goes into the view at once and into the waiting queue.
- An `ack` moves the first waiting transaction into confirmed. The view does not change.
- A `broadcast` of another client goes into confirmed. The view is built again: confirmed, then the
  waiting transactions on top. This is the rebase. It is always defined because operations are
  property-level and the rules are last-writer-wins.

The host's document is a third thing. The host builds it from the view when the view changed by
something other than the host's own edits (`Client::take_changed`), and then calls `set_shadow`. The
host finds its own edits by comparing the document with the shadow, never with the view. A diff
against the view would undo remote changes that the document does not show yet. The module
documentation of `kadakas-collab` (`client.rs`) has the details.

## 6. Per-user undo

Undo takes back **your own** edits, as **new** transactions. It never rewinds the log. It is per
client: the rules compare client IDs.

For each transaction the client records the inverse operations: the old value of each property
(`set` or `unset`), `delete` for a `create`, and `restore` for a `delete`. To undo, the client checks
the inverse operations against what it sees now, and sends those that are safe:

- A `set` or `unset` is **skipped** if another client wrote that property after you.
- The `delete` that undoes a `create` is skipped if another client wrote any property of the object,
  or if another client's work points to it (a layer they put objects on, a group they added a member
  to). It holds or carries work of somebody else, so it stays.
- A `restore` works only for your own deletion. It is skipped if the object is not deleted by the same
  transaction.
- Everything on a deleted object is skipped. Delete wins.

The undo returns what it skipped and why (`changed_by_other`, `gone`, `referenced`). Redo is the
inverse of the undo, with the same checks. A new edit of your own clears the redo steps. Edits of
other users do not. The stacks hold at most 1000 steps.

The check uses what the client has received. A change of another client that has not arrived yet is
not seen. If the server orders it before the undo, the undo overwrites it, like any later edit.
The inverse is first made when the edit is made, from the view. When the server confirms the edit, the
client makes the inverse **again**, from the confirmed state just before the transaction in server
order. So if a write of another user was ordered under yours without your client having seen it, undo
restores the value that was really underneath, not the stale one.

## 7. Offline work

A client works without a connection. The edits stay in the waiting queue, with their `op_id`. The
client saves its **journal** (the client ID, the queue, the undo stacks and the counter of `op_id`)
and its **checkpoint** (the confirmed state, its sequence number and the `log_id`) through a store.
After a restart it builds itself from them.

- The journal is saved when its text changes, and it is bounded (64 MiB): the oldest undo steps go
  first. If the waiting edits alone are too large, the client reports it and does not save.
- The checkpoint is saved at the end of each sync and when the connection ends.
- A journal or checkpoint that cannot be read, or that belongs to another user, is **reported**
  (`store_corrupt`). The client starts without it and with the client ID it was given. It never
  hides the loss.

On reconnect the client says `hello` with the `last_seq` and `log_id` of its checkpoint, catches up,
replays its queue on the new state (dropping what has no effect now, such as edits of an object that
another user deleted), and sends the queue. The identity of a transaction is (client ID, `op_id`), so
a transaction that the server ordered before the connection broke is not ordered twice.

## 8. Presence

Presence is **ephemeral**: it is not an operation, has no sequence number, is not in the log or the
snapshot, and is not saved in the document.

- A client sends at most one update per 50 ms. Updates in between merge: the latest state is sent.
- A client repeats its state every 5 seconds if nothing changed.
- A client forgets another client after 15 seconds without news, and when `presence_leave` comes.
- A client ignores presence with bad numbers or a selection of more than 1000 IDs.
- User names are cut to 64 characters.
- The server holds an update that came too soon and forwards the latest one later (section 4.4,
  rule 9).

## 9. The JSON-RPC API

An agent or a user interface can drive a collaboration client through the normal API (see
`describe`). The host gives the session an **endpoint** (`Session::set_collab_endpoint`): the place
to connect to. The core crates have no network code. A WebSocket endpoint can be written later in a
native crate behind a feature.

| Method | Purpose |
|---|---|
| `collab.status` | State of the client, the IDs, the sequence numbers, the waiting edits, the number of other clients, and the things that were quarantined. |
| `collab.connect` | Connect. The shared document replaces the open one. Refused in a session with predictable (sequential) IDs. `store` names the files of the offline log (native only). |
| `collab.disconnect` | Say goodbye. Edits go on offline. |
| `collab.sync` | Send local changes, move messages, apply changes of others to the open document. |
| `collab.presence` | Set your presence and list the others. |
| `collab.undo`, `collab.redo` | Per-user undo and redo. |

Every call that moves messages does three steps in this order: **push** the local changes (found with
the change sets of PLAT-02, compared with the shadow, cut into transactions of a safe size), **pump**
the messages, **apply** the changes of others. A truncated change log makes the push compare the
whole document with the shadow; it never reverts a remote change.

Remote changes reach the document as **one change set** with the origin `remote` (the new origin of
PLAT-02), through `Editor::apply_remote`. It lists exactly the added, changed and removed things, so
subscribers get `events.change` and keep their caches for the rest. The selection and the group
context of the session stay (objects that are gone leave the selection). The undo history of the
normal `undo` is cleared, because its steps no longer fit. While a session is connected, the normal
`undo`, `redo` and `document.load` refuse with a message that points to `collab.undo` or
`collab.disconnect`.

## 10. Not in version 1

- Creating the first shared document. The server makes it from an uploaded `.kad` file (the function
  `docsync::replica_of` reads a document into a replica).
- Permissions, roles and locking of layers.
- Cutting other big values (a long polyline, a worksheet) into parts.
- Comments and chat.
- Compression of the log and of snapshots.

Attribution: the design uses the ideas of the last-writer-wins register and of operation logs with a
central sequencer, which are common knowledge in the field of replicated data. IDs follow RFC 9562
(UUID version 7).

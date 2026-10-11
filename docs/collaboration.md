# Collaboration protocol (PLAT-07) and presence (PLAT-08)

This document describes how Kadakas clients share one document. It is written so that the
collaboration server, which lives in a separate repository (`kadakas-server`), can implement
the server side. The client side is the crate `kadakas-collab`. The server may depend on that
crate for the merge rules and the checks, or it may follow this text and write its own code.

Protocol version: **1**.

**Compatibility.** The changes since the first text are all optional, so the version stays 1. New
things are an extra error code (`document_full`; a client reads any code it does not know as a retry
error, see section 4.6), the `hash` of an `error`, `hello.last_hash`, and `chain` in `ack`, `Entry`
and `snapshot`. A new client with an old server: the old server leaves the new fields out, and the
client works as before. An old client with a new server: it ignores fields it does not know (and an
old client may stop on an error code that it does not know, so a new server sends `document_full`
only to a client that can read it, or accepts that an old client treats it as a bad message). A
new server must read a `hello` and a `submit` without the new fields. Rules 3 (one user per client
ID), 6 (the hash is `ops_digest`) and 7 (the end of the block) tighten the text of the server side;
they do not change any message.

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

The file of a **plant picture** (an `image` resource, PLT-16) is sent the same way, so changing the name of the resource or the picture of a second style sends no file. The picture of an **underlay** is not sent in the operation. The client sends it once as a blob
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

A new shared document starts from `docsync::replica_of(&Document::default())`. It has the protected
things above: the document settings, the first layer and the class "None". An empty
`Replica::new()` has none of them, so it is not a valid start.

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
are left out if the client has no state yet. The optional `last_hash` is the chain hash of the log at
`last_seq` (section 4.9); a client that does not know it leaves it out.

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
"ops": [ … ]}`, with an optional `"chain"`: the chain hash of the log after this entry (section 4.9).
The `snapshot` message has the optional `chain` too: the hash at its `seq`.

**`ack`**: `{"type": "ack", "op_id": 5, "seq": 21}`. Sent to the author of a transaction. The
optional `chain` is the chain hash of the log at `seq` (section 4.9).

**`broadcast`**: `{"type": "broadcast", "entry": Entry}`. Sent to every other connected client.

**`presence`**: `{"type": "presence", "client": "<uuid>", "user": "<uuid>", "name": "Bo",
"state": {…}}`. Another client's presence. The server also sends it when a client joins, with an
empty `state`, so the others learn who is online.

**`presence_leave`**: `{"type": "presence_leave", "client": "<uuid>"}`.

**`blob`**: `{"type": "blob", "hash": "…", "data": "…"}`, the answer to `blob_get`. **`blob_missing`**:
`{"type": "blob_missing", "hash": "…"}`.

**`error`**: `{"type": "error", "code": "…", "message": "…", "op_id": 5}`. `op_id` is left out if
the error is not about one transaction. An error about a `blob_put` (it has no `op_id`) can have a
`hash`: `{"type": "error", "code": "rate_limited", "message": "…", "hash": "<sha256 hex>"}`. A
server may also put `hash` on the errors that it sends for blocked submits (rule 7).

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
   never from a message. A client ID belongs to the first user who signs in with it (the server keeps this, also
   when the client is offline). A `hello` with a client ID of another user is `auth_failed`. A server
   must also limit a client ID to one connection at a time (a second connection with the same client ID
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
   its content. The hash is `kadakas_collab::op::ops_digest(ops)`: SHA-256 of the canonical JSON of
   the operations (keys of objects sorted, numbers in the shortest form of `serde_json`). The server
   stores it, so it must stay the same across server versions; a server that writes its own code
   must compute the same bytes. It does not keep a set of all
   numbers. For a `submit`:
   - `op_id` equal to the high-water mark and the same hash: answer with `ack` and the old sequence
     number. Do not order it again. (A client sends again after a lost connection.)
   - `op_id` equal with another hash, or below the mark: `error` with `op_id_reused`.
   - `op_id` above the mark: check and order it.
7. **Per-connection FIFO.** The server handles the submits of one connection in the order they
   come. If it refuses a submit with a **retry** error (`rate_limited`, `internal`), it must refuse
   every later submit of that connection with the same code, until a submit with the refused number, or a lower
   one, comes again. Otherwise a later transaction could be ordered before the refused one, and edits that depend on
   it would be lost. A `blob_put` refused with a retry error blocks the later submits of the
   connection with the same code, until that blob is put again. (Otherwise the next submit would
   fail as `invalid_txn`, a reject, and the edit would be lost.) The error of such a `blob_put`
   has the `hash` of the blob.
8. **Refuse before ordering** (send `error` with the `op_id`; do not use a sequence number):
   - `invalid_txn`: the transaction is malformed or breaks a limit or a value check (below);
   - `duplicate_uid`: a `create` without `merge` uses an ID that exists or ever existed in the
     replica, also twice in one transaction. (A `create` with `merge` of an ID that exists or is
     deleted is not an error: section 3.3.2.)
   - `document_full`: the transaction would make the document larger than the server allows (the
     size of the snapshot). It is a reject: the client drops the transaction.
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
`invalid_txn`. A `create` with `merge` of a living thing is checked as the current visible properties
of the thing with the properties of the `create` written over them. The crate has these checks as
functions, so a server need not copy them: `validate::check_transaction(replica, ops, blobs)`
(duplicate IDs and the value checks; `CheckError` tells which code to send),
`validate::blobs_needed(replica, ops)` (the blobs that the server must have first) and
`validate::referenced_blobs(replica)` (the blobs to keep, section 4.7). An edit of a thing that is not there or is deleted is not an error: the merge drops
it. This check is the first defence; the quarantine of section 3.4 is the second one.

**Frames that cannot be read.** A frame that is not JSON or not a known message: `bad_message`
without `op_id`, and the connection stays. A `submit` whose `ops` cannot be read: `invalid_txn` with
its `op_id` when it can be read. A frame above the limit: close with WebSocket code 1009.

A client that cannot read a frame from the server, or gets a value that is too deep, does not try
again in a loop. It stops with a fatal error (`bad_message`) and tells the host. Going back to the
server would bring the same frame again.

### 4.6 Errors

Every error code has a class. The client acts on the class:

| Code | Class | The client |
|---|---|---|
| `unsupported_protocol`, `auth_failed` | fatal | stops; the host must connect again on purpose |
| `bad_message`, `invalid_txn`, `duplicate_uid`, `document_full` | reject | drops the transaction and its undo step; its edit leaves the view |
| `rate_limited`, `internal` | retry | keeps the transaction and sends it and all later ones again after 250 ms, then 500 ms, 1 s and so on up to 30 s. If the error has a `hash`, the client puts that blob again (`blob_put`) before it sends the transactions again |
| any other code (the client reads it as `unknown`) | retry when the error has an `op_id`; otherwise the client reports it and goes on | as for a retry error, or reports it |
| `op_id_reused` | renumber | goes offline; at the next sign-in the waiting transactions get new numbers above everything the server knows |

### 4.7 Blobs

The picture of an underlay (the base64 text of a PNG, 4 KiB or more) and the file of a plant picture (the key `data` in the body of an `image` resource, 4 KiB or more; its small thumbnail stays in the operation) are *blobs*. The client
replaces it in the operation by `{"$blob": "<sha256 hex>"}` (the SHA-256 of the text), keeps the text,
and sends `blob_put` first. The server checks that the hash is the SHA-256 of `data` (`bad_message`
if not) and keeps the blob; one blob is at most 32 MiB. Before it orders a transaction, the server
puts the pictures back to check the objects (section 4.5), so a transaction that refers to a blob the
server does not have is `invalid_txn`. Messages of one connection keep their order, so the blob
arrives first.

A client that finds a reference to a blob it does not have (after a snapshot or an entry) sends
`blob_get` once, and leaves the object out of its document until `blob` arrives. Old log entries can refer to blobs that were removed later. A client that gets `blob_missing` while
it catches up keeps the thing waiting for its picture; the state at the head refers only to kept
blobs. On `blob_missing` it does not ask again on this connection (the list is cleared at the next sign-in). After a sign-in the client sends `blob_put` again for every picture that a
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

### 4.9 Log names

The name of the log (`log_id`) is a promise: **a log with this name holds every entry that the server
acknowledged under this name.** A client that comes back with the same name and a `last_seq` trusts
this and asks only for `entries` after it. If the promise breaks, the client keeps state that the
server does not have, and the two never agree again.

The server must change `log_id` whenever its log might not extend everything it acknowledged under
that name: after a restore, after loss of storage, or after a write conflict that shows the stored
head went back. A client that sees a new name takes the `snapshot` and sends again the work that the
snapshot lacks (section 4.8), so a new name costs one snapshot and loses nothing. When in doubt,
change the name.

The server cannot see a restore from a backup by itself, so it needs a way to tell. The recommended
way is the one of `kadakas-server`: a **ledger** and a **chain**.

- **The ledger.** For each document the server writes a small record outside the database, on its own
  volume that a restore of the database does not touch: the log name, the head sequence number, the
  chain hash of the head entry, and a flag *clean*. The server writes it when the document loads,
  every few seconds or every N entries, when the name changes, and at a stop (with *clean* set).
  When the document loads, the server compares the ledger with the database and gives the document a
  new name if: the names differ; the database head is below the ledger head; the heads are equal but
  the chain hashes differ; the database head is above the ledger head and the ledger is not clean (the
  server did not stop on purpose) or the chain at the ledger head differs; or the document has no
  ledger record (a missing record is a doubt). An operator can also change the name by hand
  (a command or an admin call), for example after a restore of the ledger volume. A change of name
  disconnects the clients of the document.
- **The chain.** Each entry has a chain hash, `chain_n = SHA256(chain_{n-1} || seq || author || op_id
  || ops_text)`, with `chain_0` = 32 zero bytes. The exact bytes are in the module `kadakas_collab::chain`
  (`chain_next`): `chain_{n-1}` as 32 bytes; `seq` and `op_id` as 8 bytes, big endian; `author` as its
  36 characters of lowercase text; `ops_text` as the compact JSON text of the operations that
  `chain::ops_text` writes. This is the chain of `kadakas-server` (`log_entries.chain`). Two logs with
  the same hash at a number have the same entries up to there.

**The chain on the wire (optional).** A server that keeps the chain puts it in `ack` and in each
`Entry` (`chain`, hex), and in `snapshot` (the hash at its `seq`). A client keeps the hash of its last
confirmed `seq` with its checkpoint, and sends it in `hello` as `last_hash`. The server compares
`last_hash` with its own hash at `last_seq`. If they differ, the log of the server is not the log
that the client knows, even though the name and the numbers fit: the server gives the log a **new
name** (and, as for any change of name, disconnects the other clients of the document), and the client
gets a `snapshot`. If the server no longer holds the entry at `last_seq`, it cannot compare, and the
client gets a `snapshot` anyway. All these fields are optional and have no effect when they are left
out, so old servers and old clients work with new ones (the protocol version stays 1): a client that
gets no `chain` sends no `last_hash`, and a server that gets no `last_hash` compares nothing. The test
sequencer (`Sequencer`) keeps the chain and shows the check.

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
`describe`). The host gives the session an **endpoint** (`Session::set_collab_endpoint`) or a
**factory** that makes endpoints from a URL (`Session::set_collab_endpoint_factory`). The core
crates have no network code. The WebSocket is in the crate `kadakas-collab-ws`; the program
`kadakas` gives its factory to the session (section 9.1).

| Method | Purpose |
|---|---|
| `collab.status` | State of the client, the IDs, the sequence numbers, the waiting edits, the number of other clients, and the things that were quarantined. |
| `collab.connect` | Connect. `{url, token, doc, name?, user?, store?}`. The shared document replaces the open one. Refused in a session with predictable (sequential) IDs. `store` names the files of the offline log (native only). See section 9.1. |
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

### 9.1 Connecting a program to a server (the client)

**The call.** `collab.connect {url, token, doc, store?, replace?, discard?}`:

- `url` is `ws://host:port` or `wss://host` (also `http://` and `https://`, as the same) together
  with `doc`, the document ID. A full address `ws://host:port/ws/<document ID>` works without `doc`;
  the last part must be a UUID. An address with a user name, a password, a query or a fragment is
  refused (a secret must not be in an address), and so is a path with `.` or `..` parts, `%`,
  backslashes or spaces, and a host name with odd characters.
- `token` is the token that the admin of the server made for you (a JWT). It is optional for a
  server that lets the client sign in with `hello` only.
- `store` (a path without extension) keeps the offline log in files, so the edits of a closed
  program are not lost. A third file `<store>.doc.json` says which document the store is for; a
  store of another document is refused.
- Without `url`, the call uses the endpoint that the host gave, or the default URL (below).
- The call returns at once. The connection is made in the background (name lookup, TCP, TLS and
  the upgrade, all inside 10 seconds), so `collab.connect` and `collab.sync` never block for
  seconds. The state is `connecting` until the server answered. A connection that fails shows in
  `collab.status` (`last_error`) and as a client that goes offline and tries again.

**Which document.** The whole address of the document (scheme, host, port, path) is the identity
of a connection. A call with another address starts a new client: the old document is never copied
into the new one. Edits that still wait for the old document refuse the call, unless you pass
`discard: true`. The same address again keeps the client, and so do the edits that wait.

**Wait until the client is online before you edit.** The shared document replaces the open one
when the first snapshot arrives. The program protects you from two losses:

- While the client has no snapshot yet, a call that changes the document is taken back and fails
  with a conflict error ("wait until `collab.status.state` is `online`").
- `collab.connect` refuses when the open document has changes that nobody saved (since the start,
  `file.save` or `file.open`), because the shared document would replace them. Save the document or
  pass `replace: true`.

Call `collab.sync` until `status.state` is `online`. The Python call `doc.collab.connect` does
this by default.

**Defaults and the command line.** The program sets the defaults for `collab.connect` from the
environment variables `KADAKAS_COLLAB_URL` and `KADAKAS_COLLAB_TOKEN`, or from the flags
`--collab-url` and `--collab-token` of `kadakas serve` and `kadakas mcp`. A flag shows in the list
of processes, so prefer the variables.

```
export KADAKAS_COLLAB_URL=ws://127.0.0.1:8787
export KADAKAS_COLLAB_TOKEN=<token>
kadakas serve        # then: {"method": "collab.connect", "params": {"doc": "<document ID>"}}
```

**Tokens are secrets.**

- They are kept in memory only. The client never writes a token to the journal, the checkpoint or
  any other file, and it never puts one in the status, the events, an error text or a log. The
  `Debug` text of the configuration and of `hello` hides it. The `describe()` text of an endpoint
  is the address without anything secret.
- The program removes every `KADAKAS_*TOKEN*` variable from its own environment as soon as it has
  read the flags, and it removes them again from the environment of every child process it starts
  (Python scripts, converters). A script cannot read the token.
- **The default token goes only to the default URL** (same scheme, host and port). Any other
  address, also one on this computer, needs a `token` of its own in the call, or gets none.
- A new token for the same address (the old one ran out): call `collab.connect {token}`. The
  endpoint is made again with the new token, and the next `hello` carries it.
- A token goes over `ws://` only to this computer (`127.0.0.0/8`, `::1` and the exact name
  `localhost`, which must resolve to such addresses). The transport refuses to send a token over
  `ws://` to any other host.

**The address policy.**

- A server on this computer (an IP address of `127.0.0.0/8` or `::1`, or the exact name
  `localhost`) is always allowed. A name like `foo.localhost` or `127.0.0.1.nip.io` does not
  count: it needs an entry.
- Any other server must be on the list of `--allow-collab-url` (or the variable
  `KADAKAS_COLLAB_ALLOW_URL`, a comma list): a host name, `*.domain`, or an address prefix such as
  `wss://collab.example.org/`. The default list is empty. The address of `--collab-url` is allowed
  without an entry, because the owner gave it. This is **not** the list of `--allow-url`, which is
  for map services.
- A server that is not on this computer must use `wss://`. The token would cross the network.
- A host that gave the session no WebSocket support (`kadakas-api` alone) refuses every URL.

**Signing in on the upgrade.** The client sends the token with the upgrade request as
`Authorization: Bearer <token>`. The sub-protocols `kadakas.v1, kadakas.token.<token>` are the
fall back for a proxy that does not pass the header; they are used only when the host asks for
it (`TokenMode::Auto`) and only after HTTP 400, 426 or 431. A browser always uses them. It also
sends the token in `hello`.

**A refused token ends the tries.** The answer 401 or 403 to the upgrade fails the client with
`auth_failed` (state `failed`, an event `failed`), like a refused `hello`. The session does not
try again; call `collab.connect {token}` with a new token. Other failures (the server is down, a
5xx answer, a lost network) keep the tries going.

**Frames and keep-alive.** One message is one text frame. The client sends no frame above 64 MiB
(`MAX_FRAME_BYTES`) and reads frames up to 1 GiB (`MAX_SNAPSHOT_BYTES`). It sends a WebSocket ping
every 20 s and drops a connection that was silent for 75 s. At most 4096 frames wait for the
reader; the thread stops reading while the queue is full.

**Reconnecting.** A session that connected with a `url` connects again by itself after a lost
connection, with a wait of 250 ms, 500 ms, 1 s and so on up to 30 s (the steps of section 4.6),
each wait 20% shorter or longer at random. The wait starts again at 250 ms only after the client
was online for 10 s, or got a sequence number from the server; a server that accepts the
connection and closes it at once does not reset it. The tries happen inside `collab.sync`,
`collab.presence` and `collab.undo`, so the document only changes when the caller asks for it.
`collab.status` shows `reconnecting` and `last_error`. Edits made while offline wait in the queue
and go out after the reconnect. A client that is `failed` (a refused token or protocol, a frame it
cannot read) does not try again. `collab.disconnect` stops the tries.

**The browser.** A page cannot set headers and cannot read the status of a refused upgrade: a 401
shows as a closed socket with the close code 1006, the same as a lost network. The client then
tries again with the back-off. A browser host should check the token itself (for example its
expiry).

**The crates.**

| Crate | Content |
|---|---|
| `kadakas-collab` | `Backoff` and `Reconnector` (back-off with jitter, a link that connects again, a stop after a refused token), `CollabUrl` (the address, the checks, the allow-list match, the identity), the `Transport` and `Endpoint` traits, `TransportError::Refused`. Pure. |
| `kadakas-collab-ws` | `WsEndpoint` and `WsTransport`. Native: one thread per connection that connects, reads and writes (`poll(2)` on Unix, a short wait elsewhere), rustls with the `ring` provider and the Mozilla roots of `webpki-roots` (feature `native-roots` adds the roots of the operating system; `with_root_certificate_der` adds one). Browser (`wasm32`): the `WebSocket` of the browser through `web-sys`. Feature `test-server`: an in-process WebSocket server on the reference sequencer, for tests. |
| `kadakas-api` | `Session::set_collab_endpoint_factory`, `set_collab_defaults`, `set_collab_allowed_urls`. |
| `kadakas-sys` | `process::scrub_secrets` and `remove_secrets_from_env`. |

The tests of these crates need no Docker. The tests against the real server (concurrent edits, a
restart of the server, offline edits, presence, per-user undo, a plug-in object in a session that
does not have the plug-in, a new token, a switch to another document) are in the `kadakas-server`
repository, in `tests/e2e`.

### 9.2 Collaboration in the app (the window)

The app (`kadakas-app`) has a user interface for this client. It does **not** run a second client.
Every step goes through the calls of section 9 on the one `Session` of the window, so all rules of
section 9.1 stay: the server copy replaces the open document on joining, tokens stay in memory, and
so on. The logic that needs no window is in `kadakas_ui_core::collab` (tested without a window); the
drawing is in `crates/kadakas-app/src/collab_ui.rs`.

**File > Collaboration.** The submenu has Connect..., Disconnect and a link that shows the People
palette. Both menus of the built-in profiles hold the command `file.collaboration` (a profile file may
move or drop it). The same commands are in Quick Search. There is no new key: the commands have no
default key in the Vectorworks-like profile or in the AutoCAD-like profile.

**The Connect dialog.** The fields are the server address, the document ID, the token and the name
that the others see.

- A full address `ws://host:port/ws/<document ID>` fills the document field. An address with a
  user name, a password, a query or a fragment is refused in the dialog before any call.
- The defaults come from `--collab-url` and `KADAKAS_COLLAB_URL`. The default token (`--collab-token`
  or `KADAKAS_COLLAB_TOKEN`) moves into the session when the program starts and the token variables
  leave the environment of the program, as for `kadakas serve`. The dialog never shows it: an empty
  token field means "use the default token", and the dialog says so when the address is the default
  one. The default token still goes only to the default URL (the API decides).
- `--allow-collab-url` (or `KADAKAS_COLLAB_ALLOW_URL`) works as for `kadakas serve`. A server that
  is not on this computer needs an entry (or must be the default URL) and `wss://`.
- A token (a JWT) whose `exp` claim is more than 60 s in the past (leeway for a wrong clock) is refused by the window before any call ("The token has run out"), because a browser cannot see a refused upgrade.
- The token field is a password field. The token is never written to the preferences, to the
  layout file or to a log. The dialog wipes the field, and the edit state of the field (its undo
  history), when the call is settled.
- The server wants `hello.user` to be the user of the token. The window reads the `sub` claim of the
  token (a JWT; no signature check, the server does that) and passes it as `user`.

**Unsaved work.** `collab.connect` refuses a join when the open document has changes that nobody
saved. The window shows a question ("Replace the open document?") and calls again with `replace:
true` only after the person agrees. Edits that were made offline for another document or user get a
second question and `discard: true`. The window reads these cases from the stable `reason` of the
error of the API (`unsaved_changes`, `edits_wait`, `already_connected`), never from its text. This
`reason` exists in the process only: it is **not** in the JSON-RPC answer, so an agent (Python, MCP)
sees only `kind: conflict` and must read the message or call `file.save` first. The window tells
the session when it saved the document (in the browser, the revision that the download started
with, so edits made during the download stay unsaved), so a saved document joins without a question.
The first question has the buttons Replace, Save first (the question goes, the document is saved, and
the person connects again) and Cancel. The two questions keep each other's answer, so a second
question never loses the first answer. The token stays in the dialog until the questions are
answered.

**While joining.** Until the first snapshot has arrived (`waiting_for_snapshot` in `collab.status`)
the window shows a box that blocks the window, the keys do nothing, and an edit that still gets
through is refused. This holds also in the wait between two tries of a first connection (the client
is `offline` then, but the session tries again by itself; a browser with a wrong token or a server
that is down stays here), and the box shows the "No answer" note when it lasts. The box has a
Disconnect button. While a link is active (joined, joining, waiting for the next try, or failed),
File > New, File > Open (also the file picker), the recovery of an unsaved copy after a crash, and
the browser Open are refused, because the new document would be pushed into the shared one as edits.
The recovery dialog waits (its files stay) until the link is gone. Connect stays possible during
the wait between tries, so a new token can be given. After Disconnect the person can open another
document; connecting to the same address later would send that document as edits, so the person
should connect to another document or restart the program.

**The status item.** The status bar shows `Collaboration: <state>`:

| State | Meaning |
|---|---|
| Offline | no connection is wanted |
| Connecting | the first connection is being made |
| Connected | online; the text adds "N edits waiting" when edits wait for the server |
| Reconnecting | the link was lost and the program tries again by itself |
| Refused | the server refused the token; no more tries (connect again with a new token) |
| Connection failed | another fatal error (for example a protocol version that does not match) |
| No answer | the first connection has had no answer for 20 s. A browser cannot see why an upgrade failed (a wrong token looks like a lost network), so the window says this and names the address and the token as likely causes |

A click opens the dialog (offline) or the People palette. The tooltip shows the address (no token),
the number of other people and the last error.

**Messages for errors.** Each error class of section 4.6 has a sentence in the language of the UI,
shown in the status line and kept in the People palette: a refused token, a protocol that does not
match, a full document ("the server refused your last edit, and it was taken back"), a refused edit,
a busy server, a server fault (both of these: "your edits wait and are sent again"), numbers of edits
used before, a state that went back, an offline log that could not be saved or read, and any other
code (with the code and the English text of the server in the sentence).

**Presence.** The window sends the pointer (in mm, only over the plan), the selection and the view
(center, mm per pixel, size) through `collab.presence`, about every 100 ms when something changed
(0.1 mm is not a change). The client library keeps the 50 ms limit and the repeat every 5 s. The
others are drawn on the canvas: a pointer with the name on a label in the color of the user, and a
frame and a faint fill around each selected object. A user who changed nothing for 30 s is dimmed,
and the user is gone after 15 s without news (the client expires it). The color of a user comes from a hash of the user
ID, and two users on one document get different colors while the palette (8 colors) has a free
one. The People palette (Window menu) lists you and the others with their color, a mark for a quiet
user and the number of selected objects. Presence is never in the document.

**Undo and redo.** While the session has a collaboration client (joined, joining, between two
tries, or offline after a Disconnect), Edit > Undo, Edit > Redo and their keys call `collab.undo`
and `collab.redo`, so a person never takes back the work of somebody else; the menu items follow the
`can_undo` and `can_redo` of the client. If a change was kept because another person changed it
later, the status line says so. Without a client, the normal undo works.

**Waiting edits.** Disconnect with edits that the server has not confirmed (`pending` above zero), and
closing the window, ask first. In the browser the page asks with the `beforeunload` question of the
browser. A person who disconnects keeps the edits in the program until it closes.

**The browser.** The web build reads the address fragment `#collab=<ws url>&doc=<id>&token=<t>`
(and optionally `&name=<n>`), joins at once, and removes the token pair from the address bar with
`history.replaceState`, also when the rest of the fragment is not valid. A link that is pasted into
the address bar of the open page (`hashchange`) is consumed the same way, and joins if the page is
not joined already. A fragment never goes to a
server. The address in the fragment counts as the default server for that page (the person opened the
link), and the API still requires `wss://` for any server that is not on this computer. The browser
uses the sub-protocol for the token (section 9.1).

**The test hook (web build only).** A web build made with the cargo feature `test-hook`
(`KADAKAS_WEB_FEATURES=test-hook scripts/build-web.sh`) has `window.kadakasTest` when the address of the
page also has the query `?kadakas_test=1`: `cmds` (push `JSON.stringify({id, method, params})` to
run an API call), `results` (the answers, by id) and `state` (JSON with the number of objects, the
link state as `offline`, `connecting`, `connected`, `reconnecting`, `refused` or `failed`, the other
people, and the drawing area in screen pixels). A normal web build has no hook at all. The browser test of the
`kadakas-server` repository (`scripts/test-browser.sh`) uses it only to read the state and to run
some API calls; it draws with the mouse.

**Limits.** The window does not keep an offline log in files (`store`), so edits that wait when the
program closes are lost; the viewports of the others are sent but not drawn; and the name comes from
the login name of the operating system (empty in the browser) or from the dialog.

## 10. Not in version 1

- Creating the first shared document from the content of a `.kad` file. A new, empty shared
  document starts from `docsync::replica_of(&Document::default())` (section 3.3). Uploading a file as
  the first content is later: `docsync::replica_of` reads any document into a replica, so the server
  can use it for this.
- Permissions, roles and locking of layers.
- Cutting other big values (a long polyline, a worksheet) into parts.
- Comments and chat.
- Compression of the log and of snapshots.

Attribution: the design uses the ideas of the last-writer-wins register and of operation logs with a
central sequencer, which are common knowledge in the field of replicated data. IDs follow RFC 9562
(UUID version 7).

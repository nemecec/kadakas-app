# Collaboration protocol (PLAT-07) and presence (PLAT-08)

This document describes how Kadakas clients share one document. It is written so that the
collaboration server, which lives in a separate repository (`kadakas-server`), can implement
the server side. The client side is the crate `kadakas-collab`. The server may depend on that
crate for the merge rules, or it may follow this text and write its own code.

Protocol version: **1**.

## 1. The idea

- A shared document is a **log of transactions**. The server puts them in one order and gives
  each a **sequence number**. The first has 1.
- Every client applies the same transactions in the same order with the same rules. So all
  clients end with the same document. There is no other merge step.
- A client shows its own edits at once (**optimistic apply**) and sends them to the server. When
  the order of the server differs, the client replays its waiting edits on the new base
  (**rebase**).
- The server does not read or change the operations. It checks their form, refuses duplicate IDs,
  orders them and sends them on.
- Presence (cursors, selections, viewports) is separate. It has no sequence number and is never
  stored.

The server needs to keep three things for each shared document: the **state** (a replica, see
section 3), the **log** (the transactions, at least the recent ones), and for each user the
transactions it has already ordered (to answer a repeated submit).

The server in `crates/kadakas-collab/src/testkit.rs` (`Sequencer`) shows all of this in about
300 lines. It is for tests only. It has no network, accounts, permissions or storage.

## 2. Operations

An operation works on one **thing**. A thing has a kind (`object`, `layer`, `class`, `resource`
or `document`) and an ID. IDs are UUIDs (version 7 for new things, so two clients never make the
same ID). The `document` kind has exactly one thing, with the nil ID. It holds the settings of
the document (`georef`, `drawing`).

A thing has **properties**. A property is a top-level field of the JSON form of the thing, for
example `shape`, `attrs`, `layer`, `class`, `data` and `parent` for an object, `name` and
`visible` for a layer, `name` and `body` for a resource. The property is the smallest unit that
can win or lose a conflict. The property `id` does not exist. It is the identity.

There are five operations. In JSON, the field `op` names the operation.

| `op` | Fields | Meaning |
|---|---|---|
| `create` | `target`, `props` | Make a thing with all its properties. |
| `delete` | `target` | Delete a thing. |
| `restore` | `target`, `deleted_by` | Bring back a deleted thing, if the deletion was written by `deleted_by`. |
| `set` | `target`, `prop`, `value` | Set one property. |
| `unset` | `target`, `prop` | Remove one property. |

`target` is `{"kind": "object", "id": "<uuid>"}`. A **writer** is `{"user": "<uuid>", "op": n}`:
a user and the number of one of its transactions.

Example:

```json
{"op": "create", "target": {"kind": "object", "id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"},
 "props": {"layer": "00000000-0000-0000-0000-000000000001",
           "class": "00000000-0000-0000-0000-000000000000",
           "shape": {"kind": "rect", "min": [0, 0], "max": [5000, 3000]}}}
```

A **transaction** is a list of operations from one user action (one command). It has an
`op_id`: a number that counts up from 1 for each user and is never used twice, also after a
restart. The pair (user, `op_id`) is the identity of a transaction and the **writer** of all its
operations.

The operations come from the **result** of a command, not from the command. The client compares
the document with its replica for the things that the change set of the command names (PLAT-02).
This works for every command, also for scripts, and it sends exactly the properties that changed.

Form rules (the server must check them and refuse a transaction that breaks them with
`invalid_txn`):

- A transaction has 1 to 20 000 operations.
- Property names have 1 to 128 characters. No property is named `id`.
- No `create` makes the `document` kind.

## 3. The merge rules

The replica holds, for each thing: who created it, who deleted it (if it is deleted), and for each
property its value, its last writer and whether it was removed. Deleted things stay in the replica
as tombstones with their properties.

Apply the operations of a transaction in order, with the writer of the transaction:

- **create**: if the ID exists or ever existed (also deleted), drop the operation (`duplicate`).
  Otherwise make the thing with the properties.
- **delete**: drop it if the thing is protected (the `document` thing, the first layer
  `00000000-0000-0000-0000-000000000001`, the class "None"
  `00000000-0000-0000-0000-000000000000`), unknown or already deleted. Otherwise mark the thing
  as deleted by the writer.
- **restore**: drop it if the thing is unknown, not deleted, or deleted by another writer than
  `deleted_by`. Otherwise it is alive again, with the properties it had.
- **set** and **unset**: drop them if the thing is unknown, deleted, or the property is `id`.
  Otherwise write the value (or the removal) with the writer.

To "drop" means the operation has no effect, and the transaction is still ordered. A dropped
operation is not an error. Every replica drops the same operations because they apply the same
rules in the same order.

What this gives:

- **Last writer wins per property.** The later transaction in server order wins. Two users who
  change different properties of one object both keep their change.
- **Delete wins over edit.** Edits after a delete are dropped. That includes edits that the
  author made before they saw the delete.
- **Duplicate IDs.** UUIDv7 makes this impossible by accident. The rule still holds, so a faulty
  client cannot overwrite a thing. The server must also refuse such a transaction before it
  orders it (`duplicate_uid`, see the rules of the server).
- **Layers, classes and resources** are things like objects, with the same rules. One user can
  delete a layer, class or resource while another user uses it. The replica keeps the reference
  as it is. When a client builds the document from the replica, it moves an object whose layer or
  class is gone to the first layer or the "None" class, and it clears a group parent that is gone.
  No object is lost. Renaming the same layer twice: the last name wins. Two layers with the same
  name are allowed: the ID is the identity.
- **Order.** The z-order of objects and the order of layers are not shared in version 1. A client
  keeps its own order, and new things go to the end, sorted by ID.
- **Not shared:** the active layer and the active class (personal), the selection, the view.

### The snapshot

A replica as JSON is the **snapshot**. Things are in a sorted list (by kind in the order `object`,
`layer`, `class`, `resource`, `document`, then by ID):

```json
{"things": [{"kind": "object", "id": "…", "created_by": {"user": "…", "op": 1},
             "deleted_by": {"user": "…", "op": 4},
             "props": {"shape": {"value": {…}, "writer": {"user": "…", "op": 2}},
                       "name": {"value": null, "writer": {…}, "removed": true}}}]}
```

`deleted_by` and `removed` are left out when they do not apply. The baseline writer, for the first
content of a document, is the nil user with number 0.

Two replicas are equal when their snapshots, as canonical JSON text, are equal. The tests of the
crate compare this text.

## 4. Messages

Messages are JSON objects with a `type` field. Send one message per frame (a WebSocket text
frame, or one line). The transport is not part of this protocol. Use TLS in any real deployment.

### Client to server

**`hello`**: the first message.

```json
{"type": "hello", "protocol": 1, "user": "<uuid>", "name": "Ann", "token": "…", "last_seq": 17}
```

`last_seq` is the last sequence number that the client has. It is left out if the client has no
state yet. The `token` is for the server's own sign-in (the protocol does not say what it is).

**`submit`**: a transaction.

```json
{"type": "submit", "op_id": 5, "ops": [ … ]}
```

**`presence`**: what this user shows to the others.

```json
{"type": "presence", "state": {"cursor": [1200.0, 800.0], "selection": ["<uuid>"],
 "viewport": {"center": [0, 0], "scale": 2.0, "size": [1280, 800]}}}
```

All fields of `state` are optional. Numbers must be finite. `scale` is above 0. A selection has at
most 1000 IDs.

**`bye`**: the client leaves.

### Server to client

**`welcome`**: the answer to a good `hello`.

```json
{"type": "welcome", "protocol": 1, "head_seq": 20, "your_last_op": 5,
 "peers": [{"user": "<uuid>", "name": "Bo", "state": {…}}]}
```

`head_seq` is the last sequence number of the log. `your_last_op` is the highest `op_id` of this
user that the server has ordered (0 if none). `peers` are the other users online with their
latest presence.

Directly after `welcome`, the server sends **one** of these:

- **`entries`**: `{"type": "entries", "entries": [Entry, …]}`. All log entries after the
  client's `last_seq`, in order. It can be empty. Use it when `last_seq` is not above `head_seq`
  and the log still holds the entries after it.
- **`snapshot`**: `{"type": "snapshot", "seq": 20, "state": <snapshot>}`. Use it when the client
  has no `last_seq`, is ahead of the server, or is too far behind.

An **Entry** is `{"seq": 18, "author": "<uuid>", "op_id": 3, "ops": [ … ]}`. The client is
online after it has this message.

**`ack`**: `{"type": "ack", "op_id": 5, "seq": 21}`. Sent to the author of a transaction.

**`broadcast`**: `{"type": "broadcast", "entry": Entry}`. Sent to every other connected client.

**`presence`**: `{"type": "presence", "user": "<uuid>", "name": "Bo", "state": {…}}`. Another
user's presence. The server also sends it when a user joins, with an empty `state`, so the others
learn who is online.

**`presence_leave`**: `{"type": "presence_leave", "user": "<uuid>"}`. A user went offline.

**`error`**: `{"type": "error", "code": "…", "message": "…", "op_id": 5}`. `op_id` is left out
if the error is not about one transaction.

### The rules of the server

1. **Before `hello`** the server accepts nothing else (`bad_message`). A second `hello` is a
   `bad_message`.
2. **Protocol version.** If `protocol` differs, send `error` with `unsupported_protocol` and
   close.
3. **Sign-in.** If the token is not good, send `error` with `auth_failed` and close.
4. **Ordering.** For a good `submit`: give the next sequence number, apply the operations to the
   server replica with the writer (user, `op_id`), append the entry to the log, send `ack` to the
   author and `broadcast` to all other connected clients. Every connected client must receive
   every sequence number exactly once and in order, because clients treat a gap as a broken
   connection.
5. **Repeated submit.** If (user, `op_id`) was ordered already, answer with `ack` and the old
   sequence number. Do not order it again. A client sends its waiting transactions again after a
   lost connection, so this happens.
6. **Refuse before ordering.** Send `error` with the `op_id`. Do not use a sequence number. The cases are:
   `invalid_txn` for a malformed transaction, `duplicate_uid` if a `create` uses an ID that exists
   or ever existed in the replica, also twice in one transaction.
7. **Presence.** Check the numbers (`bad_message` if they are not valid). Drop updates that come
   faster than once per 50 ms from one user. Send the rest to the other connected clients. Do not
   store presence in the log, the snapshot or the document. A server can keep the latest state of
   each connected user to fill `peers` in `welcome`.
8. **Leaving.** On `bye` or when the connection ends, send `presence_leave` to the others.
9. **Log size.** The server may drop old log entries. A client that needs them gets a `snapshot`
   instead.

Error codes: `unsupported_protocol`, `auth_failed` (both end the connection), `bad_message`,
`duplicate_uid`, `invalid_txn`, `rate_limited`, `internal`.

### The rules of the client

- `hello` first. Then wait for `welcome` and for `entries` or `snapshot`. In that time the
  client edits only locally.
- On `welcome`, drop waiting transactions with `op_id` up to `your_last_op`: the server has them.
- On `entries` or `snapshot`: the client sets or updates its confirmed state, then **replays its
  waiting transactions** on it and removes the operations that have no effect now (for example, an
  edit of an object that another user deleted). Then it sends the rest with `submit`, in order.
- Live: on `broadcast`, apply to the confirmed state and replay the waiting transactions on
  top. On `ack`, move the transaction into the confirmed state.
- A sequence number that is not the next one means a gap. The client goes offline and connects
  again with its `last_seq`.
- On `error` with an `op_id`, the client drops that transaction and its undo step. Its edit
  disappears from the view.

## 5. Optimistic apply and rebase

The client keeps two replicas: **confirmed** (the server state at the sequence number the client
has) and the **view** (confirmed plus the waiting transactions). The view is what the user sees.

- A local edit goes into the view at once and into the waiting queue.
- An `ack` moves the first waiting transaction into confirmed. The view does not change.
- A `broadcast` of another user goes into confirmed. The view is built again: confirmed, then the
  waiting transactions on top. This is the rebase. It is always defined because operations are
  property-level and the rules are last-writer-wins.

The module documentation of `kadakas-collab` (`client.rs`) has the same text with the
details.

## 6. Per-user undo

Undo takes back **your own** edits, as **new** transactions. It never rewinds the log.

For each transaction the client records the inverse operations: the old value of each property
(`set` or `unset`), `delete` for a `create`, and `restore` for a `delete`. To undo, the client
checks the inverse operations against what it sees now, and sends those that are safe:

- A `set` or `unset` is **skipped** if another user wrote that property after you.
- The `delete` that undoes a `create` is skipped if another user wrote any property of the object.
- A `restore` works only for your own deletion. It is skipped if the object is not deleted by the
  same transaction.
- Everything on a deleted object is skipped. Delete wins.

The undo returns what it skipped and why, so the user interface can say that values of other users
were kept. Redo is the inverse of the undo, with the same checks. A new edit clears the redo
steps. The stacks hold at most 1000 steps.

The check uses what the client has received. A change of another user that has not arrived yet is
not seen. If the server orders it before the undo, the undo overwrites it, like any later edit.

## 7. Offline work

A client works without a connection. The edits stay in the waiting queue, with their `op_id`.
The client can save its **journal** (the queue, the undo stacks and the counter of `op_id`) and its
**checkpoint** (the confirmed state and its sequence number) through a store. After a restart it
builds itself from them. The journal is small and is saved after each change. The checkpoint is
saved at the end of each sync and when the connection ends.

On reconnect the client says `hello` with the `last_seq` of its checkpoint, catches up, replays
its queue on the new state (dropping what has no effect now, such as edits of an object that
another user deleted), and sends the queue. The identity of a transaction is (user, `op_id`), so
a transaction that the server ordered before the connection broke is not ordered twice. The
objects in it have UUIDv7 IDs, so they are the same things on every client.

## 8. Presence

Presence is **ephemeral**: it is not an operation, has no sequence number, is not in the log or
the snapshot, and is not saved in the document.

- A client sends at most one update per 50 ms. Updates in between merge: the latest state is sent.
- A client repeats its state every 5 seconds if nothing changed.
- A client forgets another user after 15 seconds without news, and when `presence_leave` comes.
- A client ignores presence with bad numbers or a selection of more than 1000 IDs.
- User names are cut to 64 characters.

## 9. The JSON-RPC API

An agent or a user interface can drive a collaboration client through the normal API (see
`describe`). The host gives the session an **endpoint** (`Session::set_collab_endpoint`): the
place to connect to. The core crates have no network code. A WebSocket endpoint can be written
later in a native crate behind a feature.

| Method | Purpose |
|---|---|
| `collab.status` | State of the client, the user, the sequence numbers, the waiting edits and the number of other users. |
| `collab.connect` | Connect. The shared document replaces the open one. |
| `collab.disconnect` | Say goodbye. Edits go on offline. |
| `collab.sync` | Send local changes, move messages, apply changes of others to the open document. |
| `collab.presence` | Set your presence and list the others. |
| `collab.undo`, `collab.redo` | Per-user undo and redo. |

Local changes are found with the change sets of PLAT-02. When the document is rebuilt after a
change of another user, it **replaces** the open document (subscribers get `events.reset`) and the
local undo history of the normal `undo` is gone. With collaboration, use `collab.undo`.

## 10. Not in version 1

- Creating the first shared document. The server makes it from an uploaded `.kad` file (the
  function `docsync::replica_of` reads a document into a replica).
- Permissions, roles and locking of layers.
- Sharing the z-order of objects and the order of layers.
- Comments and chat.
- Compression of the log and of snapshots.

Attribution: the design uses the ideas of the last-writer-wins register and of operation logs with
a central sequencer, which are common knowledge in the field of replicated data. IDs follow
RFC 9562 (UUID version 7).

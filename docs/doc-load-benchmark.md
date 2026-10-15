# Load and save of `.kad` files: measurements

These numbers are for DOC-19 (the plain form, sections 1 to 3) and for DOC-20 to DOC-22 (the
container, section 4). They are the reasoning behind the limits for the size of a document file and
for the container. The values are decided:

- **Agents and API sessions** cannot be asked, so they have a hard limit,
  `SessionLimits::document_file_bytes`: 512 MiB by default (the document limit of the paid
  collaboration tier) and 32 MiB for `strict()` (the snapshot limit of the free tier). The error
  names the CLI flag `--max-document-mib` that raises it.
- **The desktop app** has no fixed cap for the files of the person. Above 500 MB (decimal) it asks first,
  with the memory estimate of 1.7 times the file size (the top of the range below). It refuses a
  file only when that estimate is more than the physical memory of the computer.

The values follow from the measurements below: 1.1 to 1.7 times the file size in memory, and a
1.08 GB file that opens in 8.5 s with about 1.8 GB resident.

For a container (DOC-20) the numbers change in two ways, and section 4 has them: the file on disk is
the compressed size, so the limits and the question of the app use the **uncompressed size** from
the manifest (the sum of all entries for the limit of an agent, the drawing and the history for the
estimate of the memory), and the estimate of the memory is **1.9 times** the text of the drawing and
the history, because the pictures stay in the file.

## 1. How to measure (the plain form)

The example `crates/kadakas-api/examples/doc_stream_bench.rs` makes a file, loads it and saves it. It
asserts nothing about time. Run it in a release build, one process per step, so that the peak
resident memory of the process is that of one step:

```
cargo build --release -p kadakas-api --example doc_stream_bench
/usr/bin/time -l target/release/examples/doc_stream_bench gen 100 /tmp/x/f.json
/usr/bin/time -l target/release/examples/doc_stream_bench load /tmp/x/f.json eager
/usr/bin/time -l target/release/examples/doc_stream_bench load /tmp/x/f.json slice
/usr/bin/time -l target/release/examples/doc_stream_bench save /tmp/x/f.json /tmp/x/out.json
```

The file has half of its bytes in plain objects (circles, rectangles, polylines), a quarter in site
models (20 000 points each) and a quarter in underlay pictures (2 MB of PNG data each). `eager` is
`files::open_document` with every picture read (the numbers of section 2 were made with the
reader, `reader`, of the code before DOC-20: the same work for a plain file). `slice` reads the whole
file into memory and uses `Document::from_slice`.
The file of these sections is the plain form: `gen 100 /tmp/x/f.json`.

## 2. Numbers (the plain form, before DOC-20)

One run on an Apple silicon laptop (release build, local SSD, a busy machine). Times are
indicative. "Heap peak" comes from a counting allocator. "RSS" is the peak resident memory of the
process.

| File | Step | Time | Heap peak | Peak RSS |
|---|---|---|---|---|
| 9.7 MB | load, reader | 0.08 s | 14 MB | 22 MB |
| 9.7 MB | load, slice | 0.03 s | 24 MB | 27 MB |
| 106 MB | load, reader | 0.81 s | 92 MB | 102 MB |
| 106 MB | load, slice | 0.35 s | 197 MB | 206 MB |
| 541 MB | load, reader | 4.1 s | 644 MB | 887 MB |
| 541 MB | load, slice | 1.8 s | 1 186 MB | 1 428 MB |
| 1 083 MB | load, reader | 8.5 s | 1 289 MB | 1 784 MB |
| 1 083 MB | load, slice | 3.9 s | 2 372 MB | 2 869 MB |

Save (streaming, atomic): 0.06 s, 0.37 s, 1.9 s and 3.5 s for the four files. The save adds almost
nothing to the memory of the loaded document. The test `stream_load.rs` measures 12 KB for a
93 MB file.

## 3. What the numbers say (the plain form)

- The document in memory is about 1.1 to 1.2 times the size of the pretty-printed file. A load from
  a reader adds a small buffer to that. It adds one more buffer the size of the biggest text, which
  is a picture. The old load (`read_to_string`, then a `Value`) needed three to four times the file size.
- The reader is about 2.2 times slower than the slice, and the slice needs the file size in extra
  memory. Use the reader for files and the slice for bytes that are already in memory.
- A file of 1 GB needs about 1.8 GB of resident memory to open. A file of 2 GiB can need 4 GB or
  more. This is the reason for the factor 1.7 in the question of the app (1.78 GB for 1.08 GB is
  1.65 times, 887 MB for 541 MB is 1.64 times, and the smaller files stay below 1.7), and for the
  hard limits of agent sessions, where a host shares one process between many sessions.
- The limit of a session for a free account (`strict()`) follows the snapshot limit of the free
  collaboration tier. Its memory limit (`max_memory_bytes`, 512 MiB) is above the memory that a
  32 MiB file needs (about 55 MB).

## 4. The container (DOC-20 to DOC-22)

The same example makes, loads and saves the container, and has the new steps:

```
doc_stream_bench gen  <MB> <file.kad|file.json>   the form follows the extension
doc_stream_bench load <file> lazy|eager|slice     lazy: the pictures stay in the file
doc_stream_bench save <file> <out>                load lazily, save to another path
doc_stream_bench saveover <file>                  load lazily, change one object, save over the file
```

The document is the one of section 1 (half plain objects, a quarter site models, a quarter pictures
of 2.7 MB of base64 text). The two sizes are `gen 100` and `gen 1000`: 106 MB and 1.08 GB as plain
JSON. A measure of one step is one process. The machine was busy and the memory of macOS is
compressed under pressure, so the resident size and the time of the 1 GB steps change from run to
run (the plain file loaded in 10.4 to 18.1 s and showed a peak between 0.97 and 1.78 GB, for the same
work). The **heap peak** (a counting allocator) does not change, so it is the number to compare. The
other numbers are the range of two or three runs.

### Size on disk

| Document | Plain JSON | Container | `document.json` (uncompressed, deflated) | Blobs (stored) |
|---|---|---|---|---|
| 106 MB | 106.1 MB | 36.1 MB | 82.1 MB, 12.1 MB (6.8 times) | 9 pictures, 24.0 MB |
| 1.08 GB | 1 083.2 MB | 369.8 MB | 837.8 MB, 125 MB (6.7 times) | 92 pictures, 245.3 MB |

The container is three times smaller. The pictures are stored as they are (random data here, so
deflate would not help) and `document.json` goes through deflate level 1.

### Open (load)

| Document | Step | Time | Heap peak | Peak footprint |
|---|---|---|---|---|
| 106 MB | plain JSON, before (the code of DOC-19) | 0.84 to 1.47 s | 92.2 MB | 99 to 100 MB |
| 106 MB | plain JSON, after | 0.89 to 1.02 s | 92.2 MB | 99 to 101 MB |
| 106 MB | container, **lazy** (the app, `file.open`) | 0.78 to 1.70 s | **76.6 MB** | 81 to 82 MB |
| 106 MB | container, eager (a library, an import) | 0.90 to 1.03 s | 91.8 MB | 104 MB |
| 1.08 GB | plain JSON, before | 10.9 to 13.9 s | 1 288.8 MB | 0.97 to 1.78 GB |
| 1.08 GB | plain JSON, after | 10.4 to 18.1 s | 1 288.8 MB | 0.96 to 1.79 GB |
| 1.08 GB | container, **lazy** | 9.2 to 9.6 s | **1 105.1 MB** | 0.87 to 1.60 GB |
| 1.08 GB | container, eager | 9.9 to 10.9 s | 1 289.1 MB | 1.13 to 1.79 GB |

The lazy open keeps 14 to 17 percent less in memory for this mix and is not slower, although it also
inflates the drawing. The saving is the memory of the pictures: they are 25 percent of the text but
take less in memory (a decoded picture is three quarters of its base64 text). A document that is
mostly pictures saves more. The time of the open is the parse of `document.json`; it is the same as for the plain form within the noise of the machine.

### Save

| Document | Step | Time | Heap peak (on top of the open document) |
|---|---|---|---|
| 106 MB | plain JSON, before | 0.37 to 0.40 s | 88.0 MB |
| 106 MB | container to a new path (the pictures are copied) | 0.60 to 0.69 s | 70.6 MB |
| 106 MB | container over the file it was opened from | 0.61 to 0.67 s | 70.6 MB |
| 106 MB | plain JSON to container | 0.86 to 0.91 s | 93.4 MB |
| 106 MB | container to plain JSON | 0.37 to 0.43 s | 97.0 MB |
| 1.08 GB | plain JSON, before | 5.3 to 8.1 s | 1 235.4 MB |
| 1.08 GB | container to a new path | 7.1 to 8.9 s | 1 052.1 MB |
| 1.08 GB | container over the file it was opened from | 8.1 to 9.0 s | 1 052.1 MB |
| 1.08 GB | plain JSON to container | 9.1 to 12.9 s | 1 240.8 MB |
| 1.08 GB | container to plain JSON | 3.9 to 4.2 s | 1 299.7 MB |

The save of a container is slower than the plain save by the deflate of `document.json`: about 5 s for
838 MB (about 170 MB/s at level 1, the level that is used; the `zlib-rs` backend made a bigger file,
418.6 MB against 369.8 MB, in the same time, so it is not used). The pictures cost no CPU time of
the codec: the 92 pictures (245 MB) are copied from the old file as they are, without a decode, an
encode or a hash (`Writer::copy_raw`). The save over the file adds the copy to `.bak` that every save
makes. In the plain form the pictures are read again and written again each time.

### The numbers behind the limits

- **The estimate of the memory (`open_estimate::needed_bytes_container`): 1.9 times the uncompressed
  bytes of `document.json` and the history.** The heap peak of the lazy open is 1.32 times the
  837.8 MB, and the largest footprint seen in all runs was 1.60 GB, which is 1.9 times. (The plain
  form keeps its 1.7: the largest footprint was 1.79 GB for 1 083 MB, 1.65 times.) The text of a
  container holds more objects and fewer pictures per byte than the plain file, so a byte costs
  more. The question of the app comes when this text is above 500 MB (decimal), and the refusal when
  the estimate is above the physical memory.
- **The size that counts for an agent (`SessionLimits::document_file_bytes`)** is checked against the
  file size and against the sum of the entry sizes of the manifest, and each entry against it too.
  A 36 MB container of the benchmark holds 106 MB of entries, so a limit of 64 MiB refuses it with a
  message that names the limit and the flag to raise it (the test `a_container_that_expands_beyond_the_limit_is_refused_with_the_hint`).
- **The ratio of inflation (`Limits::max_ratio`): 200 times, checked for entries above 1 MiB.** The
  benchmark document inflates 6.8 times. The most repetitive legal document that we could make (300 000
  objects that differ only in their id) inflates 95 times (circles) and 110 times (polylines). Zeros
  inflate about 1 000 times. 200 is far above any real drawing and well below a bomb. The size of
  the entries (the memory) is bounded by the total limit anyway, and a read stops at the declared
  size.
- **The entries of a container: at most 65 534** (the 16 bit field of ZIP; ZIP64 is not supported),
  **each at most 4 GiB less 2 bytes**. The directory and the manifest of the most entries are 11.7 MB
  and 13.2 MB, so their bounds are 32 MiB (`Limits::max_directory_bytes`, `max_manifest_bytes`). A
  container of 65 533 entries is written in 0.33 s and opened in 0.18 s (`cargo test --release -p
  kadakas-container perf_ -- --ignored --nocapture`).
- **Many pictures.** A container with 20 000 pictures of 12 KB (247 MB) is written in 1.9 s and opened
  in 2.8 s (the best of the runs on a busy machine; `cargo test --release -p kadakas-file perf_ --
  --ignored --nocapture`). The open reads the first 8 characters of each picture, which is two small
  reads of the file for each. The first version cloned the directory for each of them and took
  minutes for 20 000: `Container` now shares its entries, so a copy costs a pointer.
- **A picture (a blob) is at most 32 MiB of text** (`MAX_BLOB_TEXT`, the bound of collaboration). A
  picture is at most 8 MiB, which is 11.2 MiB of base64 text.
- **Not done: binary arrays** for the points of site models and for meshes. The measurement says
  where the bytes are: the site models of the benchmark are a quarter of the text and deflate 7 times
  in `document.json`, so binary arrays would save little on disk. They would save parse time
  (the numbers are text today), but they need new (de)serialization of each shape with a version of
  its own. That is not simple, so it is a follow-up with its own measurement of the parse time.

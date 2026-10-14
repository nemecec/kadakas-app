# Load and save of `.kad` files: measurements

These numbers are for DOC-19. They are the reasoning behind the limits for the size of a document
file. The values are decided:

- **Agents and API sessions** cannot be asked, so they have a hard limit,
  `SessionLimits::document_file_bytes`: 512 MiB by default (the document limit of the paid
  collaboration tier) and 32 MiB for `strict()` (the snapshot limit of the free tier). The error
  names the CLI flag `--max-document-mib` that raises it.
- **The desktop app** has no fixed cap for the files of the person. Above 500 MB it asks first,
  with the memory estimate of 1.7 times the file size (the top of the range below). It refuses a
  file only when that estimate is more than the physical memory of the computer.

The values follow from the measurements below: 1.1 to 1.7 times the file size in memory, and a
1.08 GB file that opens in 8.5 s with about 1.8 GB resident.

## How to measure

The example `crates/kadakas-api/examples/doc_stream_bench.rs` makes a file, loads it and saves it. It
asserts nothing about time. Run it in a release build, one process per step, so that the peak
resident memory of the process is that of one step:

```
cargo build --release -p kadakas-api --example doc_stream_bench
/usr/bin/time -l target/release/examples/doc_stream_bench gen 100 /tmp/x/f.kad
/usr/bin/time -l target/release/examples/doc_stream_bench load /tmp/x/f.kad reader
/usr/bin/time -l target/release/examples/doc_stream_bench load /tmp/x/f.kad slice
/usr/bin/time -l target/release/examples/doc_stream_bench save /tmp/x/f.kad /tmp/x/out.kad
```

The file has half of its bytes in plain objects (circles, rectangles, polylines), a quarter in site
models (20 000 points each) and a quarter in underlay pictures (2 MB of PNG data each). `reader` is
`files::read_document` (what Open uses). `slice` reads the whole file into memory and uses
`Document::from_slice`.

## Numbers

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

## What the numbers say

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

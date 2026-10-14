# Load and save of `.kad` files: measurements

These numbers are for DOC-19. They help to choose the limit for the size of a document file
(`SessionLimits::document_file_bytes`). The default is 2 GiB and `strict()` has 128 MiB. Both values
are provisional.

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
  more. The limit for a free account (`strict()`) must follow the memory limit of the session.

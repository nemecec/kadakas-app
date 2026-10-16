# Vendored quickjs-ng

This folder holds the C source of **quickjs-ng 0.17.0**, a maintained fork of QuickJS by Fabrice
Bellard and Charlie Gordon. License: MIT (`LICENSE`). Kadakas builds it to WebAssembly in
`../build.rs` (see `../c/shim.c`). The source is vendored so that builds work offline and the text
can be reviewed.

| | |
|---|---|
| Upstream | https://github.com/quickjs-ng/quickjs |
| Tag | `v0.17.0` |
| Archive | https://github.com/quickjs-ng/quickjs/archive/refs/tags/v0.17.0.tar.gz |
| SHA-256 of the archive | `559bc4c420475e55c7ab4510adbc562f55d7524d75e8e89d79ce4bb02f5687d9` |

The files are copied from the archive without change: `quickjs.c`, `quickjs.h`, `cutils.h`,
`dtoa.c`, `dtoa.h`, `libregexp.c`, `libregexp.h`, `libregexp-opcode.h`, `libunicode.c`,
`libunicode.h`, `libunicode-table.h`, `list.h`, `quickjs-atom.h`, `quickjs-c-atomics.h`,
`quickjs-opcode.h`, `builtin-array-fromasync.h`, `builtin-iterator-zip.h`,
`builtin-iterator-zip-keyed.h` and `LICENSE`. Not copied: `quickjs-libc.c` (files, processes,
the clock and timers: the script engine has none of them), the command line programs, the
tests and the tools.

## Update

1. Download the archive of the new tag, check it against the checksum of the release, and
   unpack it in an empty folder.
2. Copy the files above over the files in this folder, and change the table above.
3. Update `docs/attribution.md` and `docs/notices-extra.txt` (version and copyright lines), then
   run `scripts/notices.sh`.
4. Build and run the tests: `cargo test -p kadakas-script-js --features engine`.

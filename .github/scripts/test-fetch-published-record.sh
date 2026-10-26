#!/usr/bin/env bash
# Tests fetch-published-record.sh with a fake `gh`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fails=0
mkdir -p "$tmp/bin"
# MODE: none (no release), empty (no asset), has (the asset), error (a 502).
cat > "$tmp/bin/gh" <<'EOT'
#!/usr/bin/env bash
case "$1 $2" in
  "release view")
    case "$MODE" in
      none) echo "release not found" >&2; exit 1 ;;
      error) echo "HTTP 502: Bad Gateway" >&2; exit 1 ;;
      empty) echo "kadakas-update-linux.tar.gz"; echo "manifest-beta.json" ;;
      has) echo "manifest-beta.json"; echo "highest-published.json" ;;
    esac ;;
  "release download")
    while [ $# -gt 0 ]; do if [ "$1" = "--dir" ]; then d="$2"; fi; shift; done
    if [ "${DOWNLOAD_FAILS:-}" = 1 ]; then echo "HTTP 502" >&2; exit 1; fi
    echo '{"beta": "0.1.0-beta.1"}' > "$d/highest-published.json" ;;
esac
EOT
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH"
check() { # label mode want-exit want-file [download-fails]
  local got=0
  rm -rf "$tmp/d"
  MODE="$2" DOWNLOAD_FAILS="${5:-}" "$here/fetch-published-record.sh" "$tmp/d" >/dev/null 2>&1 || got=$?
  local file=no; [ -s "$tmp/d/highest-published.json" ] && file=yes
  if [ "$got" = "$3" ] && [ "$file" = "$4" ]; then echo "ok: $1"; else echo "FAIL: $1 (exit $got, file $file)" >&2; fails=$((fails + 1)); fi
}
check "no release: missing record is fine" none 0 no
check "release without the asset: missing record is fine" empty 0 no
check "release with the asset: downloaded" has 0 yes
check "a 502 on the listing stops" error 1 no
check "a failed download stops" has 1 no 1
[ "$fails" = 0 ] || exit 1
echo "all fetch-published-record tests passed"

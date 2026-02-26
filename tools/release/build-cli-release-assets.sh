#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --version vX.Y.Z --out-dir dist" >&2
  exit 1
}

version=""
out_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --out-dir) out_dir="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$version" && -n "$out_dir" ]] || usage

mkdir -p "$out_dir"

./tools/release/build-cli-release-asset.sh --version "$version" --goos linux --goarch amd64 --out-dir "$out_dir"
./tools/release/build-cli-release-asset.sh --version "$version" --goos linux --goarch arm64 --out-dir "$out_dir"
./tools/release/build-cli-release-asset.sh --version "$version" --goos darwin --goarch amd64 --out-dir "$out_dir"
./tools/release/build-cli-release-asset.sh --version "$version" --goos darwin --goarch arm64 --out-dir "$out_dir"

(
  cd "$out_dir"
  : > checksums.txt
  for f in *.tar.gz; do
    sha256sum "$f" >> checksums.txt
  done
)

echo "built release assets in ${out_dir}"

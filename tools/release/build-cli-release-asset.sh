#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --version vX.Y.Z --goos <linux|darwin> --goarch <amd64|arm64|arm> [--goarm 7] --out-dir <dir>" >&2
  exit 1
}

version=""
goos=""
goarch=""
goarm=""
out_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --goos) goos="${2:-}"; shift 2 ;;
    --goarch) goarch="${2:-}"; shift 2 ;;
    --goarm) goarm="${2:-}"; shift 2 ;;
    --out-dir) out_dir="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$version" && -n "$goos" && -n "$goarch" && -n "$out_dir" ]] || usage

version_nov="${version#v}"
arch_label="$goarch"
if [[ "$goarch" == "arm" ]]; then
  [[ -n "$goarm" ]] || { echo "--goarm required for arm" >&2; exit 1; }
  arch_label="armv${goarm}"
fi

stem="surf_${version_nov}_${goos}_${arch_label}"
mkdir -p "$out_dir"

tmp_dir="$(mktemp -d)"
stage="${tmp_dir}/${stem}"
mkdir -p "$stage"

envs=("CGO_ENABLED=0" "GOOS=${goos}" "GOARCH=${goarch}")
if [[ -n "$goarm" ]]; then
  envs+=("GOARM=${goarm}")
fi

env "${envs[@]}" go build -trimpath -buildvcs=false -ldflags "-s -w" -o "${stage}/surf" ./cmd/surf
chmod 0755 "${stage}/surf"
[[ -f README.md ]] && cp README.md "${stage}/README.md"

if [[ -f LICENSE ]]; then
  cp LICENSE "${stage}/LICENSE"
fi

tar -C "${tmp_dir}" -czf "${out_dir}/${stem}.tar.gz" "${stem}"
rm -rf "$tmp_dir"

echo "built ${out_dir}/${stem}.tar.gz"

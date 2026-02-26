#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --tag vX.Y.Z" >&2
  exit 1
}

tag=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

[[ -n "$tag" ]] || usage

actual="$(sed -n 's/^const surfVersion = "\(.*\)"$/\1/p' cmd/surf/version.go)"
if [[ -z "$actual" ]]; then
  echo "unable to parse surf version from cmd/surf/version.go" >&2
  exit 1
fi

if [[ "$actual" != "$tag" ]]; then
  echo "version mismatch: cmd/surf/version.go=${actual}, tag=${tag}" >&2
  exit 1
fi

echo "release tag and cmd/surf/version.go are aligned (${tag})"

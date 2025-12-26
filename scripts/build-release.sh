#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <target> <version>" >&2
  exit 1
fi

target="$1"
version="$2"
version="${version#v}"

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="${root_dir}/dist"

mkdir -p "$dist_dir"

cargo build --locked --release --target "$target"

bin_path="${root_dir}/target/${target}/release/actr"
if [[ ! -f "$bin_path" ]]; then
  echo "Binary not found: $bin_path" >&2
  exit 1
fi

asset_name="actr-${version}-${target}"
tar_path="${dist_dir}/${asset_name}.tar.gz"

tar -czf "$tar_path" -C "$(dirname "$bin_path")" actr

sha_path="${tar_path}.sha256"
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$tar_path" | awk '{print $1}' > "$sha_path"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$tar_path" | awk '{print $1}' > "$sha_path"
else
  echo "Missing sha256 tool (shasum or sha256sum)." >&2
  exit 1
fi

echo "Built ${tar_path}"

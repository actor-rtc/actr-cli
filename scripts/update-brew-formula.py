#!/usr/bin/env python3
import argparse
from pathlib import Path
import re
import sys


def read_sha(dist_dir: Path, asset_name: str) -> str:
    sha_path = dist_dir / f"{asset_name}.sha256"
    if not sha_path.exists():
        raise FileNotFoundError(f"Missing sha256 file: {sha_path}")
    return sha_path.read_text().strip().split()[0]


def replace_after_marker(text: str, marker: str, url: str, sha: str) -> str:
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if marker in line:
            url_index = None
            for j in range(i + 1, len(lines)):
                if lines[j].lstrip().startswith("url "):
                    indent = re.match(r"(\s*)url", lines[j]).group(1)
                    lines[j] = f'{indent}url "{url}"'
                    url_index = j
                    break
            if url_index is None:
                raise ValueError(f"url line not found after marker: {marker}")

            for k in range(url_index + 1, len(lines)):
                if lines[k].lstrip().startswith("sha256 "):
                    indent = re.match(r"(\s*)sha256", lines[k]).group(1)
                    lines[k] = f'{indent}sha256 "{sha}"'
                    return "\n".join(lines)
            raise ValueError(f"sha256 line not found after marker: {marker}")
    raise ValueError(f"Marker not found: {marker}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--formula", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--dist", required=True)
    args = parser.parse_args()

    version = args.version.lstrip("v")
    tag = f"v{version}"

    dist_dir = Path(args.dist)
    formula_path = Path(args.formula)
    text = formula_path.read_text()

    targets = {
        "macos-arm64": "aarch64-apple-darwin",
        "linux-x86_64": "x86_64-unknown-linux-gnu",
    }

    assets = {}
    for label, target in targets.items():
        asset = f"actr-{version}-{target}.tar.gz"
        assets[label] = {
            "asset": asset,
            "url": f"https://github.com/{args.repo}/releases/download/{tag}/{asset}",
            "sha": read_sha(dist_dir, asset),
        }

    text, count = re.subn(
        r'^(\s*version\s+")([^"]+)(")',
        rf"\1{version}\3",
        text,
        flags=re.MULTILINE,
    )
    if count == 0:
        raise ValueError("version line not found in formula")

    text = replace_after_marker(
        text,
        "# TARGET: macos-arm64",
        assets["macos-arm64"]["url"],
        assets["macos-arm64"]["sha"],
    )
    text = replace_after_marker(
        text,
        "# TARGET: linux-x86_64",
        assets["linux-x86_64"]["url"],
        assets["linux-x86_64"]["sha"],
    )

    formula_path.write_text(text)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        raise

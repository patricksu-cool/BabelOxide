"""Stage installed runtime packages and record portable release provenance."""

import argparse
import hashlib
import importlib.metadata
import json
import shutil
from pathlib import Path


def file_sha256(path: Path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stage_packages(source: Path, destination: Path):
    """Copy runtime dependencies without environment hooks or test/build caches."""
    shutil.copytree(
        source,
        destination,
        ignore=shutil.ignore_patterns(
            "__pycache__",
            "*.pyc",
            "*.pyo",
            ".pytest_cache",
            "tests",
            "test",
            "_tests",
            "test_*.py",
            "bert_perf_test.py",
            "onnxruntime_test.py",
            "direct_url.json",
            "_virtualenv.py",
            "_virtualenv.pth",
        ),
    )


def finalize_bundle(bundle: Path):
    """Keep license notices with dependencies and hash the shipped files."""
    for name in (
        "config",
        "gradio_files",
        "pdf2zh_files",
        ".env",
        "credentials.json",
        "secrets.json",
    ):
        if (bundle / name).exists():
            raise ValueError(f"Release contains local state: {name}")
    packages = []
    for dist in importlib.metadata.distributions(path=[str(bundle / "site-packages")]):
        license_text = (
            dist.metadata.get("License-Expression")
            or dist.metadata.get("License")
            or "See distribution metadata and bundled notices"
        )
        packages.append(
            {
                "name": dist.metadata["Name"],
                "version": dist.version,
                "license": license_text,
                "license_files": [
                    str(dist.locate_file(f).relative_to(bundle)).replace("\\", "/")
                    for f in dist.files or []
                    if "license" in str(f).lower() and dist.locate_file(f).is_file()
                ],
            }
        )
    packages.sort(key=lambda p: p["name"].lower())
    (bundle / "THIRD_PARTY_COMPONENTS.json").write_text(
        json.dumps(packages, indent=2) + "\n", encoding="utf-8"
    )
    records = [
        {
            "path": p.relative_to(bundle).as_posix(),
            "bytes": p.stat().st_size,
            "sha256": file_sha256(p),
        }
        for p in sorted(bundle.rglob("*"))
        if p.is_file() and p.name != "RELEASE_FILES.json"
    ]
    (bundle / "RELEASE_FILES.json").write_text(
        json.dumps(records, indent=2) + "\n", encoding="utf-8"
    )
    return len(records), sum(r["bytes"] for r in records)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--finalize", action="store_true")
    args = parser.parse_args()
    if args.finalize:
        print(finalize_bundle(args.bundle))
    elif args.source:
        stage_packages(args.source, args.bundle / "site-packages")
    else:
        parser.error("--source is required for staging")


if __name__ == "__main__":
    main()

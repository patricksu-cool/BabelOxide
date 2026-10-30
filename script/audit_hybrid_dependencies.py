"""Query public OSV advisories for the pinned Windows build; development only."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
API = "https://api.osv.dev/v1/"


def request(endpoint: str, payload: dict | None = None):
    data = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(  # noqa: S310 -- fixed HTTPS OSV origin; no user URL
        API + endpoint,
        data=data,
        headers={"Content-Type": "application/json", "User-Agent": "pdf2zh-build-audit"},
    )
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=45) as response:  # noqa: S310 -- fixed HTTPS origin
                return json.load(response)
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
            if attempt == 2:
                raise
            time.sleep(attempt + 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    python_source = parser.add_mutually_exclusive_group(required=True)
    python_source.add_argument("--worker-wheels", type=Path, help="Captured bundle provenance/WHEELS.json, including Worker security replacements")
    python_source.add_argument("--legacy-engine", action="store_true", help="Explicitly audit the unchanged legacy Python inventory")
    args = parser.parse_args()
    metadata = json.loads(args.metadata.read_text(encoding="utf-8-sig"))
    node_ids = {node["id"] for node in metadata["resolve"]["nodes"]}
    versions = {
        ("crates.io", package["name"], package["version"])
        for package in metadata["packages"]
        if package["id"] in node_ids
        and (package.get("source") or "").startswith("registry+")
    }
    if args.worker_wheels:
        python_input = args.worker_wheels
        wheels = json.loads(python_input.read_text(encoding="utf-8-sig"))
        if not isinstance(wheels, list) or len(wheels) != 85:
            raise ValueError("Expected the captured 85-distribution Worker wheel list")
        engine = {item["name"]: item for item in wheels}
        if len(engine) != len(wheels):
            raise ValueError("Duplicate Worker distribution")
    else:
        python_input = ROOT / "docs/rust-migration/PYTHON_DEPENDENCY_INVENTORY.json"
        engine = json.loads(python_input.read_text(encoding="utf-8"))["engine"]
    for name, item in engine.items():
        if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name) or not re.fullmatch(r"[A-Za-z0-9.+!-]+", item["version"]):
            raise ValueError("Invalid public distribution name/version")
        versions.add(("PyPI", name, item["version"]))
    packages = [
        {"package": {"ecosystem": ecosystem, "name": name}, "version": version}
        for ecosystem, name, version in sorted(versions)
    ]
    args.output.mkdir(parents=True, exist_ok=False)
    hits = []
    for offset in range(0, len(packages), 50):
        pending = [(query, query.copy()) for query in packages[offset : offset + 50]]
        while pending:
            result = request("querybatch", {"queries": [query for original, query in pending]})
            results = result["results"]
            if len(results) != len(pending):
                raise RuntimeError("OSV result count differs from query count")
            following = []
            for (original, _query), item in zip(pending, results, strict=True):
                for vulnerability in item.get("vulns", []):
                    hits.append({**original, **vulnerability})
                if token := item.get("next_page_token"):
                    following.append((original, {**original, "page_token": token}))
            pending = following
    records = {}
    for identifier in sorted({hit["id"] for hit in hits}):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", identifier):
            raise RuntimeError("OSV returned an invalid advisory identifier")
        records[identifier] = request("vulns/" + urllib.parse.quote(identifier, safe=""))
    result = {
        "queried_at_utc": datetime.datetime.now(datetime.UTC).isoformat(),
        "source": "https://api.osv.dev/v1/querybatch",
        "scope": "Windows Cargo resolve graph registry packages and explicitly selected Python closure; not native binaries or proof of reachability",
        "python_input": {"name": python_input.name, "sha256": hashlib.sha256(python_input.read_bytes()).hexdigest(), "profile": "worker" if args.worker_wheels else "legacy"},
        "query_count": len(packages),
        "counts": {
            ecosystem: sum(query["package"]["ecosystem"] == ecosystem for query in packages)
            for ecosystem in ["crates.io", "PyPI"]
        },
        "packages": packages,
        "matches": hits,
        "advisories": records,
    }
    (args.output / "osv.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    summary = {
        "query_count": len(packages),
        "counts": result["counts"],
        "matched_packages": len({(hit["package"]["ecosystem"], hit["package"]["name"], hit["version"]) for hit in hits}),
        "advisories": len(records),
        "matches": [{"package": hit["package"], "version": hit["version"], "id": hit["id"]} for hit in hits],
    }
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()

"""Collect reproducible PDF structure and text metrics with Poppler."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


def poppler(command: str, path: Path, *options: str) -> str:
    result = subprocess.run(
        [command, *options, str(path), *(["-"] if command == "pdftotext" else [])],
        capture_output=True,
        check=True,
    )
    return result.stdout.decode("utf-8", errors="replace")


def metrics(path: Path) -> dict:
    info = poppler("pdfinfo", path)
    page_count = int(re.search(r"^Pages:\s*(\d+)", info, re.MULTILINE).group(1))
    text = poppler("pdftotext", path, "-enc", "UTF-8", "-layout")
    page_texts = text.split("\f")[:page_count]
    if len(page_texts) != page_count:
        raise ValueError(f"Could not extract all {page_count} pages from {path}")

    image_counts = [0] * page_count
    for row in poppler("pdfimages", path, "-list").splitlines()[2:]:
        fields = row.split()
        if fields and fields[0].isdigit():
            image_counts[int(fields[0]) - 1] += 1

    pages = [
        {
            "text_characters": len(page_text),
            "words": len(page_text.split()),
            "text_sha256": hashlib.sha256(page_text.encode("utf-8")).hexdigest(),
            "formula_symbols": len(re.findall(r"[=≤≥±∑∆µ∫∂]", page_text)),
            "embedded_images": image_counts[index],
        }
        for index, page_text in enumerate(page_texts)
    ]
    return {
        "file": path.name,
        "bytes": path.stat().st_size,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "page_count": page_count,
        "pages": pages,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("pdfs", type=Path, nargs="+")
    args = parser.parse_args()
    result = {path.name: metrics(path) for path in args.pdfs}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()

"""Compare Poppler-rendered page images from two PDF runs."""

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def compare_pages(baseline_prefix: Path, candidate_prefix: Path, page_count: int):
    pages = []
    for page_number in range(1, page_count + 1):
        baseline = np.asarray(
            Image.open(f"{baseline_prefix}-{page_number}.png").convert("RGB"),
            dtype=np.int16,
        )
        candidate = np.asarray(
            Image.open(f"{candidate_prefix}-{page_number}.png").convert("RGB"),
            dtype=np.int16,
        )
        if baseline.shape != candidate.shape:
            raise ValueError(f"Page {page_number} dimensions differ")
        difference = np.abs(baseline - candidate)
        pages.append(
            {
                "page": page_number,
                "changed_pixel_fraction": float(np.any(difference != 0, axis=2).mean()),
                "mean_channel_difference": float(difference.mean()),
            }
        )
    return pages


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline_prefix", type=Path)
    parser.add_argument("candidate_prefix", type=Path)
    parser.add_argument("--pages", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    pages = compare_pages(args.baseline_prefix, args.candidate_prefix, args.pages)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(pages, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()

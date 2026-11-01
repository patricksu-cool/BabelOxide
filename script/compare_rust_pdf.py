"""Compare controlled old/new PDFs by structure, coordinates and Poppler rasters."""

import argparse
import hashlib
import json
import shutil
import subprocess
from pathlib import Path

import numpy as np
import pymupdf
from PIL import Image


def signature(document):
    pages = []
    for page in document:
        characters = []
        for block in page.get_text("rawdict")["blocks"]:
            for line in block.get("lines", []):
                for span in line["spans"]:
                    characters.extend(
                        [char["c"], [round(value, 4) for value in char["bbox"]]]
                        for char in span["chars"]
                    )
        images = [
            {
                "bbox": [round(value, 4) for value in image["bbox"]],
                "width": image["width"],
                "height": image["height"],
                "digest": image["digest"].hex(),
            }
            for image in page.get_image_info(hashes=True)
        ]
        pages.append(
            {
                "text": page.get_text(), "characters": characters, "images": images,
                "geometry": list(page.rect),
                "fonts": [
                    [list(font[1:]), hashlib.sha256(document.extract_font(font[0])[3]).hexdigest()]
                    for font in page.get_fonts(full=True)
                ],
            }
        )
    return pages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    renderer = shutil.which("pdftoppm")
    if renderer is None:
        parser.error("pdftoppm is required for the rendering comparison")
    render = args.output.parent / (args.output.stem + "-render")
    render.mkdir(parents=True, exist_ok=True)
    results = []
    for mode in ("mono", "dual"):
        old = next(args.baseline.glob(f"*.{mode}.pdf"))
        new = next(args.candidate.glob(f"*.{mode}.pdf"))
        with pymupdf.open(old) as left, pymupdf.open(new) as right:
            a, b = signature(left), signature(right)
        record = {
            "mode": mode,
            "page_counts": [len(a), len(b)],
            "text_equal": [page["text"] for page in a] == [page["text"] for page in b],
            "character_coordinates_equal": [page["characters"] for page in a]
            == [page["characters"] for page in b],
            "images_equal": [page["images"] for page in a]
            == [page["images"] for page in b],
            "fonts_equal": [page["fonts"] for page in a] == [page["fonts"] for page in b],
            "geometry_equal": [page["geometry"] for page in a] == [page["geometry"] for page in b],
            "render": [],
        }
        for label, path in (("baseline", old), ("candidate", new)):
            subprocess.run(
                [
                    renderer,
                    "-r",
                    "96",
                    "-png",
                    str(path),
                    str(render / f"{label}-{mode}"),
                ],
                check=True,
                capture_output=True,
            )
        for index in range(1, min(len(a), len(b)) + 1):
            with Image.open(render / f"baseline-{mode}-{index}.png") as image:
                old_pixels = np.array(image.convert("RGB"), dtype=np.int16)
            with Image.open(render / f"candidate-{mode}-{index}.png") as image:
                new_pixels = np.array(image.convert("RGB"), dtype=np.int16)
            if old_pixels.shape != new_pixels.shape:
                record["render"].append({"page": index, "same_dimensions": False})
                continue
            difference = np.abs(old_pixels - new_pixels)
            record["render"].append(
                {
                    "page": index,
                    "same_dimensions": True,
                    "changed_pixel_fraction": float(np.any(difference, axis=2).mean()),
                    "mean_channel_difference": float(difference.mean()),
                }
            )
        results.append(record)
    report = {"renderer": "Poppler 96 DPI RGB", "runs": results}
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return int(
        any(
            item["page_counts"][0] != item["page_counts"][1]
            or not all(
                item[key]
                for key in ("text_equal", "character_coordinates_equal", "images_equal", "fonts_equal", "geometry_equal")
            )
            or any(row.get("changed_pixel_fraction", 1) != 0 for row in item["render"])
            for item in results
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())

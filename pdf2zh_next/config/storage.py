"""TOML value conversion and atomic file persistence.

Each writer owns a temporary file beside its destination. Concurrent processes
may replace one another's complete settings; they never share a temporary file.
"""

import os
import tempfile
import time
from pathlib import Path

import tomlkit


def decode_toml_content(content: dict) -> dict:
    return {
        key: decode_toml_content(value)
        if isinstance(value, dict)
        else None
        if isinstance(value, str) and value == "null"
        else value
        for key, value in content.items()
    }


def encode_toml_content(content: dict) -> dict:
    return {
        key: "null"
        if value is None
        else encode_toml_content(value)
        if isinstance(value, dict)
        else value
        for key, value in content.items()
    }


def write_toml_atomic(file_path: Path, content: dict) -> None:
    """Replace a complete TOML file, preserving the old file on write failure."""
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=file_path.parent,
            prefix=f".{file_path.name}.",
            suffix=".temp.toml",
            delete=False,
        ) as stream:
            temporary_path = Path(stream.name)
            tomlkit.dump(encode_toml_content(content), stream)
            stream.flush()
            os.fsync(stream.fileno())
        # Windows can briefly deny replacement while another writer or scanner
        # holds the destination. Never fall back to truncating the old file.
        for attempt in range(5):
            try:
                temporary_path.replace(file_path)
                break
            except PermissionError as exc:
                if getattr(exc, "winerror", None) not in {5, 32, 33} or attempt == 4:
                    raise
                time.sleep(0.01 * 2**attempt)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)

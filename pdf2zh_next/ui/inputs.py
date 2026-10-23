"""Prepare GUI uploads and URL downloads without constructing the interface."""

import cgi
import shutil
from pathlib import Path

import gradio as gr
import requests


def _get_unique_dest_path(output_dir: Path, original_name: str) -> Path:
    """
    Get a unique destination path in the given output directory.

    This will try the original name first, then append an incrementing
    suffix like "_2", "_3", etc. until an unused name is found.
    """
    output_dir = Path(output_dir)
    stem = Path(original_name).stem
    suffix = Path(original_name).suffix

    # First try the original name
    candidate = output_dir / f"{stem}{suffix}"
    if not candidate.exists():
        return candidate

    # Increment index until we find an unused name
    index = 2
    while True:
        candidate = output_dir / f"{stem}_{index}{suffix}"
        if not candidate.exists():
            return candidate
        index += 1


def download_with_limit(url: str, save_path: str, size_limit: int = None) -> str:
    """
    This function downloads a file from a URL and saves it to a specified path.

    Inputs:
        - url: The URL to download the file from
        - save_path: The path to save the file to
        - size_limit: The maximum size of the file to download

    Returns:
        - The path of the downloaded file
    """
    chunk_size = 1024
    total_size = 0
    with requests.get(url, stream=True, timeout=10) as response:
        response.raise_for_status()
        content = response.headers.get("Content-Disposition")
        try:  # filename from header
            un_used, params = cgi.parse_header(content)
            filename = params["filename"]
        except Exception:  # filename from url
            filename = Path(url).name
        filename = Path(filename).stem + ".pdf"
        save_path = Path(save_path).resolve()

        # Use a unique destination path to avoid overwriting existing files
        file_path = _get_unique_dest_path(save_path, filename)
        if not file_path.resolve().is_relative_to(save_path):
            raise gr.Error("Invalid filename")
        with file_path.open("wb") as file:
            for chunk in response.iter_content(chunk_size=chunk_size):
                total_size += len(chunk)
                if size_limit and total_size > size_limit:
                    raise gr.Error("Exceeds file size limit")
                file.write(chunk)
    return file_path


def _prepare_input_file(
    file_type: str,
    file_input: str | list,
    link_input: str,
    output_dir: Path,
    state: dict | None = None,
) -> list[Path]:
    """
    This function prepares the input file(s) for translation.

    Inputs:
        - file_type: The type of file to translate (File or Link)
        - file_input: The path to the file to translate or list of paths
        - link_input: The link to the file to translate
        - output_dir: The directory to save the file to

    Returns:
        - A list of paths of the input files
    """
    prepared_files = []

    if file_type == "File":
        # Handle normal case: file_input is provided by the File component
        if file_input:
            # Handle single file or multiple files
            if isinstance(file_input, str | Path):
                inputs = [file_input]
            else:
                inputs = file_input

            for f_in in inputs:
                # Gradio provides file paths as NamedString or str in temp
                src_path = Path(f_in.name if hasattr(f_in, "name") else f_in)
                # Use original filename without UUID prefix as requested, but avoid collisions
                dest_name = src_path.name
                dest_path = _get_unique_dest_path(output_dir, dest_name)
                shutil.copy(src_path, dest_path)
                prepared_files.append(dest_path)
        # Fallback case: rely on state when File component value is empty
        elif state and state.get("uploaded_files") and state.get("display_map"):
            for original_name in state.get("uploaded_files", []):
                src_str = state["display_map"].get(original_name)
                if not src_str:
                    continue
                src_path = Path(src_str)
                if not src_path.exists():
                    # Skip missing temp files; user may need to re-upload
                    continue
                dest_name = src_path.name
                dest_path = _get_unique_dest_path(output_dir, dest_name)
                shutil.copy(src_path, dest_path)
                prepared_files.append(dest_path)

        if not prepared_files:
            # Still nothing prepared: propagate original error
            raise gr.Error("No file input provided")

    else:
        if not link_input:
            raise gr.Error("No link input provided")
        try:
            file_path = download_with_limit(link_input, output_dir)
            prepared_files.append(Path(file_path))
        except Exception as e:
            raise gr.Error(f"Error downloading file: {e}") from e

    return prepared_files

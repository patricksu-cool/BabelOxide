"""Reproduce upstream synchronous Blocks loop ownership without this application.

Run with the project Python. Exit 1 means resource warnings were reproduced;
the JSON retains every warning rather than suppressing the diagnostic.
"""

import argparse
import asyncio
import gc
import json
import warnings
from pathlib import Path

import gradio as gr
from gradio.utils import safe_get_lock
from gradio.utils import safe_get_stop_event


def reproduce():
    with warnings.catch_warnings(record=True) as observed:
        warnings.simplefilter("always", ResourceWarning)
        safe_get_lock()
        first = asyncio.get_event_loop()
        safe_get_stop_event()
        second = asyncio.get_event_loop()
        record = {
            "gradio": gr.__version__,
            "replaces_loop": first is not second,
            "first_loop_closed_after_replacement": first.is_closed(),
            "second_loop_closed": second.is_closed(),
        }
        # These two explicitly observed loops belong to this diagnostic.
        first.close()
        second.close()
        asyncio.set_event_loop(None)
        for _ in range(2):
            with gr.Blocks() as demo:
                gr.Textbox()
            demo.close()
            del demo
        gc.collect()
        record["warnings"] = [
            {"category": type(w.message).__name__, "message": str(w.message)}
            for w in observed
        ]
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = reproduce()
    text = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(text, encoding="utf-8")
    print(text, end="")
    return int(bool(result["warnings"]))


if __name__ == "__main__":
    raise SystemExit(main())

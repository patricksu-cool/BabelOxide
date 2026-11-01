"""Parse the Actions YAML and verify distribution/security contracts."""
from pathlib import Path
import yaml


def main():
    root = Path(__file__).resolve().parents[1]
    workflow = yaml.load((root / ".github/workflows/build.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    assert set(workflow["on"]) == {"push", "pull_request", "workflow_dispatch"}
    assert workflow["permissions"] == {"contents": "read"}
    job = workflow["jobs"]["build"]
    entries = job["strategy"]["matrix"]["include"]
    assert {(row["runner"], row["platform"])
            for row in entries} == {("windows-latest", "windows-x64"), ("macos-latest", "macos-arm64")}
    assert "continue-on-error" not in job
    actions = [step["uses"] for step in job["steps"] if "uses" in step]
    assert set(actions) == {"actions/checkout@v6", "actions/setup-python@v6", "actions/cache@v5", "actions/upload-artifact@v6"}
    upload = next(step for step in job["steps"] if step.get("uses") == "actions/upload-artifact@v6")
    assert upload["with"]["retention-days"] == "7"
    assert upload["with"]["if-no-files-found"] == "error"
    assert upload["with"]["path"].endswith("-${{ matrix.platform }}.zip")
    assert upload["with"]["name"].startswith("BabelOxide-v")
    assert not any("release" in action.lower() or "push" in action.lower() for action in actions)
    print("build.yml parsed; matrix, permissions, maintained Node24 Actions, artifacts and retention contracts passed")


if __name__ == "__main__":
    main()

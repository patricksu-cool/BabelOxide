"""Parse the Actions YAML and verify distribution/security contracts."""
from pathlib import Path
import yaml


def main():
    root = Path(__file__).resolve().parents[1]
    workflow = yaml.load((root / ".github/workflows/build.yml").read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    for path in sorted((root / ".github/workflows").iterdir()):
        if path.suffix in {".yml", ".yaml"}:
            candidate = yaml.load(path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
            assert isinstance(candidate["on"], dict) and set(candidate["on"]) == {"workflow_dispatch"}, path.name
    assert workflow["permissions"] == {"contents": "read"}
    job = workflow["jobs"]["build"]
    entries = job["strategy"]["matrix"]["include"]
    assert {(row["runner"], row["platform"])
            for row in entries} == {("windows-latest", "windows-x64"), ("macos-latest", "macos-arm64")}
    assert "continue-on-error" not in job
    actions = [step["uses"] for step in job["steps"] if "uses" in step]
    assert set(actions) == {"actions/checkout@v6", "actions/setup-python@v6", "actions/cache@v5", "actions/upload-artifact@v6", "actions/upload-artifact@v4"}
    upload = next(step for step in job["steps"] if step.get("uses") == "actions/upload-artifact@v6")
    assert upload["with"]["retention-days"] == "7"
    assert upload["with"]["if-no-files-found"] == "error"
    assert upload["with"]["path"].endswith("-${{ matrix.platform }}.zip")
    assert upload["with"]["name"].startswith("BabelOxide-v")
    logs = next(step for step in job["steps"] if step.get("name") == "Upload CI logs")
    assert logs["uses"] == "actions/upload-artifact@v4"
    assert logs["if"] == "${{ always() }}"
    assert logs["with"]["path"] == ".test-output/ci/"
    assert logs["with"]["include-hidden-files"] == "true"
    assert logs["with"]["retention-days"] == "7"
    assert logs["with"]["if-no-files-found"] == "warn"
    assert "${{ matrix.platform }}" in logs["with"]["name"]
    assert "${{ github.run_attempt }}" in logs["with"]["name"]
    assert job["steps"][-1] is logs
    assert all("continue-on-error" not in step for step in job["steps"])
    assert not any("release" in action.lower() or "push" in action.lower() for action in actions)
    print("Workflows are manual-only; matrix, read-only permissions, complete ZIP and always-uploaded hidden CI logs with 7-day retention passed")


if __name__ == "__main__":
    main()

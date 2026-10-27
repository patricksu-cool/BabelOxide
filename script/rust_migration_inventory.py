"""Write source import and installed engine dependency evidence; development only."""

import ast
import importlib.metadata
import json
from collections import defaultdict
from pathlib import Path

import tomllib
from packaging.markers import default_environment
from packaging.requirements import Requirement
from packaging.utils import canonicalize_name

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/rust-migration"


def dependency_closure(requirements):
    environment = default_environment()
    pending = [(Requirement(item), "application-root") for item in requirements]
    visited = defaultdict(set)
    distributions = {}
    parents = defaultdict(set)
    while pending:
        requirement, parent = pending.pop()
        name = canonicalize_name(requirement.name)
        parents[name].add(parent)
        extras = set(requirement.extras) | {""}
        if extras <= visited[name]:
            continue
        visited[name].update(extras)
        distribution = importlib.metadata.distribution(name)
        distributions[name] = {
            "version": distribution.version,
            "license": (
                distribution.metadata.get("License-Expression")
                or distribution.metadata.get("License")
                or "not declared"
            )[:240],
        }
        for raw in distribution.requires or []:
            child = Requirement(raw)
            if child.marker and not any(
                child.marker.evaluate({**environment, "extra": extra})
                for extra in visited[name]
            ):
                continue
            pending.append((child, name))
    return {
        name: {
            **data,
            "parents": sorted(parents[name]),
            "active_extras": sorted(visited[name] - {""}),
        }
        for name, data in sorted(distributions.items())
    }


def module_graph():
    modules = {}
    for path in sorted((ROOT / "pdf2zh_next").rglob("*.py")):
        parts = list(path.relative_to(ROOT).with_suffix("").parts)
        package = parts[-1] == "__init__"
        if package:
            parts.pop()
        modules[".".join(parts)] = (path, package)
    edges = set()
    external = defaultdict(set)
    dynamic = []
    for module, (path, package) in modules.items():
        tree = ast.parse(path.read_text(encoding="utf-8-sig"))
        current_package = module if package else module.rsplit(".", 1)[0]
        for node in ast.walk(tree):
            targets = []
            if isinstance(node, ast.Import):
                targets = [alias.name for alias in node.names]
            elif isinstance(node, ast.ImportFrom):
                if node.level:
                    prefix = current_package.split(".")
                    base = ".".join(prefix[: len(prefix) - node.level + 1])
                    base = ".".join(filter(None, [base, node.module]))
                else:
                    base = node.module or ""
                targets = [
                    f"{base}.{alias.name}"
                    if f"{base}.{alias.name}" in modules
                    else base
                    for alias in node.names
                ]
            elif isinstance(node, ast.Call):
                name = (
                    node.func.id
                    if isinstance(node.func, ast.Name)
                    else getattr(node.func, "attr", "")
                )
                if name in {"__import__", "import_module"}:
                    literal = (
                        node.args[0].value
                        if node.args and isinstance(node.args[0], ast.Constant)
                        else None
                    )
                    dynamic.append(
                        {
                            "module": module,
                            "line": node.lineno,
                            "literal": literal if isinstance(literal, str) else None,
                        }
                    )
            for target in targets:
                if target in modules:
                    edges.add((module, target))
                elif target:
                    external[module].add(target.split(".", 1)[0])
    return {
        "scope": "all pdf2zh_next Python source; static imports, not runtime execution",
        "modules": {
            name: str(path.relative_to(ROOT)).replace("\\", "/")
            for name, (path, _) in sorted(modules.items())
        },
        "edges": [{"from": source, "to": target} for source, target in sorted(edges)],
        "external_import_roots": {
            name: sorted(values) for name, values in sorted(external.items())
        },
        "dynamic_import_sites": dynamic,
    }


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    graph = module_graph()
    (OUTPUT / "PYTHON_MODULE_GRAPH.json").write_text(
        json.dumps(graph, indent=2) + "\n", encoding="utf-8"
    )
    names = {name: f"m{index}" for index, name in enumerate(graph["modules"])}
    mermaid = ["flowchart LR"]
    mermaid.extend(f'    {identifier}["{name}"]' for name, identifier in names.items())
    mermaid.extend(
        f"    {names[edge['from']]} --> {names[edge['to']]}" for edge in graph["edges"]
    )
    (OUTPUT / "PYTHON_MODULE_GRAPH.mmd").write_text(
        "\n".join(mermaid) + "\n", encoding="utf-8"
    )
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    environment = default_environment()
    roots = [
        item
        for item in project["project"]["dependencies"]
        if not Requirement(item).marker
        or Requirement(item).marker.evaluate(environment)
    ]
    original = dependency_closure(roots)
    original[canonicalize_name(project["project"]["name"])] = {
        "version": project["project"]["version"],
        "license": project["project"]["license"],
        "parents": [],
        "active_extras": [],
    }
    engine = dependency_closure(["babeldoc==0.6.3"])
    evidence = {
        "environment": environment,
        "engine": engine,
        "original_application": original,
        "original_only": sorted(set(original) - set(engine)),
        "scope": "installed metadata closure, BabelDOC without optional GPU extras; does not certify clean runtime packaging or vulnerability absence",
    }
    (OUTPUT / "PYTHON_DEPENDENCY_INVENTORY.json").write_text(
        json.dumps(evidence, indent=2) + "\n", encoding="utf-8"
    )
    print(
        json.dumps(
            {
                "modules": len(names),
                "static_edges": len(graph["edges"]),
                "engine_distributions": len(engine),
                "original_distributions": len(original),
            }
        )
    )


if __name__ == "__main__":
    main()

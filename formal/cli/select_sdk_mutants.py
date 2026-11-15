"""Select the frozen SDK constructor cohort from a complete framework session."""

import argparse
import ast
import hashlib
import json
from pathlib import Path


CLASSES = {
    "src-tauri/resources/r2t2_native.py": "NativeRuntime",
    "src-tauri/resources/qwen3_native.py": "QwenNativeRuntime",
}


def constructor_scopes(source_root):
    scopes = {}
    for path, name in CLASSES.items():
        source = (Path(source_root) / path).read_bytes()
        classes = [node for node in ast.parse(source).body
                   if isinstance(node, ast.ClassDef) and node.name == name]
        functions = [node for owner in classes for node in owner.body
                     if isinstance(node, ast.FunctionDef) and node.name == "__init__"]
        if len(classes) != 1 or len(functions) != 1:
            raise ValueError(f"Expected one {name}.__init__ in {path}")
        function = functions[0]
        scopes[path] = {"class": name, "start": (function.lineno, function.col_offset),
                        "end": (function.end_lineno, function.end_col_offset),
                        "sha256": hashlib.sha256(source).hexdigest()}
    return scopes


def partition(work_items, source_root):
    root = Path(source_root).resolve()
    scopes = constructor_scopes(root)
    selected, outside, seen = [], [], set()
    for item in work_items:
        if item.job_id in seen:
            raise ValueError(f"Duplicate job ID: {item.job_id}")
        seen.add(item.job_id)
        classifications = set()
        if not item.mutations:
            raise ValueError(f"No mutation in job {item.job_id}")
        for mutation in item.mutations:
            path = Path(mutation.module_path)
            if path.is_absolute():
                path = path.resolve().relative_to(root)
            path = path.as_posix()
            if path not in scopes:
                raise ValueError(f"Unexpected SDK mutation module: {path}")
            scope = scopes[path]
            start, end = tuple(mutation.start_pos), tuple(mutation.end_pos)
            inside = scope["start"] <= start and end <= scope["end"]
            overlaps = start < scope["end"] and scope["start"] < end
            if overlaps and not inside:
                raise ValueError(f"Mutation crosses constructor boundary in job {item.job_id}")
            classifications.add(inside)
        if len(classifications) != 1:
            raise ValueError(f"Mixed constructor scope in job {item.job_id}")
        (selected if classifications.pop() else outside).append(item)
    return selected, outside


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("generated", type=Path)
    parser.add_argument("selected", type=Path)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    if args.selected.exists() or args.report.exists():
        parser.error("selected session and report must be fresh paths")
    from cosmic_ray.work_db import WorkDB, use_db

    root = Path(__file__).resolve().parents[2]
    with use_db(args.generated, mode=WorkDB.Mode.open) as database:
        if database.num_results:
            parser.error("generation session must not contain executed results")
        items = database.work_items
    selected, outside = partition(items, root)
    args.selected.parent.mkdir(parents=True, exist_ok=True)
    with use_db(args.selected) as database:
        database.add_work_items(selected)
    receipt = {"generated": len(items), "selected": len(selected), "outside": len(outside),
               "selected_ids": [item.job_id for item in selected],
               "outside_ids": [item.job_id for item in outside],
               "scopes": constructor_scopes(root)}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps({key: receipt[key] for key in ("generated", "selected", "outside")}))


if __name__ == "__main__":
    main()

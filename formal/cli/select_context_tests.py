"""Conservative test selection against a hash-bound execution-context dataset."""

import ast
import difflib
import hashlib
import json
import sys


SOURCES = ("scripts/light_whisper_cli.py", "scripts/build_cli.py",
           "src-tauri/resources/r2t2_native.py", "src-tauri/resources/qwen3_native.py")


def select_invocation(payload, expected_sha256, current_state, full_test_ids):
    actual = hashlib.sha256(payload).hexdigest()
    selection = {"tests": list(full_test_ids), "full": True, "reason": "untrusted dataset file"}
    if not full_test_ids or len(set(full_test_ids)) != len(full_test_ids):
        raise ValueError("Current test cohort must be nonempty and unique")
    if expected_sha256 and expected_sha256 == actual:
        try:
            dataset = json.loads(payload)
        except (ValueError, UnicodeError):
            dataset = {}
        selection = select_tests(dataset, current_state["sources"], current_state["bindings"], full_test_ids)
    return {**selection, "dataset_sha256": actual,
            "source_sha256": {path: hashlib.sha256(text.encode()).hexdigest()
                              for path, text in current_state["sources"].items()},
            "command": [sys.executable, "-m", "unittest", "-f", *selection["tests"]]}


def select_tests(dataset, current_sources, current_bindings, full_test_ids):
    if not full_test_ids or len(set(full_test_ids)) != len(full_test_ids):
        raise ValueError("Current test cohort must be nonempty and unique")

    def full(reason):
        return {"tests": list(full_test_ids), "full": True, "reason": reason}

    if not isinstance(dataset, dict) or dataset.get("schema") != 1 or dataset.get("complete") is not True:
        return full("untrusted or incomplete dataset")
    if dataset.get("tests") != list(full_test_ids):
        return full("test cohort changed")
    if not current_bindings or dataset.get("bindings") != current_bindings:
        return full("input bindings changed")
    sources = dataset.get("sources", {})
    if not isinstance(sources, dict) or set(sources) != set(SOURCES) or set(current_sources) != set(SOURCES):
        return full("source cohort changed")
    for path, entry in sources.items():
        if not isinstance(entry, dict):
            return full("invalid baseline source record")
        text = entry.get("text")
        if not isinstance(text, str) or hashlib.sha256(text.encode("utf-8")).hexdigest() != entry.get("sha256"):
            return full("invalid baseline source hash")
    changed = [path for path in SOURCES if current_sources[path] != sources[path]["text"]]
    if len(changed) != 1:
        return full("baseline or multiple changed sources")
    path = changed[0]
    original, current = sources[path]["text"], current_sources[path]
    try:
        tree = ast.parse(original)
        ast.parse(current)
    except (SyntaxError, ValueError, TypeError):
        return full("source syntax unavailable")
    changes = [item for item in difflib.SequenceMatcher(
        a=original.splitlines(), b=current.splitlines(), autojunk=False).get_opcodes() if item[0] != "equal"]
    if len(changes) != 1:
        return full("ambiguous source changes")
    operation, first, last, new_first, new_last = changes[0]
    if operation != "replace" or last - first != 1 or new_last - new_first != 1:
        return full("source change is not a single-line replacement")
    line = first + 1
    functions = [node for node in ast.walk(tree) if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))]
    definitions = [node for node in ast.walk(tree)
                   if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef))]

    def start(node):
        return min([node.lineno, *(decorator.lineno for decorator in node.decorator_list)])

    owners = [node for node in definitions if start(node) <= line <= node.end_lineno]
    owner = min(owners, key=lambda node: (node.end_lineno - start(node), -node.col_offset)) if owners else None
    inside = isinstance(owner, (ast.FunctionDef, ast.AsyncFunctionDef)) and owner.body[0].lineno <= line
    header = any(node.lineno <= line < node.body[0].lineno or line == node.lineno
                 or any(decorator.lineno <= line <= decorator.end_lineno for decorator in node.decorator_list)
                 for node in functions)
    imports = any(isinstance(node, (ast.Import, ast.ImportFrom))
                  and node.lineno <= line <= node.end_lineno for node in ast.walk(tree))
    if not inside or header or imports:
        return full("module initialization, function signature or import changed")
    context_map = sources[path].get("contexts")
    if not isinstance(context_map, dict):
        return full("untrusted context map")
    contexts = context_map.get(str(line))
    if not isinstance(contexts, list) or not contexts or any(context not in full_test_ids for context in contexts):
        return full("missing or untrusted execution context")
    selected = [test for test in full_test_ids if test in contexts]
    if not selected:
        return full("empty test selection")
    return {"tests": selected, "full": False, "reason": "all observed covering tests"}


def main():
    import os
    from pathlib import Path
    import unittest
    from context_measurements import load_suite, snapshot

    root = Path(__file__).resolve().parents[2]
    sys.path.insert(0, str(root))
    sys.path.insert(0, str(root / "src-tauri/resources"))
    state = snapshot(root, os.environ)
    try:
        payload = Path(os.environ["LIGHT_WHISPER_CONTEXT_DATASET"]).read_bytes()
    except (KeyError, OSError):
        payload = b""
    expected = os.environ.get("LIGHT_WHISPER_CONTEXT_SHA256", "")
    try:
        dataset = json.loads(payload)
    except (ValueError, UnicodeError):
        dataset = {}
    trusted = (expected == hashlib.sha256(payload).hexdigest() and isinstance(dataset, dict)
               and dataset.get("complete") is True and dataset.get("bindings") == state["bindings"])
    if trusted:
        full_ids = dataset.get("tests", [])
    else:
        _, full_ids = load_suite(root)
    selection = select_invocation(payload, expected, state, full_ids)
    print(json.dumps({**selection, "full_test_ids": full_ids}), flush=True)
    loader = unittest.TestLoader()
    suite = loader.loadTestsFromNames(selection["tests"])
    result = unittest.TextTestRunner(verbosity=2, failfast=True).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())



import re
import sys
from pathlib import Path


def main() -> int:
    source = Path("formal/AppWorkflow.tla").read_text(encoding="utf-8")
    actions = set(
        re.findall(
            r"^([A-Z][A-Za-z0-9]*)(?:\([^\n]*\))?\s*==",
            source.split("Init ==", 1)[1].split("Next ==", 1)[0],
            re.MULTILINE,
        )
    )
    actions.add("Init")
    observed: set[str] = set()
    pattern = re.compile(r"^<([A-Z][A-Za-z0-9]*) line [^\n]+>: (\d+):(\d+)$", re.MULTILINE)
    for name in sys.argv[1:]:
        output = Path(name).read_text(encoding="utf-8")
        if "Model checking completed. No error has been found." not in output:
            raise SystemExit(f"TLC did not complete successfully: {name}")
        observed.update(
            action
            for action, _distinct, transitions in pattern.findall(output)
            if int(transitions) > 0
        )
    if not sys.argv[1:]:
        raise SystemExit("pass the TLC workload logs")
    missing = sorted(actions - observed)
    if missing:
        raise SystemExit("unreachable AppWorkflow actions: " + ", ".join(missing))
    print(f"All {len(actions)} AppWorkflow actions are reachable across {len(sys.argv) - 1} workloads.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

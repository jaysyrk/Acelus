import json
import re
import sys
from pathlib import Path

CALL = re.compile(r'rpc<[^>]*>\(\s*"([^"]+)"|rpc\(\s*"([^"]+)"')
IMPLEMENTED = re.compile(r'"([a-zA-Z]+\.[a-zA-Z]+)"\s*=>')


def main():
    root = Path(__file__).resolve().parent.parent

    schema = json.loads((root / "schema" / "rpc.json").read_text())
    declared = set(schema.get("methods", {}))

    handler = (root / "core" / "acelusd" / "src" / "handler.rs").read_text()
    implemented = set(IMPLEMENTED.findall(handler))

    called = {}
    for path in sorted((root / "ui" / "src").rglob("*.ts")):
        if path.name == "mock.ts":
            continue
        for line_number, line in enumerate(path.read_text().splitlines(), 1):
            for match in CALL.finditer(line):
                method = match.group(1) or match.group(2)
                called.setdefault(method, (path.relative_to(root), line_number))

    failures = 0

    for method, (path, line) in sorted(called.items()):
        if method not in implemented:
            print(f"{path}:{line}: calls {method}, which acelusd does not implement")
            failures += 1
        elif method not in declared:
            print(f"{path}:{line}: calls {method}, which schema/rpc.json does not declare")
            failures += 1

    for method in sorted(implemented - declared):
        print(f"core/acelusd/src/handler.rs: implements {method}, which schema/rpc.json omits")
        failures += 1

    if failures:
        print(f"\n{failures} mismatch(es). The clients and the daemon must agree on the method set.")
        return 1

    print(f"{len(called)} called, {len(implemented)} implemented, {len(declared)} declared; all agree")
    return 0


if __name__ == "__main__":
    sys.exit(main())

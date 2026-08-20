import sys
from pathlib import Path

ALLOWED_PREFIXES = {
    ".rs": ("///", "//!"),
    ".zig": ("///", "//!"),
    ".go": ("//go:",),
    ".ts": (),
    ".css": (),
}

LINE_COMMENT = {".rs", ".zig", ".go", ".ts"}
BLOCK_COMMENT = {".rs", ".go", ".ts", ".css"}
BACKTICK_STRING = {".go", ".ts"}

SKIP_DIRS = {".git", "target", "zig-out", ".zig-cache", "node_modules", "dist"}


def scan(text, suffix):
    allowed = ALLOWED_PREFIXES[suffix]
    findings = []
    i = 0
    line = 1
    n = len(text)
    block_depth = 0

    while i < n:
        ch = text[i]

        if ch == "\n":
            line += 1
            i += 1
            continue

        if block_depth:
            if text.startswith("*/", i):
                block_depth -= 1
                i += 2
            elif suffix == "rs" and text.startswith("/*", i):
                block_depth += 1
                i += 2
            else:
                i += 1
            continue

        if ch == '"':
            i = skip_quoted(text, i, '"')
            continue

        if ch == "`" and suffix in BACKTICK_STRING:
            end = text.find("`", i + 1)
            segment = text[i:end if end != -1 else n]
            line += segment.count("\n")
            i = n if end == -1 else end + 1
            continue

        if ch == "'":
            nxt = skip_char_literal(text, i, suffix)
            if nxt != i:
                i = nxt
                continue

        if suffix == ".zig" and text.startswith("\\\\", i):
            i = text.find("\n", i)
            if i == -1:
                i = n
            continue

        if suffix == ".rs" and ch in "rb" and starts_raw_string(text, i):
            i, consumed_lines = skip_raw_string(text, i)
            line += consumed_lines
            continue

        if ch == "/" and suffix == ".ts" and starts_regex(text, i):
            i, consumed_lines = skip_regex(text, i)
            line += consumed_lines
            continue

        if text.startswith("//", i) and suffix in LINE_COMMENT:
            rest = text[i:text.find("\n", i) if text.find("\n", i) != -1 else n]
            if not rest.startswith(allowed):
                findings.append((line, rest.strip()))
            i += len(rest)
            continue

        if text.startswith("/*", i) and suffix in BLOCK_COMMENT:
            findings.append((line, text[i:i + 40].split("\n")[0].strip()))
            block_depth = 1
            i += 2
            continue

        i += 1

    return findings


REGEX_PRECEDERS = set("(,=:[!&|?{};+-*%~^<>") | {"\n"}


def starts_regex(text, i):
    if text.startswith("//", i) or text.startswith("/*", i):
        return False

    j = i - 1
    while j >= 0 and text[j] in " \t":
        j -= 1
    if j < 0:
        return True
    if text[j] in REGEX_PRECEDERS:
        return True

    word = ""
    k = j
    while k >= 0 and (text[k].isalpha() or text[k] == "_"):
        word = text[k] + word
        k -= 1
    return word in {"return", "typeof", "case", "in", "of", "new", "delete", "void", "throw"}


def skip_regex(text, i):
    j = i + 1
    in_class = False
    while j < len(text):
        ch = text[j]
        if ch == "\\":
            j += 2
            continue
        if ch == "\n":
            return i + 1, 0
        if ch == "[":
            in_class = True
        elif ch == "]":
            in_class = False
        elif ch == "/" and not in_class:
            return j + 1, 0
        j += 1
    return i + 1, 0


def skip_quoted(text, i, quote):
    i += 1
    while i < len(text):
        if text[i] == "\\":
            i += 2
            continue
        if text[i] == quote:
            return i + 1
        if text[i] == "\n":
            return i
        i += 1
    return i


def skip_char_literal(text, i, suffix):
    j = i + 1
    if j < len(text) and text[j] == "\\":
        j += 2
    else:
        j += 1
    if j < len(text) and text[j] == "'":
        return j + 1
    return i


def starts_raw_string(text, i):
    j = i + 1
    while j < len(text) and text[j] == "#":
        j += 1
    return j < len(text) and text[j] == '"'


def skip_raw_string(text, i):
    j = i + 1
    hashes = 0
    while j < len(text) and text[j] == "#":
        hashes += 1
        j += 1
    terminator = '"' + "#" * hashes
    end = text.find(terminator, j + 1)
    if end == -1:
        return len(text), text[i:].count("\n")
    return end + len(terminator), text[i:end].count("\n")


def main():
    root = Path(__file__).resolve().parent.parent
    failures = 0
    checked = 0

    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix not in ALLOWED_PREFIXES:
            continue
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue

        checked += 1
        for line, snippet in scan(path.read_text(encoding="utf-8"), path.suffix):
            rel = path.relative_to(root)
            print(f"{rel}:{line}: comment in code: {snippet[:80]}")
            failures += 1

    if failures:
        print(f"\n{failures} comment(s) found across {checked} files.")
        print("Acelus code carries no explanatory comments; see CONTRIBUTING.md.")
        return 1

    print(f"no comments found in {checked} source files")
    return 0


if __name__ == "__main__":
    sys.exit(main())

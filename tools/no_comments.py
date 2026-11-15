from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
SKIP = {"target", ".git"}


def strip_comments(source):
    out = list(source)
    i = 0
    n = len(source)
    while i < n:
        if source[i] == 'r':
            k = i + 1
            while k < n and source[k] == '#':
                k += 1
            if k < n and source[k] == '"':
                hashes = k - i - 1
                end = source.find('"' + ('#' * hashes), k + 1)
                i = n if end < 0 else end + hashes + 1
                continue
        if source[i] == '"':
            i += 1
            while i < n:
                if source[i] == '\\':
                    i += 2
                elif source[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            continue
        if source[i] == "'" and i + 2 < n:
            if source[i + 1] == "\\" and source[i + 3:i + 4] == "'":
                i += 4
                continue
            if source[i + 2] == "'":
                i += 3
                continue
        if source.startswith("//", i):
            end = source.find("\n", i)
            end = n if end < 0 else end
            for j in range(i, end):
                out[j] = " "
            i = end
            continue
        if source.startswith("/*", i):
            start = i
            depth = 1
            i += 2
            while i < n and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            for j in range(start, i):
                if out[j] not in "\r\n":
                    out[j] = " "
            continue
        i += 1
    return "".join(out)


def files():
    for folder in ("crates", "prototypes", "tests", "tools"):
        base = ROOT / folder
        if base.exists():
            yield from base.rglob("*.rs")
    for pattern in (".github/**/*.yml", ".github/**/*.yaml", ".github/**/*.sh", ".config/*.toml", "*.toml", "crates/**/*.toml", "prototypes/**/*.toml", "tools/**/*.toml", "crates/**/*.wgsl", "prototypes/**/*.wgsl"):
        yield from ROOT.glob(pattern)


def strip_hash_comments(source):
    result = []
    for line in source.splitlines(keepends=True):
        quote = None
        escape = False
        for index, char in enumerate(line):
            if quote:
                if escape:
                    escape = False
                elif char == "\\":
                    escape = True
                elif char == quote:
                    quote = None
            elif char in "\"'":
                quote = char
            elif char == "#" and (index == 0 or line[index - 1].isspace()):
                if index == 0 and line.startswith("#!"):
                    break
                line = line[:index].rstrip() + ("\n" if line.endswith("\n") else "")
                break
        result.append(line)
    return "".join(result)


def main():
    errors = []
    for path in files():
        original = path.read_text(encoding="utf-8")
        cleaned = strip_comments(original) if path.suffix in {".rs", ".wgsl"} else strip_hash_comments(original)
        if "--fix" in sys.argv:
            cleaned = "\n".join(line.rstrip() for line in cleaned.split("\n"))
            path.write_text(cleaned, encoding="utf-8", newline="")
        elif cleaned != original:
            errors.append(str(path.relative_to(ROOT)))
    if errors:
        print("Comments found:")
        print("\n".join(errors))
        return 1
    return 0


raise SystemExit(main())

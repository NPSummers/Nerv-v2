import os
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parent.parent
EXPR_TAGS = {"Integer", "Float", "String", "Boolean", "Name", "Unary", "Binary", "Call", "Member", "Method", "Index", "Construct", "Array", "Repeat", "Try", "If", "Match", "Lambda"}
OPERATOR = {"Add": "Plus", "Sub": "Minus", "Mul": "Star", "Div": "Slash", "Rem": "Percent", "Eq": "EqEq", "Neg": "Minus"}


class DebugReader:
    def __init__(self, text):
        self.tokens = re.findall(r'"(?:\\.|[^"\\])*"|[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?|[A-Za-z_]\w*|[{}\[\]():,]', text)
        self.index = 0

    def take(self):
        value = self.tokens[self.index]
        self.index += 1
        return value

    def peek(self):
        return self.tokens[self.index] if self.index < len(self.tokens) else ""

    def sequence(self, close):
        values = []
        while self.peek() != close:
            values.append(self.value())
            if self.peek() == ",":
                self.take()
            elif self.peek() != close:
                raise ValueError(f"unexpected debug token {self.peek()}")
        self.take()
        return values

    def value(self):
        token = self.take()
        if token.startswith('"'):
            token = re.sub(r'\\u\{([0-9a-fA-F]+)\}', lambda match: json.dumps(chr(int(match[1], 16)))[1:-1], token)
            return json.loads(token.replace(r"\0", r"\u0000"))
        if token in ("[", "("):
            return self.sequence("]" if token == "[" else ")")
        if token in ("true", "false"):
            return token == "true"
        if re.fullmatch(r"[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?", token):
            return float(token) if "." in token or "e" in token.lower() else int(token)
        result = {"tag": token}
        if self.peek() == "(":
            self.take()
            result["args"] = self.sequence(")")
        if self.peek() == "{":
            self.take()
            fields = {}
            while self.peek() != "}":
                name = self.take()
                if self.take() != ":":
                    raise ValueError("expected debug field")
                fields[name] = self.value()
                if self.peek() == ",":
                    self.take()
            self.take()
            result["fields"] = fields
        return result


def walk(value):
    if isinstance(value, dict):
        yield value
        for member in value.values():
            yield from walk(member)
    if isinstance(value, list):
        for member in value:
            yield from walk(member)


def reference_events(module):
    def signature(expr):
        kind = expr["fields"]["kind"]
        tag = {"Int": "Integer", "Bool": "Boolean", "Field": "Member", "Struct": "Construct", "ArrayRepeat": "Repeat"}.get(kind["tag"], kind["tag"])
        span = expr["fields"]["span"]["fields"]
        return tag, span["start"], span["end"]

    def direct(value):
        if isinstance(value, dict):
            if value.get("tag") == "Expr" and "fields" in value:
                yield signature(value)
            else:
                for member in value.values():
                    yield from direct(member)
        if isinstance(value, list):
            for member in value:
                yield from direct(member)

    events = []
    for expr in walk(module):
        if expr.get("tag") != "Expr" or "fields" not in expr:
            continue
        kind = expr["fields"]["kind"]
        tag = kind["tag"]
        payload = ""
        if tag in {"Int", "Float", "String", "Bool", "Name"}:
            payload = kind["args"][0]
        if tag in {"Field", "Method", "Struct"}:
            payload = kind["fields"]["name"]
        if tag in {"Unary", "Binary"}:
            op = kind["fields"]["op"]["tag"]
            payload = OPERATOR.get(op, op)
        events.append((*signature(expr), payload, tuple(direct(kind))))
    return Counter(events)


def arena_events(rows, raw):
    nodes = {row[0]: row for row in rows}
    if len(nodes) != len(rows) or not rows or rows[-1][1] != "Module":
        raise ValueError("missing module root or duplicate node IDs")
    for index, tag, start, end, text, children in rows:
        if not 0 <= start <= end <= len(raw):
            raise ValueError(f"invalid span: {index}")
        raw[:start].decode("utf-8")
        raw[:end].decode("utf-8")
        if any(child not in nodes or child >= index for child in children):
            raise ValueError(f"invalid arena child: {index}")

    def children_of(row):
        children = row[5]
        if row[1] == "Arm":
            return children[-1:]
        if row[1] == "Assign":
            target = nodes[children[0]]
            return (target[5] if target[1] in {"Member", "Index"} else []) + children[1:]
        return children

    def direct(index):
        row = nodes[index]
        if row[1] in EXPR_TAGS:
            yield tuple(row[1:4])
        else:
            for child in children_of(row):
                yield from direct(child)

    events = []

    def visit(index):
        row = nodes[index]
        _, tag, start, end, text, children = row
        if tag in EXPR_TAGS:
            payload = text
            if tag == "Integer":
                payload = int(text.replace("_", ""))
            if tag == "Float":
                payload = float(text.replace("_", ""))
            if tag == "Boolean":
                payload = text == "true"
            if tag == "String":
                payload = json.loads(text)
            args = tuple(item for child in children for item in direct(child))
            events.append((tag, start, end, payload, args))
        for child in children_of(row):
            visit(child)

    visit(rows[-1][0])
    return Counter(events)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--parser", required=True, type=Path)
    parser.add_argument("--reference", type=Path, default=Path(os.environ.get("NERV_REFERENCE", str(ROOT.parent / "Nerv-v2"))))
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    executable = args.parser.resolve(strict=True)
    sources = sorted((args.reference / "example").rglob("*.nerv")) + sorted((args.reference / "stdlib").glob("*.nerv"))
    sources += sorted((ROOT / "src").glob("*.nerv")) + sorted((ROOT / "tests").glob("*.nerv"))
    cases = ["", "-- empty\n", "fn main() => 1 + 2 * 3 - 4 / 2\n", "fn main() => (1 + 2) * 3\n",
             "fn main() => not true and false or true\n", "fn main() => -9223372036854775808\n",
             "fn f<T, T>() => 0\n", "fn main( => 0\n", "fn main() => [1,]\n", "fn main() => 1 = 2\n",
             "fn main() => 9223372036854775808\n", 'fn main() => "bad\\q"\n', "fn main() => 1e999\n",
             "fn main()\n    if true\n        return\n    else\n        return 1\n",
             "trait Read\n    fn read(self) -> i64\nimpl Value for Read\n    fn read(self) -> i64 => 1\n",
             "fn main() => [1] * 3 + [2]\n", "fn main() => foo.bar(1):get(2)[0]?\n",
             "fn main()\n    match value\n        Some(x) => if true\n            x\n        else\n            0\n        None => 0\n",
             "struct Bad.Name\n    value: i64\n", "enum Bad.Name\n    Value\n", "trait Bad.Name\n    fn f(self)\n",
             "fn main() => 1 { value: 2 }\n", "fn main() => foo(1,)\n", "fn main() => 'x'\n",
             "fn f(x) => x\n", "struct S\n    fn f<T>(self) => 0\n", "fn main()\n    else\n        0\n"]
    for left in ("1", "2 + 3", "-4", "foo(5)", "a[0]"):
        for op in ("+", "-", "*", "/", "==", "<", "and", "or"):
            cases.append(f"fn main() => ({left}) {op} 6 * 7 + 8\n")
    fixtures = ROOT / "target/parser-cases"
    fixtures.mkdir(parents=True, exist_ok=True)
    for index, text in enumerate(cases):
        file = fixtures / f"case_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        sources.append(file)
    accepted = 0
    expressions = 0
    for file in sources:
        old = subprocess.run([str(stage0), str(file.resolve()), "--parse"], capture_output=True, text=True, encoding="utf-8", timeout=30)
        new = subprocess.run([str(executable), "--parse", str(file.resolve())], capture_output=True, text=True, encoding="utf-8", timeout=30)
        if old.returncode != new.returncode:
            raise RuntimeError(f"acceptance mismatch: {file}\nreference: {old.stderr}\nnew: {new.stderr}")
        if old.returncode:
            if new.stdout:
                raise RuntimeError(f"partial AST on rejected source: {file}")
            continue
        reader = DebugReader(old.stdout)
        reference = reference_events(reader.value())
        if reader.index != len(reader.tokens):
            raise RuntimeError(f"unconsumed reference debug output: {file}")
        actual = arena_events([json.loads(line) for line in new.stdout.splitlines()], file.read_bytes())
        if reference != actual:
            raise RuntimeError(f"AST mismatch: {file}\nmissing: {list((reference - actual).items())[:5]}\nextra: {list((actual - reference).items())[:5]}")
        accepted += 1
        expressions += reference.total()
    print(f"Matched acceptance on {len(sources)} files/cases; compared {expressions} expression shapes/spans across {accepted} accepted modules")


if __name__ == "__main__":
    main()

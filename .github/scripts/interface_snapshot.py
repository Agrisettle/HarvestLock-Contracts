#!/usr/bin/env python3
"""Render, diff and classify the escrow contract's exported interface (#244).

Reads the `contractspecv0` custom section straight out of a built WASM and
decodes its XDR `SCSpecEntry` stream (layout: stellar-xdr
`Stellar-contract-spec.x`), so the guard needs nothing beyond Python 3 and a
built WASM. It does not need the Stellar CLI, and its output does not change
when the CLI's `contract info interface` formatting changes between releases.

Output is one line per atomic interface fact, sorted, so a diff names exactly
what moved:

    fn <name>(<arg>: <type>, ...) -> <type>
    event <Name>(...)
    struct <Name>.<field>: <type>
    union <Name>::<Case>[(<types>)]
    enum <Name>::<Case> = <n>
    error <Name>::<Case> = <n>

Only functions, events, and the user-defined types they reach are listed.
That is the ABI a caller or indexer can observe. It also makes the output
identical whether or not the build ran the CLI's spec "shaking" pass, which
removes types no function or event references.

Usage:
    interface_snapshot.py render <wasm>
    interface_snapshot.py check  <wasm> <snapshot>   # exit 1 on any difference
    interface_snapshot.py update <wasm> <snapshot>
"""

import difflib
import sys

SECTION = "contractspecv0"

HEADER = """\
# Exported interface of the HarvestLock escrow contract (#244).
#
# Generated from the built WASM's `contractspecv0` section by
# .github/scripts/check_interface_snapshot.sh; do not edit by hand. That
# script fails CI-style on any difference and prints a PATCH / MINOR /
# MINOR **BREAKING** hint per docs/versioning.md §2.
#
# A diff here is an ABI change. In particular, any new `fn` line is a new
# entry point, and docs/threat-model.md entry 26 ("Admin, upgrade, pause, or
# owner takeover": defended by absence) relies on reviewers noticing a new
# admin/upgrade/pause function. This file is how that gets noticed.
#
# To accept an intended change, run:
#   bash .github/scripts/check_interface_snapshot.sh --update
# and commit the result with the change that caused it.
"""

PRIMITIVES = {
    0: "Val",
    1: "bool",
    2: "()",
    3: "Error",
    4: "u32",
    5: "i32",
    6: "u64",
    7: "i64",
    8: "Timepoint",
    9: "Duration",
    10: "u128",
    11: "i128",
    12: "u256",
    13: "i256",
    14: "Bytes",
    16: "String",
    17: "Symbol",
    19: "Address",
    20: "MuxedAddress",
}


class SpecError(Exception):
    pass


# ---------------------------------------------------------------- WASM ----


def _leb128(buf, pos):
    result = shift = 0
    while True:
        if pos >= len(buf):
            raise SpecError("truncated LEB128 in WASM")
        byte = buf[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7


def spec_section(wasm):
    """Concatenated payload of every `contractspecv0` custom section."""
    if wasm[:4] != b"\0asm":
        raise SpecError("not a WASM module (bad magic)")
    pos, out, found = 8, bytearray(), False
    while pos < len(wasm):
        sec_id = wasm[pos]
        size, pos = _leb128(wasm, pos + 1)
        end = pos + size
        if sec_id == 0:
            name_len, p = _leb128(wasm, pos)
            if wasm[p : p + name_len].decode("utf-8", "replace") == SECTION:
                out += wasm[p + name_len : end]
                found = True
        pos = end
    if not found:
        raise SpecError(f"no `{SECTION}` custom section in WASM")
    return bytes(out)


# ----------------------------------------------------------------- XDR ----


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def done(self):
        return self.pos >= len(self.data)

    def u32(self):
        if self.pos + 4 > len(self.data):
            raise SpecError("truncated XDR")
        v = int.from_bytes(self.data[self.pos : self.pos + 4], "big")
        self.pos += 4
        return v

    def string(self):
        n = self.u32()
        raw = self.data[self.pos : self.pos + n]
        self.pos += n + (-n % 4)  # XDR pads opaque data to 4 bytes
        return raw.decode("utf-8")

    def array(self, item):
        return [item() for _ in range(self.u32())]


def read_type(r):
    t = r.u32()
    if t in PRIMITIVES:
        return ("prim", PRIMITIVES[t])
    if t == 1000:
        return ("option", read_type(r))
    if t == 1001:
        return ("result", read_type(r), read_type(r))
    if t == 1002:
        return ("vec", read_type(r))
    if t == 1004:
        return ("map", read_type(r), read_type(r))
    if t == 1005:
        return ("tuple", r.array(lambda: read_type(r)))
    if t == 1006:
        return ("bytesn", r.u32())
    if t == 2000:
        return ("udt", r.string())
    raise SpecError(f"unknown SCSpecType {t}")


def fmt_type(t):
    kind = t[0]
    if kind == "prim":
        return t[1]
    if kind == "option":
        return f"Option<{fmt_type(t[1])}>"
    if kind == "result":
        return f"Result<{fmt_type(t[1])}, {fmt_type(t[2])}>"
    if kind == "vec":
        return f"Vec<{fmt_type(t[1])}>"
    if kind == "map":
        return f"Map<{fmt_type(t[1])}, {fmt_type(t[2])}>"
    if kind == "tuple":
        return "(" + ", ".join(fmt_type(x) for x in t[1]) + ")"
    if kind == "bytesn":
        return f"BytesN<{t[1]}>"
    return t[1]  # udt


def udt_refs(t):
    kind = t[0]
    if kind == "udt":
        yield t[1]
    elif kind in ("option", "vec"):
        yield from udt_refs(t[1])
    elif kind in ("result", "map"):
        yield from udt_refs(t[1])
        yield from udt_refs(t[2])
    elif kind == "tuple":
        for x in t[1]:
            yield from udt_refs(x)


def read_entries(data):
    """Decode the SCSpecEntry stream into (functions, events, udts)."""
    r = Reader(data)
    functions, events, udts = [], [], {}
    while not r.done():
        kind = r.u32()
        if kind == 0:  # function
            r.string()  # doc
            name = r.string()
            inputs = r.array(lambda: (r.string(), r.string(), read_type(r))[1:])
            outputs = r.array(lambda: read_type(r))
            functions.append((name, inputs, outputs))
        elif kind == 1:  # struct
            r.string(), r.string()  # doc, lib
            name = r.string()
            fields = r.array(lambda: (r.string(), r.string(), read_type(r))[1:])
            udts[name] = ("struct", fields)
        elif kind == 2:  # union
            r.string(), r.string()
            name = r.string()

            def case():
                ck = r.u32()
                r.string()  # doc
                cname = r.string()
                types = r.array(lambda: read_type(r)) if ck == 1 else None
                return (cname, types)

            udts[name] = ("union", r.array(case))
        elif kind in (3, 4):  # enum, error enum
            r.string(), r.string()
            name = r.string()
            cases = r.array(lambda: (r.string(), r.string(), r.u32())[1:])
            udts[name] = ("enum" if kind == 3 else "error", cases)
        elif kind == 5:  # event
            r.string(), r.string()
            name = r.string()
            prefix = r.array(r.string)

            def param():
                r.string()
                pname = r.string()
                ptype = read_type(r)
                loc = r.u32()
                return (pname, ptype, "topic" if loc == 1 else "data")

            params = r.array(param)
            fmt = {0: "single", 1: "vec", 2: "map"}.get(r.u32(), "?")
            events.append((name, prefix, params, fmt))
        else:
            raise SpecError(f"unknown SCSpecEntryKind {kind}")
    return functions, events, udts


# ------------------------------------------------------------- render ----


def render(wasm_bytes):
    functions, events, udts = read_entries(spec_section(wasm_bytes))

    # Reachability from the observable surface (functions + events).
    pending = []
    for _, inputs, outputs in functions:
        for _, t in inputs:
            pending += udt_refs(t)
        for t in outputs:
            pending += udt_refs(t)
    for _, _, params, _ in events:
        for _, t, _ in params:
            pending += udt_refs(t)
    # Error enums are always part of the surface: a function returns them
    # through the generic `Result<_, Error>` spec type, not a named
    # reference, so reachability alone would never find them, yet their codes
    # are exactly what callers branch on.
    pending += [name for name, (kind, _) in udts.items() if kind == "error"]
    reached = set()
    while pending:
        name = pending.pop()
        if name in reached:
            continue
        reached.add(name)
        if name not in udts:
            raise SpecError(f"type `{name}` is referenced but not defined in the spec")
        kind, body = udts[name]
        if kind == "struct":
            for _, t in body:
                pending += udt_refs(t)
        elif kind == "union":
            for _, types in body:
                for t in types or []:
                    pending += udt_refs(t)

    lines = []
    for name, inputs, outputs in functions:
        args = ", ".join(f"{n}: {fmt_type(t)}" for n, t in inputs)
        ret = fmt_type(outputs[0]) if outputs else "()"
        lines.append(f"fn {name}({args}) -> {ret}")
    for name, prefix, params, fmt in events:
        ps = ", ".join(f"{n}: {fmt_type(t)} @{loc}" for n, t, loc in params)
        lines.append(f"event {name}(topics: [{', '.join(prefix)}]; {ps}; data: {fmt})")
    for name in reached:
        kind, body = udts[name]
        if kind == "struct":
            for fname, t in body:
                lines.append(f"struct {name}.{fname}: {fmt_type(t)}")
        elif kind == "union":
            for cname, types in body:
                tail = "" if types is None else "(" + ", ".join(fmt_type(t) for t in types) + ")"
                lines.append(f"union {name}::{cname}{tail}")
        else:
            for cname, value in body:
                lines.append(f"{kind} {name}::{cname} = {value}")
    return sorted(lines)


def read_snapshot(path):
    with open(path, encoding="utf-8") as f:
        return [
            line.rstrip("\n")
            for line in f
            if line.strip() and not line.startswith("#")
        ]


# ------------------------------------------------------------ classify ----


def _owner(line):
    """`(kind, type-or-fn name)` a snapshot line belongs to."""
    kind, rest = line.split(" ", 1)
    for sep in ("(", ".", "::", " "):
        rest = rest.split(sep, 1)[0]
    return kind, rest


def classify(old, new):
    """Return (bump, reasons) for old -> new, per docs/versioning.md §2."""
    old_set, new_set = set(old), set(new)
    removed = sorted(old_set - new_set)
    added = sorted(new_set - old_set)
    if not removed and not added:
        return "PATCH", ["exported interface unchanged"]

    old_owners = {_owner(line) for line in old}
    breaking, additive = [], []
    for line in removed:
        breaking.append(f"removed or changed: {line}")
    for line in added:
        kind, owner = _owner(line)
        if (kind, owner) not in old_owners:
            additive.append(f"new {kind}: {line}")
        elif kind in ("fn", "event"):
            # Same name, different line: the signature changed. The old
            # line is already listed above as removed.
            breaking.append(f"new signature for existing {kind}: {line}")
        elif kind == "struct":
            # A new field changes the struct's XDR shape for every existing
            # caller (precedent: Deployment 9, `OracleConfig` gains
            # `denominated_amount`, recorded as BREAKING).
            breaking.append(f"field added to existing struct: {line}")
        else:
            # A new case on an existing enum/union/error (precedent:
            # Deployment 10's new `Disputed` status, recorded as additive).
            additive.append(f"case added to existing {kind}: {line}")

    # A changed signature shows up as a removal plus an addition with the
    # same owner; it is already counted as breaking via the removal.
    if breaking:
        return "MINOR **BREAKING**", breaking + additive
    return "MINOR", additive


# ---------------------------------------------------------------- main ----


def main(argv):
    # The hints quote "docs/versioning.md §2"; keep that legible on consoles
    # whose default encoding isn't UTF-8 (e.g. Windows).
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    if len(argv) < 3 or argv[1] not in ("render", "check", "update"):
        print(__doc__.split("Usage:")[1].rstrip(), file=sys.stderr)
        return 2
    cmd, wasm_path = argv[1], argv[2]
    try:
        with open(wasm_path, "rb") as f:
            current = render(f.read())
    except (OSError, SpecError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1

    if cmd == "render":
        print("\n".join(current))
        return 0
    if len(argv) < 4:
        print("error: missing <snapshot> path", file=sys.stderr)
        return 2
    snap_path = argv[3]

    if cmd == "update":
        with open(snap_path, "w", encoding="utf-8", newline="\n") as f:
            f.write(HEADER + "\n" + "\n".join(current) + "\n")
        print(f"wrote {snap_path} ({len(current)} lines)")
        return 0

    try:
        pinned = read_snapshot(snap_path)
    except OSError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    bump, reasons = classify(pinned, current)
    n_fns = sum(1 for line in current if line.startswith("fn "))
    if bump == "PATCH":
        print(f"ok: exported interface matches {snap_path} ({n_fns} functions)")
        print("hint: interface-wise this is a PATCH (docs/versioning.md §2 rule 1);")
        print("      a state-machine or payout change still makes it MINOR")
        return 0

    print(f"error: exported interface differs from {snap_path}\n")
    sys.stdout.writelines(
        difflib.unified_diff(
            [line + "\n" for line in pinned],
            [line + "\n" for line in current],
            fromfile=f"{snap_path} (committed)",
            tofile="built WASM",
        )
    )
    print(f"\nversion hint (docs/versioning.md §2): {bump}")
    for reason in reasons:
        print(f"  - {reason}")
    new_fns = [line for line in current if line.startswith("fn ") and line not in pinned]
    if new_fns:
        print("\nreview: new or changed entry points. docs/threat-model.md entry 26")
        print("        says there is no admin / upgrade / pause function; check that")
        print("        none of these adds one:")
        for line in new_fns:
            print(f"  + {line}")
    print("\nIf the change is intended, regenerate and commit the snapshot:")
    print("  bash .github/scripts/check_interface_snapshot.sh --update")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))

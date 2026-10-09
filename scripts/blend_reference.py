#!/usr/bin/env python3
"""Generate blend-mode test vectors from a literal transliteration of paint.net 3.36's
`Data/UserBlendOps.Generated.H.cs` macros, using the original `masTable` read from the
OpenPDN source. Output: crates/pnn-core/src/blend_vectors.in (a Rust slice expression).

Usage: python3 -I scripts/blend_reference.py [path/to/OpenPDN]
"""

import random
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
pdn = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "reference" / "OpenPDN"
header = (pdn / "src/Data/UserBlendOps.Generated.H.cs").read_text()
table_src = header[header.index("masTable ="):]
table_src = table_src[: table_src.index("};")]
MAS = [
    (int(m, 16), int(a, 16), int(s))
    for m, a, s in re.findall(r"(0x[0-9A-Fa-f]+),\s*(0x[0-9A-Fa-f]+),\s*(\d+)", table_src)
]
assert len(MAS) == 256


def int_scale(a, b):
    r = a * b + 0x80
    return ((r >> 8) + r) >> 8


def int_div(n, d):
    m, a, s = MAS[d]
    return (n * m + a) >> s


def colorburn(a, b):
    if b == 0:
        return 0
    return max(0, 255 - int_div((255 - a) * 255, b))


def colordodge(a, b):
    return 255 if b == 255 else min(255, int_div(a * 255, 255 - b))


def reflect(a, b):
    return 255 if b == 255 else min(255, int_div(a * a, 255 - b))


def overlay(a, b):
    if a < 128:
        return int_scale(2 * a, b)
    return 255 - int_scale(2 * (255 - a), 255 - b)


FUNCS = {
    "Normal": lambda a, b: b,
    "Multiply": int_scale,
    "Additive": lambda a, b: min(255, a + b),
    "ColorBurn": colorburn,
    "ColorDodge": colordodge,
    "Reflect": reflect,
    "Glow": lambda a, b: reflect(b, a),
    "Overlay": overlay,
    "Difference": lambda a, b: abs(b - a),
    "Negation": lambda a, b: 255 - abs(255 - a - b),
    "Lighten": max,
    "Darken": min,
    "Screen": lambda a, b: b + a - int_scale(b, a),
    "Xor": lambda a, b: a ^ b,
}


def unpack(c):
    return c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF, (c >> 24) & 0xFF


def blend(lhs, rhs, f, opacity):
    lb, lg, lr, la = unpack(lhs)
    rb, rg, rr, ra = unpack(rhs)
    if opacity != 255:  # NAME##BlendOpWithOpacity: j = ALPHA_WITH_OPACITY
        ra = int_scale(ra, opacity)
    y = int_scale(la, 255 - ra)
    total = y + ra
    if total == 0:
        return 0
    x = int_scale(la, ra)
    z = ra - x
    m, a_, s = MAS[total]

    def ch(l, r):
        return ((l * y + r * z + f(l, r) * x) * m + a_) >> s

    b, g, r = ch(lb, rb), ch(lg, rg), ch(lr, rr)
    a = int_scale(la, 255 - ra) + ra
    return (b + (g << 8) + (r << 16) + (a << 24)) & 0xFFFFFFFF


def main():
    rng = random.Random(0x9D17)
    edge = [0x00000000, 0xFFFFFFFF, 0xFF000000, 0x00FFFFFF, 0x80808080, 0x7F7F7F7F, 0x01020304]
    pairs = [(l, r) for l in edge for r in edge]
    pairs += [(rng.getrandbits(32), rng.getrandbits(32)) for _ in range(40)]
    out = ["&["]
    for name, f in FUNCS.items():
        for opacity in (255, 128, 77, 0):
            for lhs, rhs in pairs:
                exp = blend(lhs, rhs, f, opacity)
                out.append(f"    (BlendMode::{name}, {lhs:#010x}, {rhs:#010x}, {opacity}, {exp:#010x}),")
    out.append("]")
    dst = ROOT / "crates/pnn-core/src/blend_vectors.in"
    dst.write_text("\n".join(out) + "\n")
    print(f"wrote {len(out) - 2} vectors to {dst.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

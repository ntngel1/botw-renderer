#!/usr/bin/env python3
"""Survey raw EFTB (v20, BotW Wii U) emitter fields across the game's effect files.

Research helper for docs/research/eft-runtime.md: reads the dump directly
(never writes game data into the repo). Usage:

  ptcl_fields.py hist <off> <fmt> [files...]   histogram of a field (fmt: b B h H i I f)
  ptcl_fields.py find <off> <fmt> <value>      emitters whose field equals value
  ptcl_fields.py attrs                         attribute-node magic counts and sizes
  ptcl_fields.py dump <emitter-name>           hex/float dump of one emitter's data
  ptcl_fields.py attr <MAGIC> [bytes]          hex/float dump of an attribute node per emitter

Default files: Bootup.pack:Effect/GameResident.sesetlist plus every
content/Effect/*.sesetlist of the update. GAME env var overrides the dump root.
"""
import os, sys, struct, glob, collections, pickle, signal

signal.signal(signal.SIGPIPE, signal.SIG_DFL)

GAME = os.environ.get("GAME", os.path.expanduser(
    "~/Documents/PetProjects/zelda/game-data/botw-wiiu"))
CACHE = os.environ.get("PTCL_CACHE", os.path.join(os.environ.get("TMPDIR", "/tmp"), "ptcl_fields_cache.pkl"))


def yaz0(b):
    if b[:4] != b"Yaz0":
        return b
    size = struct.unpack(">I", b[4:8])[0]
    out = bytearray(size)
    s, d = 16, 0
    while d < size:
        code = b[s]; s += 1
        for bit in range(8):
            if d >= size:
                break
            if code & (0x80 >> bit):
                out[d] = b[s]; d += 1; s += 1
            else:
                b1, b2 = b[s], b[s + 1]; s += 2
                dist = ((b1 & 0xF) << 8 | b2) + 1
                n = b1 >> 4
                if n == 0:
                    n = b[s] + 0x12; s += 1
                else:
                    n += 2
                for _ in range(n):
                    out[d] = out[d - dist]; d += 1
    return bytes(out)


def sarc(b):
    b = yaz0(b)
    assert b[:4] == b"SARC"
    data_off = struct.unpack(">I", b[0xC:0x10])[0]
    hdr = struct.unpack(">H", b[4:6])[0]
    sfat = hdr
    n = struct.unpack(">H", b[sfat + 6:sfat + 8])[0]
    nodes = []
    for i in range(n):
        h, attr, st, en = struct.unpack(">IIII", b[sfat + 12 + 16 * i: sfat + 28 + 16 * i])
        nodes.append((attr, st, en))
    sfnt = sfat + 12 + 16 * n
    names = sfnt + 8
    out = {}
    for attr, st, en in nodes:
        no = (attr & 0xFFFF) * 4
        e = b.index(b"\0", names + no)
        out[b[names + no:e].decode()] = b[data_off + st:data_off + en]
    return out


def default_files():
    files = []
    boot = os.path.join(GAME, "update/content/Pack/Bootup.pack")
    files.append((boot, "Effect/GameResident.sesetlist"))
    for p in sorted(glob.glob(os.path.join(GAME, "update/content/Effect/*.sesetlist"))):
        files.append((p, None))
    return files


def load(path, entry):
    raw = open(path, "rb").read()
    if entry:
        raw = sarc(raw)[entry]
    b = yaz0(raw)
    if b[:4] == b"SARC":  # sesetlist is a SARC holding the ptcl
        for k, v in sarc(b).items():
            if k.endswith(".ptcl") or v[:4] == b"EFTB":
                return v
    return b


def walk(b, fname):
    """Yield (file, set, emitter, data_off, attrs{magic:(off,size)}, parent)."""
    def nodes(off):
        while off is not None:
            magic = b[off:off + 4].decode("latin1")
            size, child, nxt, attr, data = struct.unpack(">IIIII", b[off + 4:off + 24])
            yield off, magic, size, (None if child == 0xFFFFFFFF else off + child), \
                (None if attr == 0xFFFFFFFF else off + attr), off + data
            off = None if nxt == 0xFFFFFFFF else off + nxt

    def emitters(off, sname, parent):
        for o, m, size, child, attr, data in nodes(off):
            if m != "EMTR":
                continue
            name = b[data + 0x10:data + 0x50].split(b"\0")[0].decode("latin1")
            attrs = {}
            for ao, am, asz, _, _, ad in (nodes(attr) if attr else []):
                attrs[am] = (ad, asz)
            yield fname, sname, name, data, attrs, parent
            if child:
                yield from emitters(child, sname, name)

    for o, m, size, child, attr, data in nodes(0x30):
        if m != "ESTA" or child is None:
            continue
        for so, sm, ssz, schild, sattr, sdata in nodes(child):
            sname = b[sdata + 0x10:sdata + 0x50].split(b"\0")[0].decode("latin1")
            if schild:
                yield from emitters(schild, sname, None)


def all_emitters():
    if os.path.exists(CACHE):
        return pickle.load(open(CACHE, "rb"))
    res = []
    blobs = {}
    for path, entry in default_files():
        fname = os.path.basename(path) + (":" + entry if entry else "")
        try:
            b = load(path, entry)
        except Exception as e:
            print("skip", fname, e, file=sys.stderr)
            continue
        if b[:4] != b"EFTB":
            continue
        blobs[fname] = b
        for fn, s, e, d, attrs, parent in walk(b, fname):
            res.append((fn, s, e, bytes(b[d:d + 0xA88]), {k: bytes(b[v[0]:v[0] + v[1]]) for k, v in attrs.items()}, parent))
    pickle.dump(res, open(CACHE, "wb"))
    return res


def field(data, off, fmt):
    return struct.unpack(">" + fmt, data[off:off + struct.calcsize(fmt)])[0]


def main():
    cmd = sys.argv[1]
    ems = all_emitters()
    if cmd == "hist":
        off, fmt = int(sys.argv[2], 0), sys.argv[3]
        c = collections.Counter(field(d, off, fmt) for _, _, _, d, _, _ in ems)
        for v, n in c.most_common(40):
            print(f"{v!r:>16} {n}")
        print("emitters:", len(ems), "distinct:", len(c))
    elif cmd == "find":
        off, fmt, val = int(sys.argv[2], 0), sys.argv[3], sys.argv[4]
        val = float(val) if fmt == "f" else int(val, 0)
        for fn, s, e, d, a, p in ems:
            if field(d, off, fmt) == val:
                print(fn, s, e, "child of " + p if p else "")
    elif cmd == "attrs":
        c = collections.Counter(); sz = collections.defaultdict(set)
        for *_, a, p in ems:
            for k, v in a.items():
                c[k] += 1; sz[k].add(len(v))
        for k, n in c.most_common():
            print(k, n, sorted(sz[k])[:8])
        print("emitters:", len(ems), "children:", sum(1 for e in ems if e[5]))
    elif cmd == "dump":
        for fn, s, e, d, a, p in ems:
            if e == sys.argv[2] or s + "/" + e == sys.argv[2]:
                print(fn, s, e, "attrs", list(a))
                for o in range(0, len(d), 16):
                    w = struct.unpack(">4I", d[o:o + 16])
                    fl = struct.unpack(">4f", d[o:o + 16])
                    print(f"{o:04x}: " + " ".join(f"{x:08x}" for x in w) + "  " + " ".join(f"{x:11.4g}" for x in fl))
                break
    elif cmd == "attr":
        magic = sys.argv[2]
        for fn, s, e, d, a, p in ems:
            if magic in a:
                v = a[magic]
                print(fn, s, e, len(v))
                for o in range(0, min(len(v), int(sys.argv[3], 0) if len(sys.argv) > 3 else 0x40), 16):
                    w = struct.unpack(">4I", v[o:o + 16].ljust(16, b"\0"))
                    fl = struct.unpack(">4f", v[o:o + 16].ljust(16, b"\0"))
                    print(f"  {o:04x}: " + " ".join(f"{x:08x}" for x in w) + "  " + " ".join(f"{x:11.4g}" for x in fl))


if __name__ == "__main__":
    main()

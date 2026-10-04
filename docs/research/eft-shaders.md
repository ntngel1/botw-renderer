# Particle shaders of the effect library (EFTB v20)

Research of 2026-10-03 for the particle über-shader. Sources: the Wii U
dump (update v208, `content/Effect/*.sesetlist` and
`Pack/Bootup.pack` → `Effect/GameResident.sesetlist`), the Cemu runtime
shader dumps in `reference/visual-formulas/cemu-sessions/*/shaders`, and a
light look at `U-King.rpx` in Ghidra. Everything below about formulas is
read from the game's own shader code (native GX2 programs, read through
Cemu's translation of exactly those bytes); nothing is fitted. Each claim is
tagged **F** (fact: read from code or data, evidence given) or **H**
(hypothesis: consistent with the evidence, needs the named check).
Field names for emitter bytes come from the code that reads them; where
`botw-formats::ptcl::emitter` (SI-FMT-13, third-party names) disagrees,
this document says so.

Generated material (outside Git, `reference/visual-formulas/eft-shaders/`):
`raw/` decompressed PTCL files, `glsl/` Cemu GLSL with reflection names,
`fold/` the same folded into readable code (one file per program,
`vs_<sha1[:12]>.glsl` / `ps_…`, header lists the files and emitters that
use it), `coverage.tsv` (every emitter → its programs, dumped or not).
Tools: [`tools/research/eft_shaders.py`](../../tools/research/eft_shaders.py)
and [`cemu_glsl_fold.py`](../../tools/research/cemu_glsl_fold.py); the
uniform mapper and simplifier are copies of the original renderer's
(`cemu_uniform_map.py`, `cemu_glsl_simplify.py`, `cemu_glsl_deps.py`).

## 1. Where the shaders are and how an emitter picks one

**F. Container.** `SHDA` (top-level node) → one child `SHDB`; its data (at
the node's data offset, 0x100-aligned in the file) is a complete
GFX2 file: `Gfx2` header 0x20 (version 7.1), then `BLK{` blocks (header
0x20: magic, header size, major, minor, type, data size, id, index). Block
types: 3 GX2VertexShader header, 5 VS program, 6 GX2PixelShader header,
7 PS program, 2 padding, 1 end. Headers are the WUT structs (VS 0x134 bytes:
size/program at +0xD0/+0xD4, mode +0xD8, then count/pointer pairs for
uniform blocks +0xDC, uniform vars +0xE4, initial values +0xEC, loops
+0xF4, samplers +0xFC, attributes +0x104; ring size +0x10C, stream-out
flag +0x110, strides +0x114. PS 0xE8: size/program +0xA4/+0xA8, mode
+0xAC, blocks +0xB0, vars +0xB8, initials +0xC0, loops +0xC8, samplers
+0xD0). Pointers are unrelocated: `0xD06xxxxx` = offset into the header
block's data, `0xCA7xxxxx` = into the program block; low 20 bits are the
offset. The n-th header pairs with the n-th program block of its stage.
The `SHDA` node's child-count field is not its number of children (always
one `SHDB`); in 663 of 867 files it equals the number of distinct program
pairs the emitters use. Its meaning is open and nothing here needs it.

**F. All 4378 distinct programs** (3108 VS, 1270 PS over 867 files) use
uniform-block mode (`mode` 1), no uniform variables, no initial values, no
loop constants.

**F. Selection.** Emitter data (0xA88 bytes, the `EMTR` node data):

| Offset | Type | Meaning |
|---|---|---|
| 0x914 | u32 | index into the file's VS list |
| 0x918 | u32 | index into the file's PS list |
| 0x91C | u32 | second program pair: VS index (always equal to 0x914 when used) |
| 0x920 | u32 | second pair: PS index; (0, 0) at 0x91C/0x920 = no second pair |
| 0x948 | char[16] | define the second pair was compiled with: `"ATEST_ONLY"` in all 1446 emitters that have one, empty otherwise |

Indices are per file (`GameResident` programs are not shared by index with
other files). Verified on all 9726 emitters of all 867 files: every index is
in range, and for both pairs the PS's input semantics
(`spi_ps_input_cntl` low bytes) are a subset of the VS's exported semantics
(`spi_vs_out_id`), which would not hold with any other pairing. Example:
`Rain_Distance` — `Cloud` VS0/PS0, `Cloud_Around` VS1/PS1, `Rain` VS2/PS2;
`Animal_Crow_A` `Bird_Feather_1` first pair VS1/PS1, second VS1/PS2.

**F. The stream-out program.** `GameResident` VS 956
(`12a043b6effc`, the only VS with `vgt_strmout_buffer_en` = 3, strides
16/16) is referenced by no emitter index: it is the per-frame simulation
pass of `calc_type` 2 emitters (writes position and velocity, see §6).
`GameResident` PS 481 (`60a647089b15`, reads nothing) is its companion.
**H**: the CPU picks it by fixed index/name; check the eft draw code.

**F. ATEST_ONLY second pair.** Same VS, a smaller PS (example
`ps_5658141df7db`: plain colour, no lighting). 1261 of the 1446 users are
`calc_type` 2. **Open**: which pass draws it (depth/shadow prepass?) — CPU.

**F. Some programs write four render targets** (`passPixelColor0/1/3/5`,
e.g. `ps_f5504d1e9cb1`, `ps_cc89af17e48e`): normal packed to 0.5·n+0.5,
material bytes, constant colours (0.0314, 0.3765) — a G-buffer path for
opaque mesh particles (rocks, debris). Not needed for translucent effects;
documented only by its existence.

## 2. Binding layout (reflection)

**F. Uniform blocks** are bound at fixed locations; every program that has
a block has it at the same location with the same size:

| Loc | Block | Size | Notes |
|---|---|---|---|
| 6 | `sysViewUniformBlock` | 320 | camera, §3.1 |
| 7 | `sysEmitterStaticUniformBlock` | 1872 = 0x750 | emitter data 0…0x74F, §3.2 |
| 8 | `sysEmitterDynamicUniformBlock` | 192 | per emitter per frame, §3.3 |
| 9 | `sysEmitterFieldUniformBlock` | 288 | only the stream-out VS |
| 10 | `sysEmitterPluginUniformBlock` | 256 | stripes, area loop (§5.10) |
| 11 | `sysCustomShaderReservedUniformBlockParam` | 256 | BotW custom shader params |
| 12 | `sysCustomShaderUniformBlock0` | 16 | 4 programs |
| 13 | `sysCustomShaderUniformBlock1` | 624 | BotW scene light/fog copy, §7 |
| 14 | `sysCustomShaderUniformBlock2` | 96 | BotW wind, §7 |

CPU evidence: `FUN_03b64338` looks up each name with its location
(`"sysViewUniformBlock",6` … `"sysCustomShaderUniformBlock3",0xf`) for VS
and PS and sets a mask bit per block found (location 15 `…Block3` exists in
code, unused by the files).

**F. Samplers** (location = texture unit): `sysTextureSampler0/1/2` 0/1/2
(PS), `sysFrameBufferTexture` 3, `sysDepthBufferTexture` 4 (PS and VS),
`sysCurlNoiseTextureArray` 5 (stream-out VS, type 3D/array),
`sysCustomShaderTextureSampler0…3` 6…9, `sysCustomShaderCubeSampler0` 10
(type 4 cube), `sysCustomShaderShadowArraySampler0` 12 (stream-out VS),
`sysCustomShaderShadowArraySampler1` 13 (VS, sampled as a plain 2D at
row 0).

**F. Attributes** are float4 (normal float3) and get locations in
alphabetical order of the names present, so the location of a name varies
by program; bind by name. Names: `sysPosAttr`, `sysLocalPosAttr`,
`sysLocalVecAttr`, `sysLocalDiffAttr`, `sysScaleAttr`, `sysRandomAttr`,
`sysInitRotateAttr`, `sysColor0Attr`, `sysEmtMat0/1/2Attr`,
`sysEmtRTMat0/1/2Attr`, `sysInPos`, `sysInVec`, `sysNormalAttr`,
`sysTangentAttr`, `sysTexCoordAttr`, `sysVertexColor0Attr`,
`sysEmitterPluginAttr0…4`. Meanings in §5.1.

**F. Varyings.** Semantic numbers are assigned per program pair by the
compiler; the same number does not mean the same quantity in every pair
(e.g. UV0 is `Sem8` in most pairs, `Sem7` in `ps_8cf0e7c8099a`). Always read
the VS and PS of a pair together.

## 3. Uniform block layouts

### 3.1 `sysViewUniformBlock` (320 bytes)

| Offset | Content | Evidence |
|---|---|---|
| 0x000 | view matrix, 4 rows (row r at 0x10·r) | rows dotted with world position; PS uses rows 0–2 on normals |
| 0x040 | projection, 4 rows (VS reads rows 2–3 at 0x060/0x070) | z and w of a view-space point after a camera offset |
| 0x080 | view-projection, 4 rows | `clip = dot(row_i, (world, 1))` |
| 0x0C0 | billboard matrix, 3 rows (x,y,z,w) — camera rotation inverse | `world = center + Σ row_i·local` in billboard type 0 |
| 0x100 | camera look direction (xyz) | cross products in velocity billboards, atan in type 7 |
| 0x110 | camera position (xyz) | `eye − p` everywhere |
| 0x120 | near n | `view_z = depthTex·(f−n) + n` |
| 0x124 | far f | dead particles are placed at clip z = 5·f |
| 0x128 | n·f | `view_z = −n·f / (z01·(f−n) − f)` |
| 0x12C | f − n |  |
| 0x130 | 3 floats, 4 uses (not read) |  |

`z01 = clip.z/clip.w·0.5 + 0.5` is reconstructed in the VS as
`(z·0.5 + w/2)/w`. **H**: the 0x120 names follow from the algebra
(`−nf/(z(f−n) − f)` is the GX2 perspective inverse); confirm on the CPU fill.

### 3.2 `sysEmitterStaticUniformBlock` (0x750 bytes) = emitter data 0…0x74F

**F.** The block is 1872 = 0x750 bytes; every offset the shaders read
matches the emitter layout: key counts at 0x60 (read as integers for the
random-colour pick), key tables 0x3C0/0x440/0x4C0/0x540/0x600/0x680,
pattern tables (slot 0 at 0x120, slot 1 at 0x1B0, read with dynamic index),
UV animation per slot at 0x2C0/0x310/0x360, near/far alpha 0x5D0,
soft particle 0x5F4, rotation 0x700…0x738, gravity 0xB0/0xBC, air
resistance 0xC0. Emitter data is 0x100-aligned in the file, which is GX2's
uniform-buffer alignment.

**F (CPU).** `eft_Resource_InitEmitterResource` (`0x03b61e98`), after
`eft_EmitterResource_Setup` (the load-time patch): saves the first 0x50
bytes, sets the buffer descriptor `er+8 = res`, `er+0xC = 0x750`, byte-swaps
it to GPU order (`0x03b68578`) and copies the 0x50 saved bytes back. So
the block **starts at emitter byte 0** and is 0x750 long; uniform byte X =
emitter byte X (bytes 0…0x4F — node name — stay big-endian and are never
read). `er+0x14` (= `res + 0x50` for GPU types, a private 0x700-byte copy
for CPU types) is a different pointer.

> **Conflict with the current bake.** `crates/bake/src/eft.rs`
> (`STATIC_START = 0x50`, 0x700 bytes) and `eft-runtime.md` §7 ("UBO byte =
> res − 0x50") shift every offset by 0x50. With that shift the shaders would
> read the flag word from 0xA0, colour keys from 0x410, gravity from 0x100,
> and 0x740/0x744 would fall outside the block. The block must be emitter
> bytes 0x000…0x74F (patched, little-endian words), or the shader offsets in
> this document minus nothing.

**F. Fields the CPU fills at load.** These offsets are read by shaders but
are zero in every one of the 9726 emitters of the dump, so the runtime
writes them:

| Offset | Content (from the shader's use) |
|---|---|
| 0x050 | u32 flags A (bit table below) |
| 0x054 | u32 flags B (stream-out fields, bit table in §6) |
| 0x05C | u32, bits 0…6 read by one program |
| 0x080 + 4k | loop period in frames of track k (k = colour0, alpha0, colour1, alpha1, scale); 0 → no loop |
| 0x094 + 4k | random-start factor of track k |
| 0x300/0x304, 0x350/0x354, 0x3A0/0x3A4 | per texture slot: UV repeat (u, v) — the `rep` in §5.8 |

The CPU side (`eft-runtime.md` §4.2, §7, `eft_EmitterResource_Setup 0x03b5f62c`) confirms the sources: loop periods from 0x8E4…0x8F4 when the
loop flags 0x8D8…0x8DC are set, random-start flags 0x8DD…0x8E1 as 1.0/0.0;
constant colours/alphas copied into key 0 and keys padded to 8; UV repeat
from the sampler's mirror flags (0 → (1, 1), 1 → (2, 1), 2 → (1, 2),
3 → (2, 2)); flag words as in the table below. The CPU's mirror of the loop
(`t = fmod(age + rnd·start·P, P)/P`) equals the shader's
`fract(rnd·start + age/P)`.

**F. Flags A (0x050)** — what each bit switches in the shader:

| Bit | Effect |
|---|---|
| 0x1, 0x2, 0x4 | scale fluctuation wave type (CPU: `0x9EF >> 4`); read by the programs with fluctuation (params 0xE0…0xFC) |
| 0x8 | world gravity (CPU: 0x7F1): the stream-out VS multiplies g by the transposed RT rotation (world → emitter space) when set |
| 0x10 / 0x20 / 0x40 / 0x80 | slot-0 pattern mode: fit-to-life / clamp / loop / random (§5.8) |
| 0x100 / 0x200 / 0x400 / 0x800 | the same for slot 1 |
| 0x10000 / 0x20000 / 0x40000 | random rotation direction for X / Y / Z (§5.5) |
| 0x80000 / 0x100000 | random flip of slot-0 U / V |
| 0x200000 / 0x400000 | random flip of slot-1 U / V |
| 0x800000 / 0x1000000 | slot 2 U / V (same pattern) |
| 0x2000000 / 0x4000000 | random start cell for the loop pattern, slot 0 / slot 1 |
| 0x20000000 | camera offset in depth (`clip.z −= (1 − z/w)·[0xD8]`) |
| 0x40000000 | camera offset towards the eye (`p += normalize(eye − p)·[0xD8]`) |

**F. Static-block fields read by the shaders** (offsets in the emitter
data; names by use):

| Offset | Use |
|---|---|
| 0x060 | u32×6 key counts (colour0, alpha0, colour1, alpha1, scale, param) |
| 0x0B0, 0x0BC | gravity vector, gravity scale (`g = [0xB0]·[0xBC]`) |
| 0x0C0 | air resistance r (velocity × r per frame) |
| 0x0D0/0x0D4 | quad pivot offset: local x += [0xD0]/2, y += [0xD4]/2 |
| 0x0D8 | camera offset distance (flags 0x20000000/0x40000000) |
| 0x0E0…0x0FC | read by BotW custom code (0xE8/0xEC around 20 in most files) |
| 0x100/0x104 | UV distortion strength (u, v), shader type 1/2 |
| 0x110 + 0x90·s | slot s flipbook: cell count (int), frames per cell (int), random cells, table of 32 i32 from +0x10 |
| 0x2C0 + 0x50·s | slot s UV animation: scroll add, scroll init, scroll random, scale add, scale init, scale random (vec2 each), rotate add/init/random, —, repeat (runtime), divisions |
| 0x3B0 | colour scale (multiplies RGB of colour0 and colour1) |
| 0x3C0/0x440/0x4C0/0x540/0x600/0x680 | 8 keys × (xyz, time) each |
| 0x5C8/0x5CC | fade by view angle (PS: `smoothstep(|N·V|)` between them) |
| 0x5D0/0x5D4 | near fade: alpha × sat((z − a)/(b − a)) |
| 0x5D8/0x5DC | far fade: alpha × (1 − sat((z − a)/(b − a))) |
| 0x5E0 | depth cut: alpha 0 where |scene − fragment| ≥ value (`ps_b7624bc01631`) |
| 0x5F0 | stretch along velocity |
| 0x5F4 | soft-particle distance |
| 0x700/0x710/0x720/0x730 | rotation init (also in `sysInitRotateAttr`), init random, add, add random (xyz) |
| 0x72C | rotation resistance |
| 0x740/0x744 | camera-distance size limits (§5.7) |

### 3.3 `sysEmitterDynamicUniformBlock` (192 bytes)

| Offset | Content | Evidence |
|---|---|---|
| 0x000 | colour0 multiplier RGBA | × key colour0 RGB, × alpha0 |
| 0x010 | colour1 multiplier RGBA |  |
| 0x020 | emitter time t (frames) | `age = t − sysLocalVecAttr.w` |
| 0x02C | frame step Δ (frames since the last update) | stream-out: `v·r^Δ`, `p += Δ·v`; draw VS adds it to the age used for motion |
| 0x030 | emitter alpha (fade in/out etc.) | × every alpha |
| 0x034/0x038/0x03C | particle scale x/y/z of the set | × size |
| 0x040 | emitter matrix (3 rows × 4) | world = M·(p, 1) for follow type 0 |
| 0x080 | emitter RT matrix (3 rows × 4, scale removed by normalising columns in the shader) | polygon billboards, stream-out |
| 0x0B0 | not read |  |

CPU fill (`eft_Emitter_UpdateDynamicUbo 0x03b6b724`, `eft-runtime.md` §7):
0x000 = emitter colour0 × EAC0 × set colour, 0x00C alpha0 × EAA0, 0x010/0x01C
the same for colour1, 0x020 emitter frame, 0x024/0x028 = 1.0, 0x02C frame
step, 0x030 set alpha × fade, 0x034… particle scale × set matrix scale ×
scale fade, 0x040 SRT, 0x080 RT (each 3 rows + `0,0,0,1`).

### 3.4 BotW blocks

`reserved` (loc 11), `cus1` (loc 13), `cus2` (loc 14): see §7. Their CPU
fill is BotW code, not eft; offsets are named by use there.

## 4. Pipeline per calc type

**F.** From the attributes of the VS each emitter uses (all 9726):

| calc (0x752) | follow (0x753) | VS reads |
|---|---|---|
| 0 CPU | 0 | per-particle attributes, current emitter matrix (`dyn` 0x040) |
| 0 | 1, 2 | + `sysEmtMat0…2` and `sysEmtRTMat0…2` (matrix captured per particle) |
| 1 GPU | 0 | `sysLocalPos/Vec` + closed-form motion, `dyn` 0x040 |
| 1 | 1, 2 | + `sysEmtMat0…2` |
| 2 GPU stream-out | 0 | `sysInPos/sysInVec` (simulated each frame by VS 956), `dyn` 0x040 |
| 2 | 1, 2 | + `sysEmtMat0…2` |

Stripe/plugin emitters (calc 0 with `sysEmitterPluginAttr*`) are a separate
VS family (ribbons, `plugin` block).

## 5. Vertex shader (draw pass)

Reference programs (all in `fold/`): `vs_137327b2b3a1` (GPU, billboard 0,
plain), `vs_e6cc513b8fef` (MountainCloud: stream-out, Y billboard, mesh,
BotW custom 3), `vs_e0a9f75d496f` (FieldRain01 `rain_near`: stream-out,
velocity billboard, area loop, custom 3), `vs_123be76ef9fe` (pattern
animation in full).

### 5.1 Attributes

| Attribute | xyz | w |
|---|---|---|
| `sysPosAttr` | quad/mesh vertex (local) | vertex index 0–3 (corner; custom expansion) |
| `sysLocalPosAttr` | birth position (emitter space) | life in frames |
| `sysLocalVecAttr` | initial velocity | birth time (emitter frames) |
| `sysScaleAttr` | particle size x, y (z unused) | motion multiplier (× gravity+velocity term) |
| `sysRandomAttr` | 4 randoms | 4th random |
| `sysInitRotateAttr` | initial rotation (radians) | — |
| `sysInPos`/`sysInVec` | current position/velocity from stream-out | — |
| `sysEmtMat*` | rows of the emitter matrix at birth | — |
| `sysNormalAttr`, `sysTexCoordAttr`, `sysVertexColor0Attr`, `sysTangentAttr` | mesh (`PRIM`) data | |

The randoms are four `rand()` in [0, 1) per particle (CPU fact,
`eft-runtime.md` §3 step 15).

### 5.2 Age and culling (all GPU programs) — F

```
age = dyn.time − LocalVec.w
if age < 0 or age >= float(int(LocalPos.w)):   // not born or dead
    clip = (0, 0, 5·far, 0)                    // degenerate, culled
```

### 5.3 Animation tracks — F

Track time for track k (colour0, alpha0, colour1, alpha1, scale):

```
P = emt[0x080 + 4k], R = emt[0x094 + 4k]
t = sign(P) == 0 ? age/life : fract(rand.x·R + age/P)
```

(computed as `(age/life)·(1 − sign(P)) + sign(P)·fract(…)`). The param
track (0x680) uses age/life. Key interpolation with n keys (unrolled per
key count, n is a compile-time constant of the program):

```
value = v0                       if t < t0
      = lerp(v_i, v_{i+1}, (t − t_i)/(t_{i+1} − t_i))   if t_i ≤ t < t_{i+1}
      = v_{n−1}                  if t ≥ t_{n−1}
```

written with `step(t_i, t)` products, so a key boundary belongs to the
following segment. Random-key track (source 3; CPU names 0 constant,
2 animated, 3 random): key index `int(rand.x · count)`, read as
`emt[0x3C + idx]` (colour0 table), and `emt[0x4C + idx]` for colour1
(count at 0x68).

Outputs: `colour0.rgb = keys0.rgb · dyn[0x000].rgb · emt[0x3B0]`,
`alpha0 = keysA0 · dyn[0x00C]`, same for colour1/alpha1 with `dyn[0x010]`.

### 5.4 Position — F

```
T = age + dyn.Δ                       // motion time (rotation uses age)
if r == 1:   G = g·T²/2,                 V = T
else:        G = g·(T − (r^T − 1)/ln r)/(1 − r),   V = (1 − r^T)/(1 − r)
p_local = LocalPos.xyz + Scale.w · (G + LocalVec.xyz · V)      // calc 1
p_local = InPos.xyz                                             // calc 2
center = Mat · (p_local, 1)    Mat = dyn emitter matrix (follow 0) or sysEmtMat (follow 1, 2)
```

with `g = emt[0xB0]·emt[0xBC]`, `r = emt[0xC0]`. This is the exact closed
form of `v ← v·r; p ← p + v` per frame.

### 5.5 Rotation — F

Per axis a ∈ {x, y, z} with randoms (x: rand.y, y: rand.z, z: rand.x for
the direction flip; x: (rand.x+rand.y), y: (rand.y+rand.z),
z: (rand.x+rand.z) for the speed spread):

```
s_a   = flag(0x10000<<a) && floor(rand·2) > 0 ? −1 : 1
w_a   = (add_a + (2·mean(rand_i, rand_j) − 1)·addRand_a) · s_a
θ_a   = InitRotate_a·s_a + (rand_a − 0.5)·initRand_a + w_a·h(age)
h(age)= rr == 1 ? age : (1 − rr^age)/(1 − rr),   rr = emt[0x72C]
θ_a   wrapped to [−π, π)
```

Rotation matrix by `rotation_type` (0x8AB; only 4, 5, 6 occur in the
dump), applied to the local vertex v:

| Type | Matrix | Verified in |
|---|---|---|
| 4 "YZX" | Rx·Rz·Ry | `vs_137327b2b3a1` |
| 5 "XYZ" | Rz·Ry·Rx | `vs_4d52b9666608` |
| 6 "ZXY" | Ry·Rx·Rz | `vs_652eb0623133` |

Local vertex before rotation:
`v = (size.x·(Pos.x + emt[0xD0]/2), size.y·(Pos.y + emt[0xD4]/2), Pos.z)`,
`size.x = Scale.x · scaleKeys(t).x · dyn[0x034]`, `size.y` likewise
with `.y`/`dyn[0x038]`.

### 5.6 Billboard types (`0x8AA`) — F unless marked

| Type | Placement of the rotated vertex v |
|---|---|
| 0 | `world = center + B·v`, B = view block 0x0C0 (camera-aligned) |
| 1 | basis facing the eye position: f = normalize(eye − center), x = normalize(up_cam × f) (up_cam = view row 1), y = f × x; `world = center + x·v.x + y·v.y + f·v.z` |
| 2 | Y billboard: the yaw atan2(eye − center) in xz is subtracted from the Y rotation angle before the matrix; world = center + R·v (`vs_e6cc513b8fef`) |
| 3 | polygon in the emitter's XY: `world = Mat·(p) + RT·v` with RT = dyn 0x080 columns normalised |
| 4 | polygon XZ (same RT path; **H** y/z swapped — read `vs_c3ba42e73198` fully) |
| 5 | velocity-aligned: d = normalize(motion direction), x = normalize(d × look), `world = center + x·v.x + d·v.y + (x × d)·v.z`; d from `p(T) − p(age)` (calc 1) or `InVec` (calc 2), with a fallback for |d| < 0.001 |
| 6 | velocity-aligned polygon (mesh), `vs_032ddc41a257` — not read in full |
| 7 | Y billboard by camera direction (yaw from atan2(look.x, look.z)), plus the camera offset projected through view 0x060/0x070 — `vs_558bccd44820`, not read in full |

### 5.7 Size by camera distance — F

`vs_123be76ef9fe`, `vs_e0a9f75d496f`:
```
d = |center − eye|
k = d > A ? max(d, B)/B : min(d, A)/A      A = emt[0x740], B = emt[0x744]
world = center + (world − center)·k
```

The operation is program-specific, not enabled by nonzero 0x740/0x744.
Captured programs also contain near-only (`vs_e0a9f75d496f`: `min(d,A)/A`)
and far-only (`vs_36a0d3baca4d`: `max(d,B)/B`) variants. The renderer maps
these captured uniform reads in `effects/programs.rs`. In particular,
`vs_99aaf84627f7` (LavaHaze), `vs_e6cc513b8fef` (MountainCloud) and the
volcano programs do not read those uniforms, despite 50/50 defaults in
some emitter blocks. Applying the operation globally made these particles
tens of times larger at distant viewpoints, causing sky artifacts and
heavy overdraw. Programs without a recovered sizing variant use no sizing.

### 5.8 UV — F

Per slot s with base S = 0x2C0 + 0x50·s, x', y' = vertex xy after the
random flips (flags in §3.2, flip if `rand > 0.5`):

```
cell  = pattern cell (below), col = cell % div.u, row = cell / div.u
rep   = emt[S+0x40]/div.u (u), emt[S+0x44]/div.v (v)        div = emt[S+0x48]
scale = age·scaleAdd + rand·scaleRand + scaleInit + scaleRand
scroll= age·scrollAdd + scrollInit + scrollRand·(1 − 2·rand)
u = x'·(scale.u − 1) + rep.u·(x' + 0.5 + col) − scroll.u
v = y'·(1 − scale.v) − rep.v·(y' − 0.5 − row) − scroll.v
```

(`rand` is a component of `sysRandomAttr`; which component per slot is
switched by `emt[0x054] & 1`). UV rotation (stripe VS `vs_431daefe715b`):
angle `−(age·rotAdd + rotRand·rand + rotInit + rotRand)`, rotating
`(x'+0.5, y'−0.5)` before the scale.

**Pattern (flipbook)**, slot 0 shown (`vs_123be76ef9fe`), N = int(count),
F = int(frames per cell):

```
i    = int(age / F)
cell = f10 · int(age/life · N)                         // fit to life
     + f20 · (i >= N ? N − 1 : i mod N)                // clamp
     + f40 · (i + f2000000 · int(rand·N))              // loop (+ random start)
     + f80 · int(rand·N)                               // random
cell = cell mod N (sign-safe)
cell = table[cell]       // i32 at emt[0x120 + 4·cell], slot 1: 0x1B0
```

### 5.9 Alpha factors — F

```
z_view   = −n·f / (z01·(f−n) − f)                       (§3.1)
near     = sat((z_view − emt[0x5D0])/(emt[0x5D4] − emt[0x5D0]))
far      = 1 − sat((z_view − emt[0x5D8])/(emt[0x5DC] − emt[0x5D8]))
Sem4.x   = dyn[0x030] · near · far · (area-loop fade) · (BotW masks)
Sem4.y   = param track (0x680)
```

Programs without a fade simply omit the factor (compile-time switch).

### 5.10 Area loop plugin (`EAA0`, weather) — F

`vs_e0a9f75d496f`, `plugin` block:

```
for i in x, y, z:
  f_i = fract(((P + plugin[0x000]) · A_i + plugin[0x090 + 4i]) / S_i / 2 + 0.5)
  A_i = (plugin[0x060+4i], plugin[0x070+4i], plugin[0x080+4i]),  S_i = plugin[0x0B0 + 4i]
P' = Σ_i S_i·(2f_i − 1)·B_i + plugin[0x050]      B_x = plugin[0x020…0x028], B_y 0x030…, B_z 0x040…
mode = plugin[0x0A0]: 1 → cull if P'.y > plugin[0x0A4]; 2 → cull if P'.y < plugin[0x0A4]
fade_i = plugin[0x010 + 4i] > 0 ? sat((1 − |2f_i − 1|)/plugin[0x010 + 4i]) : 1   (product → alpha)
```

**H**: `plugin` 0x000 is the camera-relative offset of the box, A/B its axes
(a matrix and its inverse); from the CPU plugin code.

## 6. Stream-out simulation (calc 2), VS 956 — partly read

**F** (`vs_12a043b6effc`): reads `sysInPos/InVec` (current) or, at the
first frame (`age ≤ 0.001`), `sysLocalPos/Vec`; writes position and
velocity to two stream-out buffers:

```
p' = p + Δ·v                          (Δ = dyn[0x02C])
v' = v·r^Δ + Δ·g                       (r = emt[0xC0], g = emt[0xB0]·emt[0xBC];
                                        g = RTᵀ·g when flags A & 0x8, world gravity)
```

followed by the eft "fields" switched by flags B (0x054): 0x2 random
(sum of four sines with periods `field[0x10…0x1C]·field[0x0C]` and
amplitudes `field[0x20…0x2C]`, defaults (4, 3, 1.5, 2) when
`field[0x34] ≠ 1`), 0x4, 0x8, 0x10, 0x20 (with `field[0xB4]`), 0x40 and
0x80 (curl noise from `sysCurlNoiseTextureArray`), 0x100, 0x200/0x400/0x800
(which emitter matrix the field works in). BotW extensions read
`sysCustomShaderTextureSampler0` (gathered height map → terrain collision)
and `sysCustomShaderShadowArraySampler0`. **Open**: a full read of the
field formulas (1721 folded lines); only needed for emitters with fields
(attributes `FCSF`, `FCLN`, …).

## 7. BotW custom shaders (0x92C)

**F.** `emitter[0x92C]` (u32) is BotW's custom shader index: 0 (5111
emitters) standard eft; 3 (3148: weather, clouds, haze, waterfalls, fire,
smoke); 4 (1441: lit mesh debris, leaves); 1, 2, 5 (≤ 13). 0x930 and 0x934
are its switch words (bits observed: 0x934 bits 0–18, 24, 28, 30 with
index 3). They compile into the programs; the blocks `reserved`, `cus1`,
`cus2` and samplers 6–13 appear only in custom programs. Programs of the
same custom index ignore parts of the standard combiner (same PS for
different combiner bytes, e.g. `ps_c2b1c7dcbefe` used by 200 emitters).

**F. Custom 3, VS outputs** (`vs_e6cc513b8fef`, `vs_e0a9f75d496f`):

- **Ambient light** `Sem14.rgb = texture(ShadowArraySampler1, (u, 0))`,
  `u = ((1 − sat((1 − v_screen)·0.7 + 0.15)) + 2.5)/12` (clouds; v = 0 at
  the top of the screen) or `u = ((1 − sat(param·reserved[0x30]·0.25 + 0.5)) + 8.5)/12` (rain). So it reads a 12×1 table between texels 2/3 or
  8/9. **H (strong)**: this is the `LightAnalyzer` table (`gsys_user4`,
  the original renderer `wiiu-character-shading.md`: field hemisphere light uses
  `lerp(T[2], T[3], …)` from the same 12×1 texture); confirm the binding of
  unit 13.
- **Fog** per vertex, the same model as the scene fog (the original renderer
  `wiiu-deferred-shading.md`, PS 140):
  `A = cus1[0x178]·(1 − (1 − sat(z·cus1[0x160] − cus1[0x164]))^cus1[0x170])`;
  LUT colour `texture(TextureSampler2, (acos-term of V·cus1[0x1C0], 0.5 + 0.5·(1 − tA)^cus1[0x174]))·k`; B with `cus1[0x250…0x26C]` (or
  `0x180…0x18C` in rain) and `cus1[0x19C]`; height fog
  `H = sat(cus1[0x1B4]·(min(eye.y, cus1[0x1B8]) − y) + cus1[0x1B0])·cus1[0x1AC]`.
  Out: `Sem15 = (LUT.rgb·k, A)`, `Sem16 = (B, H, …, (1 − cube weight))`.
- **Wind and sway** (`reserved` 0x008…0x024, 0x054; `cus2` 0x008…0x01C):
  sinusoidal sway with phase from world position and time, growing with
  age and distance from the emitter, plus a wind push ∝ (height in the
  particle)²; UV scroll by wind (`reserved[0x02C]·cus2[0x008/0x00C]`).
- **Corner expansion** `±reserved[0x000]` along the quad axes by vertex
  index (`sysPosAttr.w`).
- **Rain mask** (rain): `texture(TextureSampler3, (x·cus1[0x208] − cus1[0x200], z·cus1[0x208] − cus1[0x204])).x` → alpha 0 where the
  particle is below `cus1[0x20C]·(1 − h)`. **H**: a top-down depth map
  (like the field's `gsys_depth_shadow_quarter`).
- Normal (mesh) rotated with the particle → `Sem11` for the cube lookup.

**F. Custom 3, PS** (`ps_da1089a7498c`, `ps_991ea4f8496e`,
`ps_adb2bc8ca1a2`, `ps_f45836b968a2`, `ps_d3bad5d39db8`):

```
L   = Sem14.rgb + cus1[0x01C] · reserved[0x004] · cube(N = Sem11, lod = cus1[0x010]).rgb · k
      (k = 1 or 1 − Sem16.w; the cube term is absent in rain/snow programs)
C   = combine(…) · L
C   = lerp(C, cus1[0x1A0].rgb, Sem16.y)        // height fog colour
C   = lerp(C, Sem15.rgb, Sem15.w)               // LUT fog
C   = lerp(C, cus1[0x190].rgb, Sem16.x)         // fog B colour
A   = Sem4.x · sat(alpha combine) · soft
```

The combine part inside custom 3 is program-specific (e.g. clouds:
`lerp(0.9·tex1², tex0², tex0.a)·tex2²` colour and
`alpha1·alpha0·tex2.a·(tex0.a + tex1.a)·vc.a²` alpha in
`ps_da1089a7498c`; `MountainCloud`: tex0 RG as a UV offset for tex1
(`tex = 2·t − 1`), alpha `tex1.a·sat(tex0.a + 2·alpha0 − 1)` in
`ps_991ea4f8496e`). **Open**: the mapping custom switch bits → code
blocks; to be done per used emitter by reading its program (they are all
in `fold/`), or from the custom-shader source if it is ever found.

**H.** `cus1` is a BotW-packed copy of the environment fog/light values
(the fog formula matches field PS 140 term by term, with a different
packing); its CPU fill (BotW code, not eft) settles the mapping to
`env`/`bwinfo` values.

## 8. Pixel shader, standard path (custom 0) — F

Inputs from the VS: colour0/alpha0, colour1/alpha1, UV0/UV1 (one vec4),
UV2, the alpha factor `Sem4.x`, clip position for soft particles,
vertex colour (meshes). Combiner bytes (emitter 0x8F8 + i):

| i | Offset | Meaning (verified values) |
|---|---|---|
| 0 | 0x8F8 | colour process: 0 C = c0; 1 C = c0·T; 2 C = lerp(c1, c0, T); 3 C = c0·T + c1 |
| 1 | 0x8F9 | alpha process: 0 a = a0·Ta; 1 a = a0·a1·Ta; 3 a = a1·(Ta − a0); 4 a = a1·sat((Ta − a0)·4) |
| 2, 3 | 0x8FA, 0x8FB | tex1, tex2 colour blend into T: 0 multiply, 1 add, 2 subtract |
| 5, 6 | 0x8FD, 0x8FE | tex1, tex2 alpha blend into Ta: 0 multiply, 1 add, 2 subtract |
| 4, 7 | 0x8FC, 0x8FF | primitive (vertex) colour / alpha blend (values 0–2) |
| 8–10 | 0x900–0x902 | tex0/1/2 colour input: 0 rgb, 1 one (white), 2 one minus |
| 11–13 | 0x903–0x905 | tex0/1/2 alpha input: 0 a, 3 one minus a; 2 seen as constant 1 (`ps_e5c22483a164`) |
| 14, 15 | 0x906, 0x907 | primitive colour / alpha input |
| 16 | 0x908 | shader type: 0 normal; 2 tex0 rg as UV offset for tex1/tex2 (`(2t−1)·emt[0x100/0x104]·Sem`); 1 refraction: framebuffer sampled at screen uv + `(2t−1)·25·emt[0x100/0x104]` (`ps_7fc7faaaa65c`) |
| 17 | 0x909 | 1 in 96 % of emitters; effect not isolated |

Process 2 with a single texture: `C = c1 + (c0 − c1)·T`. Texture
linearisation: `t² ` per colour channel **only when the sampler's byte
+0x17 is 1** (sampler record at 0x9F8 + 0x20·s; verified across
`ps_6392c086c8f2` (0: not squared), `ps_6e74c0fff3a1` and
`ps_22bfa8c2bdfb` (1 squared, 0 not)); alpha is never squared. Inputs
"one minus" use the (possibly squared) value. A texture slot without a
texture contributes the constant 1.

```
Ta  = combined alpha;  a = process(…)
alpha_out = Sem4.x · sat(a) · soft
soft  = sat((z_scene − z_frag)/emt[0x5F4])
z_scene = depthTex(screen uv)·(f − n) + n      z_frag from the clip position (§3.1)
discard if alpha_out ≤ alphaTestRef           (Cemu's GX2 alpha test; ref from the CPU)
```

Some programs add `smoothstep(emt[0x5C8], emt[0x5CC], |N·V|)` (view-angle
fade) and a depth cut (`emt[0x5E0]`). Output is not premultiplied.
**H (strong)**: `sysDepthBufferTexture` is the scene's normalized linear
depth (`(z − n)/(f − n)`, the NLD of the original renderer's
`wiiu-sky-resources.md`); confirm the texture bound to unit 4.

Special variants seen (read their files when needed): gradient map
(`ps_dcab6965eda1`: tex1 sampled at (0.5, a0 + r·tex0.r), luminance
weights 0.2989/0.5866/0.1145), depth glow (`sat(1 − d/5)`), view fade
`sat(−z·0.1 − 0.6)` (`ps_62bc0dbabd07`).

## 9. Blend modes — F (CPU)

`eft_SetBlendType` (`0x03b5f420`, named by the CPU research) calls
`GX2SetColorControl(LOGIC_OP_COPY, …)` then `GX2SetBlendControl` per type
(GX2 enums; colour and alpha use the same factors):

| Type | src | dst | op |
|---|---|---|---|
| 0 normal | SRC_ALPHA | INV_SRC_ALPHA | ADD |
| 1 add | SRC_ALPHA | ONE | ADD |
| 2 subtract | SRC_ALPHA | ONE | DST_MINUS_SRC (reverse subtract) |
| 3 multiply | ZERO | SRC_COLOR | ADD (alpha: INV_DST_COLOR, ONE) |
| 4 screen | INV_DST_COLOR | ONE | ADD |

So 3 is multiply and 4 is screen (`ptcl::emitter` comment has them
swapped — SI-FMT-13). The argument is `RenderState.blend_type` (0x89E,
`eft_SetRenderState 0x03b5f5ac`, `eft-runtime.md` §6.3; values 0/1/2/3/4 =
6259/3257/3/19/188 emitters).

## 10. Coverage of the Cemu dumps

**F.** 927 of 4378 programs (589 VS, 338 PS) are in the dumps; 2145 of
9726 emitters have both programs (`coverage.tsv`). Weather that is
covered: `FieldRain01*` (rain near/far), `FieldSnow*`, `FieldRainDepth01`,
`Rain/Snow/SandStorm_Distance` clouds, `MountainCloud*`, `VolumeMaskFog*`,
`PlacementHaze`, `FX_CliffWhiteWaterFall_01`. **Missing** (needs a capture
before porting): `Rain_Distance` `FarWeather_Rain/Rain` (VS
`d0e2918f7f77`, PS `11cec2036c7e`), `FieldRain02b/rain_middle`,
`EnvSandStorm/*`, `ThunderBallCommon/*`, demo rain variants. Missing
programs can still be read from the native code with a Latte disassembler
(not written; the matched programs above give the vocabulary).

## 11. Reproduction

```sh
python3 tools/research/eft_shaders.py manifest "$DUMP/update/content/Effect/Rain_Distance.sesetlist" --emitters
python3 tools/research/eft_shaders.py manifest "$DUMP/update/content/Pack/Bootup.pack:Effect/GameResident.sesetlist"
python3 tools/research/eft_shaders.py glsl "$OUT"/raw/*.ptcl --cemu "$REF"/cemu-sessions/*/shaders --out "$OUT/glsl" --fold
python3 tools/research/eft_shaders.py coverage "$OUT"/raw/*.ptcl --cemu "$REF"/cemu-sessions/*/shaders > "$OUT/coverage.tsv"
```

The Python Yaz0 reader is slow on `GameResident` (~10⁷ bytes); the `raw/`
files were made with `cargo run -p botw-formats --example unyaz0` and
`pack_ls` (`OUT=` extraction). Tests: `python3 tools/research/*_test.py`.
Renaming refuses when the Cemu ↔ native constant-cache pairing is not
channel-exact and bijective (DOT4 operands are paired per channel).

## 12. Open questions

1. Settled by the CPU research except the bits not isolated here; the bake
   must use emitter bytes 0…0x74F for block 7 (see §3.2 conflict).
2. Who binds block 7 (the resource itself or a copy), units 4 (depth: NLD?),
   10 (cube: which cube map), 13 (`LightAnalyzer` 12×1?), 8/9 (fog LUT,
   top-down depth), and fills `cus1`/`cus2`/`reserved` (BotW code).
3. `dyn` 0x000/0x010/0x030 composition (set colour × emitter colour,
   fade); `dyn` 0x02C exact meaning at draw time (frame step).
4. The `+scaleRand` term of the UV scale (shader adds
   `scaleRand·(1 + rand)`; check against the CPU texture-anim setup, which
   may store a pre-negated random).
5. Billboard types 4, 6, 7 and the stripe/plugin VS family: read in full.
6. Stream-out fields (flags B) in full; collision with terrain.
7. Custom shader 3/4 switch bits → features; custom 1/2/5.
8. Alpha input 1/2, primitive blends and 0x909 — no program isolates them.
9. When the `ATEST_ONLY` pair and the MRT programs are drawn.
10. Missing dumps listed in §10 (rain streaks of `Rain_Distance` first).

### Deferred volcano particles (`ps_8ad4988869ca`)

The far plume's custom shader 4 fragment program writes targets 0, 1, 3
and 5. Target 0 contains `(4/255, 96/255, 0, alpha)` material data; target
1 contains clamped color, target 3 an encoded view-space normal, and
target 5 the excess emissive color. It is not a regular color-overlay
program. Until that deferred pass is ported, custom shader 4 is opt-in
as an explicitly incomplete debug preview (SI-EFX-36); ordinary smoke
and cloud emitters (custom shader 3) remain active.

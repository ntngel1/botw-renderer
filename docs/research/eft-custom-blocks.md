# eft plugin and BotW custom-shader uniform blocks (Wii U v208)

Research of 2026-10-03 for the weather and cloud particles (BotW custom
shader 3). It answers what the CPU writes into the four blocks the eft
über-shader reads beyond the standard ones
([eft-shaders.md §2](eft-shaders.md#2-binding-layout-reflection)):

| Loc | Block | Size read | Who fills it |
|---|---|---|---|
| 10 | `sysEmitterPluginUniformBlock` | 0xC0 written (block 0x100) | eft, area-loop plugin (EP04) draw override `0x03b68820` |
| 11 | `sysCustomShaderReservedUniformBlockParam` | 0x80 written | eft default: copy of the emitter's `CSDP` node |
| 13 | `sysCustomShaderUniformBlock1` (`cus1`) | 0x270 | BotW, once per draw pass: `0x03786fc4` |
| 14 | `sysCustomShaderUniformBlock2` (`cus2`) | 0x60 | BotW, per emitter draw: custom 3/4 render-state callback `0x038714c4` |

Source: `U-King.rpx` (update v208). The Ghidra GUI instance was not
responding during this session (process paged out, no reply even to
`jstack`), so the code was read in a separate **headless** Ghidra 12.1.4
on a fresh import of the same RPX (no auto-analysis; functions created on
demand, decompiled and disassembled; a small command server script).
Function names from the GUI project (`eft_*`) are therefore not used as
evidence and nothing was renamed in the project. Paired-single code
(`psq_l`, `ps_madds*`) decompiles badly; those parts were read from the PPC
listing. Bulk outputs (decompiles, listings, the server script, the `EP04`
and `CSDP` dumps) are in the session scratchpad, not in the repo (writing
to `game-data/reference/` was not permitted in this session).

Conventions as in [eft-runtime.md](eft-runtime.md): **F** = read from
code (address given) or counted in the data with
`tools/research/ptcl_fields.py`; **H** = hypothesis. `em+X` emitter,
`er+X` emitter resource, `res+X` emitter data, `set+X` emitter set.
Offsets in a block are bytes; `E[k]` is the k-th float of an object.

## 1. How blocks are bound (F)

- Block slots: global table `0x10597c04 + 4·(loc − 6)` holds the GX2
  location of each block (filled by `0x03b64104`: view 6, static 7,
  dynamic 8, field 9, plugin 10, reserved 11; `0x10597c1c + 4k` = custom
  block k at 12 + k). `0x03b68674(slot, {ptr,size}, vs, ps)` calls
  `GX2SetVertexUniformBlock` / `GX2SetPixelUniformBlock`; the vs/ps flags
  come from the shader's block mask `shader+0x530` (built by `0x03b64338`).
- **Callback sets** (11 function pointers, 0x2c bytes) live in the eft
  System: emitter plugins `System+0x15a4 + 0x2c·(id−1)` (id = `er+0x1a0`,
  1…4), custom actions `System+0x12b8 + 0x2c·(id−1)` (`res+0x968`, enable
  byte `System+0x12a3+id`), custom shaders `System+0x12b8 + 0x2c·(8+id)`
  (`res+0x92c`, enable byte `System+0x12ac+id`; id 0 uses slot 8 if
  enabled). Stored per emitter at creation (`0x03b5abc0`): plugin `em+0x2b0`,
  custom shader `em+0x2ac`, custom action `em+0x2a8`.
- Slots by use: `+0x04` emitter init, `+0x10` emitter per-frame update,
  `+0x14` **draw override** (called before the normal draw, `0x03b591c8`;
  non-zero return = the callback drew the emitter itself), `+0x18`
  finalize, `+0x1c` CPU particle calc, `+0x28` **render-state** (bind
  blocks and textures; called from `eft_Renderer_DrawEmitter 0x03b58d34` →
  `0x03b56370`).
- `0x03b56370` (per draw of an emitter, after the static/dynamic/field
  blocks): custom-action `+0x28`, then custom-shader `+0x28`; if the
  emitter has no custom-shader `+0x28` but has a `CSDP` node, the default
  `0x03b646f4` binds the reserved block (§3). Then, **only if** the plugin
  set's `+0x28` is null, `0x03b648e4` binds the raw plugin node data
  (`er+0x1a4`, 0x80 bytes) as the plugin block.
- Global custom blocks: `eft_System_BeginRender 0x03b66e90` (called by the
  gsys particle draw pass `0x039fd3fc`) uploads the view block and, for
  k = 0…3, copies `System+0x17d0[k]` (size `System+0x17e0[k]`) to a fresh
  buffer and binds it as custom block k. BotW sets k = 1 (`cus1`) through
  `0x039fbb0c` from `0x0378753c` (§4).
- BotW registration of the custom shaders: `0x037822e0` (called from the
  BotW particle manager init `0x03782ce4`). Custom 3 and 4 share the
  functions: init `0x03870c70`, update `0x03870ec8`, finalize
  `0x03870dec`, render-state `0x03873924` / `0x0387392c` = `0x038714c4`
  with r4 = 0 / 1, particle calc `0x03873914` / `0x0387391c` = `0x0387225c`
  with r4 = 0 / 1. Custom 1: init `0x03788fb4`, `+0x10` `0x03789100`,
  `+0x18` `0x0378905c`, render-state `0x03789268` (it uploads custom block 0,
  0x240 bytes; not needed for weather). Custom 2: `0x03873cc8`; 5:
  `0x03873db4`; 8 (default): `0x03873e5c`.

## 2. Plugin block, EP04 "area loop" (F)

### 2.1 Node data (`EP04`, 0x50 bytes; 61 emitters)

The node payload is 0x50 bytes (the dump's "112" includes the next node's
header). Fields by the code that reads them (`0x03b68820`), values from
the dump (`ptcl_fields.py attr EP04 112`):

| Off | Type | Meaning | Data |
|---|---|---|---|
| 0x00 | vec3 | offset step between repeated draws | rain_near (2.3, 1, 6), rain_far (18, 15, 23) |
| 0x0C | f32 | number of extra draws; draws = trunc(value + 1) | 2/3/4/9 (heavy variants larger) |
| 0x10 | vec3 | half-size S of the box | (13, 5, 13), (80, 64, 80), snow far (60, 48, 60) |
| 0x1C | f32 | cut height (shader `plugin[0xA4]`) | 0 everywhere |
| 0x20 | vec3 | box centre offset (camera space in camera mode, local in world mode) | (0, 2, −11), (0, 5, −85) |
| 0x2C | i32 | cut mode (shader `plugin[0xA0]`: 1 above, 2 below) | 0 everywhere |
| 0x30 | vec3 | edge fade widths, in box fractions | 0 for rain/snow; 0.4/0.35 wind, 0.5 dust bokeh |
| 0x3C | f32 | ≠ 0: box follows the camera | 1 in 50 emitters, 0 in 11 |
| 0x40 | vec3 | Euler rotation of the box (`0x03b5ee80`) | 0 everywhere |
| 0x4C | — | not read | |

### 2.2 Callbacks

Plugin 4 is registered by eft itself (`0x03b695d8`): only `+0x14` =
`0x03b6959c` → `0x03b68820(em, shaderIndex, userParam)` and a `+0x28` stub
that returns 1 (so the raw-node default bind of §1 never happens). There
is no calc callback: the CPU never moves the particles; the wrap is done
entirely in the vertex shader.

### 2.3 What `0x03b68820` does per draw of the emitter

1. Box matrix M (4×4, row-vector convention: rows 0–2 axes, row 3
   translation):
   - camera mode (`EP[0x3C] ≠ 0`, `0x03b68d14…0x03b6909c`): view block of
     the current core (`System+0x554 + core·0x140`, the CPU copy of
     `sysViewUniformBlock`). `R = Euler(EP[0x40])`,
     `T'_r = dot(view[0x0C0 + 0x10·r].xyz, EP[0x20..0x28])` for r = 0…2
     (the billboard rows: the offset is in camera space),
     `M = [R ; T' + eye]` with `eye = view[0x110..0x118]`. With the dump's
     zero rotation: `M = [I ; eye + B·EP[0x20]]`.
   - world mode (`EP[0x3C] = 0`, `0x03b68a18…0x03b68d10`):
     `M = [Euler(EP[0x40]) ; EP[0x20]] × emitterSRT` (the emitter matrix
     `em+0x22c`, 3×4, expanded to 4×4). Not needed for weather.
2. `Minv = M⁻¹` (Gauss–Jordan with pivoting, `0x03b691e8…0x03b6937c`).
3. For k = 0 … trunc(EP[0x0C] + 1) − 1 (`0x03b6938c…0x03b69554`): allocate
   0xC0 bytes, fill (below), bind as the plugin block (`0x03b648cc`), call
   `eft_Renderer_DrawEmitter 0x03b58d34` (so the custom-shader callback and
   the reserved block run again for every copy), then
   `offset += EP[0x00..0x08]`.

**The emitter is drawn trunc(EP[0x0C]) + 1 times**, each copy shifted by
k·step before the wrap — this is how rain and snow get their density
(rain_near: 3 draws, FieldSnowHeavy far: 10).

### 2.4 Block layout (0xC0 bytes written; 0xC0…0xFF unwritten)

| Off | Value | Shader use (§5.10 of eft-shaders) |
|---|---|---|
| 0x000 | offset = k·EP[0x00..0x08], w: — (0x00C = EP[0x3C]) | added to the particle world position |
| 0x010 | EP[0x30], EP[0x34], EP[0x38], 0 | fade widths (fade_i applied when > 0) |
| 0x020 | M row 0 (axis x) | `B_x` |
| 0x030 | M row 1 | `B_y` |
| 0x040 | M row 2 | `B_z` |
| 0x050 | M row 3 (box centre in world) | added after the wrap |
| 0x060 | Minv row 0 | `A_i` x components (`plugin[0x060+4i]`) |
| 0x070 | Minv row 1 | y components |
| 0x080 | Minv row 2 | z components |
| 0x090 | Minv row 3 | the `+plugin[0x090+4i]` term |
| 0x0A0 | float(EP.i32[0x2C]), EP[0x1C], 0, 0 | mode, cut height |
| 0x0B0 | EP[0x10], EP[0x14], EP[0x18], 0 | half-size S |

So in camera mode with zero rotation the shader computes
`local = P + k·step − (eye + B·EP[0x20])`, `f = fract(local/(2S) + ½)`,
`P' = S·(2f − 1) + eye + B·EP[0x20]`: every particle is wrapped into the
box of half-size S centred `EP[0x20]` in front of the camera, k copies
shifted by `EP[0x00]`.

### 2.5 EP01…EP03 (stripes)

Not decoded. Their blocks are computed by the stripe code and bound with
`0x03b648cc` from `0x03b69518` (area loop), `0x03b77c6c`, `0x03b77dac`,
`0x03b77eec`, `0x03b78028` (plugin 2), `0x03b7b390`, `0x03b7b4e4`,
`0x03b813d0`, `0x03b81524`, `0x03b8166c`, `0x03b817bc` (plugins 1/3);
registrations at `0x03b7b6ec`, `0x03b78280`, `0x03b81a48`.

## 3. Reserved block = the `CSDP` node (F)

`0x03b646f4`: if `er+0x188` (`CSDP` data) is set, copy **0x80 bytes** from
it into a temporary buffer and bind it at location 11. The custom 3/4
render-state callback calls it first (`0x038714c4` → `0x03b56340`), so for
all custom-3 emitters **`reserved[x]` = `CSDP` data byte x** (floats in
file order). The `CSDP` payload is 0x64 bytes for custom 3 (3148
emitters) and 0x34 for custom 4 (1441), so bytes after the payload are
whatever follows in the file (next node header); the shaders read only
up to 0x060.

CPU readers of `CSDP` fields (the rest is shader-only; names by use in
eft-shaders §7):

| Off | CPU use |
|---|---|
| 0x28 | cube-map index for `CubeSampler0`: `min(uint(value), 7)` (`0x038714c4`); also read by shaders (`floor(v·0.996·64)`) |
| 0x40…0x4C | point in emitter space tracked for `cus2[0x020..0x03C]` when `res+0x934` & 0x40000000 (`0x03870ec8`) |
| 0x38, 0x5C | CPU particle calc only (`0x0387225c`) |

## 4. `cus1` (0x270 bytes, F)

### 4.1 Producer and timing

`0x0387c5c4` is the gsys particle manager's pre-draw callback
(`PtclMgr+0x6e7c`, set in `0x03782ce4`); `0x039fd400` calls it right
after `eft_System_BeginRender`. It calls `0x0378753c` → allocate 0x270
bytes, fill with `0x03786fc4(PtclMgr, buf, drawCtx)`, register as custom
block 1 (`0x039fbb0c`). Because `BeginRender` binds the registered buffer
**before** the callback refreshes it, each pass draws with the values
produced at the previous pass (one draw pass late; same frame except the
first pass).

`drawCtx` = the per-view gsys particle context (`view+0x20c`, constructor `0x039fb0a8`); `drawCtx+0xf2c` = a per-view 0x13c-byte **particle environment record** `E` (array `PtclMgr+0x6ecc`, `0x03783780`), filled every frame by the KSys environment writer `0x033ff8cc` (the same function that builds the gsys environment block; its local `pf[k]` = `environment[25 + k/4]`, `docs/research/wiiu-deferred-shading.md`, section "Fog pre-ding"). `PtclMgr` = `*(0x1047c210)`.

### 4.2 The record `E` (`0x033ff8cc`, PPC `0x03400b30…0x03400d48`)

| E word | Value |
|---|---|
| 0–3 | e25.xyz, 1.0 |
| 4–7 | e26 (scatter fog: 1/(far−near), near/(far−near), …) |
| 8–11 | e27 (scatter atten, horz, density, e27.w) |
| 12–15 | e28.x, e28.y, e29.z, e29.x (ad hoc fog, rain-program packing) |
| 16–19 | e28 (ad hoc near/far terms) |
| 20–23 | e29 (atten_grd, —, atten_sky, 1 − minscale_sky) |
| 24–27 | e30 (ad hoc fog colour, alpha = strength) |
| 28–31 | `fog_scatter` colour rgba (`KSys+0xa8` object `+0xe8…+0xf4`, = e13; alpha = moisture/100) |
| 32–35 | −Start·k, k = 1/(End − Start) (k = 1 when End = Start), **250.0**, 0 (Start/End `+0xb8`/`+0xc8` of the same object) |
| 36–39 | `dir_main` Direction (`KSys+0xa0` `+0x11c…+0x124`, direction the light travels), 0; without the light (0, 0, 1, 0) |
| 40–43 | top-down map transform from `KSys+0x858` (sky occlusion): ((cx − s/2)/s, (cz − s/2)/s, 1/s, ±h) with centre `+0x7dc/+0x7e0`, size s `+0x780`, h `+0x788` (negated when `+0xe0c` bit 1 is clear) |
| 44 (0xb0) | texture: sky LUT of the sky object (`KSys+0x854` → record `+0x670`, +4) = `gsys_user0` |
| 45 (0xb4) | texture: sky-occlusion depth `KSys+0x858` `+0x188 + i·0x17c` (i = `+0xde0`) if `+0xe0c` bit 1, else default `*(0x1047ebc8)+0x38` |
| 46 (0xb8) | texture: `+0x480 + i·0x17c` of the same object (= `gsys_depth_shadow_quarter`) or the default |
| 47 (0xbc) | texture: `(gsys+0x2b0c)→+0x1c8 [view·0x850] + 8` (used by the volume-mask flag 0x1000; **H** volume-mask buffer) |
| 48 (0xc0) | texture: **LightAnalyzer** 12×1 table (`KSys+0x1e0` → `+0x198 [view·0x6f0]` + 0x340 = `gsys_user4`) |
| 49 | 1 − LightAnalyzer `cForceShadowRatio` (`+0x6ec`, default 1) |
| 50 | `KSys+0xb58` = min(SkyMgr `+0x2114` + flash L (`+0x2f0`), 1) (`0x03657c94` → `0x03408770`; the same value gives `base_light_change_ratio` = 1 − it, weather.md §5.2) |
| 51–54 | words 7…10 of `(gsys+0x2b0c)→+0x1c0`; 54 = 0 unless `0x039cf9d8(gsys+0x28d8)` |
| 55–62 | `gsys+0x2b08` cloud colours × intensities (not used by `cus1`) |
| 63–70 | `KSys+0x85c` `+0x26c…+0x278`, `+0x338…+0x344` (`0x033ff87c`) |
| 71–78 | e37.x, then 0, 0, 0, 1.4, 2.0, 0.35, 0.1 (rodata constants) |

### 4.3 `cus1` layout (`0x03786fc4`)

| Off | Value | Used by (folded programs) |
|---|---|---|
| 0x000–0x008 | shadow cascade splits (`drawCtx+0x5ac` shadow record `+0x238` words 1–3) | lit/shadowed programs (19), stream-out VS |
| 0x00C | 0 | |
| 0x010 | E[50] (`KSys+0xb58`); 0 when `PtclMgr+0x1353c` bit 26 | **cube LOD** in custom-3 PS (`textureLod(Cube0, …, cus1[0x010])`) |
| 0x014 | shadow record `→+0x1440` `+0x50c` | |
| 0x018 | T(`PtclMgr+0x6f78`) (sheltered smoothed wind speed, §4.4) | stream-out VS |
| 0x01C | clamp(1 − E[49], 0, 1) = cForceShadowRatio; 1.0 when bit 26 | **cube light scale** in custom-3 PS |
| 0x020–0x028 | wind direction `PtclMgr+0x6f80…0x6f88` | stream-out VS |
| 0x02C | T(`PtclMgr+0x6f7c`) (smoothed wind speed) | stream-out VS |
| 0x030, 0x070, 0x0B0 | 3 cascade matrices 4×4 (shadow record `+0x270` cascades, stride 0x574, `+0x284`) | shadowed programs |
| 0x0F0 | 3×4 matrix, shadow record `→+0x1440` `+0x490` | |
| 0x120 | 4×4 matrix, shadow object `+0xf64` | (not read) |
| 0x160–0x17C | E[4..11] = e26, e27 | fog A (rain, clouds) |
| 0x180–0x18C | E[12..15] = e28.x, e28.y, e29.z, e29.x | fog B in rain programs |
| 0x190–0x19C | E[24..27] = e30 | fog B colour, strength |
| 0x1A0–0x1AC | E[28..31] = `fog_scatter` colour, moisture | height fog colour, strength |
| 0x1B0–0x1BC | E[32..35] = −Start·k, k, 250, 0 | height fog `sat(k·(min(eye.y, 250) − y) − Start·k)` |
| 0x1C0–0x1CC | E[36..39] = `dir_main` direction | LUT u, LUT brightness |
| 0x1D0 | E[0..3] = e25, 1 | (not read) |
| 0x1E0, 0x1F0 | E[63..66]·s, E[67..70]·s, s = `*(0x1047c20c)` (4.0 in the file) | 2 programs |
| 0x200–0x20C | E[40..43] top-down map transform | rain mask (`TextureSampler3`) |
| 0x210–0x21C | E[51..54] | 3 programs |
| 0x220, 0x224 | `drawCtx+0xf8 · +0x100` | stream-out VS, 1 PS |
| 0x228 | 1/max(…) of `drawCtx+4` object (`0x03a7e470`) | |
| 0x22C | 0 | |
| 0x230 | 2·`drawCtx+0x104`·`+0xf8`·`+0x100` | stream-out VS |
| 0x234 | 2·`drawCtx+0x108`·`+0x100` | stream-out VS |
| 0x238, 0x23C | 0 | |
| 0x240–0x24C | (`PtclMgr+0x1fefc`, —, `+0x1ff04`) clamped to length 0.1851, 0 | stream-out VS |
| 0x250–0x26C | E[16..23] = e28, e29 | fog B in cloud programs (`B = e30.w·(1 − (1 − tB)^e29.x)·(1 − e29.w·sat(V·up)^e29.z)`) |

So the weather programs need only fog/light values our renderer already
has (`fog.rs`, `look.rs`, `daynight.rs`): e26/e27 (scatter fog A), e28–e30
(ad hoc fog B), `fog_scatter` colour/Start/End/moisture (height fog H, with
the fixed 250 m cap and **no** cloud-noise term in particles), `dir_main`,
plus E[50] and the textures of §6.

### 4.4 Wind values (F)

`0x0378655c` (per frame, BotW particle manager): only when the world mode
`*(0x1046f428)+0xc1a` ∈ {0, 1, 3}, else all zero. Wind object
`PtclMgr+0x6f0c` (`0x03671acc` update, `0x03671b74` returns `+0x30`: speed,
direction xyz):

```
s   (+0x6f7c) += (W.speed − s)·0.99            (0x1047c208)
dir (+0x6f80) = W.dir
s2  (+0x6f78) += (s·c − s2)·0.07               c = 1, or 0.1 / 0.3 when 0x03786370 reports shelter
d   = dot(normalize(cameraRight.xz), dir.xz)   (camera matrix via 0x03c6ff74/0x03c6fdb4)
+0x6f8c = d·s;   +0x6f90 = −(1 − |d|)·s
```

`T(x)` (`0x03786f34`) is a piecewise-linear table at `PtclMgr+0x1febc`
(16 entries for x = 0…15, set in `0x03782ce4`): 0, 1.15, 1.5, 1.85, 2.05,
2.25, 2.4, 2.55, 2.7, 2.85, 2.96, 3.075, 3.19, 3.3, 3.4, 3.5 (x > 14 →
3.5, x < 0 → 0).

## 5. `cus2` (0x60 bytes, F)

Filled per emitter draw by the custom 3/4 render-state callback
`0x038714c4` (custom 4 additionally sets render-target state through
`0x03a00074`/`0x03787944`); flags `F0 = res+0x930`, `F1 = res+0x934`.
Per-emitter state `U = em+0x3ac` (16 bytes, allocated by init
`0x03870c70`): U[0], U[1] floats, U[2] pointer to a 0x24-byte point
record (only with F1 & 0x40000000), U[3] byte.

| Off | Value | Condition |
|---|---|---|
| 0x000 | 1.0 if draw path `res+0x764` is 17 or 18, else 0 | always |
| 0x008, 0x00C | U[0], U[1] (wind UV scroll) | F1 & 0x40 |
| 0x010, 0x014, 0x018 | wind direction x, 0, z: `set+0x140` object `+0x40…+0x48` normalised, or the global `PtclMgr+0x6f80` and `+0x6f88` | F1 & 0x40 |
| 0x01C | min(speed, 10)·0.2; speed = length of the set vector, or `PtclMgr+0x6f7c` | F1 & 0x40 |
| 0x020–0x028 | point record `+0xc…+0x14` (previous point), 0x02C = 0 | F1 & 0x40000000 |
| 0x02C | `PtclMgr+0x13a5c` | F1 & 0x2000 (overrides) |
| 0x030–0x038 | point record [0..2] − [3..5] (motion of the tracked point), 0x03C = 0 | F1 & 0x40000000 |
| 0x040–0x05C | E[71..78] (e37.x, 0, 0, 0, 1.4, 2.0, 0.35, 0.1), with 0x044 = U[3] (1 when the ground under the emitter is material class 7, `0x0369e974`) | F0 & 0x800000 |

U update (`0x03870ec8`, every frame): `U[0] += PtclMgr+0x6f8c · 0.05 · step`, `U[1] += PtclMgr+0x6f90 · 0.05 · step` (0.05 = `0x1047c8a0`, step
= the core's frame step). So the cloud UV scroll
`reserved[0x02C]·cus2[0x008/0x00C]` moves sideways with the wind
component across the view and always "away" with the facing component.

## 6. Textures (F)

Global per pass (`0x0387c5c4`, `0x039fe174` binds to fixed units for VS
and PS): unit 6 (`TextureSampler0`) = `drawCtx+0xf24` object `+0x58`,
unit 7 (`TextureSampler1`) = `+0x28`, unit 12 (`ShadowArraySampler0`) =
`+0x14` (only with a shadow object), unit 11 = `drawCtx+0x38`, unit 9
(`TextureSampler3`) = E[45] when set.

Per emitter (`0x038714c4`, `0x039fb9fc` = bind to the shader's custom
slot; slot order `shader+0x534 + 8·slot`: 0–3 Texture, 4–7 TextureArray,
8–11 Texture3D, 12–15 Cube, 16–19 CubeArray, 20–23 Shadow, 24–27
ShadowArray, from `0x03b63ec0…0x03b640e4`):

| Sampler (unit) | Texture | Flag (F1) |
|---|---|---|
| `CubeSampler0` (10) | `drawCtx+0x18 + 4·min(int(CSDP[0x28]), 7)` | 0x1 |
| `ShadowArraySampler1` (13) | E[48] = **LightAnalyzer** 12×1 (`gsys_user4`) | 0x200 (needs E) |
| `TextureSampler2` (8) | E[44] = **sky LUT** (`gsys_user0`) | 0x100 |
| | `drawCtx+0xf24` object `+0x30` | 0x4000000 |
| `TextureSampler3` (9) | E[45] = sky-occlusion top-down depth | 0x10000c00 |
| | E[47] (volume-mask texture, **H**) | 0x21000 |
| | `drawCtx+0xf24` object `+0x34` | 0x8000000 |

Weather emitters (data): `FieldRain01/02a` rain_near/far F1 = 0x700 (LUT,
LightAnalyzer, top-down depth; no cube, no `cus2` wind), `FieldSnow`
0x10000601 (+ cube index 1), `FieldRain02b` 0x10000740 (+ `cus2` wind),
`FieldRainDepth01` 0x10002a00, `MountainCloud`/`FarWeather_Cloud`
0x10000341/0x10000351 (cube index 4, wind), `FarWeather_Rain`
0x10000301, `VolumeMask*` 0x1000, `FieldHaze_Lv01` 0x10058345,
`EnvSandStorm` 0x10000301; F0 = 0 except clouds 0x480000 and
`FarWeather_Rain` 0x80000.

## 7. For the port

- Plugin block: reproduce §2.3/§2.4 exactly, including the repeated draws
  (trunc(EP[0x0C]) + 1 copies, offset k·EP[0x00]) — without them rain and
  snow are 3–10× too sparse. The bake must keep the `EP04` payload (0x50
  bytes) per emitter.
- Reserved block: the `CSDP` payload as is (pad to 0x80 with zeros; the
  game's trailing bytes are never read).
- `cus1`: fill 0x160–0x26C from the fog/light state we already compute
  (§4.3), 0x010 = min(fade + flash, 1) (0 in ordinary play), 0x01C = 1;
  the shadow and stream-out parts only for the programs that read them.
- `cus2`: wind from our WindMgr port (§4.4, §5); U accumulators per
  emitter.
- Textures: sky LUT, LightAnalyzer table, sky-occlusion top-down depth
  (and its transform 0x200–0x20C) — all three exist as gsys resources in
  the original renderer's notes; the cube maps of `drawCtx+0x18[]` are open.

## 8. Open questions

1. `drawCtx+0x18[0..7]` cube maps (index 1 for rain/snow, 4 for clouds):
   no writer found in the gsys particle code or the KSys writer; search
   the gsys/KSys cube-map code for stores into the `0x039fb0a8` context.
2. Default texture `*(0x1047ebc8)+0x38` (which agl primitive texture) and
   the volume-mask texture E[47] (`gsys+0x2b0c` object).
3. Meaning of F1 bits not tied to a bind here (0x4 → `em+0x45c = 2`,
   0x10, 0x40000, 0x8000, 0x100000/0x20000 in the CPU particle calc,
   0x2000000 → `0x03787a0c`), and F0 0x80000/0x400000.
4. `SkyMgr+0x2114` (the "fade" in `KSys+0xb58`), `PtclMgr+0x13a5c`,
   `PtclMgr+0x1fefc/+0x1ff04`, `set+0x140` (per-set wind object) — writers
   not followed.
5. The point record of `cus2[0x020..0x03C]` (`0x03870ec8`) only roughly
   read (paired-single code); not used by the weather emitters.
6. EP01–EP03 stripe blocks (§2.5).

## 9. LightAnalyzer texels 8 and 9 ("effect ambient", F)

The rain/snow VS reads `gsys_user4` at u = ((1 − sat(p·reserved[0x30]·0.25
+ 0.5)) + 8.5)/12, i.e. between the centres of texels 8 (sat(…) = 1) and
9 (sat(…) = 0). Both are written by LightAnalyzer step 4
(`uking_pass_shader` binary 487, Cemu `728b9bc3556c3de1` PS); read from
the Cemu GLSL with the `cContext` map of `cemu_uniform_map.py`
(uf_remappedPS 0…7 → cContext vec4 1, 7, 5, 3, 4, 0, 2, 6), names by the
`light_analyzer` manifest. Each output texel picks its branch by
`gl_FragCoord.x`; the 12×1 target is drawn once.

### 9.1 Formula (F: read from the GLSL, no ALU executed)

Common terms (as in the original renderer's character/field notes):

```text
x    = cExposure(0.5,0.5).r · cMainLightColor.w      (sunlit share)
e    = x³
avg  = dot(cCubeMap2D(0,0).rgb, (0.2989, 0.5866, 0.1144))
cube(d) = textureLod(cCubeMap, d, 3).rgb
```

Texels 8 (k = 0) and 9 (k = 1):

```text
g    = k·cCharaAmbientGraY + cCharaAmbientGraYOffset
d    = normalize(cLook.x, 1 − 2g, −cLook.z)      (same direction as T[4,5])
c    = cube(d)
M    = max(c.r, c.g, c.b);  s = 1 − min(c)/M
f    = (s + 0.0001)^(cEffectAmbientSat − 1)      (no "sat min" factor:
                                                  the lerp is folded to 1)
c'   = M + (c − M)·f
T[8|9].rgb = c' · cAmbientMasterIntensity · cAmbientOffsetFieldScale
             / (avg + lerp(cAmbientOffsetFieldMin, cAmbientOffsetFieldMax, e))
T[*].a     = cExposure.r (every texel's alpha is the raw exposure texel)
```

So T[8,9] = (the T[4,5] colour with the effect saturation curve instead
of the character one) × T[1].x (the field ambient scale); no
`cAmbientScaleChara`, no character offsets. With the defaults (g = 0 → up,
g = 1 → down; `effect_ambient_sat` = 2 → f = s + 0.0001) the curve
**desaturates**: c' = M − (M − c)·s, a nearly grey sky stays white-ish
at M, a saturated one keeps part of its tint. (The character curve, power
−0.5, raises saturation instead.)

In `deferred_light.wgsl` terms (Master = 1 as the renderer already
assumes; `ahead` = the camera forward's horizontal x/z, as in
`character_material.wgsl`):

```wgsl
fn effect_ambient(dir: vec3<f32>, mean: f32, e: f32) -> vec3<f32> {
    let c = cube_at(normalize(dir), 3.0);
    let m = max(c.r, max(c.g, c.b));
    let s = 1.0 - min(c.r, min(c.g, c.b)) / m;
    let f = pow(s + 1e-4, EFFECT_SAT - 1.0);          // EFFECT_SAT = 2.0
    return (m + (c - m) * f) * FIELD_SCALE
        / (mean + mix(FIELD_OFFSET_MIN, FIELD_OFFSET_MAX, e));
}
// T[8] = effect_ambient(ahead + vec3(0, 1, 0), mean, e)   (g = 0)
// T[9] = effect_ambient(ahead - vec3(0, 1, 0), mean, e)   (g = 1)
// rain: mix(T[8], T[9], 1 − sat(p·reserved[0x30]·0.25 + 0.5))
```

### 9.2 Parameters (F: RPX v208)

`ksysla` constructor `0x038b37c0` (names by string pointers, defaults by
the stores after each registration) and the uniform upload at
`0x038b4990…` (`lfs fN, off(r28)` then `li r5, index`, `bl 0x03a83f6c`;
the uniform declaration in `0x038b48a4` is 2×vec4, 16 floats, vec4, 5
floats, which is exactly the manifest's offset order):

| uniform (index) | `ksysla` param (+offset) | default |
|---|---|---:|
| `cCharaAmbientGraY` (3) | `chara_ambient_gra_y` (+0x3e0) | 1.0 |
| `cCharaAmbientGraYOffset` (4) | `chara_ambient_gra_y_offset` (+0x3f0) | 0.0 |
| `cEffectAmbientSat` (9) | `effect_ambient_sat` (+0x440) | 2.0 |
| `cAmbientOffsetFieldMin` (15) | `ambient_offset_field_min` (+0x480) | 0.1 |
| `cAmbientOffsetFieldMax` (16) | `ambient_offset_field_max` (+0x490) | 0.5 |
| `cAmbientOffsetFieldScale` (17) | `ambient_offset_field_scale` (+0x4a0) | 1.0 |
| `cAmbientOffsetFar` (11) | `ambient_offset_far` (+0x3d0) | 1.0 |
| `cAmbientMasterIntensity` | object +0x6e8 | 1.0 at creation |

There is no `effect_ambient_sat_min` parameter (the strings around
`0x10331754` list none, and the shader has the constant 1). Uniform 9 is
loaded from +0x440, the slot whose name pointer is `"effect_ambient_sat"`
(`0x10331754`) and whose default store is 0x40000000.

### 9.3 Texels 2/3 confirmed (F)

`d = (0, +1, 0)` for texel 2 and `(0, −1, 0)` for texel 3 (the
`gl_FragCoord.x > 3` select), LOD 3, no saturation curve:
`T[2,3] = cube((0, ±1, 0))·Master/(avg + cAmbientOffsetFar)` — as
`field_ambient` has it (with Master = 1).

Also read in passing: T[10] = max(cCubeMap2D.rgb/(avg +
cAmbientOffsetCubeMap), 0.1) (no Master); T[11] = (lerp(u28, u30,
sat(x·luma(cMainLightColor.rgb) − u29)), 1, 1) with u28/u29/u30 =
`cLightPrePassThreshold`/`…MinAmbientIntensity`/`…AmbientIntensityOffset`;
by the upload order these receive `light_prepass` (0.1),
`light_prepass_threshold` (3) and (H, index 21 not checked)
`light_prepass_offset` (3) — the names do not line up, so treat the
T[11] parameter mapping as unverified.

### 9.4 Hypotheses / open

- H: the rain VS sampler filters linearly, so the u between 8.5/12 and
  9.5/12 is a real blend of T[8] and T[9]; the sampler state was not
  checked.
- H: `cLook.x/.z` are the camera forward's horizontal components (from
  the original renderer's character note; the matrix offsets there look garbled).
  The y is replaced by 1 − 2g, so the forward's length (cos pitch)
  matters before `normalize`.
- The table's storage format (agl 0x1a) and so any clamping of values
  above 1 was not checked; the T[11] values (≈3) suggest a float format.
- `cExposure` (and so x, e) still comes from SI-LGT-07 in the renderer.

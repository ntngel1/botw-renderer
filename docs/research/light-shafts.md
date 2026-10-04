# Light shafts: agl VolumeMask → merge_mask → scene fog

Research of 2026-10-03, BotW Wii U EU v208 (`U-King.rpx`, the matching
binary of `docs/research/wiiu-render-cpu.md`). Sources:
shader archives `agl_technique` and `uking_pass_shader` (manifests under
`game-data/reference/visual-formulas/shader-archives/`), the Cemu snapshot
`20260928T012415Z-cache-replay` (exact-byte `matches.json`), Ghidra
(read only; no names were added). Formulas below are written in our own
words from the Cemu GLSL of exactly matched programs and from the PPC/CPU
code; no game shader text is copied. Bulk outputs (scratch tools, census)
live in `game-data/reference/visual-formulas/model-water-glass/` and
`.../cpu/`.

## Summary

BotW has **no screen-space god-ray pass**. "Light shafts" are a *volumetric shadow mask*: agl's `VolumeMask` (`aglvolm`) slices the view frustum into many camera-facing layers, looks up the sun's cascade shadow map at each layer, accumulates lit/unlit along the view ray into a small float texture, blurs it, and the result reaches the screen only through the scene fog: the merged mask is bound as `gsys_user2`, whose `.x` scales the sun in-scatter of the fog (`m` in the fog formula of `docs/research/wiiu-deferred-shading.md#fog-pre-shading-ps-140-read-into-cemu-glsl`). Where the air between the camera and a surface is in shadow, the in-scattered fog near the camera is darker; lit air keeps the full in-scatter. That is what reads as shafts.

`VolumeMaskColor` / `VolumeMaskIntencity` (with the climate and weather
`CalcVolumeMaskIntencity`) are written by the weather update into the
VolumeMask units' `render_mask_color`; **no draw of the mask reads that
colour** (see "The palette colour" — open).

The `radial_blur` / `radial_blur_compose` programs exist, are used by the
game (in the Cemu cache), and are recorded below for completeness, but no
link between them and the VolumeMask chain was found.

## Facts

### Objects and setup (CPU)

- `FUN_039cfeac` (gsys system setup, matching v208): when flag `+0x880`
  of its config is set, it builds the VolumeMask object (`0x03a5f984`,
  0x428 bytes, name `aglvolm`) at `+0x234` and initialises it with
  `{view count, 2}` (`0x03a60200`): **two units per view**. RadialBlur is
  a separate object at `+0x214` (flag `+0x8f0`, ctor `0x03b3a604`, init
  `0x03b3b4a0`).
- Unit parameters (agl param list `unit_%d`, defaults in `0x03a60200`),
  copied to a 0x2c-byte runtime record by `0x03a60124` and back by
  `0x03a63f54`:

  | record | parameter | agl default |
  |---|---|---|
  | +0x0 u8 | `enable` | 1 |
  | +0x1 u8 | `autoAdjustNear` | 0 |
  | +0x2 u16 | `layer_number` (0–256) | 256 |
  | +0x4 u8 | `layer_reduce_level` (0–8, clamped to 5) | 3 |
  | +0x5 u8 | `shadowmap_pcf_type` | 0 |
  | +0x6 u8 | `gaussian_kernel` | 3 |
  | +0x8 f32 | `layer_dist_nonlinear` (0–4; within 0.01 of 1 → 1.0) | 1.0 |
  | +0xc f32 | `range_near` | 1.0 |
  | +0x10 f32 | `range_far` | 25.0 |
  | +0x14 f32 | `shadowmap_amp_low` | −1.0 |
  | +0x18 f32 | `shadowmap_amp_high` | 1.0 |
  | +0x1c..+0x28 | `render_mask_color` RGBA | (1, 0.788, 0.46, 1) |

  Common: `shadowmap_reduce_level` (`+0x1b4`, 2), `depth_convolve_max`
  (`+0x1b8`, 0), `shadow_convolve_max` (`+0x1b9`, 1), enable (`+0x3ec`).
- **KSys overrides both units** in `0x03405f48` (PPC
  `0x0340671c..0x03406870`; f28..f31 = 0.0, 1.0, 2.0, 0.1 from
  `0x102bfd24`, `0x102bfd3c`, `0x102c0388`, `0x102bfd30`):

  | | unit 0 (mask A) | unit 1 (mask B) |
  |---|---|---|
  | enable / autoAdjustNear | 1 / 1 | 1 / 1 |
  | layer_number | 96 | 128 |
  | layer_reduce_level | 3 (1/8 of the view) | 4 (1/16) |
  | pcf / gaussian_kernel | 0 / 0 | 0 / 0 |
  | nonlinear | 1.0 | 1.0 |
  | range_near / range_far | 0.1 / 20.0 (`0x102c0398`) | 0.1 / 400.0 (`0x102c0530`) |
  | amp_low / amp_high | 0.0 / 2.0 | −1.0 (`0x102c0534`) / 1.0 |

  and the common `shadowmap_reduce_level` = 0 (`+0x1b4`; the two
  convolve flags are kept). The shadow source is set per view by
  `0x03a61024` (cascade count `+0x1a8`, shadow texture `+0x94`, the
  cascade matrices and split lengths); the matched programs are the
  3-cascade variants (`CASCADE_STEP=3`).

### Draw order (`0x03a63d9c`, per view, when common enable and the view's flag)

1. `0x03a61370` — targets per unit: `volume mask - layer`, agl format 9
   (`R16_float` by `lib/agl/include/common/aglTextureEnum.h`), size =
   view size >> `layer_reduce_level`; `volume mask - depth` (format 0x3c)
   of the same size.
2. `0x03a61b18` — reduced depth: a chain of half-size passes with
   `volume_mask_reducedepth` (agl_technique program 15). Matched in the
   cache: PS 463 (`COVOLVE_MAX=0, USE_ARRAY_TEX=0`): it writes the depth
   texel at the pass uv to the output depth (plain copy, no min/max).
3. `0x03a621bc` — shadow-map reduce chain; skipped, KSys sets its level 0.
4. For each enabled unit: `0x03a63108` (layers; used because the
   raymarch flag `+0x1d4` is 0 — `volume_mask_raymarch` 542–613 is absent
   from the cache), then `0x03a639a0` (blur).
5. Later, KSys `0x034030c4` (PPC `0x03403ad8..0x03403c80`) merges the two
   unit masks with `merge_mask` and the result becomes `gsys_user2`.

### Layer pass (`volume_mask_layer`, VS 494 / PS 495; CPU `0x03a63108`)

CPU, per unit (record `u`, camera near `n_c` = view `+0x8c`, far `f_c` =
`+0x90`, `tx, ty` = view `+0x1ac/+0x1b0`, the tangents of the half
angles):

```text
N      = u.layer_number
near   = u.range_near ; span = u.range_far − u.range_near
if u.autoAdjustNear and n_c > 0:
    near = n_c ; span = max(0.5·(range_far − range_near), range_far − n_c)
for i = N−1 down to 0:                          (back to front)
    t = i / (N−1)
    d = near + span · (u.nonlinear == 1 ? t : pow(t, u.nonlinear))
    if d <= n_c: stop
    z = d + 7.6294e-6                           (0x37000000, 0x10360d98)
    cLayerTrans = (tx·z, ty·z, z, i) ; draw one quad
cLayerRenderInfo      = (1/(N−1), n_c, f_c, 1 − n_c/f_c)
cLayerNum             = N
cDepthShadowAmplifier = (amp_high − amp_low, amp_low)
cSizeRCP              = 1 / shadow texture size
cViewProjMtx          = the view's projection (16 floats, view +0x1c)
cViewInvMtx           = view → world (3 rows)
cDepthShadowProj[4c..4c+3], cDepthShadowLength[c] — per cascade c
```

Blend (agl render-state words decoded as R6xx registers, as in `wiiu-deferred-shading.md` "Mixing"): the layer pass ORs `CB_BLEND_CONTROL` to `0x01040104` → color and alpha `src·SRC_ALPHA + dst·ONE` (additive, weighted by the output alpha).

VS (register uniforms; regs by the manifest offsets: 0–2 split lengths,
3–14 cascade matrices, 15 `cLayerTrans`, 16–18 `cViewInvMtx`, 19–22
projection):

```text
pv = (2·aPos.x·L.x, 2·aPos.y·L.y, −L.z, 1)       L = cLayerTrans; quad ±0.5
pw = cViewInvMtx · pv                             (world point on the layer)
c  = 2 if L.z > len[2] else 1 if L.z > len[1] else 0
shadowCoord = cDepthShadowProj[c] · (pw, 1)       (4 rows)
gl_Position = projection · pv ; pass c as the array layer
```

PS (uniform pairing verified against the native ALU constant reads with a
scratch variant of `cemu_uniform_map.py` for constant-file selectors):

```text
s   = shadowCompare(cascade array, layer c, sc.xy/sc.w, saturate(sc.z/sc.w))   (PCF 1)
out = (s·amp.x + amp.y, 0, 0, cLayerRenderInfo.x)
```

So each layer adds `(lo + (hi − lo)·s) / (N − 1)` to the R16f target:
unit 0 (0.1–20 m, 96 layers) adds `2s/95` (0 shadowed, 2/95 lit);
unit 1 (0.1–400 m, 128 layers) adds `(2s − 1)/127` (−1/127 shadowed,
+1/127 lit). A layer's quad is rasterised over the whole view at its
depth; whether it is clipped by the scene depth is **not established**
(see open questions).

### Blur (`0x03a639a0`)

Two passes of agl `image_filter_gaussian` (agl_common), horizontal then
vertical (`FUN_03ac6afc(…, kernel = u.gaussian_kernel, dirX, …)`), from
the layer texture through a temp (`volume mask - temp depth`, same format)
into the unit's `volume mask - mask` (`0x03a61144`, same size and format).
KSys sets kernel index 0 = `GAUSSIAN_KERNEL=3`; programs 16–23 are in the
cache. Read from the GLSL (VS `81eb264a750163d9`, PS `cc1b168d8d87968b`):
two bilinear taps at ±0.5 texel along the axis, weights 0.5 each, i.e. a
1-2-1 kernel per axis (`cTexSize` → texel size not checked on the CPU).

### Merge (`merge_mask`, uking_pass_shader PS 553 = Cemu `273bcd33c9f8ce3c`)

```text
out.r = maskA.r + (maskB.r − maskA.r) · cVolumeMask.x
out.g = indoorMask.r
out.b = out.a = maskB.r
```

CPU (`0x034030c4`): target `0x250` of the KSys per-view record
(KSys `+0x8d8`, stride 0xf7c) is created with agl format 10
(`R8_G8_uNorm` → the mask is clamped to 0..1 here), size = the larger of
the two mask sizes. A = unit 0 mask, B = unit 1 mask, indoor = the record's
sampler `+0x254`, which after the draw is re-pointed at the merged target
(`0x03403c78..0x03403c80`). `cVolumeMask = (k, 0, 0, 0)`:
`k` = KSys `+0x868` when unit 1 is enabled; 0.0 when unit 1 is disabled;
`f30` (not read) when unit 0 is disabled. KSys `+0x868` is set to **0.5**
in `0x034055e0` (`0x102bfd10`); no other writer was found by an
immediate-offset scan of stores. With both units on: **mask = ½A + ½B**.
`cIndoorMask` gets the bss vector `0x10549e0c` (not read).

### Binding and the consumer

`0x033ff8cc` (environment/user-texture writer) puts the record's `+0x254`
into user-texture slot 2 = **`gsys_user2`** when the env object's
`+0x5e0` flag, the VolumeMask common enable (`+0x3ec`) and the view's
VolumeMask flag are all set; otherwise the slot is null. The scene fog
(pre-shading PS 140, and PS 108 / 104 with the same constants) reads
`u2 = gsys_user2(screen uv)`:

```text
m = lerp(0.1k + (1 − 0.1k)·saturate(0.005z − 1.8), 1, saturate(u2.x))   (sun in-scatter LUT scale)
w = 1 − 0.85·u2.y                                                         (indoor mask)
```

(formula from `docs/research/wiiu-deferred-shading.md`).
The VolumeMask therefore never adds light; it only lets lit air keep the
full in-scatter near the camera (z ≲ 560 m) while shadowed air drops to the
distance-faded base.

### The palette colour (`VolumeMaskColor`, `VolumeMaskIntencity`)

`ENV_UpdateWeatherPalettes` (`0x036425b8`), PPC `0x0364b14c..0x0364b538`:
the palette's `VolumeMaskColor.rgb` (+0x3dc) and `VolumeMaskIntencity`
(+0x3f8, as the 4th component) are blended between palette sets like the
other colours; on palette row 0 the intensity is multiplied by the
climate and weather `CalcVolumeMaskIntencity` (`f26`, `f31`) and the
colour by `F` (gate `0x0364b260`). Then
`a ← a·(1 + (k1 − 1)·k2)` (`k1`, `k2` from `r14+0x3bb0`, `+0x3cf0`),
`a ← a·(1 − X)` with `X` = manager `+0x2114`; when `f21 ≠ f29` the RGBA
is scaled by `1 − f22`; for `WorldMgr+0x530 == 2` `a` is replaced by a
table value (`r14+0x3d5c`). If the result differs from the previous one
(`r14+0x3c70`), it is written into **every unit's `render_mask_color`**
(`+0x1c..+0x28`) via `0x03a63f54`. The environment writer `0x033ff8cc`
then copies unit 0's colour into the KSys per-view record words
`0x33..0x36` (alpha forced to 0 when `FUN_039cf9d8(env+0x28d8)` is 0).
None of the VolumeMask draw functions (`0x03a61370…0x03a639a0`), the merge
or the fog GLSL reads that colour. Its reader is not found.

### Radial blur (separate, use unknown)

agl_technique `radial_blur` (`NUM_SAMPLES` 2…20; matched: 8 samples,
VS 240 `32fb7ad18a99d76a`, PS 241 `554b1ca21226b046`) and
`radial_blur_compose` (VS 252 `5c7f419d800ee0a6`, PS 253 `b9b506ffd249b639`).
Uniforms `cBlurPosition` (reg 0), `cRadius` (reg 1), `cVtxColor[4]` (regs
2–5):

```text
blur VS:  a = aPos.xy·cRadius.xy ; position = (2a + C.xy, C.z, 1)    C = cBlurPosition
          c = C.xy·0.5 + 0.5 ; k = C.w/7
          uv_j = a·(1 − j·k) + c (v flipped), j = 0..4
blur PS:  rgb = (T(uv0) + 2T(uv1) + 2T(uv2) + 2T(uv3) + T(uv4)) / 8 ; a = 1
compose:  a fan (vertex 0 = centre, rings of 32 vertices) around C with
          colour cVtxColor[0] at the centre, cVtxColor[1 + (v−1)/32] on ring v;
          out = vtxColor.rgb · T(uv).rgb, alpha = vtxColor.a
```

## Hypotheses (not proven)

- The layer quads are depth-tested against the reduced depth (the reduced
  depth is bound as the layer pass's depth target; otherwise the
  accumulation would not depend on the scene). The depth word of the
  agl render state is only ANDed with `~3` in both the layer and the
  reduce passes, so the bit meaning there is not the R6xx `DB_DEPTH_CONTROL`
  layout and was not decoded.
- The layer target is cleared to zero before the layers
  (`FUN_03a76b2c(1.0, …, &0x1054a4fc, …)` looks like a clear with a
  colour at bss `0x1054a4fc`, value not read).
- Two units (near 20 m fine, far 400 m coarse) are combined 50/50 so that
  near shafts get detail and far ones exist at all.

## For our renderer

- `crates/render/src/look.wgsl` (`apply_haze`) uses only the base part of
  `m` (`SI-LGT-12` mark), i.e. it behaves as if `gsys_user2.x = 0`
  everywhere: lit air near the camera is too dark compared with the game.
  Porting the mask means: cascade shadow lookups on 96 + 128 slices
  (or an equivalent per-pixel integration with the same slice positions
  and weights), R16f accumulation at 1/8 and 1/16 resolution, 1-2-1 blur
  per axis, ½A + ½B clamped to 0..1, then the `m` lerp. Until then it is a
  stand-in and needs an SI-ID.

## Open questions

1. Who reads `render_mask_color` / KSys record words 0x33–0x36 (the
   palette's `VolumeMaskColor`/`VolumeMaskIntencity`)? Candidates: a uking
   pass via the `gsys_environment`/custom payload, or `volume_mask_drawtex`
   (`cColor`, PS 615/617 — not in the Cemu cache).
2. Writers of KSys `+0x868` other than the 0.5 initialiser; value of `f30`
   in the merge.
3. Depth test of the layer quads and the clear value (above).
4. When is the env object's `+0x5e0` flag set (field vs. interiors)?
5. What uses the RadialBlur object (`+0x214`)?
</content>
</invoke>

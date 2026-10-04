# Model water (behave 103) and glass / energy (behave 102)

Research of 2026-10-03, BotW Wii U EU v208. Closes the "which programs"
part of `SI-MAT-02` (`archived STAND-INS.md notes`). Sources: all
`Model/*.sbfres` of update/base/DLC, the `uking_mat` manifest and native
code (`shader-archives/uking_mat`), the Cemu snapshot
`20260928T012415Z-cache-replay` (exact-byte `matches.json`), the existing
deferred/water research in `docs/research/`
(`wiiu-deferred-shading.md`, `wiiu-water-variants.md`,
`wiiu-material-variants.md`). Formulas are our own paraphrase of the
matched Cemu GLSL (uniform names by `tools/research/cemu_glsl_simplify.py`,
native-paired); no game shader text is copied. Local outputs (not in Git):
`game-data/reference/visual-formulas/model-water-glass/`
(`behave-102-103-materials.txt` — census of every such material,
`model-*.txt`, `variants-*.json`, `program-families.txt`,
`ps*-*.txt` / `vs7965-gbuffer.txt` — simplified GLSL).

## Summary

- 204 materials: 178 behave 103, 26 behave 102, all `uking_mat`.
  **Every one** has `gsys_deferred_shading_material=1`,
  `gsys_gbuffer_xlu=1`, `gsys_gbuffer_xlu_opa=0`, RenderState mode 0
  (custom; 7 glass materials mode 3), `CB_BLEND_CONTROL` 0x20010504 and
  colour control 0x00cc0000 or 0x00cc0100 (blend only on target 0, the
  material id, whose output alpha is 1 → no effect).
- By the pass mask of `wiiu-water-variants.md#mask-gbuffer-xlu-and-general-list-water-water-s-event-camera`
  (flags 17 and 19 → mask 0x20) they are drawn in
  **`Model(GBuffer/Xlu)`** (`0x039b1ce8`), the same route as TeraWater —
  not in a forward translucent pass. Their programs are the
  `gsys_assign_gbuffer` variants of `uking_mat`.
- They write the G-buffer with **material id 22** (= Xlu 16 | class 6,
  the water class), so the deferred lighting treats both water and glass as water: `preshading_field_water` (uking_sys 140–143) and `field_water` (24–27), whose formulas are already in `wiiu-deferred-shading.md` (`final = fog.rgb + s·C + s²·L·(1−a)`, cube-map reflection with Schlick-like `F = f + 0.02(1−f)`).
- Outputs (`wiiu-deferred-shading.md`, G-buffer slots): **0** material
  id, **1** albedo, **3** normal (+ a packed scalar in `.w`), **5** the
  frame colour buffer ("Emission" surface): the refracted/absorbed scene
  behind (`L·(1−a)` for water, `L·T + Fog` for glass). The scene behind
  is `gsys_light_prepass` layer 1 (pre-shading of the opaque scene),
  sampled at a refracted screen uv.
- Scrolling: texture coordinates are `tex_srtN · uv` in the VS
  (`uking_texcoordN_srt`); the FMAT values are static, and the model
  files carry a **texture-SRT animation** (FRES group 5; e.g.
  `DgnObj_DungeonWater50x50_A.sbfres` has one) — that animation scrolls
  the normals. Waterfalls also displace vertices
  (`uking_vtx_position_modify_calc_type=9`).

## Facts

### Programs (`uking_mat`, model 0)

`material_variants.py` with the native overrides ignored (programs carry
`gsys_renderstate=3`, `gsys_alpha_test_func=6` — the same 0→3 / 0→6
substitution as TeraWater). Each family is 12 programs:
`gsys_assign_type` visualize / material / zonly / gbuffer × variation
0–2 (the ElectricBallGenerator material matches 36: three 12-program
blocks, two with `gsys_weight=0`, one with 1; the authored options do not
pick the block; all three gb programs share the same PS).
"gb" = the `gsys_assign_gbuffer` variation 0 program; PS hashes are Cemu
files (exact bytes):

| Family | Materials (examples) | gb program → Cemu PS | material-0 PS in cache |
|---|---|---|---|
| 7920, 7932 | `Mt_DungeonWater_A` (far models) | 7929 / 7941 — PS not in cache | — |
| 7944 | `Mt_DungeonWater_Flow`, `Mt_TerraWater02/05`, `Mt_WaterSurface` | 7953 → `5108f68f7424be0f` | — |
| 7956 | `Mt_DungeonWater_A` (`DgnObj_DungeonWater50x50_A`) | 7965 → `5108f68f7424be0f` | — |
| 7968, 7980 | `Mt_TerraWater03/04`, `Mt_WaterSeal`, `Mt_Water` | 7977 / 7989 → `3a7e6b48dae31305` | `b05324255e41d159` |
| 11256, 11268 | `Mt_WaterFall_M_B(1)` | 11265 / 11277 → `451d2e2784b26342` | `cd251bb188c3c83e` |
| 11280, 11292 | `Mt_RemainsWaterWave`, `Mt_DgnObj_IbutsuWaterFall_A_02` | 11289 / 11301 → `4c0bd596e3aef4a6` | `cd251bb188c3c83e` |
| 11376, 11388 | `Mt_Water_DesertIceRoom_A1`, `Mt_WaterFall_Small_A` | 11397 → `bd17596561c1db0c` | `e89623af6c98ee45` |
| 11400 | `Mt_WaterFall_M_A` (FldObj_WaterFallBottom) | 11409 → `2ad57015b4440bdf` | `434b1caf71f1a747` |
| 11412 | `Mt_WaterFallCliffWhite_A` | 11421 → `5fd52425328105c6` | `5a66a594b76ae9b7` |
| 11424 | `Mt_WaterFall_M_A` (Akkare), `Mt_DungeonWater_Flow` (B), `Mt_Basin`, `Mt_Waterfall`, `Mt_DgnObj_IbutsuWaterFall_A_01` | 11433 → `2dbcd44cbf44d418` | `5a66a594b76ae9b7` |
| 11436 | `Mt_Fountain` | 11445 → `25eaf63e9b1c7eb6` | — |
| 11616 | `Mt_FirstShrineBedWater_A` | 11625 → `64796ee3269f4f2c` | — |
| 7824, 7836 | `Mt_CmnTex_Glass_HatenoHouseWindow_A` (+ far) | 7833 → `fe54e700de02ff78`, 7845 → `eb890f96215a0225` | `30febe2aabeb1389` |
| 7860 | `Mt_Glass_HatenoRef_A` | 7869 → `38a0a4a7d28bd34e` | — |
| 7872 | `Mt_DgnObj_ElectricBallGenerator_A_03`, `…ElectricGlass_A_01`, castle royal window | 7881 → `7d14d4c7e060a1e2` | — |
| 7908 | `Mt_DgnObj_IbtWind_Glass*` (Vah Medoh) | 7917 — not in cache | — |

The census (`behave-102-103-materials.txt`) groups the 204 materials into
23 option signatures; the families above cover all groups whose sample
models were matched. The material-type PS (`gsys_assign_material`) of
several families is also in the cache; when the game uses it (cube-map
capture? a forward Xlu path?) is not established.

### Options that select the shader structure

Read from the per-family option table (all differences confirmed in the
GLSL of the matched PS):

| Option | Effect in the G-buffer PS |
|---|---|
| `uking_enable_scene_color0_depthdiff1_effect=1` (7944, 7956, 7968, 7980) | depth-difference absorption with `const_color3/5` (below) |
| `uking_enable_scene_color0_fog=1` (glass, 11388, 11436) | background = `L·T + Fog` from the pre-shading Shadow/Fog targets |
| `uking_enable_gbuffer_xlu_blend=1` (7968, 7980, 11xxx) | reads `gsys_gbuffer_albedo/normal/emission` (+ id) and lerps every output from the existing G-buffer value by the vertex alpha `Sem5.w` |
| `uking_vtx_position_modify_calc_type=9` (waterfalls) | VS displacement |
| `uking_enable_backface_modify=1` | normal reflected on back faces (`gl_FrontFacing`) |

### Basic model water: PS 7965 (`Mt_DungeonWater_A`, gb, variation 0)

Samplers: `_s0`, `_n0`, `_t0`, `_e0` (assign for this material:
`_s0←_n0`, `_n0←_n0`, `_t0←_n0`, `_e0←_e0`: one normal texture read with
two SRT layers), `gsys_normalized_linear_depth`, `gsys_gbuffer_material_id`,
`gsys_light_prepass` (array). `dec(x) = 2.007874·x − 1.007874`
(the same native constants as TeraWater). Varyings: `Sem0` two uv pairs,
`Sem1.zw` emission base uv, `Sem3` normal, `Sem4` tangent (w = sign),
`Sem6` view-space position `P`, `Sem7` clip position.

```text
uv0   = Sem7.xy / Sem7.w
s     = dec(_s0(Sem0.xy).xy)                   distortion / detail normal
n     = dec(_n0(Sem0.zw).xy)
q     = 1 / (−P.z · context[17].y)
z0    = depth(uv0)·context[16].x + context[14].x
w     = saturate((P.z + z0) / 2)               fade-in of refraction over 2 units of depth gap
cand  = uv0 + w · indirect_scale1.xy · q · s
zs    = depth((floor(cand·context[18].xy) + 0.5)·context[18].zw)
zc    = −(P.z + context[14].x)·context[15].x
uv    = zc > zs ? uv0 : cand                   (strict; refraction rejected when it lands in front)
L     = gsys_light_prepass(uv, layer 1).rgb
D     = P.z + (depth(uv)·context[16].x + context[14].x)
b     = saturate(c3.x·(c5.x + D)),  j = saturate(c3.y·(c5.y + D)),
k     = saturate(c3.z·(c5.z + D)),  g = saturate(c3.w·(c5.w + D))     c3/c5 = const_color3/5
a     = saturate(exp2(const_value3 · log2(b)))                         (b ≤ 0: see TeraWater note)
E     = _e0(Sem1.zw + indirect_scale4.xy · dec(_t0(Sem0.zw).xy)).rgb
C     = saturate(E·(1 − j) + saturate(const_value0·(1 − k)))
col   = c1·a + (c0 − c1·a) · c4.z · C                                  c0/c1/c4 = const_color0/1/4
N     = Sem4.xyz·v.x + Sem4.w·cross(Sem3, Sem4)·v.y + Sem3·v.z,
        v = normalize(s.x + n.x, s.y + n.y, sqrt(1 − saturate(n·n)))
out0  = (22/255, (id_g·255 | uk_user_data.z·255)/255, 22/255, 1)      id_g = existing material-id .g at uv0
out1  = (g·col, 1/255)
out3  = (0.5·N + 0.5, (4·floor(63.75·saturate(const_value1·(1 − C.r))) + 1)/255)
out5  = (L·(1 − a), 1)
```

Compared with the TeraWater G-buffer PS (`wiiu-water-variants.md` The same refraction, depth guard, `D`, `a`, `col` and `out5`; the per-vertex `T0..T5` table is replaced by the material constants (`T0→c0`, `T1→c1`, `T2→c3`, `T3→c5`, `T4.b→c4.z`, `T5.r→const_value0`), the angle factor `1 + const_value2·(1 − h)` and the flow term `v` are absent, the distortion uses `_s0` with `indirect_scale1` (TeraWater: primary normal, `indirect_scale2`), and `out3.w` is `const_value1`-based.

Example constants (`Mt_DungeonWater_A`): `c3 = (0.07, 0.65, 0.9, 6)`,
`c5 = (0, 1.06, 0.8525, 0)`, `c1 = (0.001, 0.0085, 0.011)`,
`c0 = (1, 1, 1)`, `c4.z = 1`, `const_value0 = 0.3`, `const_value1 = 0.99`,
`const_value3 = 0.25`, `indirect_scale1 = (0.15, 0.15)`,
`indirect_scale4 = (−0.1, −0.1)`, `tex_srt0 = scale 0.1`,
`tex_srt1/2 = scale 0.2`, `tex_srt3 = scale 100`.

### xlu-blend water: PS 7977 (`Mt_TerraWater03/04`, `Mt_WaterSeal`)

Same body as 7965, plus reads at `uv0` of the existing albedo (`A₀`),
normal (`N₀`), emission (`E₀`) and material id. With `α = Sem5.w`
(vertex alpha):

```text
out1.rgb = A₀.rgb + (g·col − A₀.rgb)·α
out3.xyz = N₀.xyz + (0.5N + 0.5 − N₀.xyz)·α ;  out3.w from N₀.w the same way
out5.rgb = E₀·f + (L·(1 − a) − E₀·f)·α       f = bit 0 of A₀.a·255
out0     = (22/255, id | user, …, …)          (alpha channel derived from A₀.a bits·(1 − α))
```

### Waterfalls: PS 11433 (`Mt_WaterFall_M_A`, `Mt_DungeonWater_Flow` B, …)

Samplers `_a0`, `_s0` (RGBA colour layers), `_n0`, `_e0` (two normal
layers), plus the G-buffer reads of xlu-blend. With `n = dec(_n0)`,
`e = dec(_e0)` (uv pairs `Sem0.xy` for `_n0/_a0`, `Sem0.zw` for
`_e0/_s0`):

```text
cand  = uv0 + q·(indirect_scale2·n + indirect_scale3·e)    + the same depth guard
v     = normalize((n + e)/2, (sqrt(1−|n|²) + sqrt(1−|e|²))/2)   (back faces: reflected)
avg   = (_a0 + _s0)/2   (RGBA)
D     = P.z + z(uv)
α     = saturate(Sem5.w · (saturate(avg.a + Sem5.z − 1)
                           + const_value3·(1 − saturate(const_value0·D − const_value1))))
t     = saturate(const_color1.x·D − const_color1.y)
e2    = saturate(exp2(const_value6·log2(saturate(const_value7·D))))
alb   = saturate(const_color0.rgb·e2 + t·avg.rgb·α)
out5  = L·(1 − e2)                 ; all outputs lerped from the G-buffer by Sem5.w
```

(read from the GLSL; `out3.w`/`out1.w` packing and the vertex
displacement in the VS are not written down here.)

### Glass / energy: PS 7845 (Hateno window) and 7881 (generators)

Samplers `_a0`, `_s0`, `gsys_light_prepass`, `gsys_static_depth_shadow`,
`gsys_normal_modify`. During `Model(GBuffer/Xlu)` the latter two are the
pre-shading **Shadow** and **Fog** targets
(`GBUFFER_HandlePreShadingXluPassEvent` `0x039ab208`,
`wiiu-deferred-shading.md`), so `T = Shadow.z` is the fog transmittance
and `Fog.rgb` the in-scatter.

```text
s    = dec(_s0(uv).xy)
N    = Sem4.xyz·s.x + Sem4.w·cross(Sem3, Sem4)·s.y + Sem3·sqrt(1 − saturate(s·s))   (not renormalised)
f    = saturate(1 + dot(normalize(Sem6.xyz), N))           (0 facing the camera, 1 at grazing)
uvb  = uv0                                                 (7845)
uvb  = uv0 + indirect_scale1.xy · s / (−Sem6.z·context[17].y)   (7869, 7881: refracting glass; no depth guard)
out0 = (22/255, uk_user_data.z, uk_user_data.z (7845) or 0 (7881), 1)
out1 = (_a0(uv).rgb · Sem5.x · saturate(const_value1·f), 1/255)
out3 = (0.5N + 0.5, (4·floor(63.75·const_value0) + 1)/255)
out5 = (L(uvb)·T(uvb) + Fog(uvb), 1)                       L = light_prepass layer 1
```

So glass is "water without absorption": the fogged background goes into
the colour buffer, the deferred water lighting adds the cube-map
reflection with its Fresnel, and `_a0 × const_value1 × f` tints it more
at grazing angles. Behave 102 has **no deferred material of its own**
(`DeferredMain` has none with behave 102); it shares id 22.

### Render order (from the existing CPU research)

1. Opaque G-buffer, pre-shading of the opaque scene (Diffuse/Shadow/Fog
   targets).
2. `GBUFFER_HandlePreShadingXluPassEvent` event 3: binds
   `gsys_light_prepass` ← PreShading Diffuse, `gsys_static_depth_shadow`
   ← Shadow, `gsys_normal_modify` ← Fog.
3. `Model(GBuffer/Xlu)` (list 1, mask 0x20): TeraWater and all behave
   102/103 model materials, sorted by the model-unit priority
   (`gsys_priority`, e.g. −20 with hint `field_ground` for
   `Mt_DungeonWater_A`).
4. Pre-shading of Xlu ids (`preshading_field_water`, 140–143) and the
   final `field_water` (24–27).

## Hypotheses

- `gsys_assign_variation` 0–2 (all three present for some families) are
  the same variations the deferred programs have; which one a draw uses
  was not traced.
- The forward `gsys_assign_material` programs of these families serve the
  cube-map capture (`gsys_cube_map=1` on all glass) and/or another view;
  not established.

## For our renderer

`crates/render/src/water_material.wgsl` already implements the TeraWater
G-buffer + deferred chain; model water families 7944/7956/7968/7980 are
the same chain with the material constants mapped as above, so they can
reuse it (plus the texture-SRT animation of the model file). Glass needs
the same deferred water lighting with `out5 = L·T + Fog` and no
absorption. Waterfalls need the vertex-displacement VS and the two-layer
colour/normal PS (both read below, "Read during the port").

## Read during the port (2026-10-03)

Same sources (Cemu snapshot `20260928T012415Z-cache-replay`, simplified
with `tools/research/cemu_glsl_simplify.py` against the programs' native
code); the port is `crates/render/src/model_water.{rs,wgsl}`
(`docs/STAGE4-water-glass.md`).

- **Family by options.** Leaving `uking_enable_output_object_attribute`
  aside, every one of the 204 census materials matches a sampled family
  signature exactly. Decisive options: behave 103 with
  `uking_enable_scene_color0_depthdiff1_effect=1` → water (7920–7956), with
  `uking_enable_gbuffer_xlu_blend=1` too → 7968/7980; behave 103 otherwise
  (xlu-blend) → `uking_color3_calc_type` 21 = 11412/11424, 8 = 11400,
  `uking_normalmap_blend_ratio` 200 = 11256/11268/11280/11292 (unique
  among the 103 signatures); behave 102 with `uking_enable_calc_color2=0`
  → 7824–7872 (7908 enables combiner colours 2–4).
- **Sem5 is `_c0`.** VS 11433 (Cemu `31540cb9…`, the code of programs
  11404–11435) exports the unorm-8 vertex colour (attribute Sem12) as
  Sem5. Hateno's water and glass meshes all carry `_c0`; the window glass
  has (0.65, 0.65, 0.65, 1), the well's blended water an alpha ramp.
- **VS 11404–11435 (families 11400/11412/11424): height push.**
  `tc_k = tex_srt_k · uv0` for k = 0, 1, 2 (Sem0 = tc0, tc1);
  `h = _v0(tc2, level 0).xyz` (`_v0 ← _i0`, `CmnWaterFall_A_Height_Ind`);
  the bone-space position moves by `normal ⊙ h · const_value2 · c0.g`
  (per axis) before `gsys_shape`.
- **VS 11272–11303 (families 11268/11292):** no push; Sem0 = (tex_srt0 ·
  `_u0`, tex_srt1 · `_u1`): `uking_texcoordN_mapping` 1 is the second UV
  set.
- **PS 11409 (11400)** = PS 11433 except: opacity
  `k = saturate(saturate(mean.a + c0.b − 1) + const_value3·(1 − saturate(const_value0·D − const_value1)))` (no vertex alpha), every
  output blended by `saturate(c0.a²)`, no back-face mirror.
- **PS 11265 / 11301 (11256/11268, 11280/11292)**, the waterfall-bottom
  foam: layers and normal maps mixed by `m = saturate(2·c0.r²)`
  (`A = a0 + (s0 − a0)·m`, normals likewise), `k = saturate(t·(A.a + c0.r − 1))` with `t = saturate(const_color1.x·D − const_color1.y)`,
  `e2 = saturate(exp2(const_value3·log2(saturate(const_value7·D))))`,
  albedo `saturate(A.rgb·k + saturate(const_color0·e2))`, colour buffer
  `L·(1 − e2)`, gloss blended towards 1, all blended by `c0.a`.
- **PS 11433 details:** back faces (`gl_FrontFacing`) reflect the normal
  across `T × (N × T)`; the gloss is blended from the G-buffer's towards
  1 by `c0.a`; the albedo term is `saturate(const_color0·e2 + saturate(t·mean.rgb·α))`.
- **`log2(0)`:** the programs guard it as Cemu's `isinf → −FLT_MAX`
  (R600 LOG_CLAMPED), so `saturate(exp2(p·log2(max(x, 0))))` is 0 for
  x = 0 and p > 0 and 1 for p ≤ 0.
- **Texture-SRT animations.** FSHU in FRES groups 3 (shader parameter) and
  5 (texture SRT) of FRES 4.5.0.3: `ShaderParamMatAnim` entries are 0x20
  bytes, `ParamAnimInfo` 0x10, `AnimCurve` 0x24, constants 8; a curve's
  target is the byte offset in the `tex_srt` value (16 = translation x,
  20 = y), flags bits 8–9 / 12–13 the pre / post wrap. Hateno's water
  models carry `<unit>_Auto` / `_auto` animations (FSHU flag 4 =
  looping), e.g. `DgnObj_DungeonWater50x50_A_01_auto` (4000 frames,
  linear scroll of srt0/srt2 x by 30 and a cubic wobble), the well's
  `TwnObj_Village_HatenoWell_A_01_Auto` (600 frames); the well also has a
  non-`_Auto` one. Which animation the game starts, and at what rate, is
  not traced (SI-MWT-03).
- **Component selection.** `TerraWater01_Emm` is BC4 with select
  (r, r, r, r); `TerraWater01_Nrm` BC5 with (r, g, 1, 1): the program
  reads `.rgb` / `.xy` through it.

## Open questions

1. VS of `Mt_FirstShrineBedWater_A` (type 18) and the PS of families
   11376, 11388, 11436, 11616: not read (none in Hateno's reach).
2. Programs 7929/7941 (far `Mt_DungeonWater_A`) and 7917 (Vah Medoh
   glass) are not in the Cemu cache.
3. Where the forward material programs are used.
4. Exact `out0.ba` / `out3.w` / `out1.a` semantics of the xlu-blend
   variants (bit packing of `A₀.a`).
5. Who starts a model's material animation and at what rate (the
   `_Auto` naming is read, not traced).
</content>
</invoke>

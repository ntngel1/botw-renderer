# Wii U: Character Lighting (`chara_*` `DeferredMain`)

Date: 2026-09-28 Subsystem: render, characters. Task:  surface lighting  (upstream reference: `../tasks/deferred-lighting.md`). Context: [ delayed lighting and water ](wiiu-deferred-shading.md), [ materials `uking_mat`](wiiu-material-variants.md).

## Question and sources

- What programs `uking_sys_shading` highlight opaque characters and
  What formulas; where do their inputs come from (environmental light, shadows, G-buffer flags).
- Dump: Wii U update v208 (EU title `00050000-101c9500`, as in neighboring ones)
  `Pack/Bootup_Graphics.pack` → `Model/SystemModel.sbfres` (model `DeferredMain`) and `Shader/uking_sys.product.sbfsha`; `uking_pass_shader.sharcb` (program `light_analyzer`); `Link.sbfres` (`Mt_Face`). All recovered in `game-data/reference/visual-formulas/`, outside Git.
- Cemu: `20260928T012415Z-cache-replay` image, exact byte mapping
  (`matches.json`); uniforms — `tools/research/cemu_uniform_map.py`.
- Ghidra: `U-King.rpx` (EU v208), read only; addresses below.
- Switch decompilation was not used.

## The result

### Materials `DeferredMain` → programs

The `SystemModel.sbfres` (`ALL_OPTIONS=1 model_info`) material options are decoded using the keys of 148 manifest `uking_sys` (model 1) programs. All matches are accurate (all `gsys_deferred_shading_pass=1`) materials; the group programs differ only from `gsys_assign_variation` 0-3.

| Materials | compare id | behave | Programme programmes | Cemu PS (variation 0) |
|---|---:|---:|---|---|
| `chara_nonmetal`, `chara_nonmetal_direct` | 8, 9 | 0 | 0–3 | `7c6e02daf538f23e` |
| `chara_metal` | 10 | 1 | 4–7 | `1bd1c50969e7e4ac` |
| `chara_grossy` | 11 | 10 | 8–11 | `ac099496a4733213` |
| `chara_hair` | 12 | 100 | 12–15 | `c580a12764680547` |
| `chara_skin` | 13 | 101 | 16–19 | `1f8a651427ddb2a8` |
| `chara_eye` | 14 | 104 | 20–23 | `b2e61dfb5275f27d` |
| `field_water` | 6 | 103 | 24–27 | `2e2543216c04766d` |
| `field_leaf` | 7 | 105 | 28–31 | `3179b85d41bfb80d` |
| `field_hybrid` | 4 | 2 | 32–35 | `8d24f32f18e6de47` |
| `preshading_chara` (type 7) | 8 | 0 | 104–107 | `1c7db40ff5d693ab` |
| `preshading_field_leaf` (type 7) | 6 | 105 | 108–111 | `fb2e18ae56397ca7` |
| `preshading_field` (Type 7, SSAO) | 4 | 2 | 112–115 | `bec68ec6f40a864f` |
| `preshading_clear` (type 8) | 0 | 0 | 116–119 | `3f3264b305b0a43a` |
| `preshading_shadow_field_leaf` (type 9) | 6 | 105 | 120–123 | `9c0b7031078fba88` |
| `preshading_shadow_chara` (type 9) | 8 | 0 | 124–127 | `45e72a252caba763` |
| `preshading_shadow_field` (type 9) | 4 | 0 | 128–131 | `88133ee405eaae28` |
| `preshading_shadow_clear` (Type 10) | 0 | 0 | 132–135 | `2364006f2b86ab25` |
| `preshading_chara_xlu`, `_field_water`, `_field_xlu` (Type 11) | 24, 22, 20 | 0, 103, 2 | 136–139, 140–143, 144–147 | see [ to ](wiiu-deferred-shading.md) |

Programs 36-103 (types 1-6) - `mat*` of other models `SystemModel` and `debug_simple`; not included in the lighting of the scene. the previous entry "PS 108 - `preshading_field`" incorrect: 108 - foliage, relief - 112.

### Frame chain for an opaque character

1. **G-buffer** (`uking_mat`, for `Mt_Face` program 10607, Cemu
   `d4670aaae44bf58c`; `Mt_Upper_Skin` — 5111. Outputs: material id (skin 13/255), albedo, normal, emission. `albedo.a·255 = 65 + ((ao.w·255 & 0xf0) >> 3)`: bit 0 = 1 (flare/alpha), bits 1-4 — the senior half-byte channel `w` `_ao0` card (transmission, `uking_transmission_channel` 40), bits 5-7 = 2 = `uking_chara_size`. Normal is written with a length of `(0.75 + 0.25·ao.x)` — **AO is encoded in the length of normal Z**; `normal.w` — quantized gly.
2. **Shadows of** (PS 124, `preshading_shadow_chara`)
   (`GBUFFER` +0x2e4c, in the remaining passes of `gsys_sssss`): x = shadow - PCF 4 samples of cascades (`gsys_depth_shadow_cascade`) with a normal displacement only at `N·L ≥ 0.35`, behind the cascades - the baked shadow of the world `gsys_dynamic_reflection(xz·0.0001 + 0.5)·env[42]` in the 4th power, plus `depth_shadow_off`; y - `uking_tex1` in the reflected view × (−N·V); z = 0.5 inside the cascades, otherwise 1; w = 1.
3. **Pre-shading** (PS 104, `preshading_chara`, 5 outputs).
   opaque character (id  ⁇  24): Shadow = (shadow from Expand, cloud shadow `sat(gsys_projection0 + proj_shadow_off)·depth_shadow_scale`, mist transmission, **0**); Fog = fog color; **Diffuse (layer 0 `gsys_light_prepass`) = 0**; Specular (layer 1) = simplified illuminated color (for refraction under water); Upscale = average coded normal. The fog is the same as water (PS 140).
4. **Main Passage** (PS 16 leather / PS 0 `nonmetal`), blend
   `src + dst·src.a` on target 0 (see water).

### Main Pass Formula (PS 16 and PS 0, read in Cemu GLSL)

Uniforms (native map): `u0..u4` — context`(tan·aspect, tan(fovy/2), fovy)`4], `uking_dynamic_toon_light_adjust_for_demo`7], [15], [16], [18] (near/far, `(tan·aspect, tan(fovy/2), fovy)`, `1/(far−near)`, `far−near`, `(W, H, 1/W, 1/H)` - layout [water-water](wiiu-water-variants.md#layout-and-coefficients)); `u5`, `u7` — environment[4], [5] (direction and color of the main light); `u6` — scene_material[2] (`.w` = `base_light_change_ratio`); `u8` — scene_material[0] = `uking_dynamic_toon_light_adjust_for_demo`PS 16: albedo 1, normal 2, linear depth 4, half-deep 6 `gsys_light_prepass` 8, `gsys_static_depth_shadow` 9 (← Shadow), `gsys_normal_modify` 11 (← Fog), depth 12.

Designations: `N` - normal, `|n|` - its length to normalization, `V` - unit vector from camera to point, `L = env4·ratio` - direction, **kuda** light goes (to light - `−L`), `A` - albedo G-buffer, `S` - Shadow, `F` - Fog, `Amb = Sem2.rgb`, `w = Sem2.w` (VS, from `gsys_user4`, see below), `C = env5` (color of light), `z` - range, `d = 1/(z·tan(fovy/2))`, `k = d·2^(chara_size−2)`, ZXQQ16.

```text
r = sat(1 + N·V) rim (0 face to camera, 1 on silhouette)
b = sat(−V·L) camera looking at light
ao = sat((sat(4|n|−3)−0.5)·4) = sat((ao−0.5)·4) by G-buffer
s = S.y·sat((S.x − 0.4)·10) shadow (cascades/baked) × clouds
t = sat(((sat(N·(−env4)) − 0.4)·75·(1 + 0.1d))) step of light
lit  = lerp(1, s, sat(w))
cw   = sat(w + s)
q    = sat(4k)·sat((e1·sat(N·normalize(V − 0.8L) + 1)·b − 0.4)·75)
       [ skin: + b·sat((f/15·sat(N·H) − 0.5)·40)·0.15] contour against light
D    = ao·lerp(sat(q), lit, t)          [nonmetal: + ((a8 & 0x1c)/28)·((a8 & 2)/2)]
Dc   = lerp(0.8·Amb, C, cw)·toon_adjust
light = Dc·ratio·D + Amb·(1 + (sat(ao + 0.8) − 1)·sat(5b))
       [ skin: + w·(1 − D)·Dc·(0.08, 0.04, 0)]
out = S.z·(A′·light) + F.rgb, out.a = bit0(a8)·S.z2
```

`H = normalize(−V − L)` (semivector), `a8 = A.a·255`, `f` - bits 1-4 `a8`. `e1, e2, e3` - screen edge detector: from the depth at the point `uv + (nₓ, −n_y)·(W⁻¹, H⁻¹)·o` , the position `P′`, `Δ = P′ − P`, `e = (V·Δ)/|Δ − V(V·Δ)|` is restored:

- `e1` = `sat(e·0.01)`, offset along the screen projection of normal by
  `6·min(k, 2)` pixels x and `1.2·min(k, 2)` y;
- `e2` = `sat(0.12·fovy·e)`, the displacement of `2·sat(k)` pixels along the normal.
- `e3` = `sat(0.12·fovy·e)`, displacement of `3·sat(k)` pixels to light
  (screen projection of `L`).

Albedo with "backlit" (`uking_deferred_enable_albedo_highlight` is not needed here - the branch is built in):

```text
g    = r·pow(sat(N·normalize(−L − 0.3V)), 20)        (nonmetal: r^0.4·pow(…, 12))
g2 = g·sat(1.2k)·normal.w2
h0   = sat((g2 − 0.01)·50)·0.05 + sat((g2 − 0.05)·20)·0.12
v    = r·e2·sat(5 − 5·V·L)·sat((sat(N·(−L)) − 0.4)·75) − 0.4
h1   = sat(lit + 0.08)·sat(h0) + sat(6k)·sat(75v)·0.4
h    = sat(sat(r·b·sat((e3 − 0.5)·40)·8)·sat(2k)·0.25 + sat(h1))
M = max(A), sat = 1 − min(A)/M
A_s = M + (A − M)·sat((1 − h)·sat)/sat discoloration
A' = A_s·sat(M + h·M^0.6·(4 − 3·sat)/M lightening
```

(plus decoding G-buffer `×1.07321 − 0.000528` and reverse `×0.931784 + 0.0004925`, together is an identity.) For an opaque character, `S.w = 0` and `light_prepass = 0`; branches with them (`(1−r)^0.2·S.w`, `prepass·v384/luma`) fall out. With `S.w = 0` , the `0.3·S.w` stepshift is zero: **is the boundary of light always at `N·L = 0.4`**, the width of `1/(75(1 + 0.1d))` through `N·L` — hence the sharp shadow on the face.

### Other classes (Cemu GLSL, same entrances)

Designations above; `sat_ = 1 − min(A)/max(A)`, `M = max(A)`, `gl = normal.w`, `fl` - bits 2-4 `a8`/28, `b1` - bit 1 `a8`, `rimL = sat(r·b·sat((e3 − 0.5)·40)·8)·sat(2k)·0.25`, `rimE = sat(6k)·sat((r·e2·sat(5 − 5V·L)· sat((sat(N·(−L)) − 0.4)·75) − 0.4)·75)·0.4`, `Hs = normalize(−L − 0.3V)`.

- **Hair, PS 12** (Cemu `c580a12764680547`): `D = fl·b1 + ao·lerp(q, lit, t)` (q without skin additive); `g = sat(6k)·gl²·pow(sat(N·Hs), 6)`;
  `h = sat(rimL + sat(h1·sat(lit + 0.5) + rimE))`, `h1 = sat(sat((g − 0.14)·70)·0.7)·sat(sat_ + 1.9M + 0.1)·0.1 + sat((sat(6k)·fl·(1 − b1)·g − 0.14)·70)·(1 − sat(3M(1 − sat_)))·0.15`; `light = Dc·sat(D) + (w·(1 − sat(D))·Dc·0.08 + Amb)·amb_mod` (warm supplement on all channels); albedo - like the skin.
- **Metal, PS 4** (`1bd1c50969e7e4ac`): shadow only half, `lit_m = 0.5·lit + 0.5`; `D = fl·b1 + ao·lit_m·t` (without q); glare in direction
  species-bound `Hm = normalize(−1.5·L_view.x, 0.6, 1)`, `g = pow(sat(N_view·Hm), e^(gl² + 3))`, `hm = sat(sat((g − 0.005)·100)·0.2 + sat((g − 0.14)·70)·0.3)`; `E = sat(lit_m·sat(N·(−L) + 0.9)·hm·8gl² + rimL)·3 + sat(D)`; `light = Dc·E + Amb·amb_mod`; albedo unchanged.
- **Eyes, PS 20** (`b2e61dfb5275f27d`): `k = d/4` (without `chara_size`)
  `fade = 0.6·sat(0.15d) + 0.4`; `D = ao·lerp(q, lit, t)·fade`; `light = Dc·D + Amb·1.8·fade·amb_mod`; over (after multiplied by albedo) the `((a8 & 0xfc)/252)·(0.6·sat(0.075d) + 0.4)·(0.8·Dc + 0.2)` glare.
- **Gloss, PS 8** (`ac099496a4733213`): Like `nonmetal`, but the glare is
  `pow(Expand.y, 1.5gl² + 1)`, which is a spherical map of `uking_tex1` ( `preshading_shadow_chara`) from the reflected view from PS 124; shadow, like metal, is only half. Not fully dismantled.

### The light of the environment: `LightAnalyzer` → `gsys_user4` → `Sem2`

The main aisle VS (Cemu `1a14de8e58d5b30a`, full-screen triangle) reads `gsys_user4` (12×1): `Sem2.rgb = lerp(T[4], T[5], v)` (v - 0 at the top of the screen, 1 at the bottom), `Sem2.w = T[0].y`.

`gsys_user4` - index 4 of the KSys array (`ENV_CopySkyFogsToCloudObject` `0x033ff8cc`: `local_230` = object KSys+0x1e0, the form x0x6f0texture +0x340) The object is `gfx::LightAnalyzer` (record designer) `0x038b4280`program `light_analyzer` into `uking_pass_shader`); +0x340 ← Goal +0x638 (12×1, format) 0x1a) in the + mode0x388 = 0 (`0x038b48a4`Passage (Passage)`0x038b4d70`): (a) step 5 folds the cube map into 1×1 (average 14 directions, `cCubeMap2D`(b) Steps 1–2 reduce the goal **Expand** (x = "in the sun" from PS 124/120/128) to 1 pixel`cExposure`step 2x `cForceShadowRatio`); (c) step 4 (Cemu) `728b9bc3556c3de1`) writes 12 texels:

```text
e     = (cExposure·main.w)³,  avg = luma(cCubeMap2D)
T[0]  = (x, sat(1.2x), Master),  x = cExposure·main.w
T[4,5]= c = cube(normalize(look.x, 1 − 2g, −look.z), LOD 3),
        g = chara ambient gra y offset (+ chara ambient gra y for T[5])
        M = max(c), s = 1 − min(c)/M,
        c′ = M + (c − M)·lerp(chara_sat_min, 1, e)·(s + 0.0001)^(chara_sat − 1)
        T = c′·Master·chara_scale/(avg + lerp(chara_min, chara_max, e))·cAmbientScaleChara
```

(T[1] field, T[2,3] cube up/down, T[6,7] gray, T[8,9] effects, T[10] medium cube, T[11] thresholds light prepass.) Parameters - object `ksysla` ( `0x038b37c0` constructor, entry in uniforms `0x038b48a4` in order of announcements).

| Parameter. | Shut up. | | Parameter. | Shut up. |
|---|---:|---|---|---:|
| `chara_ambient_gra_y` | 1.0 | | `ambient_offset_chara_min` | 0.1 |
| `chara_ambient_gra_y_offset` | 0.0 | | `ambient_offset_chara_max` | 0.5 |
| `chara_ambient_sat` | 0.5 | | `ambient_offset_chara_scale` | 1.75 |
| `chara_ambient_sat_min` | 0.75 | | `ambient_offset_field_min/max/scale` | 0.1 / 0.5 / 1.0 |
| `field_ambient_sat` / `_min` | 0.5 / 0.8 | | `ambient_offset_cubemap` / `_far` | 0.01 / 1.0 |
| `effect_ambient_sat` | 2.0 | | `light_prepass` / `_threshold` / `_offset` | 0.1 / 3 / 3 |

`cAmbientMasterIntensity` (+0x6e8) and `cForceShadowRatio` (+0x6ec) - 1.0 when created; `cAmbientScaleChara` (+0x6d8) - global vec4 `0x1054a51c`, changes `0x038b55c4`. `cLook` - (+8, +8, +0x28) camera matrices (horizontal components of the look). `cMainLightColor.rgb` - color `dir_main` × intensity (`FUN_030c04ec`), `.w` = `dVar45 − KSys+0xb58`, `dVar45` = 1 by default (conditional branches are not disassembled). - `*.ksysla` ladies do not appear to act.

**Interpretation.** The light of the character's surroundings **It doesn't depend on the normal.**: these are two colors of the sky (cube maps of the environment) - above and below the view, with raised saturation and normalized for the average brightness of the environment, with a smooth transition from top to bottom on the screen. `w = sat(1.2·sunshot)`: in the shadow (forest, cave) shadows on the character weaken, direct light is colored in `0.8·Amb`.

## Reproduction and limitations

```sh
REF=/absolute/game-data/reference/visual-formulas
ALL_OPTIONS=1 target/release/examples/model_info $REF/shading/SystemModel.sbfres
python3 tools/research/cemu_uniform_map.py \
  $REF/shader-archives/uking_sys/model001-program00016-ps.code \
  $REF/cemu-sessions/20260928T012415Z-cache-replay/shaders/1f8a651427ddb2a8_000000000f249a49_ps.txt
python3 tools/research/cemu_uniform_map.py \
  $REF/shader-archives/uking_pass_shader/00487-stage1.code \
  $REF/cemu-sessions/20260928T012415Z-cache-replay/shaders/728b9bc3556c3de1_0000000000001ec9_ps.txt
```

Verified: Options matching → programs (exact), uniforms cards PS 16, PS 0, PS 104, VS 16, LightAnalyzer step 4 (tool), CPU addresses. No ALUs were executed; formulas - reading GLSL, not native-probe. `cMainLightColor.w` outside the default branch, `cAmbientScaleChara`writer `toon_adjust` (scene_material[0]How G-buffer reads hair `uking_grossy_intensity` (2, FORMATS (archived reference)), loading `ksysla` (file not found), which cube map is tied as `cCubeMap`/`gsys_cube_map`, full formula `chara_grossy` texture `uking_tex1`difference `nonmetal_direct` (id 9 with the same programs 0-3), flags `albedo.a` materials other than the face. task (upstream reference: `../tasks/deferred-lighting.md`).

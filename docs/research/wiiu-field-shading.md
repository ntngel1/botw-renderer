# Wii U: Lighting of relief, objects and grass (`field_hybrid`, `field_leaf`) and SSAO

Date: 2026-09-28, supplemented by 2026-09-29 (grass, distant trees). Subsystem: render, relief, objects, grass. Task:  illumination of surfaces of  (upstream reference: `../tasks/deferred-lighting.md`). Context: [ characters and `LightAnalyzer`](wiiu-character-shading.md), [ delayed lighting and water](wiiu-deferred-shading.md).

## Question and sources

- What formulas the game illuminates opaque terrain and objects (id 4, ed.)
  `field_hybrid`), from where their surrounding, shadow and SSAO light come.
- Dump: Wii U update v208 (EU `00050000-101c9500`).
  `Pack/Bootup_Graphics.pack` → `Shader/uking_sys.product.sbfsha` (`uking_sys_shading`), `Shader/uking_terrain.product.sbfsha`, `Model/SystemModel.sbfres` ( `DeferredMain`), `Model/SystemModel.Tex2.sbfres` ( `ssao` texture); `uking_pass_shader.sharcb` (`light_analyzer`). Extracted from `game-data/reference/visual-formulas/`, outside Git.
- Cemu: `20260928T012415Z-cache-replay` image, exact match
  Bytes (`matches.json`); uniforms - `tools/research/cemu_uniform_map.py` (for PS 128 did not work: Cemu left direct block indexes).
- Ghidra `U-King.rpx` (EU v208, read only) – for sky map from above
  (`vis`); Switch decompilation was not used.

## The result

### Chain and programmes

| Step. | Programme | Cemu | Exit |
|---|---|---|---|
| G-buffer relief | `uking_terrain` 9–11 | PS `583ea8604da62310` | albedo, normal (flags below) |
| shadow + SSAO (`preshading_shadow_field`, type 9) | 128 | VS `0d6127fbed646d2b`, PS `88133ee405eaae28` | Expand target (`gsys_sssss`) |
| pre-shading (`preshading_field`, type 7) | 112 | VS `bb50d2ee4fa87bc2` et al., PS `bec68ec6f40a864f` | Shadow, Fog, Diffuse, Specular, Upscale |
| main passageway (`field_hybrid`) | 32 | VS `0bcd653c18367d59`, PS `8d24f32f18e6de47` | color, blended `src + dst·src.a` |

Options (manifest): PS 128 `direct_shadow_calc`, `direct_shadow_pcf=1`, `enable_direct_ssao`; PS 112 — `direct_ambient_calc`, `enable_ssao`, `direct_world_ao`, `direct_fog`, `enable_simple_shading`; PS 32 — `enable_extra_specular`, `enable_albedo_highlight`, `enable_rain_effect`. pre-shading reads half-deep (`gsys_half_normalized_linear_depth`), and the PS 32 selects its texel by gather half depth and target Upscale (average coded normal): **pre-shading, apparently in half resolution** with the choice of the nearest depth and normal texel (interpretation by samplers, target sizes are not checked).

**G-buffer relief** (PS `583ea8604da62310`, read): `albedo.a = floor(63.75·m)·4/255` - bits 0-1 = 0, metal in bits 2-7; `normal.w = floor(63.75·gl)·4/255 + 1/255` - **bit 0 = 1** (flag), gloss in bits 2-7. `gl` is mixed from the blue channel `MaterialCmb` of two dot materials (`t10.z`) with corrections that are not disassembled. Albedo Flag = 0, so `out.a = 0` in PS 32 and mixing does not change the result.

**G-buffer objects** (`uking_mat`, variant `gsys_assign_gbuffer`, `gsys_weight` −1/0 - without scanning; family of programs found `material_variants.py`, variant - by `word[51]` key, Cemu - `matches.json`; read 2026-09-28):

| Material (model) | Programme | Cemu PS |
|---|---|---|
| `Mt_Tree_Trunk` (`Obj_TreeBroadleaf_A_L`) | 7581 | `75aa0204f03640f0` |
| `Mt_Wood_MossWood_A` (`Obj_TreeBroadleafFallenTree_A_M_01`) | 5241 | `dafc2b62b82bffbd` |
| `Mt_Rock` (`FldObj_BridgeRockWhite_A_01`, `tma`/`tmc`) arrays | 12801 | `c9aec35aaf241787` |
| `Mt_RockCliff_A` (`FldObj_CliffWhiteLumpRock_A_03`, arrays) | 13065 | `523f4f480a34f724` |
| `Mt_EnemyBaseRock_A_01` (`FldObj_EnemyBaseRock_A_01`) | 7497 | `8efc550576ab6f55` |

All five have a goal of 0 = (4/255 -) `field_hybrid`, `uk_object_attribute` ×2, 1); `albedo.a = 0` — **metal**bit 0 = 0; `normal.w = floor(63.75·gl)·4/255 + 1/255` — **flag** (SSAO and contour lighting are included, like the relief.) **Gloss. `gl` Blue channel of the normal map**: Normal (xy) and glossy (z) read from the slot `_s0` shader, in which ordinary objects are assigned `_n0` material`material_census`: `_s0<_n0`; `uking_grossy_color` = 402 for 18,220 of 18,275 materials with `_n0`), rocks with massifs have blue `MaterialCmb` (`tmc`) layer `texture_array_index1`It's like the topography, but it doesn't mix. The normal map is BC1 (X, Y in red and green), middle blue: `Wood_MossWood_A_Nrm` 84/255, `Wood_AnnualRing_A_Nrm` 47/255. Normal card - `2.0039·t − 1.0039`z is recovering.

The albedo of the tree (5241, 7581) is not just a texture. `sat(A + const_color0· pow(sat(1 + V̂·N), const_value0))` (V ⁇  = normalized input) `Sem6`/`Sem7`; in meaning `1 + V·N` This is the direction from the camera, VS not read. `Mt_Wood_MossWood_A` `const_color0` = (0.16, 0.12, 0.08), `const_value0` = 5 is a warm edge on sliding corners. `gsys_material` vec4 23 and 31.x (displacement by manifest).`uking_color0_calc_type` 28, A 503, B 104, C 100), **not** option `uking_enable_fresnel_cheat`: it = 1 and the rocks, which have no penis in the G-buffer. The trunk 7581 has two more layers (both of them). `Sem6.w`) and mixing with texture `PS7` down `sat(10·Sem2.z − 9)` - not disassembled.

### Designations

View space, like in the game: `P` - period, `z = −P.z`, `V = P/|P|` (from the camera) `N` - normal, `L = env4·r` - Where the main light goes (See)`r` = `base_light_change_ratio`, scene_material[2].w), `C = env5` - its color`BgDifColor × FeatureColor weather × BgDifIntencity` Palette, no exposure: [CPU](wiiu-render-cpu.md#main-light-env5-and-the-palettes-exposure)), `A` - Albedo, `m = (a8 & 0xfc)/252` (a8 = albedo.a·255) `gl` - normal.w, `flag` - bit 0 normal.w·255. `c = sat(−N·V)`, `nl = sat(−N·L)`The cupboard `gsys_cube_map` Read in the world direction (context lines)[11..13]), z axis with minus. scene material: [1] = (`cloud_ratio`, `depth_shadow_off`, `proj_shadow_off`, `rain_ratio`), [2] = (`rainfall`, `exposure`, `world_shadow_off`, `base_light_change_ratio`), [3] = (`main_rendering_light`, `skyocclusion_off`, `depth_shadow_scale`, `item_filter_alpha`) - uniforms manifest offset (`+1` in the descriptor = displacement 0x10·i + 4·j.

### PS 128: Shadows and SSAO – Expand

```text
Expand.x = sat(lerp(W⁴, Sh, sat(sm2.z + 1 − sat(0.05(z − e53.w))))·depth_shadow_scale
               + depth_shadow_off)
  Sh = lerp(pcf, 1, sat(0.05(z − e53.z)))) shadow attenuation for e53.z
  pcf - 4 samples of the cascade (ctx[58].xy - cascade boundaries, matrix ctx[42+4i..]),
        normal displacement 0.0791·(1 − (N·L)2)·nl and 0.0005·cascade;
        if the mean <1 is still a sample of the adjacent cascade, the product
  W  = dot(gsys_dynamic_reflection(X·0.0001 + 0.5, Z·0.000125 + 0.5).rgb, e42.rgb)
Expand.z = AO (for z < e53.z, otherwise 1); Expand.w = 1; Expand.y = uking_tex1
```

**SSAO** ( `enable_direct_ssao` option, read). Noise - `uking_tex0` = texture of `ssao` (`SystemModel.Tex2`, 4×4 R8G8, unit vectors of `(cos θ, sin θ) = 2·(R, G) − 1`), target coordinate `uv·(W, H)/8` (VS `Sem6`). Four samples of half depth of `d` around the center of `d₀`:

```text
Rθ = rotation on θ; a = H/W (x-shifts multiplied by a)
A± = uv ± Rθ·(−0.0036606, 0.0364835)      k_A = 14.70871·(far − near)
B± = uv ± Rθ·( 0.0199001, 0.0019967)      k_B = 10.91090·(far − near)
Δ = 0.995·d0 − d_i (> 0: sample closer to the camera)
t_i = sat(k·Δ + 0.5),  f_i = sat(1 − 0.8333333·(far − near)·Δ)
u_i = 0.5 + f_i·(0.5 − t_i)
para(+, −) = lerp(u−, t+, f+) + lerp(u+, t−, f−)
AO = 1 − sat((0.17764·pair A + 0.254544·pair B − 0.4321855)·6)
```

In meters `(far − near)·Δ ≈ 0.995·z₀ − z_i`: `t` grows by 1 per 6.8 cm (A) and 9.2 cm (B) of the protrusion, the sample further than 1.2 m forward is replaced by the addition of the opposite (`u`). `0.4321855 = 0.17764 + 0.254544` - the open surface gives 0. The radius is constant **on the screen** (3.7% and 2% of the frame height), there is no blur (noise 4×4 remains in AO).

### PS 112: pre-shading relief

VS 112 (`bb50d2ee4fa87bc2`): `Sem2.xyz = gsys_user4` texel `Sem2.w =` tex `.x` Outputs (target slots - by [writers](wiiu-deferred-shading.md)):

```text
vis = sat(lerp(sat(0.05z − 3.5) + skyocclusion off, 1, (1 − u)3))
  u  = sat(0.25·(e39.w·(1 − DSQ) − Y + 1.3 − flag·(2N_y − 1)))
  DSQ = gsys depth shadow quarter(X·e39.z − e39.x, Z·e39.z − e39.y) - map at the top (below)
Shadow = (sat(proj0 + proj_shadow_off)·lerp(min(Ex, Ew), Ex, sat(−N·L + 1 − flag)),
          0, T·(1 − H)·(1 − D), vis (fog as PS 140)
Fog = like PS 140
Diffuse (layer 0 gsys light prepass) = lerp(amb, hemi, sat(0.01z − 0.5))
                                      · lerp(1, e1.rgb, e1.w·user2.y)
  c    = cube(N, LOD 3),  M = max(c),  s = 1 − min(c)/M
  o    = 0.8·flag·(1 − AO)                        AO = Expand.z
  x    = sat(sat(s + 0.1(1 − vis)) + o)
  c′   = M + (c − M)·Sem2.z·x^Sem2.y / s
  amb  = c′·Sem2.x·(1 + vis)/2·(1 − o)
  hemi = lerp(T[2], T[3], (1 − N y)/2) (gsys user4 for u = (3 − 0.5N y)/12)
Specular (layer 1) = emission ·bit0(a8) + C·Sh·nl·F L·D/4·r4 + cube(R, LOD 3 − 1.5gl)·F E"
                    + A·(1 − m)·(C·Sh·nl·sat(nl + 0.5 + 0.5vis) + Diffuse)
Upscale = mean coded normal
```

`F_L`, `D` — like the PS 32 below, but the roughness of `1 − gl/2`, `F_E′` with `4 − 1.5gl`, without a member of geometry. Specular is a fully illuminated color (it takes the G-buffer of water as "what's under water").

**SSAO acts only on the light of the environment** and only at the normal flag: weakens it to 0.2 and **raises the saturation** shaded (`x` grows on `o`). `vis` (shading from above on the height map) weakens the environment no more than twice and also adds saturation.

### PS 32: `field_hybrid` main pass

```text
α   = sat((1 − gl)² + 0.01),  kG = (2 − gl)²/8
H = −normalize(V + L) (semivector to light and camera)
D = α2/((N·H)2(α2−1) + 1)2·r4 (without π)
G   = 1/((kG + c(1 − kG))·(kG + nl(1 − kG)))
F0  = lerp(0.04, A, m)
F_L = F0 + (1 − F0)(1 − sat(−L·H))⁵,   F_E = F0 + (1 − F0)(1 − c)⁵/(4 − 3gl)
spec = C·F_L·D·G/4·cloud_ratio·S.x·nl
env  = cube(reflect(V, N), LOD 3 − 3gl)·F_E·lerp(0.25(1 + m), 1, S.w) + prepass₁
e = screen edge: depth of uv + 1.5 pixels along screen N (Sem3),
       e = (V·Δ)/|Δ − V(V·Δ)| (as in the characters)
y    = 1 + sat(0.029·fovy·e)·flag·2(N·L)²
diff = (C·S.x·nl + prepass₀)·(1 − m)·y
out = S.z·(A·diff + spec + env) + Fog.rgb, out.a = bit0(a8)·S.z2
```

(The albedo is decoded by `×1.07321 − 0.000528` and encoded back—together identity.) **Direct light is a regular Lambert `N·L` without the** step (unlike the characters), glare is GGX with Schlick; relief stylization is in the light of the surroundings (saturation, SSAO, sky above) and the contoured brightening of `y` on silhouettes facing or away from light.

### Footage: `field_leaf` (G-buffer, PS 120, PS 108, PS 28)

Read 2026-09-28 (simplification - `tools/research/cemu_glsl_simplify.py`).

**G-buffer crown** (`Mt_Treeleaf_00/01`, `Obj_TreeBroadleaf_A_L`, `uking_mat` 8061, Cemu PS `ee44f9e2e062f0eb`): target 0 = (7/255 - `field_leaf`, `uk_user_data.z`, …, 1); `albedo.a = 2/255` — **bit 1 = 1** (below) `f₁`), metal 0, bit 0 = 0; `normal.w = 0` flag 0 (no SSAO and contour), gloss 0. Normal: base `normalize(Sem2 + Sem4)` (VS; probably normal vertex and deflection from the center of the crown) `uking_modify_normal_type` 1, VS not read) × TBN normal card (slot) `_s0` = `_n0` material, xy, `2.007874·t − 1.007874`Albedo is enhanced by translucent and glare:

```text
A′ = A·(1 + t)·Sem6.x (Sem6.x is the vertex multiplier)
t_c  = v3·sat((1 + V̂·N)² + 2·T_c − 1)·sat(−V̂·L̂)² + sat(1 + V̂·N)·v5·s^v4
s    = sat((N·−L̂)·(N·−Ĥ)·(1 − sat(v2 − v1·Sem7.z)))    Ĥ = normalize(V̂ + L̂)
```

`T` - texture slot `_n0` shader = `_z0` material (`Plant_TreeBroadleaf_A_Trs`; `uking_transmission_color` 402); `v1…v5` - `const_value1…5` (krona 0.005, −0.15, 10, 20, 30); V ⁇  - normalized `Sem7` (from the camera: `−V̂·L̂` is large when looking at the sun), `Sem7.z` - its z to normalization (-distance on the axis of view: the effect fades from 30 to 230 m); L ⁇  = `env4·r`.

**PS 120** (`preshading_shadow_field_leaf`, `9c0b7031078fba88`) → Expand (target 5): `x = sat(lerp(pcf, 1, sat(0.05(z − e53.z)))·depth_shadow_scale + depth_shadow_off)` – without the world shadow W4; the sampling point is shifted to `f₁`·1 m along N (and to `0.0791·(1 − (L·n)²)·nl` in smoothed normal, with the normal flag); `y` is retained; `z = w = 1`.

**PS 108** (`preshading_field_leaf`, `fb2e18ae56397ca7`): as PS 112, with differences:
- Shadow.x `= sat(proj0 + proj_shadow_off)·lerp(min(Ex, Ew), Ew, f₁)` —
  the crown (`f₁` = 1) **`Expand.w`**. PS 120 and PS 128 write there 1; if after them `w` does not change another passage (not found), the crowns do not take the shadow map at all, only `N·L`;
- Diffuse: cube map according to **`N + (0, 1, 0)`** (normal raised to the sky),
  LOD 3; **without SSAO** (`o` = 0); `vis`, saturation, `T[1]`, sky `T[2,3]` and shade `e1` – as in PS 112;
- Specular: Direct light `C·(S·nl + 0.075·f₁·(1 − S·nl))·sat(nl + 0.5 + 0.5vis)` (half 7.5% for foliage in the shade and back).

**PS 28** (main pass `field_leaf`, `3179b85d41bfb80d`) - PS 32 without `y` contour lighting, with:

```text
Dl   = (S·nl + 0.075·f₁·(1 − S·nl))·sat(nl + 0.5 + 0.5·vis)
diff = (C·Dl + prepass₀)·(1 − m)
spec = C·F_L·D·G/4·cloud_ratio·S.x·nl·r⁴·(1 − f₁/2)
env  = cube(reflect(V, N), LOD 3 − 3gl)·F_E·lerp(0.25(1 + m), 1, vis)·0.75·(1 − f₁/2)
out = S.z·(A·diff + spec + env) + Fog.rgb, out.a = bit0(a8)·S.z2
```

(`F_L`, `F_E`, `D`, `G`, α, kG – as in PS32; the crown gl = 0.)

### Grass and distant trees: G-buffer (`uking_grass`, `uking_tree`)

Read 2026-09-29 (Cemu) `20260928T012415Z-cache-replay`, simplification. `cemu_glsl_simplify.py`Grass and landscape trees have their own shader archives (`Bootup_Graphics.pack`: `uking_grass` models `uking_grass_blade` 12 programs, `uking_grass_cross` 8, `uking_grass_cover` 4; `uking_tree` 12); in each model, four `gsys_assign_type` The G-buffer option. `gsys_assign_gbuffer`Grass materials - model `TeraGrass` into `Pack/TitleBG.pack` → `Model/Terrain.sbfres` (**dump**, 2026-09-29; `model_info <pack> Model/Terrain.sbfres TeraGrass`): `Blade1`, `Blade2` (`uking_grass_blade_polygons=1`), `Cross1/2`, `Cover1/2`, `Translucent`. `uking_grass_lod_color` = (0.09, 0.15, 0.04, **0.7**oo `Blade1`, `w` = **0.9** bal `Blade2`, 1.0 for the bundles and cover; `const_value0` (gloss) - 0.4 for grasses, 0.3 for bundles; wind parameters (`uking_grass_wind_swell_freq_scale` 9, `_scale` 2, `_world_transform_coef` 0.022295, `_dispersion_scale` 0.005, `wind_detail_scale` 2, `_freq_scale` 2 blades of grass) -- also there. `TeraGrass` (`0x036ec9b8`).

| Model | G-buffer programs | Cemu PS (VS) | Class (target 0.x) |
|---|---|---|---|
| `uking_grass_blade` | 7 (`blade_polygons=1`), 11 | `6ba4f2eacc2b28f0` (`7f39f0470fc4928a`, `ef5f5b8b1d24ff15`) | 4 `field_hybrid` |
| `uking_grass_cross` | 3, 7 | `e42ea266ae42732c` (`9fb39f15a611f4ec`) | 4 |
| `uking_grass_cover` | 3 | `27505edff052f96b` (`e7bae847390e8d69`) | 4 |
| `uking_tree` | 7 (alpha test) | `4b7ade61b236275c` | 7 `field_leaf` |
| `uking_tree` | 11 (alpha test, `force_zprepass`, `uking_color*`) | `94b10225c6e29739` | 7 |

Program 3 blades of grass (with `uking_enable_normalmap`) and 3 trees in the picture are not.

**Grass** (PS `6ba4f2eacc2b28f0`): target 0 = (4/255), `uk_user_data.z` ×2, 1); `albedo.a = 0` (metal 0, `f₁` = 0 — **translucent**); `normal.w = floor(25.5·sat(u·(10 − 20v) − 3))·4/255` - flag 0 ()**SSAO free and contourless**), gloss up to 0.4 on one side of the top of the blade of grass (`u = attr.x/2`, `v = 1 − row/3`Albedo (layer) 1 at the root. `_a0` = `GrassAlb`, BC1 sRGB; `g` - His green:

```text
d    = summary1·mow_wide.z − G₀       G₀ = (0.0941, 0.204, 0.0314)
g′   = d.y < 0 ? lerp(g, 0.11, fade) : g
base = lerp(lerp(grass color, _a0, sat(10g′ + layer)), C far, fade)
                                      C_far = (d.y < 0 ? 0.081 : 0.09, 0.15, 0.04)
A = lerp(Sem0.w·(base + d·(1 − layer)·(6g′ − 0.06)), (0.021, 0.023, −0.079),
            10g′·(1 − mow_wide.w))
```

`G₀` regular color `.grass.extm` (24, 52, 8)/255: `grass_summary1` - the color of these grasses, raw 0-1. `Sem0.w = sat(4·mow_wide.z − 1)`. `fade = sat(uking_grass_lod_color.w − r)²`, `r` - linear decline in distance (`gsys_shape[3]`). `grass_color` - average color of landscape materials under the grass, map 47×47 3 m around the camera (see below)[below](#map-grass_color-the-color-of-materials-under-the-grass)Normal (VS): normal of the earth from `grass_summary0` (xy, y restored); `summary0.z·800` - height of the ground **plus 0.3·sat(round/3)·direction of grass** The grass is lit up almost like the ground beneath it. `GrassSpm` This PS doesn't read.

**Bundle** (`uking_grass_cross`, PS `e42ea266ae42732c`): class 4, metal 0, flag 0, gloss `floor(19.125·sat(1 − 2v))` (0.3 on the upper half); albedo only green and alpha textures`GrassCrossAlb`): `A = Sem2 + d′·sat(min(8g′ − 0.08, 0.8))`where `d′ = d` at `d.y < 0`: `(1.15d.x, 0.95d.y, d.z)`, `g′ = 0.09 + 0.1(g − 0.09)`), `Sem2 = lerp((0.021, 0.023, −0.079), (0.09, 0.15, 0.04)·(1 − layer)·sat(4·mow_wide.z − 1), mow_wide.w)` (VS; `d` here, too. `sat(4·mow_wide.z − 1)`- in uncut grass `(0.09, 0.15, 0.04)`same `C_far`It's just like the grass in the distance. `lerp(Sem3, Sem4.xyz, sat(min(Sem4.w, 1 − 2v′)))` c without normalization: curved normal of the earth and a semivector to the sun by gust of wind ([below](#bundle-vs-9fb39f15a611f4ec-shape-and-normal)) Cover (Cover)`cover`): class 4, albedo `Sem2 + 0.6·Sem0′`, normal from VS, gloss 0, flag 0.

**far-tree**Programme 7 (PS) `4b7ade61b236275c`): target 0 = (7/255 - `field_leaf`, `uk_user_data.z`albedo, alpha; albedo `Tree*Alb` as is; `albedo.a = 2·[NrmTrs.z > 0]/255` — **`f₁` = 1, where the blue `NrmTrs` zero**; normal -- `NrmTrs.xy` (`2.007874·t − 1.007874`,z restored) in the basis VS (`Sem3`, `Sem4`, their vector product; `normal.w = 0` (Gloss 0, flag 0) Alpha test `Sem1.x·(2a − 0.5 − sat(4a − 3)) + 0.07 + Sem5.x·(mask − 1)`- Mask. `TreeDitherMask`; a layer of perspective `round(Sem1.z·fract((Sem1.w − 0.25·mask)/Sem1.z) + Sem1.y)`Programme 11 (PS) `94b10225c6e29739`): same, but `f₁` = 1 always and the albedo is reinforced like the crown models: `A·(1 + v3·sat(sat(1 + V̂·N)² + v6)· sat(−V̂·L̂)² + sat(1 + V̂·N)·v5·s^v4)` (`s`, `v1…v6` = `const_value1…6` - like in G-buffer crowns, instead of `2T − 1` — `v6`What trees are drawn by Program 11 and its meanings `const_value` not installed.

### Far-tree: VS `616104995c681afa`, `gsys_user0/1` and tree buffer

Read 2026-09-30 (background agent, reading only): Cemu VS `616104995c681afa` - Program variant 7 `uking_tree` (the same microcode as the program 11) `616104995cc7da7a`but the export slot 2 `Sem5`not `Sem6`files `.code` Programs 7 and 11 in the archive coincide bytes; match uniform - `cemu_uniform_map`: `uf_remappedVS[0]` = `gsys_user1[0]`, `[1]` = `u0[2]`, `[2]` = `u0[1]`, `[3]` = `u0[4]`, `[4]` = `u0[3]`, `[5]` = `u0[5]`, `[6..8]` - View, `[9..12]` - projection. Ghidra. `U-King.rpx` EU v208 addresses below.

**Tops** (CPU, `FUN_035d0620` slot `+0x144` class `TeraTree`, vtable `0x102ec494`/`0x102ec604`/`0x102ec774`; `Terrain.sbfres` The shape has one top-stub: **5 20 bytes per tree, 3 triangles** (0.1.2), (2.1.3), (2.3.4) (descriptor) `0x102eb918` = {5, 3, 9}). `Sem0` unorm16 × 4: a place in the cage 1000 m`FUN_035d0524`: `int(frac·65536)`), `w` - shift distance `w16` (m) `Sem1` — `int((c + 1)·64)` The corner of the table, `z` = `int(2·width)`, `w` - first layer of the atlas; `Sem2.xy` - x and z of the rotated axis upward (inclination, output); `Sem8` — `int(width/height/3·256)`, `int(fract(rotY/360)·255)`, number of angles `N`, 255 (0 is hidden). Angles (x; y): `Tree0` usually (.3, .7, .1, .9, .5; 1, 1, .6, .6, -.1) – pentagon on the crown, at layers 0x53–0x55, 0x59 and 0x44, 0x5a, 0x62, 0x63 – their own (the latter is a full quad); `Tree1` (.4, .6, 0, 1, .5; 1, 1, .4, .4, -.1) Width and height - tables of species (`Tree0` 41 record `0x10472850`, `Tree1` 15 — `0x10472b84`: name, layer, `w`, `h`) x the scale of the actor; y `Tree1` ×1.3 and ×1.05; angles per layer `0x102eafc0`, `0x102eb150`For example, `Broadleaf_A_L`: layer 0, 18.164 x 13.535 m, 8 angles. `Stump`/`Trunk` — `FUN_035ccb7c`.

**VS** (`d1` - horizontal distance to `u0[1]`; `d̂` - from the `u0[2]` camera to the tree horizontally):

```text
see = [w16 < d1 ≤ u0[3].y]
f        = sat(max(u0[3].z·d1 − u0[3].w, 0) + sat(u0[4].y·(d1 − w16)))      → Sem1.x
k = 1 − u0[5].w·sat(2(1 − f))) the width is disclosed at f > 0.5
h = u0[5].y·σ·H·cy σ = Sem8.w (0 - hidden)
P′ = root + u0[5].x·σ·W·seen·cx·k·r ⁇  + (0, h, 0) + σ·h·(Sem2.x − 0.5, 0, Sem2.y − 0.5)
                                                            r̂ = (−d̂z, 0, d̂x); cx ≈ c.x − 0.5, cy ≈ c.y
P = P′ − 0.6·h·(2f − 1)·normalize(P′ − camera) shift along the beam
Sem0 = (0.5 + cx·k, 1 − cy, 0.6·u0[5].x·W·Sem0.x, 0.6·u0[5].y·H·Sem0.y) xy - uv, zw - mask
Sem1 = (f, first layer − 0.5, N, N·fract(rotY − φ + 1/(2N)) + 0.5 − Sem0.x)
Sem3, 4 = as: (−d ⁇ x, −0.01, −d ⁇ z) to the camera, (−d ⁇ z, 0.01, d ⁇ x) to the right
Sem5.x   = 0.1 + 0.1·u0[5].z·(1 − f)
```

`φ` is a tree-to-camera azimuth in rotations from +Z to +X (atan is the rational approximation of `u(1 + 0.4315797u²)/(1 + 0.7644395u² + 0.05831938u⁴)`). `−Sem0.x` shifts the angle one across the quad, and `0.25·mask` in PS - dithering: neighboring angles are mixed at the shift (output).

**`gsys_user0`** (`FUN_035cbe7c`of `FUN_03944a0c`; manager- `FUN_035ca918`): `[1]` - position of type`+0x240`; camera to output), `[2]` - camera position, `[3]` = (100, 4000, 0.001, 1), `[4]` = (1, 0.02, 8, 0.002), `[5]` = (1, 1, 16, 1) (`DAT_10472cd0…cdc`, `.data`Total: range of 4,000 m (`FUN_035ce910` lifts up to 8,000 m `max(DAT_1058d190/fov − 1, 0)` - probably zoom, `f` grows 50m after `w16` or from 1000 to 2000 m, `Sem5.x = 0.1 + 1.6(1 − f)`. **`gsys_user1[0]`** - cell angle 1000 m`FUN_035ca14c`: `(col·1000 − 5000, 0, row·1000 − 4000)`). `w16` LOD Tree Manager `FUN_033cad2c` (current distance - 50 m, it is the same or 65535) - not disassembled.

**Alpha test** (dump, `TeraTree` in `Model/Terrain.sbfres`): `Tree0` - 0.5, `Tree1` - 0.3; comparison `≥` - by Cemu. Samplers (words GX2 material): `a0`, `n0` - clamp, bilinearly, nearest mip; `tera_tree_mask` - repeat, bilinearly, nearest mip. Atlas Mips (8) and masks (6) - in `Terrain.Tex2`.

**Vuer** (2026-09-30, `far_trees.wgsl`): angle and alpha test PS 7 in the main passage of the formulas above, `f` = 1 (shift bands - its solvent of the viewer, SI-TRE-01), mask as is in the coordinates VS (`0.6·size·uv`), game mips. Not executed: form (5 vertices, tables of views instead of AABB, tilt, shift along the beam, width disclosure), `f < 1` and `w16`, range of 4000 / 8000 m, shadow to the light.

### Bundle: VS `9fb39f15a611f4ec` (shape and normal)

Read 2026-09-29: Cemu VS `9fb39f15a611f4ec` (`uking_grass_cross` program 7, `cemu_glsl_simplify.py … 1 7 … --stage VS`), PS `e42ea266ae42732c`; Ghidra `U-King.rpx` EU v208 (read only): writer `gsys_shape` `0x03704e10`, beam designer `0x037048f0` (grass manager `+0x28` object, `0x036fc028`).

**Entrances.** Top: `Sem0.xy` (snorm8) - a place in the cell, of which the world X, Z ()`gsys_shape_ex[0]`) and uv grass maps (`[1]`); `Sem1.xy` (unorm8) — `u`, `v` (`v` = 1 at the root, 0 at the top. `gsys_shape` (writer): 0 is the camera; 2 - `(T.0, tera+0x10fc·T.1, p[7], species+0x2a0)`3 - decline ([below](#decline-of-grass-blades-to-distant-color-gsys_shape3)); 4 — `(p[8]/p[9], 1/p[9], 0.3·tera+0x10fc·T.1, 0)`; 5 — **light-light** sky-object`KSys+0x854` → `+0x72c…0x734`From the sun: `cSunDir` The sky is the same vector with a minus sign. [sky-light](wiiu-sky-resources.md)), `L`Designer. `0x037048f0`: `p[7]` = 0, `p[8]` = `p[9]` = 20; in vtable class (see below)`0x1030a984`) only the destroyer, other writers `p[7]` Members 6-8 don't read this VS, writer `0x03704e10` Ghidra, 2026-10-01: 6 `(max, min, o+0x58·25·6π, 0)` swell`0x0366d4ac`, `0x0366d480`how `e58`), 7/8 — `o+0x3c…0x48`, turned on `±DAT_1047bf8c` (`0x02f06044`) and x 1/27 (as `e59`/`e60`where `o` — `mMgrs[5]` = `WindMgr` with the number of managers (`DAT_1047be88+0x45c`) > 5, or 0.

```text
n = (2·s0.x − 1, √(1 − sat(nx2 + nz2)), 2·s0.y − 1) normal earth (grass summary0)
m = (0.5 + mow.w/2)·mow.y·clamp(mow.x, 0.35, 1.35) mowing (grass mow wide)
f = s0.w·m·r3 r3 - Appearance/disappearance (member 3)
h = (1 − v)·f·T.1·tera vertex height
b   = lie′ + (e57.xy·a₁ + e57.zw·a₂)     aᵢ = 0.15g + swellᵢ·0.3·T.1·tera
P = root + h·normalize(n + 2h·(b.x, 0, b.z))
B = lerp(n + (1 − v)·(b.x, 0, b.z), L, 0.1g) → Sem3 (in form)
⁇  = normalize(−V ⁇  − L) V ⁇  - from camera to root → Sem4.xyz (in view)
g   = 0.8·sat(1.25·e32.x·s0.w·m·ρ²·w·sat(0.1d − 1.5))·sat(200 − 200|L.x|)  → Sem4.w
v′  = v·f                                                   → Sem1.y
```

`lie′ = (2·lie.x − 1, 1 − 2·lie.y)·(0.5 + mow.w/2)` quenchable `grass_lie` (`sat(5 − |10t − 5|)` by her uv) is a squinted grass; `swellᵢ` — `grass_wind_swell` at two points along the wind (`0.022295` — `wind_swell_world_transform_coef` ^ "Downward member". `−p[7]·(1 − v)·H/m` into `B.y` and `−2h·p[7]·H/m` directionally `p[7]` = 0 disappears. `ρ` The product of two running saws `lerp(fract(e59.xy·XZ − e58.w), fract(e59.zw·XZ − e58.w), e33.x)` and with `e60`), `w = lerp(s(a₁), s(a₂), e33.x)/2`, `s(a) = sin a + sin(5a/3)`, `a₁ = 6π·fract(0.0212766·(e32.yz·XZ)) − e58.z` (`a₂` s `e33.yz`): the gust runs through the field in spots. `e32`, `e33`, `e57…e60` wind `gsys_environment` ([below](#grass-winds-gsys_environment-32-33-5760-and-grass_wind_swell)).

**PS:** `N = lerp(Sem3, Sem4.xyz, sat(min(Sem4.w, 1 − 2v′)))`, written `N/2 + 0.5` without normalization; gloss `floor(19.125·sat(1 − 2v′))`; texture `(u, v′)`; alpha `Sem2.w·a`, `Sem2.w = min(1000·n.y − 600, 1)` - on slopes steeper `n.y ≈ 0.6` beams cut alpha test.

**Meaning.** At rest (no wind, `g` = 0, `grass_lie` neutral `B = n`and **normal beam - normal earth**The wind bends it to the wind more strongly the higher the point, and on the upper half of the beam there is a gust.`g` up to 0.8, further 15-25 m from the camera) pulls normal to the semivector `Ĥ` cameras and sunshine `N·Ĥ` 1 flash of glare on a running spot. The bundle grows along `n` (not vertically); when falling `f` height `v′` They shrink together — the top of the texture retains scale, the beam goes into the ground rather than contracting.

### Beam: `0x03709774` buffer and `0x03705700` drawing (density and shape)

Read 2026-09-30: Ghidra `U-King.rpx` EU v208 (Read only): Buffer generator `0x03709774`,drawing `0x03705700` (challenge next to grass-painting) `0x03703140`, `0x036febd8`), tile drawing `0x0370a1a4` quarters `0x0370a954` → `0x0370a2cc`; tables in `.data`.

**Types** (`0x1030ab78`, 12 bytes: primitive, beams on the side of `N`, width in tiles): type 0 - (4, 9, 0.2), type 1 - (5, 12, 0.4). Primitives (`0x1030ab94`, 12 bytes: vertices, triangles, indexes): 4 - 6/4/12, 5 - 3/1/3. `(u, v)` vertices (`0x1047bfd8`, 12 on float: six u, six v):

| Type | Tops of `(u, v)` | Triangles (indices writer) |
|---|---|---|
| 0 | (0, 0.48), (0.2, 0.11), (0.55, 0.08), (0.96, 0.57), (0.87, 1), (0.06, 1) | (0.1,5), (5,1,2), (5,2,4), (4,2,3) - hexagon along the contour of the beam |
| 1 | (0.25, 0.8), (0.5, 0.15), (0.75, 0.8) | (0.1,2) is a triangle in the middle of the texture, the base on v = 0.8 |

**Generator.** `sead::Random` (`0x030c48dc`) seeded `0x12345678` (`0x1047bfd4`) - once for both types, type 0, then type 1; beams - row `j`It's got a column. `i`, on a beam of three numbers `[0, 1)`: `x = (i + r₁)/N − 0.5`, `z = (j + r₂)/N − 0.5`turn `θ = 2π·r₃`Top (4 bytes):

```text
Sem0 (snorm8) = trunc(127.5·(x + w/2·cos θ·(1 − 2u))), trunc(127.5·(z + w/2·sin θ·(1 − 2u)))
Sem1 (unorm8) = trunc(255·u), trunc(255·v)
```

`FUN_04210d48` - `cos` (even polynomial by `|x|`), `FUN_04210514` - `sin`. That is, the beam - **one flat card** through the root `(x, z)` at an angle of `θ`, the width of the `w` tile (type 0 base - `u` from 0.06 to 0.87, type 1 - from 0.25 to 0.75); there are no cross quads. VS: place `= (Sem0 + 0.5)·gsys_shape_ex[0].xy + [0].zw`.

**Tyle.** Drawing bypasses tiles of grass (records at 0x14 bytes: corners) `x, z` how `short`boundary `+10`), the whole tile with one challenge, `N²` type bundles (`0x0370a1a4`:indices `N²·primitive`), or in quarters 5 m (`0x0370a954`4-bit mask - byte `+0xd` records, `0xf` - all four); second order of generator indexes sorts beams by quarter `[0, 0.5)²`Conclusion: Tile 10m, `gsys_shape_ex[0]` = (10, 10, angle of tile) (untraceable writer) 100 m2 total. **81 type 0 and type 1 beam 144**2 m and 4 m; the number of bundles does not depend on the height of the grass - it changes only the height in VS (see below).`f = s0.w·m·r₃`A quarter is drawn if the occurrence/disappearance multiplier at its point ≥ 0.3 ()`DAT_1047bf84`; at the edges of range, the whole tile - if the mask `0xf` And a tile within range.

**Not Recovered:** What is the mask of quarters (conclusion: is there grass) and the shift of tiles around the world (whether 10 m angles are multiples); the point at which the cut-off of a quarter by 0.3 (`FUN_03c6fd44`) is considered; what type is `Cross1`, which is `Cross2`.

**voir** (2026-09-30, `grass/buffer.rs::tuft_tile`, `grass/cards.rs`, `grass_cards.wgsl`): same generator and tables, 10m grid tiles of the world, both types in each tile; each top is on the ground in its place, with normal and grass (height, color) in the same place as the VS takes `grass_summary0/1` on top; a bundle whose grass is at all vertices 0 is not built (VS would put it on one line). `P = root + h·normalize(n + 2h·b)`, alpha test `Sem2.w·a ≥ 0.5` (`Sem2.w = min(1000·n.y − 600, 1)`The threshold - render state `Cross1/2` into `TeraGrass`Not executed: 0.3 quarters cut off; normal and height are the viewer net (SI-GRS-03) not `grass_summary0`; `cosf`/`sinf` Rust, not game polynomials (the difference is in rare cases of truncation of the byte).

### Grass winds: `gsys_environment` 32, 33, 57–60 and `grass_wind_swell`

Read 2026-09-29, Ghidra `U-King.rpx` EU v208 (read only), Cemu VS `7f39f0470fc4928a` blade blade blade and `9fb39f15a611f4ec` beam; climate values - dump (`WorldMgr/normal.bwinfo` in `Pack/TitleBG.pack`). Implementation - `grass/wind.rs`, `grass.wgsl`, `grass_cards.wgsl`.

Inventory 2026-10-01 (agent, not checked): not transferred and nowhere recorded - floor 0.1 m in `grass_cards.wgsl` (the essence of the discrepancy in the inventory is not specified).

**Writer.** The same environment updater `0x033ff8cc` (`pfVar15[k]` = e[25 + k/4]) takes the wind from `world::Manager` (`DAT_1047be88`) → `mMgrs[5]` (`+0x464`, index 5 = `WindMgr` (designer) `0x0366c2d0`frame `0x0366ccbc`):

```text
e32 = (s, cur.x, cur.z, clock/4800)       s = sat(str·15/12·fade)  (0x0366d420, DAT_1047be5c = 12)
e33 = (w, prev.x, prev.z, 1 − w)          w = turn²                 (DAT_1047be58 = 2)
e34/e35/e36 - Transfer uv cards grass lie / grass mow / grass mow wide (InteractMap manager)
e57 = (cur·(1 − w), prev·w)
e58 = (max[⌊63s⌋]/0.62, min[⌊63s⌋]/0.62, phase·25·6π, phase·30)
e59 = (R(+0.2)·cur/27, R(+0.2)·prev/27)   R(a) = [cos a, −sin a; sin a, cos a]  (0x02f06044)
e60 = (R(−0.2)·cur/27, R(−0.2)·prev/27)
e61 = (cur/47, prev/47) (program 7 does not read: 1/47 sewn in VS)
```

`WindMgr` fields, step - **frame of the game** (without `dt`, except for clocks and timers):

- `+0x18` speed: at flag 3 (constructor puts `+0x14` = 0x18)
  `(1 − 0.3·sat(−cos 3x·cos 5x·cos 7x))·V`, `x` (`+0x1c`) += 0.001 per frame (`DAT_1047be48`) - slow speed wave; `V` - wind of the world;
- `str` (`+0x54`) goes to `√sat(speed·fade/15)` (`DAT_1047be4c` = 15)
  1/300 per frame;
- direction `cur` (`+0x3c`) - normalized wind of the world; new
  is taken when `turn` (`+0x50`) = 0 or attenuation in 2: `prev ← cur`, `turn ← 1`, then `turn` -= 1/120 per frame (`DAT_1047be50`; 1/30 in manual wind);
- `phase` (`+0x58`) += 0.0001·str (`DAT_1047be60`), modulus 1;
- attenuation of `fade` (`+0x64`, `0x0366c80c`): once in (r + 2)·1800
  frames, if `V ≤ 10`, - a decline of 300 frames, `0x0367a120` (climate force multiplier shift, below), an increase of 60;
- `clock` - `+4` of the bell generator (`0x03940c04`): += dt (frames), modulus
  2·4800.

**The wind of the world** (`world::Manager`, Agent Report on Ghidra, 2026-09-29: `V = WindPower(climate)·m` (`0x03672fe8`; only in the field, `WindPower` Dump: 7.5 (HyrulePlain, Filone, SouthHateru), 10 (most), 5 (Eldin, KorogForest), 3 (LostWood); `m` - one for all climates, `0x036661d0`: `0.2 + 0.8r` before `FindDungeon_Activated` — `0.2 + 0.4666r`), at the beginning of 1.0 (Switch; Wii U not verified). Direction - type 0-6 (`0x03666158`, `getU32(7)`) angle `θ = −type·π/4`wind `(sin θ, cos θ)`: 0 → +Z (south), 2 → −X (west) Both are thrown once in the game hour (once in a game hour).`0x036662ec` from `0x03667d20`); angle catches up with target (`0x036783e0`on `clamp(|Δ|·(1 − 0.9^dt), 0.001·dt, 0.005·dt)` Over-frame, without ±π. Overdefinitions (former report: map edge, manual wind, DLC field) - fields `WorldMgr`The meaning of the flags is not restored (Ghidra, 2026-10-01):

- velocity `0x03672fe8`: 0 out of the field (`+0x530`  ⁇  1); in the field at `+0x651` and
  `+0x648` - `climate+0x20c` (`WindPower`) × `climate+0x230` (`m`); substitutions in order: `+0x638` = `+0x63c` = 1 → `+0x644`; `+0x64c` → `+0x5d0`; `+0x604`  ⁇  0 → `+0x5d4`;
- direction `0x03672ec8`: `(sin θ, 0, cos θ)`, `θ = +0x548[climate]`
  (climate - `0x036723a0`, index ≥ 20 → 0); with `+0x530` = 1 and `+0x638` = `+0x63c` = 1 - always `+0x548[0]`. `WindMgr` takes both through the wrappers `0x0366c678` and `0x0366ca9c` (normalizes); their branches with the argument  ⁇  0 (`0x036730bc`, `0x036731c0`) are not disassembled (hypothesis: manual wind);
- `0x036783e0` angle (from `0x03678c38`, cycle over 20 climates): type → vector
  0 (0, 0, 1), 1 (−1, 0, 1), 2 (−1, 0, 0), 3 (−1, 0, −1), 4 (0, 0, −1), 5 (1, 0, −1), 6 (1, 0, 0), 7 (1, 0, 1), −1 — `+0x59c…+0x5a4`; goal- `atan2(x, z)` The normalized type. `climate+0x22c` upon `+0x530` = 1 and ()`+0x651` or `+0x638` = `+0x63c` = 1); `+0x64c` → `+0x608`; `+0x604` ≠ 0 → `+0x600`The chase above is a common occurrence (`+0x604` < 1, `+0x64c` = 0; `dt` — `DAT_1047c258+0xc0`), otherwise in ±π with increments in `[0.1·dt, 0.2·dt]`; `+0x53c` 0 angle is equal to the goal;
- casts of `0x036662ec` (from weather `0x03667d20`, `0x03669704`)
  The saved hour `+0x368` is not equal to the current one) - only when `+0x530`  ⁇  3: type `(rand·7)>>32` (`0x03666158`) and `m` (`0x036661d0`; branch `0.2 + 0.4666r` - with `mMgrs[0]+0x12e` = 0: first throw `0.2 + 0.8r`, then the second, which is taken, - the generator advances twice; reread 2026-10-01) are written in all 20 climates (`+0x22c`, `+0x230`). `0x0367a120` from the attenuation of `WindMgr` (state 2) - wrapper: takes `mMgrs[3]` and reads `0x036661d0` .

**`grass_wind_swell`** is not the texture of the dump: 320 × 64 half float, built at start (`0x03940a78` → `0x039407b8`), values - `0x03940408` ( `DAT_1047d804…818` constants = 2, 3, 5, 7), translation into half - `0x039406d4` (mantiss cut off, denormalis → 0); by rows - min and max (`0x10595454`, `0x10595554`) for `e58.xy`:

```text
swell(u, v) = 0.3·[0.2333·a·(1 − a·cos 4πu·cos 6πu·cos 10πu) + b·(1 + 0.6·b·sin 6πu·sin 10πu·sin 14πu)]
a = sat(2·min(v, 1 − v)), b = sat(v − 0.3)/0.7 v = j/64 - wind force (e32.x)
```

Weak wind ripples, strong bloating. Sampler addressing mode not found (viewer: repeat on u, edge on v).

Generator (Ghidra, 2026-10-01): Designer `0x03940300` (from initialization) `WindMgr` `0x0366c53c`; object 0x27c bytes, `+0` and `+4` - The watch, `+8` = 4800.0, block `+0x268` = (65.5, 0.07, 0.062, 0.5) - no reader found; texture `0x03940a78` - agl-format 9 → GX2 `0x806` (`FLOAT_R16`Sample: `0x03940ccc` — `min[⌊63·s⌋]/0.61999977`, `0x03940d70` - same `max`, `s` trimmed from top 1, with `s` < 0 is line 0; their name is Getters `WindMgr` `0x0366d480` (min) and `0x0366d4ac` forcefully `0x0366d420`writer `0x033ff8cc` max `+0x210`min in `+0x214` (PPC `0x03400560..0x034006ac`- that `e58.xy` They're also called. `0x03940e14` — `(x − min)/(max − min)` (1 for a difference ≤ 0.01; `x` — `0x03940cb4`, not disassembled - and `0x03705290` (hypothesis: painting bundles) `0x03705700`; untested.

**Grass** (VS `7f39f0470fc4928a`, program 7; `r` - `Sem8.x`, random phase of the blade of grass; `H` - one-third of the length of the blade of grass):

```text
u_i  = 0.022295·(403.68·e32.w + 0.22427·r − e_i.yz·XZ)      e_1 = e32, e_2 = e33; v = e32.x
flutter = 0.057144·sat(e32.x − 0.3)·sin 4θ + 0.02·sat(2·min(e32.x, 1 − e32.x))·sin θ,  θ = 100π·(r + e32.w)
A_i  = flutter + 2·gsys_shape[2].y·swell_i                  shape[2].y = tera+0x10fc·T.2/3
W = e57.xy·A1 + e57.zw·A2 (x, z; meters)
base = (lie′ + 2·Sem1)·H/2 + (0, H·(max(1 − |lie′|², 0.2) − lerp(0.62, p[7], mow_wide.w)), 0)
P mid = root + 1.5H·normalize(base + mow wide.w·W), P tip = P mid + 1.5H·normalize(base + 1.5·mow wide.w·W)
N = n + 0.3·sat(order/3)·(P tip − P mid)
```

`403.68` = `wind_swell_freq_scale` 9 / 0.022295, `0.22427` = `_dispersion_scale` 0.005 / 0.022295, `2` = `_swell_scale` (parameters) `Blade1`/`Blade2`; `Cross1/2`: 9, 0, 1 - variance and no tremors. The grass is straight at rest (both segments along the way). `base`), the wind is bent more strongly on the upper segment. `p[7]` Series 1 and 2 are one point (middle), `H` contain `m·max(…)·(0.75 + Sem8.y/2)·s0.w·shape[2].y`; `max(…)` slash `|view.x| < 1.25` m closer to 7.5 m to the chamber: `max(1 − shape[2].w·(1 − sat(0.8·|view.x|)), 1 − sat((7.5 − d)/9))`, `view.x` - the root in line 0 of the matrix of the form (`gsys_shape[1]`), `d` - to the camera, `shape[2].w` = `species+0x2a0` (Not found by the writer) - no sermon is performed (as in the case of a `shape[2].w` = 0).

**Bundle** - formulae [higher](#bundle-vs-9fb39f15a611f4ec-shape-and-normal); `shape[4].z = 0.3·tera·T.1`There's no variance. `g` ⁇  0 only further than 15 m from the chamber, where the spots (`ρ²`) and wave 47 m (`w`) match, - the glare runs across the field of beams.

**Viewer** (`grass/wind.rs`): `WindMgr` and the wind of the world frame by frame (30 frames per second, whole frames), the swell generator by the same truncation at half; **starts in the installed** wind (force = goal, direction without mixing), and not as a game after loading; multiplier - the `FindDungeon_Activated` branch. Not executed: shortening at the camera, manual wind and the edge of the map, cover.

### Mowing and accepting: `grass_mow`, `grass_mow_wide`, `grass_lie`

Read 2026-09-29, Ghidra `U-King.rpx` EU v208 (read only, agent reports), shaders - `tera_grass.sharcb` (`Pack/TitleBG.pack` → `Terrain/System/tera_resource.Cafe_Cafe_GX2.release.ssarc`) and their GLSL from Cemu-shot `20260928T012415Z-cache-replay` (byte code match, `cemu_shader_matches.py`). Implementation - `grass/interact.rs`.

**Maps.** Manager `InteractMap` (constructor `0x035a1530`, init `0x035a25e0`) in the grass object (`FUN_036f3c04(tera)+0x18`), recording in the form of 0x6be8 bytes; center - camera (`FUN_035a4b04`, `−Rᵀt` matrix views):

| Map. | Size, format | Coverage | Centre | Cleanup |
|---|---|---|---|---|
| `grass_mow` (e35) | 2×2562 R8G8, in turn | 64 m, 0.25 m/texel | lens-box | (1, 1), edge is white |
| `grass_mow_wide` (e36) | 5122 RGBA8, ring, repeat | 512 m, 1 m/texel | step 8 m (`FUN_035a4500`) | (1, 1, 1, 1) |
| `grass_lie` (e34) | 256² R8G8 | 64 m, 0.25 m/texel | `floor(pos)` (`FUN_035a48b0`) | (0.5, 0.5) |

`uv = XZ·scale + offset`, `scale` = 1/64 and 1/512. Mixing - registers of `CB_BLEND_CONTROL` state blocks (`FUN_030c0928`/`0x030c0a80`), channels - target mask:

| Map. | Passage | Programme | Mixing, channels | Meaning. |
|---|---|---|---|---|
| mow | shift (`FUN_035a7454`) | `interact_copy` 0 | replacement, RG | past buffer with shift Δuv |
| mow | new lanes | `interact_reprint` | replacement, RG | RG wide |
| mow | Type 1/2 requests | `interact_cut` | MIN, R / G | `1 − I·(1 − |p|)` |
| mow, wide | whole-cage | `interact_cut_array` | MIN, G | 0 |
| wide | scrolling (`FUN_035a658c`) | quads | replacement | (1, 1, 1, 1) |
| wide | Type 1/2/4/8 requests | `interact_cut` (module centre) | MIN, R/G/B/A | `1 − I·(1 − |p|)` |
| lie (RGBA8 drive) | each frame (`FUN_035a6cb8`) | `interact_copy` 1 | replacement | `sat(±(2·lie − 1)·0.95^dt)` |
| lie (storage) | seeding | `interact_seed` | MAX | vector |
| lie | frame | `interact_merge` | replacement, RG | `0.5 + (v⁺ − v⁻)/2` |

Total VS of seeding and cut (`fc74d613ecb323f0`): `pos = aPos·cTexMtx.xy + cTexMtx.zw`, `p = 2·aPos` - quad ± 0.5, edge of disk `|p| = 1`Slice (slice)`ef7aa90512c20cc6`): `1 − cParam.x·(1 − |p|)` upon `cTexMtx = (r/32, r/32, Δx/32, −Δz/32)` — **disk-radius `r/2`**, completely cut down to `(1 − 1/I)· r/2`Radial seeding (sewed)`b9a3f51adde4b2b3`): `v = sat((1 − |p|)·cParam.x)· p̂`, directed instead of `p̂` `(dir.x, −dir.z)`, `cParam.x = 1/(1 − inner/r)`, `cTexMtx = (2r/32, …)` - radius `r`; square (body box, `682a45e345823e02`): from the center of the box outwards, completely within a meter of the edge of the quad (the box + 1.15 m on each side), extinguishes with height above the ground; the output - `(sat v.x, sat v.y, sat −v.x, sat −v.y)`There is no surge in shaders or CPUs: the cut disappears only when you leave the window of a wide map (256 m) or when you reset the manager.

**I can see that.** Grass: height × `mow.x·mow.y·(0.5 + wide.w/2)`; beam: × `wide.y·clamp(wide.x, 0.35, 1.35)·(0.5 + wide.w/2)` Sword cut (type 1, red) removes blades of grass, and leaves the bundles on **35 %**The bundles go only where the maps of the world hide the grass.`interact_cut_array`, below), or from a cut type 2. wide blue - fire (type 4, the color of scorched grass through `wide.z`), alpha - type 8 (half height and stubble color causing `FUN_0223dab0` Unidentifiable.

**`interact_cut_array`Where the grass is hidden forever** (read 2026-09-29, Ghidra EU v208; the old "cage mowed all" hypothesis is wrong). The mask of a grass cell 3 × 3 m - not slices: it's an OR three **static maps** (grass object designer) `0x035af354` Takes them from the card manager. `DAT_1046d688` by name, `0x032965b8`): `terrain_embedded_edge` (`grass+0x33f1c`, description in the file - "information about immersion in the landscape"), `terrain_is_in_door` (`+0x33f20`and `terrain_hidden` (`+0x33f24`"Invisibility information." Data -- `Game/Stats/archive/<quarter-quarter>.sstats` (format (archived reference)): u32 by 5 x 5 m, bit `5·(z − z₅) + (x − x₅)` Update of the grass net. `0x035b0330` in the loading states of the cell 0xb/0xc/0xd reads 3×3 bits of the cell (`0x035bd074`, the cell is whole meters of angle. `psVar60[0]`, `[2]`in `+0xc`, `+0xe`, `+0x10` Cell recording; when the cell is ready (counter) `+0x14` ≥ 15, flag `0x20000` Don't be, `0x03700a30` = 0), mask `(+0xc | +0xe | +0x10) & 0x1ff`:

- `0x1ff` is one point `{angle + 1.5, 3.0, 4.0}` (`0x102e9584` = 3.0)
  `DAT_1055802c` = 1.5),
- `{angle + (i, j) + 0.5, 1.0, 4.0}` point for each bit
  (`0x102e94c0` = 0.5, `0x102e94d8` = 1.0, `0x102e95f0` = 4.0).

The mask collects `0x035bd074` (Ghidra, 2026-10-01): a bit of `3·j + i` is a bit of `5·(z − z₅) + (x − x₅)` u32-reference, taken by `0x032980c8` at the `(x₅ + 0.5, 0, z₅ + 0.5)` point; the cell at the reference junction reads up to four (+5 m by x, by z, by both); no countdown is received - 0.

`0x036ffb40` → `0x035ad30c` point `{x, z, size, 4.0, 1, 0}` into narrow arrays (`+0x6b38`, `0x035ad214`and wide ()`+0xc8c`, `0x035ad304`) cards of each type record in turn (step 0x6be8); the record is not ready (`+0x14` bit 0 = 0, the point is not entirely inside its 64 m window`|x − c| < +0x10b0 − size/2`z, or the array is full, returns 0, and the cell waits. `+0xc` Manager – 1 without writing anything (Ghidra, 2026-10-01). `dd97826d286d859a`: `pos = ((x − cx)·f, −(z − cz)·f)`, `gl_PointSize = size·ctx.w`where `ctx.w = 256·f/2` = 4 pixels per meter at narrow (1 - wide): dot **square-close `size` meters** (Cemu has a dot size of diameter) `.w` Bottom line: in meters where any of the three bits is standing, G of both cards = 0 - no blades of grass or bundles (under stones, walls, houses) as soon as the 64 m window reaches them; wide remembers before leaving her window. Example: a stone on a plateau (-776, 2130) spot `terrain_hidden` ≈ 15 x 20 m (`cargo run -p the original format parser --example stats_mask -- <dump> -800 2150 40`).

**Sword** (`FUN_024ac534` → `FUN_024ab4ac` → `FUN_024aaab4`, each frame while the `weapon+0x99c` attack body is active; reread 2026-09-29):

- **Sensor** - `AtkPlayerBody` from `Weapon_Sword_001.bphysics`
  (keyframed, `ACTOR_MATRIX`, capsule z 0...1.5, r 0.15): position (`0x0348957c`) is the beginning of the weapon matrix, that is, the handle; blade axis (`0x03488a84`, column 2) is +z of the weapon. `AtkCollidableBoneName` from the past refers to the tissue reaction (`ClothReaction`), not to the sword.
- **`L`**: `0x0249fd34` (`Attack.Range` × `+0x998`) **overlaps** if
  The weapon has a main body.`+0xf4`; Switch `Actor::mMainBody` 0x190): `L` = the length of its shape along z in its own axes (`0x0348c6c4`: `shape→getAabb(identity)`) + 0.4 (bodies not of form; + 0.2 for one class `RigidBodyFromShape`, the capsule is half length + radius. `Weapon_Sword_001` body `Body` - polytope z −0.2505...0.849: `L` = 1.0995 + 0.4 ≈ 1.50 (the radius of the convex shape of Havok, if it enters the box, is not counted). `L ×= +0x948` (unidentifiable).
- **Slice Center**: Sensor; if `+0x938` weapons = 2 (vtable `+0x744`);
  1/2/4 — `+0x73c/0x744/0x74c`4 - thrown), + the axis of the holder's gaze (column 2 of its matrix `+0x200/0x210/0x220`) × 0.4 Player holder in state 1 `0x02d3535c` (`+0x8e4` bit 15 `+0x914` = 0): + blade axis x `L`, `y` = `y` holder, `r` = 2.7L; in states 1 and 2 - `I` State 2 is bit 15, `+0x914` ⁇  0 and virtual `*(+0xe8)+0x71c` No 0, no 0 (Ghidra 2026-10-01; meaning of bits and method not identified) Normal swing (0): `FUN_037004f0(r = 1.4L, I = 4, centre)`; a weapon with `+0x940` = 4 (`IsBlunt`) does not cut.
- **Height** (`FUN_037004f0`): Earth at point (`0x036965bc`), cut only
  at `land − 0.5 ≤ y ≤ land + 1.5`; `0x036fff00` ibid. is the effect of cut grass (rings of points `0x1047bf30` = 1.2 m × 6·k), not a map.
- **Approach**: `FUN_036ff498(r = 1.8L, ext. 0, centre + blade axis ×
  0.4)′ (at 1 to 3.7L and × 1.0; thrown at 3 m in the center of the box).

Walking is only an approximation: boxes of bodies touching the ground (`InteractMap_CollisionGround`, layers 0-5 and 12, `FUN_035a4170` → `FUN_035a393c` → `FUN_035a134c`).

**voir** (`grass/interact.rs`, `grass/hidden.rs`): cards – on the CPU with the same formulas, mixing and channels, are poured into textures when changing; storing the take through 8 bits each game frame (a small remnant of the bend remains - output from the format, not checked in the game). `interact_cut_array` - three maps of the world per meter, when a metre enters the entire 64 m window (cells, their counter readiness and range grid viewer does not repeat). `AttackSensor` a model of the sword in Link's hand`Weapon_R`, axis +z) + 0.4 m in view `L` = 1.4995 constant of `bphysics`. **Approximations:** 64 m windows go behind the camera with whole texels; the condition of the weapon 2 is taken for "in hand", `+0x948` - for 1; the viewer takes the ground under the cut at Link's feet (does not cut in the air); the sensor activity window is a timer of the viewer (`Sword::WINDUP`…`DURATION`), not AS games; tracks are only Link, box of capsule viewer (0.6 m); fire, type 2 and 8 and color of cut/scorched grass in the albedo is absent.

### Map `grass_color`: the color of materials under the grass

Read 2026-09-29, Ghidra `U-King.rpx` EU v208 (read only), Cemu VS `7f39f0470fc4928a` (`uking_grass_blade` program 7). Shader: VS transmits `uv = (X·e40.x + e40.z, Z·e40.y + e40.w)` over world X, Z blades of grass, PS reads `grass_color` (sampler PS 0) only in `base` above.

**Tethering** (kolback of blade-painting) `0x036fe9c8` → `0x03703140`): samplers are going fine `grass_wind_swell`, `grass_mow`, `grass_mow_wide`, `grass_lie`, `grass_color`, `grass_summary0/1`, `_a0` (registration of names) `0x036ec2e8`same indexes `type·8 + 10…17`); `grass_color` — `Y + 0x32a54 + 0x19c`where `Y` - object of grass (`**(X+0x10)`, `X` — `FUN_036f3c04(KSys-tera)`? `environment[40]` - 4 float at the address `Y + 0x32cd4 = S + 0x280` same-object `S = Y + 0x32a54` (writer `0x033ff8cc`, PPC `0x0340098c..0x034009a4`with flags `tera+0x944` bit 1 `X+4` bit 1.**Texture**(`0x03917ad8`, from `Y` `0x035af354`): 47×47, agl-format 0x1d → GX2 `0x1a` (**RGBA8 UNORM, not sRGB**; table `0x1047ed60`), CPU writes texels (`0x03917ce0`).**texel** (grass net update) `0x035b0330`, PPC `0x035b4f60..0x035b56ec`): grid cell - 3 m (`0x102e9584` = 3.0), the index of texel `(⌊(X − x₀)/3⌋ + 23 + shift) mod 47` (toroidal, same for Z). For the cell. `.mate` It reads at its point (`0x03698af8` → `0x036982a4`: materials - the nearest countdown, byte of the mixture - bilinearly, `(b + 0.5)/255`), the cell stores `+7` mat. `+8` mat1, `+9` mixture

```text
t = sat(((mixture - 1)/254) (254.0 - 0x102e960c)
texel = lerp(P[mat0], P[mat1], t) by by by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by-by, below.
P[m] = RGBA8 palettes of material m (0x036bdd88, float4 → bytes ×255)
```

`lerp` bybyte - `0x030bfba0(t, out, a, b)` (Ghidra, 2026-10-01): `t` is pinched in `[0, 1]`, `out[i] = (int)((float)(b[i] − a[i])·t + a[i])` is iconic difference, truncation to zero, not rounding. Material indices are limited to the number of materials (`R + 0x618`, `R = *(tera+0x2c0)+0x4a4c`).

**Palitra `P`** (`0x036e9290`One piece per frame: program `draw_texture_partial` from `tera_common.sharcb` (slot) `+0x23c`The same one that reads. `WaterAlb`, [analysis](wiiu-water-variants.md#intermediate-shader-reading-wateralb)) draws a layer `MaterialAlb` material`cTextureInfo.x` - a layer from the material table, `cTexMtx = (1, 1, 0, 0)`: all texture) in a small target of RGBA8, the CPU reads 4 bytes and stores `(b + 0.5)/255`Program variant and size of the target are not traced: `SPECIFY_MIP_LEVEL = 0` And the target in one pixel GPU takes the last mip, that is, **middle-colour** (linear: sRGB texture, UNORM target) That's inference, not verification.

Bottom line: `grass_color` - linear average color of the mixture of two landscape materials under the grass with a step of 3 m; at the dark root of the blade of grass (`10g′ < 1`It takes the color of the earth. The beginning of the grid (Ghidra, 2026-10-01): `x₀ = 3⌊X_cam/3⌋`, `z₀ = 3⌊Z_cam/3⌋` (camera view) `+0x258/+0x260`recording `0x035b0e9c/0x035b0e98`), `.mate` - in the center of the cell (angle + 1.5, `0x1055802c`), shift (`grass+0xdf80/+0xdf84`lead `0x035be6ac`: when you change the cell, the camera only drops the incoming rows. The world's cell texel is constant to the exact constant it takes. `e40`. **Not restored:** writer `e40` (probably, `1/141` and shift under the torus), sampler filter, as made by a mip of 1×1 layers (in the dump at `MaterialAlb` level 0 only.

### Decline of grass blades to distant color: `gsys_shape[3]`

Read 2026-09-29, Ghidra `U-King.rpx` EU v208. VS Grass (Cemu `7f39f0470fc4928a`): `d` is the distance from the root (height - ground `summary0.z·800`) to `gsys_shape[0].xyz`, `r = sat((s₃.x − d·s₃.y − 1)/(s₃.x − 1))`, `fade = sat(uking_grass_lod_color.w − r)²·(1 − gsys_shape_ex[3].y)`.

**Writer `gsys_shape`** — `0x03702c18` (by type and type) `t` blades, blocks on `0x2c`): member 0 - `species+0x258` (Camera Position), Member 1 - Matrix of View `+0x270`, member 2- `(p[8]·T[t].1, tera+0x10fc·0.3333·T[t].2, p[7], species+0x2a0)` (`p[8]` = 0.16, `p[7]` = 0.12 - Designer of class `0x037026c4`; he's betting. `p[0]` = 3, `p[9]` = `p[10]` = 1.0, `p[0xd]` = 0.1, `p[0xf]` = `p[0x10]` = 0.05, readers `p[10]`, `p[0xd…0x10]` Untraceable - Ghidra, 2026-10-01, member 3 - `(F/L, 1/L, 0, 10)`:

```text
k = max(tan(DAT_1047bf1c/2), tan(fovy/2))   DAT_1047bf1c = 20°
F = 1/(k·T[t].4),  L = max(F − 1/(k·T[t].3), 0.1)
r = sat(1 − d·k·T[t].3) (so far L > 0.1)
```

**`k`.** `k` = `0x03701f2c(species)` = `max(0x03701efc(), species+0x114)`where `0x03701efc` = `tanf(DAT_1047bf1c·2π/360·0.5)` = `tan 10°`Her name is originally both writers. `gsys_shape` (`0x03702c18`, `0x03704e10`; Ghidra, 2026-10-01). `species` - Record 0x2a4 bytes in appearance (`FUN_036fdfe0`, `param_1+0x10`s `+0x20` It is the geometry of the species (`FUN_03b27000` - The camera, `FUN_03b26b84` → `FUN_03b262b4` - projection; `+0x114` = her `+0xf4`which `0x03b262b4` write `tanf(fovy·0.5)` upon `fovy > 0` (same analysis) [water](wiiu-water-variants.md), `qF4`). `FUN_04210340` — `tanf`I mean, `k` - tangent half. **current** vertical angle of view, not less than `tan 10°`: The range of grass increases with narrowing of the view.

**Table `T`** - 2 lines of 5 float with **`0x1047bf54`** (code reads fields 1-4 as `bf58…bf64` + 20·t; earlier it was mistakenly the beginning of `bf58`):

| `t` | `.0` | `.1` | `.2` | `.3` | `.4` | `1/(tan25°·.3)` | `1/(tan25°·.4)` |
|---|---|---|---|---|---|---|---|
| 0 | 1.0 | 0.7 | 1.2 | 0.52305 | 0.17156 | 4.1 m | 12.5 m m |
| 1 | 2.0 | 0.9 | 1.2 | 0.16250 | 0.08578 | 13.2 m m | 25.0 m |

`.3`/`.4` pairs are the reverse “visible dimensions” of `1/(d·k)`: at `k = tan 25°` (fovy 50°) they give exactly 12.5 and 25 m – the table is designed for a reference angle of 50 ° (beams and cover below are multiplied by `0.46631 = tan 25°` directly in the code). There are no other writers in `bf54…bf7c` (xref only reading); `DAT_1047bf50` = **−1** (`.data`, not 0): a normal branch of `< 0`, and `≥ 0` is a debugging forced level of `1 << n`.

**Share of grasses of the cell** (`0x03703140`, type `t` = `param_3` < 2): `s = 1/(h/65535·1000·Y)`, `Y = grass+0x32edc` - it writes `0x035b0500` the same formula `max(tan 10°, species+0x114)`, i.e. `Y = k`; `h` - `u16` cells (`psVar21[8]`, meaning - its range to the camera in meters/1000; who writes, not checked). `f = sat((s − T.4)/(T.3 − T.4))·param_1[9]` Share (`param_1[9]` = 1.0 from the designer), `f = 0` - the cell is omitted, `FUN_03925f64` draws the fraction of the `f` type:

```text
d ≤ 1/(k·T.3) all blades of grass, drop to distant color: fade = sat(w − 1 + d·k·T.3)2
1/(k·T.3) < d < 1/(k·T.4) fade = w2, grass grass is getting smaller
d ≥ 1/(k·T.4) no grass grass
```

`w` = `uking_grass_lod_color.w`: 0.7 y `Blade1`0.9 y `Blade2` - the same numbers as `T[0].1`/`T[1].1`; by this coincidence, type 0- `Blade1`type 1 - `Blade2` (hypothesis; order of blocks) `0x2c` The type is not matched with the shapes. `TeraGrass` - stubs on 24 vertices: blades of grass are taken from the common peak buffer, `N²` per cage of 3 m (`N` = 30 for type 0, 16 for type 1; `psVar21+0x17` and 8 is the order number of indices, not the number of grasses ([below](#grass-vs-7f39f0470fc4928a-and-vertices-buffer-density-height-shape)).

**Beams** (`uking_grass_cross`, writer `0x03704e10`, 2 lines of 6 float with `0x1047bf90`, `c = 0.46631/k`): member 3 - `(F₁/L₁, 1/L₁, N₁/L₂, 1/L₂)`, `F₁ = .4·c`, `L₁ = .5·c`, `N₁ = .2·c`, `L₂ = .3·c`; VS (`9fb39f15a611f4ec`, program 7) multiplies the beam by `sat(s₃.x − d·s₃.y)·sat(d·s₃.w − s₃.z)` - linear appearance from `N₁` to `N₁ + L₂` and disappearance from `F₁ − L₁` to `F₁`:

| `t` | `.0` | `.1` (× `tera+0x10fc`) | emergence | disappearance |
|---|---|---|---|---|
| 0 | 0.2 | 1.56 | 8 → 15.5 m | 30.8 → 50.8 m |
| 1 | 0.4 | 1.44 | 14 → 22.5 m | 96 → 120 m |

**Pokroz** (`uking_grass_cover`, `0x03708118`, string `0x1047bfc4` = (0, 28, 122, 34)): disappearance of 88 → 122 m; appearance (field 0 = 0) and VS cover not disassembled.

**Not restored:** real `fovy` field camera BotW (50° - only the output from the tables; `resResourceActorCapture` silence is also 50); writer `h`; match table rows and `Blade1/2`, `Cross1/2`.

**Wewer: grass range** (2026-09-30, `grass/lod.rs`Adjustment. `--grass-reach X` / `grass_reach` into `renderer.toml` - not playing: all ranges are higher (grass grasses) `.3`/`.4` divide `X`, beams - meters × `X`) are pushed back into `X` They go with the chunk rings, and the bundles come in and the far-flung drop goes with them. `X` = 1 - the tables of the game as it is. `X²` The chankov.`uking_grass_cover`) not in the viewer, the multiplier range is not affected.

### Grass: VS `7f39f0470fc4928a` and vertices buffer (density, height, shape)

Read 2026-09-29: Cemu VS `7f39f0470fc4928a` (`uking_grass_blade` program 7, `cemu_glsl_simplify.py … 0 7 … --stage VS`); Ghidra `U-King.rpx` EU v208 (read only): `0x039255d4` buffer generator (via `0x039264dc` from landscape initialization `0x036deaa8`, PPC `0x036e17b0…0x036e1838`), creation of `0x036df3cc…0x036df484` blade blade objects ( `0x03926314` designer, `0x039264a0`), drawing of `0x03703140` cells → `0x03925f64`, `gsys_shape` writer `0x03702c18`.

**How many blades of grass.** Two objects, one per type (`0x036df3e4…`pairing `(N, primitive)` on the stack: type 0 - `N = 30`primitive 0; type 1 - `N = 16`, primitive 1. Cell - square 3 m (`0x03703140` cut her off `psVar21[0]…+3`, `psVar21[2]…+3` with a margin of 1.5 m. The buffer holds `N²` grass-block cell in 10 orders of indexes (`0x039255d4`0-7 – sorting by place `Sem0` along eight directions, 8- **reverse** order of generation: each blade of grass is inserted at the beginning of the list, `0x0308ec60` Insert by index; 9 - by quarters. `0x03925f64` painter `⌊N²·f⌋` grass-plastic `f > 0.99` - okay `psVar21+0x17`, otherwise in the order of 8, that is, the last generated first; a grid of 4×4 on the number of the blade of grass (below) makes any 16 in a row uniform across the cell. **900 grass blades of type 0 and 256 type 1 by 9 m2: 100 and 28.4 per m2** (≈128 per m2 together), further share `f` table-top `T` The number of blades of grass does not depend on the height of the grass: low grass is also thick, only shorter.

**Top of the mountain** (8 bytes, `0x039255d4`It's the same buffer for all cells. `sead::Random` (xorshift128: `0x030c48dc` - seed `0x1234567`, `x₁ = 0x6C078965·(s ^ s >> 30) + 1` etc.; `0x030c499c` - step); on a grass `i` six numbers in order: the senior 6 bits `Sem0.x`, → `Sem0.y`two `[0, 1)` Mantissa `r >> 9` into `[1, 2)` − 1) − 0.5 → `x, z`, `normalize(x, 1, z)` (`frsqrte` + Newton's step) × 128 with cut-off `Sem1` (read as snorm8, 127); two `[0, 1)` × 255 with cut-off `Sem8`Constants. `0x10339660…74` = 0.5, 128, 0, 255, 1, 3. World X, Z - `Sem0·gsys_shape_ex[0].xy + [0].zw` (Scale and start sets the CPU per cell; writer untested - conclusion: 3 m and angle of the cell):

| Bytes | Attribute | Meaning. |
|---|---|---|
| 0, 1 | `Sem0` unorm8 | place in the cell: `((i & 3)·64 + r₆)/255`, `(((i >> 2) & 3)·64 + r₆)/255` - randomly in a grid of 4×4 (`i` - the number of the blade of grass, `r₆` - 6 random bits) |
| 2, 3 | `Sem1` snorm8 | slope: horizontal `normalize(x, 1, z)`, `x, z`  ⁇  U(−0.5, 0.5); length up to 0.58, average ≈0.38 |
| 4, 5 | `Sem2` uint8 | `(u, row)` vertices from the `0x10339624` table |
| 6, 7 | `Sem8` unorm8 | two random numbers per blade of grass: wind phase, length multiplier |

Primitives (table `0x103395e8`: vertices, triangles, indices; `(u, row)` - `0x10339624 + 12·type`, `u` in bytes 0-5, row 6-11): 0 - 3/1/3, vertices (2, 0), (0, 2), (1, 3) - narrow triangle; 1 - 4/2/6, (1, 0), (0, 2), (2, 2), (1, 3) - rhombus, triangles (0, 1, 2), (2, 1, 3) (odd turns); the other primitives 2-4 grass does not use.

**Form** is the formula `base`, `P_mid`, `P_tip` in the [ wind section of ](#grass-winds-gsys_environment-32-33-5760-and-grass_wind_swell), with `H = m·near·(0.75 + Sem8.y/2)·w·shape[2].y`, `w = summary0.w`, `shape[2].y = S·0.3333·T.2 = 0.4·S`.

```text
side  = (−dir.z, dir.x)·(1 − w/2)·w·shape[2].x     dir = lie′ + 2·Sem1; shape[2].x = 0.16·T.1: 0.112 / 0.144
P = ... + (u − 1)·side; texture (u/2, 1 − row/3)
```

The length of the blade of grass is two straight segments along `1.5H`, total **`3H = 1.2·S·w·(0.75… 1.25)`**; at rest, the slope of the segments is 23° and 25° from the vertical (with an average length of `Sem1`), the height is 0.9 length. The width is proportional to the slope (`side` is not normalized) and `w(1 − w/2)`: on average ≈ 0.09 m for type 0 and 0.11 m for type 1 at `w = 1`.

**`S = tera+0x10fc` = 1.0**: the only entry is the constructor `0x036fbdd4` (`stfs` constants `0x1030a44c` = 1.0; neighboring fields `+0x10f4`, `+0x10f8`, `+0x1100` = 0, next to the line "grass debug msg"). Other readings are multipliers of grass, bundles, cover.

**Data.** `.grass.extm` in the field (Wii U 1.5.0), `probe_grass`): average byte height of 124-224 on tiles y `(-800, 2150)`, `(-300, -44)`, `(-500, 800)`, `(-900, 1800)`; often 200 to 255. `w = 0.9` The length of the blade of grass of the game is 0.81-1.35 m (average 1.08 m).

**voir** (2026-09-29, `grass.rs`, `grass/buffer.rs`, `grass.wgsl`; bundles -- `grass/cards.rs`, `grass_cards.wgsl`): same generator and cell pattern for both types, cells - on a grid of the world 3 m (section 15 m), the top carries the root on the ground, `Sem1`, `Sem8`, `(u/2, 1 − row/3)`, number in order 8 and middle of the cell; VS builds a blade of grass according to formulas above, fraction `f` - the distance from the middle of the cell at the height of the root (conclusion: writer) `h` Type material; type 0 areas are only visible at range. `T.1·S`share `f = w·m·r₃` cuts and height, and `v` textures; layout and shape - buffer game (from 2026-09-30, [higher](#beam-0x03709774-buffer-and-0x03705700-drawing-density-and-shape)Not executed: shortening at the camera; `Sem1` count `1/√`not `frsqrte` Before/after comparisons — cameras `grass_low`, `field_front_1400`, `field_west_1845`, `forest_in_e` (average difference of 6.87, worst 11.61): blades of grass in all directions, thicker and higher, FPS is not lower than before.

**Not restored:** what `summary0.w` - byte of height `.grass.extm`/255 (conclusion): `summary1` - the raw color of the same record, `G₀` = normal color (24, 52, 8)/255; writer `summary0/1` Not found -- it's textures. `+0x29c…+0x18c` object of the species, probably drawn on the GPU; type `Blade1/2` (coincidentally) `T.1` and `lod_color.w`The density and shape of the beams. [buffer](#beam-0x03709774-buffer-and-0x03705700-drawing-density-and-shape) (`0x03925f64` He only paints grass.

### Map of the sky from above: `gsys_depth_shadow_quarter` and `e39` (`vis`)

Read 2026-09-29, Ghidra `U-King.rpx` EU v208 (read only) and shaders `uking_pass_shader` It's 470-473. **not** static shadow map of gsys (`gsysStaticDepthShadow`, `FUN_039dd000`: separate dispersion map `static_shadow_depth` → `static_shadow_variance` his `light_name`and the object KSys (`KSys+0x858`constructor `FUN_033f97f4`, setting `FUN_033fa684` from `FUN_03405f48`) — “sky occlusion”: shaders `skyocl_copy`, `skyocl_depthfill`, `skyocl_normalize`.

**Binding.** `KSYS_BindUserTextureSampler` index 6 (sampler `gsys_depth_shadow_quarter`, [ table ](wiiu-deferred-shading.md)) — `local_228` update `0x033ff8cc`: texture `+0x480 + i·0x17c` object (`i` = `+0xde0`, double buffer) with the flag `+0xe0c` bit 1, or `White2D` (DSQ = 1 → `vis` = 1). The same update writes `environment[39] = (x₀/W, z₀/W, 1/W, ±H)`, `x₀ = cx − W/2`, `z₀ = cz − W/2` (`pfVar15[0x38..0x3b]`); `w` negative if the map is turned off.

**Settings** (`FUN_03405f48` → `FUN_033fa684`, numbers in the code):

| Field. | Meaning. | Meaning. |
|---|---|---|
| `+0x780` W | 192 m m | sideways |
| `+0x778` | 192 | full card texals (1 m) |
| `+0x784` | 4 | center-shift step, teksel |
| `+0x788` H | 1000 | altitude range, `e39.w` |
| `+0x78c` | 800 | height of relief at code 1 (below) |
| `+0x790` | 1 | near-chamber |
| `+0x794`, `+0x798` | 5, 1 | nucleus and number of passes of Gaussian blur |

**Centre** (`FUN_033fb2b4`, `FUN_033fa9d0`): position from the facility `KSys+0x434` (+0x60, +0x68; meaning - camera, not checked), broken into W tiles and tied inside the tiles to step 4 texel (4 m).`FUN_033fafac`, `FUN_033fac64`, up to 10 rectangles.

**Painting** (`FUN_033fc3d4`): orthocamera above centre at height `near + H` = 1001, looking down (see below)`FUN_033fc1c8`), near 1, far 1001, ±W/2 - depth `d = 1 − Y/1000`First. **relief**If there is a slab of relief covering the area (`FUN_03687b84`), `skyocl_depthfill` (PS 471) writes in **depth** (Z export) `1 − h·cNormalizeParam.x + .y` from the array of textures of the heights of the relief (`h` - normalized height 0...1  ⁇  0...800 m, where `cNormalizeParam = (800/1000, +0xdec/1000 = 0)`I mean, the same `1 − Y/1000`; without relief, the depth is cleared to 1 (Y = 0). `Model(ZOnly/Opa+AlphaMask)` bat-beat `FUN_033fb9bc` - only shapes, the material of which render info **`uking_edit_sky_occlusion` ≠ 0** (shape flag beat 1 in) `FUN_033eb704`) and render mode 0/1 (opaque or alpha test; matching with BFRES mode is not checked); for models with a flag (+10 bits 1) - even at least the size threshold `+0x34` (value not found). `skyocl_normalize` (PS 473): `sat((1 − d)·P.x + P.y)` P not found; PS 112 to be high `e39.w·(1 − DSQ)`code stores `DSQ = 1 − Y/1000` (also spelled relief) Bottom line`FUN_033fd248`): 192² → 96² (`+0x480`, "quarter" - by number of texels), Gaussian blur horizontally and vertically (agle, variant of core 5), sampler texture `CLAMP_BORDER` colored (1, 1, 1) (sead) `cWhite`, `0x1027b310`): **out-of-field**.

**Composition** (`material_census`, render info, v208): `uking_edit_sky_occlusion` = 1 for 9,870 materials (FldObj, TwnObj, DgnObj, Obj; almost none for characters, weapons, animals), = 0 for 9,308. `Obj_TreeBroadleaf_A_L` trunk and stump = 1, **kroner = 0**: leaves do not fall into the map; the giants `Obj_TreeBroadleaf_A_LL`, `_LL_02`, `_LL_Nest_01` crowns`Mt_Treeleaf_*`) = 1 - their foliage card closes (camera `foliage/forest_in_e` These are 13-35 meters: `_LL`, `_LL_Set_01`).

The meaning for `vis` (PS 112): `e39.w·(1 − DSQ)` is the world height of the top relief / shading models above the point (blurred ~2-4 m). On flat open ground `u = 0.075` (flag 1) → `vis` ≈ 0.79 within 70 m, grass and foliage (flag 0) at ground level `u = 0.325` → `vis` ≈ 0.31; then 70-90 m `vis` → 1 (`sat(0.05z − 3.5)`).

### `LightAnalyzer`: field texes (`gsys_user4` 1–3)

Step 4 (`uking_pass_shader` 487, Cemu `728b9bc3556c3de1`); uniforms by manifest `light_analyzer` (order vec4): `cLook`, `cMainLightColor`, (`cAnalyzeScale`, `cCharaAmbientGraY`, `…GraYOffset`, `cCharaAmbientSat`), (`…SatMin`, `cFieldAmbientSat`, `…SatMin`, `cEffectAmbientSat`), (`cAmbientOffsetCubeMap`, `…Far`, `…CharaMin`, `…CharaMax`), (`…CharaScale`, `…FieldMin`, `…FieldMax`, `…FieldScale`), `cAmbientScaleChara`, (`cLightPrePassThreshold`, `…MinAmbientIntensity`, `…AmbientIntensityOffset`, `cAmbientMasterIntensity`), `cForceShadowRatio`):

```text
T[1]   = (Master·field_scale/(avg + lerp(field_min, field_max, e)),
          field_ambient_sat, lerp(field_ambient_sat_min, 1, e))
T[2,3] = cube((0, ±1, 0), LOD 3)·Master/(avg + ambient_offset_far)
T[6,7] = avg·Master/(avg + ambient_offset_cubemap)
```

`e`, `avg`, `Master` – like [ characters ](wiiu-character-shading.md#the-light-of-the-environment-lightanalyzer--gsys_user4--sem2); `ksysla` omissions: `field_ambient_sat` 0.5, `_min` 0.8, `ambient_offset_field_*` 0.1 / 0.5 / 1.0, `ambient_offset_far` 1.0.

## Reproduction and limitations

```sh
REF=/absolute/game-data/reference/visual-formulas
S=$REF/cemu-sessions/20260928T012415Z-cache-replay/shaders
python3 tools/research/cemu_uniform_map.py $REF/shader-archives/uking_sys/model001-program00032-ps.code $S/8d24f32f18e6de47_0000000079249749_ps.txt
python3 tools/research/cemu_uniform_map.py $REF/shader-archives/uking_sys/model001-program00112-ps.code $S/bec68ec6f40a864f_00fffff249259249_ps.txt
```

Readable GLSL (literals, works, uniforms and samplers from manifest; for PS 120 and PS 128 - without `--code`: Cemu left direct block indexes):

```sh
python3 tools/research/cemu_glsl_simplify.py $REF/shader-archives/uking_sys/manifest.txt 1 28 \
  $S/3179b85d41bfb80d_0000000079249749_ps.txt --code $REF/shader-archives/uking_sys/model001-program00028-ps.code
```

G-buffer object: `ALL_OPTIONS=1 model_info` model → `material_variants.py` (as in [ variants of materials ](wiiu-material-variants.md)) gives a family of programs; in it G-buffer - programs with `gsys_assign_type` = `gsys_assign_gbuffer` (`key[51]`, bits 26-27 = 3); their PS - in `results` `matches.json`, uniforms - `cemu_uniform_map.py` (bank 8 = `gsys_material`, uniform offset - bytes 8-9 descriptor minus 1).

PS 32 uniforms map: u0/u1/u2 - context[16]/`sampler_locations`4]/[17], u3/u8 — environment[4]/[5], u4–u6 — context[13]/[11]/[12], u7/u9 — scene_material[2]/[1]PS 112: e43–e45 (back view), e26–e30 (fog), e39 (top map), e1 (hue) `user2`), ctx35-37 (projection of cloud shadow). `sampler_locations` programs in manifest.

Verified: Program options and samplers (manifest), reading GLSL, texture `ssao` (temporary example of `level0_slice`). No ALUs were executed.
- What's in `gsys_light_prepass` during the main pass?
  The output of the Specular PS 112 (so with G-buffer water), the PS 32 would add a fully illuminated color to its own - twice; more likely, the `LightPrePass` main aisle (local sources) replaces layer 1 with its glare, and layer 0 - Diffuse + local diffuse.
- `e53.zw` (Shadow and SSAO range, world shadow), `e1`/`gsys_user2`,
  `cloud_ratio`. `e39` and `gsys_depth_shadow_quarter` - [ section map of the sky above ](#map-of-the-sky-from-above-gsys_depth_shadow_quarter-and-e39-vis); there are no `cNormalizeParam` models, model size threshold, the core of Gaussian blur and whose position is the center (camera or player);
- G-buffer grasses and distant trees read (section above); not read by VS
  normal beams, `grass_color` and 7/11 program selection for trees
- Normal relief in G-buffer `uking_terrain` 9–11 (PS `583ea8604da62310`):
  only the packaging is read; maps of normal materials, their mixture according to `.mate` and the projection on the slopes are not disassembled;
- relief gloss formula from `MaterialCmb.b` (`uking_terrain`); metal
  objects with metal maps (`uking_metal_color`  ⁇  0 for read five is not written - which materials are written, not checked);
- Color combinator (`uking_colorN_calc_type`, sources 5xx) – formulas;
- Who writes `Expand.w` after PS 120/128 (this depends on whether you take the crowns).
  shadow map) and VS deflection of the normal crown (`Sem2 + Sem4`);
- Noise sampling (point or linear) and pre-shading target sizes.

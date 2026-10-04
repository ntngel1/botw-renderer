# Wii U: Delayed Material Lighting (`uking_sys_shading`)

Date: 2026-09-28 Subsystem: render, water. Related gap: RENDER-002 (archived reference). Context: [Water](wiiu-water-variants.md).

## Question and sources

- What makes G-buffer water (material id 22, albedo, normal, gloss)
  `L·(1−a)`) in the final color of the frame.
- Dump: Wii U update v208, `Pack/Bootup_Graphics.pack`
  `Shader/uking_sys.product.sbfsha` (extracted from `reference/visual-formulas/ shader-archives/uking_sys`) and `Model/SystemModel.sbfres` (extracted from `reference/visual-formulas/shading/`).
- Cemu: `20260928T012415Z-cache-replay` image, exact bytes matched
  `tools/research/cemu_shader_matches.py`.
- Ghidra: `U-King.rpx` (EU v208), reading only. line
  There is no `uking_deferred_shading_type` in the RPX; there are `gsys_deferred_shading_pass` (`0x10353a90`) and `gsys_deferred_shading_material` (`0x10353aac`).

## The result

**Facts dump.** BFSHA `uking_sys`, model 1 `uking_sys_shading`148 programs, dynamic options include `uking_deferred_pre_shading`, `uking_deferred_shading_type` (0–11), `gsys_gbuffer_xlu`, `gsys_assign_variation`Programmes 104-147 `pre_shading=1`, `gbuffer_xlu=1`Types 7-11. **144 Of the 148 programs in the Cemu image** `20260928T012415Z-cache-replay` (`matches.json`, 2026-09-28).

The lighting materials are in the `SystemModel.sbfres`, the `DeferredMain` model (22 materials, `uking_sys / uking_sys_shading` shader), each setting a `uking_deferred_compare_material_id`.

| Materials | compare id | shading_type | pre / xlu |
|---|---:|---:|---|
| `preshading_field_water` | 22 | 11 | 1 / 1 |
| `preshading_field_xlu` | 20 | 11 | 1 / 1 |
| `field_water` | 6 | 0 | 0 / 0 |

Id 22 matches `m = 22/255` in output0 G-buffer PS water. `preshading_field_water` options are the same as **140–143** programs (except `gsys_renderstate`/`gsys_alpha_test_func` is the same native substitution 0→3 and 0→6 as TeraWater); they differ only from `gsys_assign_variation` 0–3. Cemu-cache: 140/142/143 → PS `59cba7eb9a9c1df6` variation, 141 → PS `28e6a2be507943e4`.

PS-binding 140-143: `uking_tex0` 0, `gsys_gbuffer_normal` 2, `gsys_projection0` 4, `gsys_half_normalized_linear_depth` 5, `gsys_depth_shadow_cascade` 7, `gsys_user0` 10, `gsys_depth_shadow_quarter` 11, `gsys_user2` 14, `gsys_dynamic_reflection` 15; context, environment, scene material, material blocks. **Albedo G-buffer This passage doesn't read**, `gsys_light_prepass` Two outputs; output1 in matching GLSL is built from `gsys_environment` [13], [16], [30] output0 is from shadows/reflections, meaning outputs and their targets are not restored.

**The main passage does not illuminate the water.** Materials `DeferredMain` without pre-shading (type 0, etc.) compare id 4-14; id 22 is not among them. their PS (for example, program 0, Cemu) `7c6e02daf538f23e`) read albedo, normal, depth, `gsys_light_prepass`Static shadows; VS reads all programs `gsys_user4` Type classification of tiles by id (not checked).

**Dependences of PS 140 outputs** (`tools/research/cemu_glsl_deps.py`All branches are considered executed: output1 `uking_tex0` (PS0), `gsys_user0` (PS10), `gsys_user2` (PS14), depth (PS5), `environment[13]`, `[16]`, `[30]` and ~30 uniform component; **from `gsys_gbuffer_normal` depends**. output0 - Cascading and Quarter Shadows (PS7, PS11) `gsys_projection0` (PS4), `gsys_dynamic_reflection` (PS15), same `uking_tex0`/`user2`Which CPU textures are tied to `gsys_user0/user2` and `uking_tex0`The name shader-sampler is not called runtime resource.

**Next up the shot.** The cache has all 14 steps. `uking_pass_shader` `preshading_filter` (STEP 0–13; samplers) `cEffect`And STEP 7 has -- `cDiffuse` and `cMaterialId`STEP 9/10 has -- `cDiffuse`and `hdr_compose`Their order and relationship to pre-shading targets have not been established.

**CPU: textures `gsys_user*` (matching RPX, statically).**KSys update `0x033ff8cc` Each frame collects 10 textures and transmits them. `KSYS_SetViewUserTexture` (`0x038bf5e8`, object KSys+0x844, 0x27 words on view. `KSYS_BindUserTextureSampler` (`0x038bf714`) subtracts them to the sampler table `0x10331fec` = `[23,24,25,26,27,28,3,2,13,16]`Sampler names continue the block list. `0x1047daac` (Blocks 0-14: invalid...) `gsys_user0..2`, `gsys_material_ffl`) and begins with `0x1047dae8`; hence the KSys indexes 0-5 `gsys_user0..5`, 6 → `gsys_depth_shadow_quarter`, 7 → `gsys_depth_shadow_half`, 8 → `gsys_dynamic_reflection`, 9 → `gsys_sssss`The start of the sampler list is derived from the table neighborhood and aligned with PS anchors; a separate reader of this list is not traced. Index 0 source (Source: WEB )`gsys_user0`The output1 PS 140 is the object KSys+0x854 (`0x033f164c`It is created with the loading. `System/KSys/sky.skybin`), +0x670+4, with flags +0x88c bit2 and (bit5|bit6).Indices 8/9 array - textures of materials `preshading_shadow_clear`/`preshading_field` model `DeferredMain` (KSys+0xb70) found by name in the same update.**CPU: Pre-shading goals.**`GBUFFER_AllocatePreShadingTargets` (`0x03978418`) creates six targets on the label table `0x1047db9c`Diffuse (the only one with two layers) `FUN_03ac487c(...,2,...)`, or the outer surface , Specular (total surface GBuffer+0x2774), Shadow, Fog, Upscale, Expand (the latter only if you have GBuffer+0x2e48), each color slot is taken from a byte of the descriptor (+0x170). `gsys_light_prepass` (two-layer array, layer 1 reads G-buffer PS water), not installed.**CPU: bindings in the GBuffer Xlu aisle.**`GBUFFER_HandlePreShadingXluPassEvent` (`0x039ab208`), event 3, before `GBufferXlu(Invalidate)` trigger `GSYS_ReplaceViewSamplerTexture` (`0x0399c258`, table of samplers view + 0x10: `gsys_light_prepass` ← Goal 0 `PreShading(Diffuse)` (GBuffer+0x26cc, double-layer), `gsys_static_depth_shadow` ← Target 2 Shadow (+0x29cc) `gsys_normal_modify` ← Target 3 Fog (+0x2b4c) `gsys_projection1` ← Target 4 Upscale (+0x2ccc) `gsys_sssss` ← Expand (+0x2e4c) Target shifts – GBuffer + 0x26c8 array, step 0x180, texture by +4 (from `GBUFFER_AllocatePreShadingTargets`So, then `L` G-buffer PS water - layer 1 of the result of pre-shading an opaque scene, not a raw copy of the frame. Event 2 of the same handler highlights pre-shadying goals.**Id of material = flag Xlu | class (cache observation).**constants `n/255` in Cemu PS G-buffer: 4 (relief, etc.), 7 (folios), 8 and 10–14 (characters), 20,**22**(water and 15 PS) `uking_mat`Id 6 does not write any PS, although it is compared to the main material. `field_water`.Agrees with 22 = 16 | 6, 20 = 16 | 4, 24 = 16 | 8: bit 16 - Xlu, junior - class; separate masking code is not traced.** Final PS `field_water`.** The options of the `field_water` material (id 6, type 0) are the same as the programs 24–27 (Cemu: 24/27 → `2e2543216c04766d`, 25 → `07d4d8363600f2d4`, 26 → `9fbeb9714fd898d1`). Read in the GLSL program 24 (without native verification), bindings – by manifest and event 3:

```text
z = depth·(far−near)+near; P = position in view; N = 2·normal.xyz−1
albedo = saturate(1.07321·gbufferAlbedo − 0.000528) (inverse 0.93179x+0.000492)
up-uv = depth-aware selection for gather half-depth (PS6) and Upscale (PS7)
E = cube (rotating N via uf[6..8], LOD 3) - irradiance
R = reflected view corrected to the sphere |P|+200 around uf[3]
S = cube (rotation R, LOD 3−3·normal.w) - reflection
F = f + 0.02·(1−f) (f from angle and normal.w)
color = albedo·(sun·N·L·shadow + E·(1−F)) + S·F + solar glare + prepass1
out    = fog.rgb + fogT·color      (fog = Fog.rgb @ up-uv, fogT = Shadow.z)
```

`prepass₁` is a layer 1 `gsys_light_prepass` (Diffuse) in up-uv. Bit 0 albedo.a·255 switches a branch of glare. Bottom line for water: albedo ≈ 0.02 dark thickness gives little; turquoise remains a reflection of the sky `S·F` and `prepass₁`, where, hypothesized, writes output5 G-buffer water (`L·(1−a)`) and/or pre-shading water ( `environment[13/16/30]` environment light and `gsys_user0`). What outputs to which layers/targets go is not established.

**CPU: Target slots (initiation, matching RPX).** GBuffer is created in the `GSYS_CreateViewGBufferRecords` (`0x03a058a8`object 0x3150 on view, `GBUFFER_InitializeInstance` `0x0397625c`). `GBUFFER_InitializeTargetDescriptors` (`0x03976850`) specifies the descriptors (+0 internal agl format, +0x170 color slot, +0x171 second slot, +0x172 bit0 cleaning):

| G-buffer (`GBuffer+0x1260`) | format | sloth | second | | Pre-shading (`+0x1e10`) | format | sloth |
|---|---|---|---|---|---|---|---|
| Albedo | 0x1d | 1 | 2 | | Diffuse | 0x1a | 2 |
| Normal | 0x22 | 3 | 4 | | Specular | 0x1a | 3 |
| MaterialID | 1 | 0 | - (cleansed) | | Shadow | 0x1d | 0 |
| Emission | 0x1a | 5 | — | | Fog | 0x1a | 1 |
| Shadow | 10 | 7 | — | | Upscale | 1 | 4 |
| | | | | | Expand | 0x1d | 5 |

`GBUFFER_SetupTargetsForView` (`0x03976c08`) puts targets on slots and uses the GBuffer+0x19d0 surface for Emission, a scene color buffer, rather than a separate texture.

- Outputs 0/1/3/5 G-buffer PS water - id, albedo, normal and **buffer color**:
  `L·(1−a)` is written directly into the color of the frame as the “emission” of a water pixel.
- 0/1 pre-shading (PS 140) - targets **Shadow** and **Fog**:
  The "environmental light" from `environment[13/16/30]` and the `gsys_user0` sky is an in-scatter fog that the final `field_water` takes as `fog.rgb` and the transmission as `Shadow.z`.
- 2 pre-shading relief output (Diffuse, i.e. `gsys_light_prepass`)
  It depends on the albedo: the light prepass layer is lighting, not color.

**Mixing (FMAT render state, decoded).** agl-state (`FUN_030c0928`: polygon 0x280242, depth 0x36, color control 0xcc0100, target - blend control pair 0x25040504) shows layout: FMAT +0x14 - `CB_COLOR_CONTROL` (ROP `0xcc`, bits 8–15 – inclusion of blend by goals), +0x18 – target number, +0x1c – `CB_BLEND_CONTROL` (color: src 0-4, fn 5-7, dst 8-12; alpha: 16-20, 21-23, 24-28; bit 29 separate).

| Materials | color control | blend control | Meaning. |
|---|---|---|---|
| Main `DeferredMain` (`field_water`, `field_hybrid`, `chara_*`) | 0xcc0100 | 0x20010401 | target 0: `src·1 + dst·src.a` |
| `preshading_*`, `debug_simple` | 0xcc0000 | 0x20010504 | blend off |
| `TeraWater` `Translucent` (G-buffer water) | 0xcc0000 | 0x20010504 | blend off |

Enum GX2: 0 zero, 1 one, 4 src alpha, 5 inv src alpha; fn 0 - add. Overriding these states with a passcode is possible, but render info does not specify them.

**The result for the pixel of water.** PS. `field_water` `out.a = flag·s²`where `s = Shadow.z` (missing fog from pre-shading), flag - bit 0 of `albedo.a·255`G-buffer water writes `albedo.a = f32(1/255)`, so flag = 1.Taking into account the blend and the fact that G-buffer water without blend is recorded in the color buffer (Emission, slot 5) `L·(1−a)`:

```text
total = fog.rgb + s·C + s2·L·(1−a)
C = albedo·(sun + E·(1−f)) + S·F + sun glare + prepass1
f = (1 − saturate(−V·N′)) 5 · saturate(0.8 + shadow.x·shadow.w)
F    = f + 0.02·(1 − f)
```

`N′` is a normal slightly deflected to the sun (share of `(1−w)`, w = 0.5...1 in distance); S is a cube map in the direction of reflection corrected to the sphere of radius |P|+200, LOD `3−3·gloss`; E is a cube map according to normal, LOD 3. The exact formulas of solar members (diffuse and glare) are partially read.

**E and S on GLSL program 24 (reread 2026-09-29).** PS textures - slots 1, 2, 4, 6, 7, 8 (cube), 10 (slots)`gsys_light_prepass`, layer 1), 11, 13; `LightAnalyzer` (`gsys_user4`) the programme does not read. `E = textureLod(cup, turn N, 3.0)` comes in only `albedo·(… + E·(1−f))` - without scale and saturation of the analyzer (at the relief - `field_ambient`). S: `V = P/|P|`, `r = V − 2(N·V)N`, `D = P − c` (`c` = u3), `b = r·D`, `t = −b + sqrt(b² − (|D|² − (|P|+200)²))`direction. `P + t·r − c`, normalized and rotated in rows `inverse(A)`module `r.y` Nope.

**Centre de la droit `environment[31]` (Ghidra, reading only).** Vector 31 - UBO `0x1f0`, custom payload KSys `+0x60` (payload with) `0x190`, see. [water](wiiu-water-variants.md#layout-and-ksys-extension)The writer -- `0x033ff8cc`, PPC `0x03400034..0x03400394`if the object `KSys+0x85c` jack-up `+0x348`, `+0x60 = M·(obj+0x1a0, 1)` (`0x03c6fdb4` - point on the matrix 3×4, M - record + 0x60 of the context view (copy of the camera matrix, `0x03b26d08`); otherwise, the constant from `.bss` `0x10549e0c` (filled in at launch, value not read) Object is 0x350 bytes, created in `0x03405f48` (`0x038c1f74`), updated `0x038c28e4`: `+0x194` - the transferred position, `+0x1a0` catch up with the position of the cube map buffer (`0x03a8a6f8`) with a share `+0x18c` (3.0 on restart, immediately) Reread 2026-10-01: at a share ≥ 1 `+0x1a0` := `+0x194` (transferred position), otherwise `+0x1a0` += (buffer position -) `+0x1a0`)·share; restart (bit 1) `+0x348` removed or bit 2 raised) puts buffer position = `+0x194` (`0x03a8a6dc`) and 3.0. So the center -- **cube-card**Wewer takes the cube map where the camera is.`cubemap.rs`), and takes center = camera; catch-up is not repeated. **Uniform of the Final `field_water` (native).** `tools/research/cemu_uniform_map.py` (total bypass of CF R700, tested on PS water - repeats published table 49/53) for program 24 gives 52 calls in 58 groups:

| Cemu | Block[vec4] | Meaning. |
|---|---|---|
| u0, u1, u2 | context[16], [14], [17] | far−near, near, `tan(fovy/2)` in .y |
| u3 | environment[31] | reflector |
| u4, u5, u9 | context[0], [1], [2] | A: (.y) = world vertical in view |
| u6, u7, u8 | context[13], [11], [12] | Inverse(A): Turning to the World for a Cube Card |
| u10, u13 | environment[4], [5] | direction and color of the main directional light |
| u11.w | scene_material[2].w | `uking_dynamic_base_light_change_ratio` |
| u12.x | scene_material[1].x | `uking_dynamic_cloud_ratio` |

Scene material names - from the layout of the block `uking_sys_scene_material` (u16 offset +1: 0x11 → vec4) `cloud_ratio·ratio⁴`].x, 0x2d → vec4[2]KSys writes them in setters `FUN_034088d4` (`cloud_ratio = saturate(1 − x)`and `FUN_03408770` (`base_light_change_ratio = 1 − x`The slope of the normal thus goes to the world vertical: `N′ = normalize(N − up·(N·up)·(1−w))`, `w = 0.5 + 0.5·saturate(25/(z·tan(fovy/2)))`N' in Fresnel and the sun, N in reflection and E. The glare of the sun is multiplied by `cloud_ratio·ratio⁴`Setter Challenges: KSys Mode Customization `0x03408b48` weather-like `FUN_036425b8` (PPC `0x03642b7c..0x03642ba0`) which conveys the weight of the state of heaven 1 ("Oblusive"), so that `cloud_ratio = 1 − cloud-weight`The object in the sky is storing its current state. `+0x184`target `+0x188` transition rate `+0x18c`Update. `0x03641c44`:

- at the end of the transition (`+0x18c ≥ 0.999`) the goal is state 0 in the weather
  `FUN_0366ad14` = 0 (`Bluesky`) or 8 (`BlueskyRain`), otherwise state 1, if the cloudiness of the world is `[+0x2120] > 0.2`.
- Exponential transition: `blend += (1−blend)·(1 − 0.99^Δ)`, step clamped in
  `[0.001Δ, 0.0025Δ]` (Δ - frame multiplier); in another mode ( `+0x624` flag), `0.9^Δ` and step exactly `0.01Δ`;
- state 2 puts `FUN_03641ab8` near the change of day (time angle)
  `FUN_03661b00` > 357.5 or ≤ 1.25), and also when the `'c'` symbol in the `+0x12a` of the current scene.

Weuer repeats this machine (`climate::SkyState`, 2026-09-29): the target is the weather `WeatherMgr+0x18`, the transition is the chase game.

**Cloud of the world `SkyMgr+0x2120`** (found 2026-09-30, Ghidra EU v208, read only) No direct records with offset 0x2120, because writes it `FUN_03655de8` (update of wind and clouds `SkyMgr`) through the `r28 = SkyMgr + 0x1cac` index with offset `+0x474` (`0x036567b8`, `0x036568dc`); designer puts 0 (`0x03654394`, via `+0x2104` + 0x1c).

- target - 1 if the weather the current climate wants (`FUN_03672890`)
  not `WeatherMgr+0x18`, not `Bluesky` (0) or `BlueskyRain` (8), otherwise 0;
- in weather conditions set to the world (`WorldMgr+0x649`  ⁇  0xff without `+0x64d`), or
  with `WorldMgr+0x53c` , the value is immediately set;
- Otherwise, the value goes to the goal in a step of no more than `0.0015·v·t` and no less.
  the fifth part of it (speed `1 − 0^t` = 1: the gap is less than a step closes immediately), where `v` - `TimeMgr+0xb0` (according to the layout of Switch - `_d0 = max(mTimeStep / DefaultTimeStep, 1)`, in the usual game 1), `t` - frame multiplier; with `WorldMgr+0x610`  ⁇  -1 step `0.01·v·t`. From 0 to 1 - 667 frames (≈ 22 s), threshold 0.2 - after 134 frames (≈ 4.5 s).

Readers: `SKY_UpdateWeatherStateBlend` (threshold 0.2), `FUN_03657fac` (share of weather `+0x215c…+0x2170` × cloudiness → `+0x2174` → KSys `FUN_03408948`), `FUN_03666714` lightning (thunderstorm with clouds ≥ 0.99), clouds `FUN_0365867c`, `FUN_0365ad1c`. Weuer repeats it from 2026-09-30 (`climate::Weather::cloudiness`): the sky in bad weather becomes cloudy only after clouds pass 0.2; in `--weather` - immediately.

**What is it? `L`.** During the G-buffer Xlu `gsys_light_prepass` target `PreShading(Diffuse)` (Two-layered; with active) `agl::lght::LightPrePass` - his buffer `LIGHTPREPASS_CreateViewLightBuffer` `0x03af3f74`where the two-layer version has two render target slices 0 and 1. `GBUFFER_AllocatePreShadingTargets` goal `PreShading(Specular)` is built on the surface of GBuffer+0x2774 inside the Diffuse record, the slice is the last layer (the slice is the slicing).`FUN_03a7e48c` It gives you the number of layers. **Layer 1 Diffuse = Specular target**which pre-shading writes with output 3; this output is in PS 108 (`preshading_field_leaf`; relief - PS 112, [table](wiiu-character-shading.md#materials-deferredmain--programs)) It depends on the albedo, the emission, the normal, the cube map and the shadows -- it's a shaded color, not light. `L` This is consistent with the sand being seen through shallow water. Untested: bitwise equivalence of this color to the bottom color (fog, local sources), exit parsing 3 PS 108.

`FUN_0399d74c` transfers the FMAT render state to the gsys state as is (includes blend on targets from `CB_COLOR_CONTROL`, one `CB_BLEND_CONTROL` on all 8 targets), so that the G-buffer water writes without mixing; `prepass₁` in the final `field_water` is the same layer 1 in the pixel of water.

Not established: formula 3 pre-shading relief (exact color `L`), input setter `base_light_change_ratio`, meaning `Shadow.w`, formula pre-shading water fog (`environment[13/16/30]`, `gsys_user0`), contents of the cube map `gsys_cube_map` (partially - below).

## Cube map gsys: models around the camera (Ghidra EU v208)

Restored (static parsing, not executed; Ghidra `U-King.rpx` EU v208, dump v208): gsys draws a cube map of the **sam - relief, models and sky** around the camera.

**Edge.** `FUN_03a22ed4(obj, ctx, face)`colback `obj+0xaa4` (vtable +0xc, phase 0), passageway `Model(CubeMap/Opa+AlphaMask)` (mask appearance) `1 << face`, drawing flags 0x80), colback (phase 1), `FUN_03a8c63c(obj+0x28c, …)` face-matrix, passage `Model(CubeMap/Xlu)`, `FUN_03a8ef04` (face count; sixth, filtering) The 0x80 flag goes into rendering state.`FUN_0399be9c` → `FUN_0399a198`) and not in the selection of models.

**Kolback is the sky and clouds of KSys.** KSys in the "Main" scene (`FUN_03405f48`) configures the cube map object (`gsys+0x3f4c`designer `FUN_03a223c0`, 0xb54 bytes: `+0xaa4` = delegate `KSys+0x8ec` methodically `FUN_0340b6cc` (`0x0340b6cc`, from the KSys designer `FUN_034051f0`), `+0xc` near = 0.1, `+0x10` far = 300.0 (`0x1046f474`), `+9` = 1, `+7` = 8. `FUN_0340b6cc` It only works in phase 1 (after opaque models): with sky flags `+0x88c` bit 2 and bit 5 draws the dome `SKY_DrawPostfxSkyDome` (same) `sky_postfx_sky`as in the frame, otherwise - the hemisphere `hemi_to_env` (`FUN_03402a1c`: `cColorSky`/`cColorGrd` from `KSys+0xb0`radius `−0.95·far`If there is a cloud, then the clouds are cloudy.`AGL_WriteCloudLayerUniforms`, `FUN_03a5b498`The relief of the colback doesn't paint.

**What models.** Render info material `gsys_cube_map` sort out `FUN_0399d74c` c bitmask passage (key table) `0x1047d9c0`: 0 `gsys_dynamic_depth_shadow`, 1 `_only`, 2 `gsys_static_depth_shadow`, 3 `_only`, **4 `gsys_cube_map`**, 5 `gsys_cube_map_only`6 and beyond. `dynamic_reflection`, `multi_filter`In the aisle of the cube map, the model falls on the unit flag. `+0x16` bit 4`FUN_039e6ca4`) and the selection of shapes on 6 facet planes (`FUN_03a22990`); that the unit flag is a bit of 4 materials, not traced. `material_census` (v208, `_a0`): `gsys_cube_map` = 1 for 12,748 materials = 0 for 6,426 (`_only` Normal forest trees do not (= 1 in 11).`Obj_TreeBroadleaf_A_L`/`_LL` 37 other materials and `Obj_TreeBroadleaf_A` Link, bogoblin, sword - 0; lava - 1; from trees = 1 rare ()`Obj_TreeBaobab_A`, `Obj_TreeGiant_A`Palm trees, dead. **relief** -Yes. `Terrain.sbfres` (update `TitleBG.pack`) `TeraTerrain` `OpaqueNear/Middle/Far` and `TeraRoute` — 1; `TeraTerrain` `Translucent`/`ZOnly`, `TeraGrass`, `TeraFlower`, `TeraTree`, `TeraWater`, `Horizon` — 0.

**How shaded.** Ugh `uking_terrain` (Model 0) There are separate programs. `gsys_assign_cubemap` (12 out of 72; e.g. 12 and 66): they read only the textures of the relief, `gsys_user4` (table) `LightAnalyzer`and `_a0`/`_s0`but **not** `gsys_depth_shadow_cascade` neither `gsys_cube_map` conventionally `gsys_assign_material` - both); the same thing `uking_terrain_route`Ugh. `uking_mat` significance `gsys_assign_cubemap` No, how objects are drawn in the face, and whether they have shadows, is not established. `reference/visual-formulas/ shader-archives/*/manifest.txt`.

**Personnel.** `FUN_03a23190` is `FUN_039d6d98`) draws for challenge `obj+8` facet `obj+6`The seventh step is filtration. `FUN_03a8e7f4(obj+0x930)`centre `obj+0x604..0x60c` ← `+0x40..0x48` camera object (render info) `+0x208`, `FUN_03ae4aa0`A new cycle (`FUN_03a226e8`When the card is not assigned, the card is `+8 = +9`, `+6 = 0`: `+9 = 1` from KSys. **face-to-face**, six frames per cube and a frame per filter.

**Filtration.** `FUN_03a8e7f4` builds levels one by one: `i` level (`FUN_03a86768`) - Gaussian blur with the number of references `⌊p₂₂₄ + (i·p₂₂₈)^p₂₂c⌋` (`powf` `FUN_0421067c`), the fraction of blur `min(1, i·p₂₃₀)`. KSys sets the environment object (`FUN_039d3c68`) `+0x224` = 2.0, `+0x228` = 0.7, `+0x22c` = 2.0, `+0x220` = 4:2, 3, 6, 9, 14 references at levels 1-5. That it is the same object, from the displacement.

Not established: face size (config gsys `+0xcc4`, filled with a list of parameters `FUN_039b4a64`, the value is not read), target format (`+0x1154` chooses 0x1a instead of 0x33/0x2b), core `FUN_03a86768`, as objects are shaded in the face, `amp` sky dome in the face (`+0x140`, `+0x144` of the face matrix), binding of this object as `gsys_cube_map` in PS 32/112.

**viewer** (`crates/the original renderer/src/cubemap.rs`, with a dump): six chambers of faces on its layer of drawing (relief, objects with a `gsys_cube_map`, dome of sky and clouds), one face per frame from the camera position, near 0.1 and far 300 m; faces are copied into a cube, levels build a Bevy filter`GeneratedEnvironmentMapLight`) instead of the Gaussian chain; the light of the faces is a copy of the sun and moon without shadows. RENDER-004 (archived reference)The choice of shadows. CUBEMAP-001 (archived reference).

## Pre-shading fog (PS 140, read in Cemu GLSL)

Cemu `59cba7eb9a9c1df6` (programs 140/142/143). Blocks: `uf_blockPS1` - context, `uf_blockPS6` - environment, `uf_blockPS10` - 4 vec4 (scene material or material; not matched). The same constants (2/π, −0.85, 25, 0.001, 0.005, 2.5) are in the PS sheets 108 (`fb2e18ae56397ca7`) and characters 104, so this is the **general fog of the** scene, not just water. Native-check was not done.

```text
z = linear depth, P = beam·z in view, d = |P|, V = P/d
W   = inverse(B)·P  (environment[43..45]), up = (ctx0.y, ctx1.y, ctx2.y)
u2 = gsys user2(uv screen), w = 1 − 0.85·u2.y
A   = e27.z·(1 − (1 − tA)^e27.x)·w,             tA = sat(z·e26.x − e26.y)
B   = e30.w·(1 − (1 − tB)^e29.x)·(1 − e29.w·sat(V·up)^e29.z)·w,
                                                tB = sat(z·e28.x − e28.y)
H   = sat((e52.x − y′)·e15.x + e14.w)·e13.w·w,   y′ = W.y + 25·(2·tex0(W.xz·0.001).x − 1)
D   = u2.y·e16.w·sat(z·e18.x + e17.w)^e18.y
T   = (1 − A)(1 − B)
Fog.rgb  = e30.rgb·B + LUT·m·A·(1 − B) + e13.rgb·H·T + e16.rgb·D·T·(1 − H)
Shadow.z = T·(1 − H)·(1 − D)
LUT = gsys user0(u, v): c = lerp(cos azimuth(V, L), V·L, 1 − tA), L = e4,
      u = 1 − (2/π)·acos(0.5 − 0.5·min(c, 0.99)),
      v = 0.5 + 0.5·((1 − e27.w)·(1 − tA)^e27.y + e27.w)
m   = lerp(0.1k + (1 − 0.1k)·sat(0.005z − 1.8), 1, sat(u2.x)),
      k = sat(2.5·(Sem2.w − 0.3)) (Sem2.w - VS variation, not read)
```

`acos` polynomial `√(1−x)·(1.5707 − 0.2121x + 0.0746x² − 0.0187x³)`Azimuth is considered in view space by the components of x/z beam and `e4`Conclusion (not verified): VS programs are read `gsys_user4` (above) at VS 112 `Sem2.w` - his tex 0 `.x` ([field](wiiu-field-shading.md#ps-112-pre-shading-relief)); which texel takes VS 140 for `k`Shadow.x is a cloud shadow. `gsys_projection0` × (baked shadow) `gsys_dynamic_reflection`4  ⁇  cascades), Shadow.w - shading from top to bottom `gsys_depth_shadow_quarter` (`e39`), Shadow.y = 0.

**CPU source of parameters (writer `0x033ff8cc`, matching Wii U).** Local buffer `pfVar15[k]` = environment[25 + k/4] (`[37]` - custom+0xc0). The object of the sky KSys+0x854 (`sky.skybin`) emits fogs through `FUN_033f7aec` / `FUN_033f7b6c` / `FUN_033f7c44`. Its fields are bksky parameters with 0x10 step, the order of registration in the `0x033f164c` constructor ( `0x102bf514…0x102bf1cc` lines):

| Field. | Parameter bksky | In the environment |
|---|---|---|
| +0x770, +0x780 | `scatter_fog_near`, `_far` | e26 = (1/(far−near), near/(far−near)) |
| +0x790 | `scatter_fog_density` (≥0) | e27.z |
| +0x7a0 | `scatter_fog_atten` (≥0) | e27.x |
| +0x7b0 | `scatter_fog_horz` (≥0) | e27.y |
| +0x7c0, +0x7d0 | `adhoc_fog_near`, `_far` | e28 |
| +0x7e0 | `adhoc_fog_atten_grd` (≥0) | e29.x |
| +0x7f0 | `adhoc_fog_atten_sky` (≥0.5, omitted 0.5) | e29.z |
| +0x800 | `adhoc_fog_atten_minscale_sky` (default 0.5) | e29.w = 1 − sat(·) |
| +0x810 | `adhoc_fog_color` (rgba) | e30 |

e27.w is the field +0xd68 of the current LUT sky record (+0x670): horizon cosine −√(1 − (6360/r)2), r = 6360 + Y of the camera/1000 km`SKY_BakeInscatterLut`, [LUTE](wiiu-sky-resources.md#cpu-chain--lut-of-the-sky-matching-wii-u-2026-09-28)) No active sky table (flags +0x88c bit2 and bit6, like y `gsys_user0`) the writer puts e27.z = 0 and e30.w = 0: fogs A and B are off. Scatter fog on the fly only specifies `SKY_SetScatterFogParams` (`0x033f7b2c`) from the weather update `ENV_UpdateWeatherPalettes` (`0x036425b8`) Checked by call code: near / atten / horz - mixture of palettes (division × state of the sky, `ENV_BilerpPaletteSetValues`), to which the palette of fields (0) are added `CalcSfParamNear` / `CalcSfParamAttenuation` climate (+0x29c, +0x2ac, mixture of two climates); far and density - static `+0x6c`, `+0x7c`; then a mixture of the two climates `+0x198`. **There are no other weather multipliers of scatter fog.**: weather only enters through the weight of an overcast set. `StageType` 4 (Dungeon-Monsters): Add-ons `Remains_N` (`SfParam_near`, `_attenuation`).

**Contribution of the H-layer to rain (formula score, not frame).** For the camera. `weather/fieldcam_*` (Y = 134, `tools/style_cams.txt`) and the overcast palette (Start -20, End 400) H = sat((134 − y′ + 20)/420)·humidity/100. The ground 30 to 50 m below the chamber gives ≤ 0.17· moisture so that H reaches 0.5 at a humidity of 100 you need ≥ 190 m of drop. The thick light rain range H does not explain; it is given by the layer A with the color LUT, which in the rain becomes gray-turquoise (`SkySunColor` cloud palette `FeatureColor` e52.x = min(Y cameras, 250) (`pfVar15[0x4f]` - the transfer of line 1 inverse(B)), e53.z/w - the beginning of attenuation of cascades.

`uf_blockPS10` - `gsys_scene_material` (manifesto layout): [1].y `depth_shadow_off`, [1].z `proj_shadow_off`, [2].z `world_shadow_off`, [2].w `base_light_change_ratio` ( `e4` multiplier in L), [3].y `skyocclusion_off`, [3].z `depth_shadow_scale`.

**`gsys_user2` The mask of the “internal” space.** `FUN_034030c4` target `inner_mask`: cleaning in (1,1,1,1), Inner Mask Sub/Add models, then pass `FUN_038bfeb0` The final mask of the KSys-record +0x254If there are models, but the layer is empty, it's copied. `Black2D` (+0x1c table `agl::utl::PrimitiveTexture`, `FUN_03adda44`name-by-name `0x1047ebd0`If the mask conditions are not met at all, the slot is empty and `KSYS_BindUserTextureSampler` substitute0x30 = **`White2D`** (0xffffffffConsensus reading (interpretation): u2 = 1 inside, 0 outside; outside w = 1 and D = 0, inside the outer fogs ×0.15 and working D; scenes without a mask (probably a sanctuary) entirely "inside" Polarity after `FUN_038bfeb0` untested.

**Ad hoc fog (B).** `SKY_SetAdhocFogParams` (`0x033f7bd8`) Weather update: color, power, Start/End - object `fog_scatter` (KSys+0xa4: +0xe8…+0xf4, +0xb8, +0xc8), which the palettes re-record `FogColor/FogStart/FogEnd` (rgb palette is the same as `baglenv`); `atten_grd`/`atten_sky` - palette +0x10c/+0x11c, `minscale_sky` Force (alpha) = 0.3. `fog_scatter`) in sets of field: moisture/100, not `FogColor.a`So in field B. **always on**when the air is moist (corrected 2026-09-28: before `FogColor.a` = 0 was thought to be off in clear weather; the same fog on the dome [sky_postfx_sky](wiiu-sky-resources.md#sky-dome-sky-postfx-sky-options-8-and-12-matching-wii-u-2026-09-28)) Scatter fog palette fields: +0xdc near (+ `CalcSfParamNear`), +0xec atten (+ `CalcSfParamAttenuation`), +0xfc horz; far and density are static (`SfParam_far` = 30000, 0.85).

**LUT (`gsys_user0`) is the sky table (interpretation).** This is the texture of the LUT of the sky object (+0x670+4), from which the sky itself is drawn.`sky_postfx_sky`: u is the angle to the sun, v = 0.5 + 0.5·y look). PS 140 takes it at v = 0.5 + 0.5·y, where y = (1 − e27.w)·(1 − tA)^horz + e27.w: the color of the fog is the sky at a height that drops with a depth from the zenith to e27.w. u in PS 140 is nonlinear (option − tA) `BAKED_SUNVIEW_NON_LINEAR`).

**In the vuere (2026-09-28).** `apply_haze` (`look.wgsl`, all surfaces) performs A: `colour·(1 − A) + colour_fog·m·A`, A and m - as above, parameters - the viewer palette (near, atten, climatic displacement) and static far/density, k = 0. `sky_lut.rs` It's baked on PS 413. `read_look_at` (`look.wgsl`) takes the formula above (u, v, e27.w; LUT - lines 65-320 of texture look), brightness - fit SKY-LUT-001 (archived reference)Azimuth on Cemu GLSL: `R9i.y = (Vx·Lx + Vz·Lz)/(√(1−Vy²)·√(1−Ly²))`, `R11i.w = V·L`, L = e4 × `base_light_change_ratio`; c = lerp(`R9i.y`, `R11i.w`, 1 − tA), u — `textureUnitPS10` (xyz. ν pastries = −c, so that e4 is directed from the light (output by peak Mi; interpretation). Without dump - the previous fit of haze (`fog.rs`, choice A in FOG-COLOR-001. High H is executed (`look.wgsl`Texels 12-13: The same color `fog_scatter`what B has`FogColor` × fog multiplier in units of LUT, 2026-09-29; without LUT - fitting `FOG_PER_LUX`), force - moisture `Moisture` (`fog.rs`: hour throw - hash instead `sead::Random`morning shift, `AddMoisture` weather-bound `TempMgr`, smoothing out), with noise of heights `cloud_noise` (Lines 1–64 of the texture look, `read_look_at`, bilinearly with replay; game sampler mode untested. B executed (2026-09-28): `apply_haze`Texels 16-19 (`SkyDome::surface_texels`, `fog.rs`), only with LUT, as in the game; power and atten sky - palette sets like the game (below), near/far - `FogStart`/`FogEnd` Not executed: D (only inside), s2 for `L` Water (viewer takes the already foggy color for water) Weather thickening - the old fit (atten / thickening), in game - weather palettes.

LUT and e27.w baking are restored ([ chain LUT](wiiu-sky-resources.md#cpu-chain--lut-of-the-sky-matching-wii-u-2026-09-28)) and executed in the viewer (see above). Not established: `gsys_user4` (k), polarity of the final `inner_mask`, the connection of the LUT recording with `gsys_user0` at the code level.

**The gsys e13...e18 and the matching Wii U.** `GSYS_BuildEnvironmentUniformRecord` (`0x03a0b78c`) writes objects of type `agl::env::Fog` Circumstances in order through `GSYS_WriteEnvironmentFogRecord` (`0x03a0b28c`) in the members of block 0xb, 0x10, 0x15, 0x1a (with a smooth transition between sets). fog recording - three vec4: color rgba (+0xe8); (direction +0x104, −Start·k); (k = 1/(End − Start), Damp +0xd8). Field of object (`AGL_ConstructEnvFogObject` `0x03a97844`): Start +0xb8 (1000), End +0xc8 (10000), Damp +0xd8 (1), Color +0xe8, Direction +0x104 ((0.0,−1)). `master_field.baglenv`: `fog_scatter` → e13…e15 (**H**), `fog_world` → e16…e18 (**D**), `fog_inner` → e19…

Weather update `0x036425b8` each frame rewrites objects ( `EnvPalette` fields to Wii U: +0x2c `FogColor`, +0x54 `FogStart`, +0x64 `FogEnd`, +0x74 `YFogColor`, +0x90 `YFogStart`; order as in Switch `worldEnvMgr.h`, offsets by code):

- `fog_scatter`: Color `FogColor` (time, weather, tint), Start/End
  `FogStart`/`FogEnd`; **alpha** in sets of palette {0, 1, 21–37, 48–55} (`WEATHER_IsFieldPaletteSet` `0x03642434`) - humidity `TempMgr` / 100; the replacement is in each of the four palettes to bilinear mixture (PPC) `0x036448ec..0x03644994`: each palette is checked separately; `EnvMgr+0x3d58` ⁇  0 all four take `+0x3cfc`? `atten_sky` - humidity/100·4.8 (`0x1030023c`) in sets 0 and 1 (`0x03647420..0x036474c4`otherwise `afParam_attenuationForSky` palettes (reread 2026-09-29; viewer - `Environment::moist_fog`after the mixture at w = `EnvMgr+0x3cd8` > f29 (probably 0) color (+0xe8) stretches to `+0x3c60…+0x3c6c` weight w6`0x03644aa0`Start (+0xb8) - to `+0x3ccc` weight w3`0x03644cfc`); writer and meaning `+0x3cd8` Not traced (Ghidra, 2026-10-02);
- `fog_world`: `YFogColor`, `YFogStart`, static `YFogEnd`
- `fog_inner`: Color `fog_scatter` x coefficients, Start/End/Damp
  indoor-value manager.

Humidity (`mMgrs[4]` = `TempMgr`field +0x28, `TEMPMGR_UpdateMoisture` `0x0365d3c4`): objective = `MoistureMin + (MoistureMax − MoistureMin)·r` (`TEMPMGR_LerpClimateMoisture`), r is accidental [01), re-selected at the change of hour (`time/15`) or climate; from 5 to 9 hours r → 0.5 + 0.5r (`0x0365d2f0`When the weather transition is completed (≥ 0.9), the weather values of the environment manager (by type of weather) are added to the target; no more than 100; special cases (time flag +0x12e → 7.5, `FUN_036774b4`) not disassembled. The meaning extends to the objective: `m += (t − m)(1 − 0.9^Δ)`step-in `[0.01Δ, 0.1Δ]` (0.5Δ in special case) Reset `TempMgr` `0x0365cd68` (names designer) `0x0365ce08`, object 0x7c and the beginning of the scene `0x03673f6c`challenge `0x03677028` Unconditional, the condition before him only chooses `mMgrs[4]`; Ghidra, 2026-10-01 puts `+0x28` = 0: **Each scene starts with humidity.** And you chase it to the target, not straight away. `+0x64` = 0, bloom multipliers `+0x74` = `+0x78` = 1.0, `+0x10`, `+0x14`, `+0x18`, `+0x2c`, `+0x30`, `+0x34` = 23.0, `+0x44` = 0.4, `+0x48` = 50.0, `+0x4c` = −50.0, `+0x70` = 0.2, `+0x58` = `+0x5c` = −1, `+0x20`, `+0x24`, `+0x50`, `+0x54`, `+0x60`, `+0x68`, `+0x6c` = 0. **Hypothesis:** 23.0 is the default temperature (°C), −1 is “hour/climate not selected” (then the first frame throws a new target); the meaning of other fields is not traced.

Total high-altitude fog of the field: `H = sat((min(Y camera, 250) − y′ − FogStart)/(FogEnd − FogStart))·humidity/100`, color `FogColor`, y' = y + 25·(2·noise − 1), noise - channel R texture `cloud_noise` (64×64 BC4, `SystemModel.Tex2.sbfres`, sampler0 material = `uking_tex0`) on the world x/z·0.001. Internal D - `YFog` only inside `inner_mask`.

**Weather palette (matching Wii U)** Weather update `0x036425b8` Copy the static table 57 × 3 u32 s `0x1030024c` (`0x10300248` float 0.46 in front of it; the cycle copies with +4, index `row·3 + col`, line outside 57 → set 0: line - `PaletteSetSelect` climate (previous) `+0x190` current `+0x194`transition `+0x198`The sky is the sky, 1 is clear, 2 is a day. `SKY_UpdateWeatherStateBlend`:current `+0x184`target `+0x188`transition `+0x18c`:: 0 and 11 → (0, 1, 2), 1 → (3, 3, 3), 2 → (4, 4, 2), 3 → (6, 6, 6), 4 → (5, 5, 5), 5 → (8, 8, 8), 6 → (9, 9), 7 → (10, 10, 10), 8 → (11, 12, 11), 9 → (0.2), 10 → (13, 13, 13), 12 → (14, 14, 14, 16, 15 → (17, 2), 16, 16 → 16 (18, 18, 18), 9 (0, 18), 10 → 19, 18, 19, 18 – all `PALETTE_SET_ROWS` into `botw_formats::env`In the v208 dump, all climates except three are line 0; `DarkWoodsClimat` - 7 (set 10), `LostWoodClimate` - 1 (set 3), `KorogForest` - 10 (set 13) Set 10 in all divisions `BgDifIntencity` alpha `SkySunColor` = 0 (dam v208), `env_info --set 10`; set 13 - 4-4.5: what the game shines Forest wilds, not disassembled.

In addition to the climate, the palette set sets the map: `ChangeWeatherTag` with a parameter `PaletteSel` (≥ 0 is the dial number; probably through `EnvMgr::setPaletteSet`, `mPaletteSetOverride` with a timer 4 frames - decompilation Switch; handler `ChangeWeatherTagRoot::calc_` is not disassembled). Census MainField v208 (`actors_near`, 2026-09-29): of ~ 350 tags `PaletteSel`  ⁇  −1 for few - 4, 8, 11, 12, 13, 16, 0, 1, 2, 7, 18 (pall) (Train: 10, 040, 0, 04), from these (115 × 22.

Four sets (`+0x3cee8…+0x3cef4` = previous climate × (current, target state), current climate ×(same)) give the palette numbers through `ENV_GetPaletteIndexOfSetDivision` (`0x0364228c`: `EnvAttribute_set. PaletteSel{div}`,step of set 0xcc, `+0x178`/`+0x17c`; set > 58 - Selecting 0, silence `PaletteSel` 8 sets + divisions (Switch) `worldEnvMgr.cpp`); number > 207 `ENV_UpdateWeatherPalettes` It reads as palette 0; in dump v208, all 59 sets and numbers 0...206 are given. `ENV_BilerpPaletteSetValues` (`0x03642418`): `lerp(lerp(a, b, t), lerp(c, d, t), w)`, t is the transition of division (`+0x180`), w - transition of the state of the sky (`+0x18c`then `lerp` two-climate `+0x198`The weight of the cloudy sky (`dVar62` near, transmitted to `FUN_034088d4`how `cloud_ratio`): 1 with the current "overcast" (or 1 - transition, if the goal is different), otherwise the transition to the goal is "overcast".

`SKY_UpdateWeatherStateBlend` (`0x03641c44`The only challenge is `ENV_UpdateWeatherPalettes` `0x03642798`Order of branches (Ghidra, 2026-10-01): byte `EnvMgr+0x3cf11` ≠ 0 (`0x036414cc`) the whole function is an event branch `0x03641624` (below); otherwise, with `EnvMgr+0x3ceac` > 0.001 (`0x03641a78`) — `SKY_UpdateDayChangeStateBlend` (time angle > 357.5 or ≤ 1.25, writer) `+0x3ceac` not found; otherwise, a conventional machine in it at the end of the transition (≥ 0.999, then `+0x18c` = 1) target - "clear" in Bluesky/BlueskyRain weather`0x0366ad14` = `WeatherMgr+0x18` = 0 or 8), otherwise "opaque" if the cloudy world (`+0x2120`) > 0.2; a new transition starts with 0: `w += (1 − w)(1 − 0.99^Δ)`step-in `[0.001Δ, 0.0025Δ]` per frame, with the remainder ≤ 0.001Δ - immediately 1 (Δ - frames at 30 fps: ≈ 490 frames, ≈ 16 seconds for full transition; reread 2026-09-29).`+0x18`), not at the end of its transition. `WorldMgr+0x624` ⁇  0 and row of sets `+0x194` = 8 goal - 1, if `WorldMgr+0x5e8` ≤ `EnvMgr+0x3ce54`, otherwise 0 (the same bit is written in bytes) `+0x171` identifiable `0x03519ef4()`), chase `1 − 0.9^Δ` in exactly 0.01 Δ. At the end, outside the branch, "transition completed", `WorldMgr+0x53c` or `+0x530` = 1 s `+0x638` = `+0x63c` = 1 — `+0x18c` = 1 at once. In the vuere (`Environment::at_climate`) the sets are taken from this table and mixed by the weight of the cloudy sky. `climate::SkyState` (Automatic above) `Weather::overcast_sky`); the column "change of day", the branches of the change of day, events and lines 8 are not executed.

**Event Branch `0x03641624`** (1096 bytes, Ghidra, 2026-10-01) – state machine `EnvMgr+0x3cf00` with a timer `EnvMgr+0x3ce94` from 0 to 90, step – Δ (90 frames ≈ 3 s). Flags `EnvMgr`: `+0x3cf11` (input), `+0x3cf12` (reached 90), `+0x3cf18`, `+0x3cf19`; `Q` = `0x031ca674` = `*(DAT_1046d3ac + 0x175b0)`  ⁇  0 (object not identified):

```text
0: at +0x3cf11 and Q = 0 → timer 0, +0x3cf12 = 0, state 1
1: Q = 0 and +0x3cf18 = 0: timer += Δ, at ≥ 90 → 90, +0x3cf12 = 1, p.
   otherwise: timer −= Δ, at ≤ 0 → 0, +0x3cf12 = 0, state 0
   (in 1) w = timer/90; if sky target +0x188  ⁇  2:
   +0x184 = +0x188, +0x188 = 2, +0x18c = w - the sky goes into column 2
2: at (+0x3cf18 = 0 or +0x3cf19  ⁇  0) and +0x3cf12 = 0 and 0x036414d8() = 0
   → timer 90, state 3; at +0x3cf18  ⁇  0 and +0x3cf19 = 0 → +0x3cf12 = 0
   condition
3: timer −= Δ, at ≤ 0 → +0x3cf11 = +0x3cf12 = +0x3cf19 = 0, p.
4: at +0x3cf18  ⁇  0 and +0x3cf19 = 0: timer −= Δ, at ≤ 0 → state 0;
   other than the timer +=Δ, at ≥ 90 → state 2, +0x3cf12 = 1
(in ≥ 2) if +0x184  ⁇  2: +0x188 = +0x184, +0x184 = 2;
   +0x18c = 1 − timer/90 - the sky returns from column 2
```

`0x036414d8` — OR two object checks from the `DAT_1047e650+0x18` array (0x4e and 0x45; virtual `+0x14` and `0x03a3e880`). **Hypothesis:** are interface screens, and the branch holds the sky in column 2 "shift days" while some event screen is open. `+0x3cf11/+0x3cf18/+0x3cf19` writers are not found ( `st*` scan with `0x3d5d` offset from `EnvMgr+0x391b4` is empty; the base may be different).

**Interpretation (hypothesis).** The water lighting is multi-stage: G-buffer → pre-shading by material type (light and reflection in intermediate targets) → a separate passage that multiplies/mixes the albedo with them. The turquoise of the sea cannot be explained by any of these PSs; you need a final passage of the composition.

## Reproduction and limitations

```sh
OUT=$REF/shading/SystemModel.sbfres target/release/examples/pack_ls \
  $UPDATE/content/Pack/Bootup_Graphics.pack SystemModel.sbfres Model/SystemModel.sbfres
ALL_OPTIONS=1 target/release/examples/model_info $REF/shading/SystemModel.sbfres
```

No ALUs of these PSs were executed; Cemu matching is the presence of the program in the cache, not the order of the frame. G-buffer chain → pre-shading → 14 steps `preshading_filter` The composition is statically linked only through the CPU (render targets and bindings) `gsys_user*`choice `gsys_assign_variation`The most direct next step is to capture the water frame in Cemu (on macOS — Metal GPU capture via MoltenVK): it gives the order of passages, tied textures and blend states. The static alternative is the CPU tracker GBuffer Xlu/pre-shading render in Ghidra, then the native-probe PS 140 targets.

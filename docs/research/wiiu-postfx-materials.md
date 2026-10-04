# Wii U: Restored HDR compose and color correction boundaries

Date: 2026-09-28 Subsystem: postfx. FIDELITY (archived reference), environment-format (archived reference), gaps (archived reference)From 2026-09-29, the viewer performs both formulas and bloom (see below).`postfx.rs`, choice (archived reference)); initial families `uking_mat` now compared in [separate study](wiiu-material-variants.md)But the full formulas of his materials have not yet been restored.

## Result and degree of evidence

The original Wii U `uking_pass_shader.sharcb` recreates the **arithmetic of both variants of the `hdr_compose`**. It's an exponential mix of brightness and individual channels, followed by saturation correction. There's no Reinhard, AgX or ACES. `cBloom + cColor` 's addition is the **do** ton curve.

This is the output from native Latte ALU instructions, not a similar image or program name. GLSL from the installed graphic pack helped find the passageway; then checked archival reflection, Cemu hash, all ALU groups and operands. `cColorCorrection`: exact LUT coordinates also restored. AGL restores identity and saturation/brightness variants of the LUT generator. Fresh Cemu cache contains a version without LUT. Matching Wii U RPX found its upload `cParam`, initial values and switching condition to LUT - see [CPU-chain](wiiu-render-cpu.md#ksys-hdr-compose-uniforms-and-lut-gate)The conditional chain from the AGL LUT generator to KSys is now set — see. [LUT source and parameters](wiiu-render-cpu.md#color-correction-lut-ownership-and-inputs)Uniform values and flags of a particular frame, as well as the place of application of palette `Exposure` It's not yet installed. You can't take the formula that you found as the final processing of the image.

## Identity of sources

Local roots are allowed from `renderer.toml`. Wii U update contains `content/System/Version.txt = 1.5.0`; Cemu `log.txt` separately reports EU title `00050000-101c9500`, v208. This log entry does not prove the terms of the future control frame. SHA256 resources:

| Related to the Wii U update | SHA256 |
|---|---|
| `content/Pack/Bootup.pack` | `489831ec22e0ee0dfdcbc43ce55d9561471e7c83ff985a3f6cb36bd15a3dbb05` |
| `content/Pack/Bootup_Graphics.pack` | `dcab53143116996f63ebf9ffd1d6e3145744a24c74945a8338f631b811fee79c` |

Shader chain: `Bootup_Graphics.pack` → `System/KSys/U-King.Cafe_Cafe_GX2.release.ssarc` → `uking_pass_shader.sharcb`. After Yaz0/SARC extraction, SHARCFB has SHA256 `107deffd98a77c1003303fe420058db3138e39003ca6c77f8d11bf0a30cbc478`. All offsets below refer to this decompressed archive; the boundaries are semi-open. Metadata and instructions read little endian.

| Object | Displacement/identifier |
|---|---|
| Program `hdr_compose` | `0xc5cc3..0xc5d2a`, kind 3, base binary 546, 2 variations |
| Macro | `ENABLE_COLOR_CORRECTION_TABLE`, values `0`, `1` |
| Variant 0 PS | binary 547, record `0xc2634..0xc2b20`, GX2 `0xc2644` |
| Variant 0 code | `0xc2800..0xc2b20`, 800 bytes |
| Code SHA256 | `91fb6eeaaca2e78ca53d934950c9ebfe2bcb3c28dc00d0af4c6e87c83b8b1935` |
| Reflection | `cParam [11,1,0,0xffffffff]`; samplers `cBloom [1,0]`, `cColor [1,1]` |
| Variant 1 PS | binary 549, code `0xc2e00..0xc3130`, 816 bytes |
| Variant 1 reflection | `cBloom [1,0]`, `cColor [1,1]`, `cColorCorrection [3,2]`; no `cParam` |

The general reader and extractor are described in the [ container shaders of ](wiiu-shader-containers.md); this document and probe are not substituted for the container reader.

## Communication with Cemu and independent verification of instructions

Local clue: `~/Library/Application Support/Cemu/graphicPacks/downloadedGraphicPacks/` `BreathOfTheWild/Enhancements/37040a485a29d54e_00000000000003c9_ps.txt`. SHA256 `0e07fc7fca339e29fdd2ddc6863cdc60c6dedc5805c9b7b78f7b1ce42f741b25`Only the branch has been investigated. `#elif (disableClarity == 1)`, lines 2100–2247; author's comment refers to Cemu 1.20.1. The remaining branches contain custom changes, their formulas are not proof of BotW. Later obtained own dump Cemu 2.6: picture `20260928T012415Z-cache-replay`file with the same name and paired `.bin`. The binary matches the PS 547 by byte; GLSL shows two texture2D samples, one remapped uniform vector and the same arithmetic. This is an output from the accumulated cache, without reference to a specific scene. Probe is re-created on Cemu itself. `.bin`: 3009 cases, maximum error f64 `3.33e-16`.

The algorithm from Cemu `c717fcab1ccc3e0b0b97499a4d9b04a77084e347`: [`_calcShaderHashGeneric`, `LatteSHRC_UpdatePSBaseHash`, import table](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/Core/LatteShader.cpp). On the original 800 bytes, the sum of the two rolling hashes is `0x37040a485a29554e`. Native GX2 `SPI_PS_IN_CONTROL_0` by `+0x08` = `0x14000001`, one `SPI_PS_INPUT_CNTL_0` by `+0x14` = `0x100`; no positional input. Import key = `rotl64(0x100,7) = 0x8000`. Without geometry shader, the result of `0x37040a485a29d54e` matches the name GLSL. This is not a cryptographic proof of text equivalence; additionally verified all operas below.

Instruction fields are checked by [Cemu LatteInstructions.h](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/ISA/LatteInstructions.h) and [ table opcodes](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompilerInstructions.h). Option 0 contains `TEX → ALU → EXPORT_DONE`; ALU clause code-relative `0x100..0x298`, 14 groups. ADD, MUL, MUL_IEEE, MAX, MOV, DOT4, EXP IEEE, RECIP_IEEE and MULADD, source signs/channels, clamp and write masks.

## Formula of the option without LUT

Let `C = sample(cBloom, uv).rgb + sample(cColor, uv).rgb`. For nonnegative HDR input and `L > 0`:

```text
w = f32_bits(0x3e99096c, 0x3f162b6b, 0x3dea4a8c)
  ≈ (0.2989, 0.5866, 0.1144)
k = f32_bits(0x3fb8aa3b) ≈ log2(e)
L = dot(C, w)
a = 1 - exp2(-k * L)
B = C * (a / L)
P = 1 - exp2(-k * C)
q = clamp(B + (P - B) * a², 0, 1)
d = f32_bits(0x3f2aaaab) * (q.r + q.g + q.b) - 1
s = cParam.x + (1 - d²) * cParam.y
m = max(q.r, q.g, q.b)
out.rgb = m + (q - m) * s
```

This is an algebraic record, not a bit-by-bit replacement for the GPU. The weight sum is close to one, but not equal to it: not to normalize `w` without new evidence. `exp2` with the specified f32 `k` is close to `exp(-x)`, but the replacement is not bit-by-bit. The exact zero RGB returns zero RGB in the proven non-IEEE MUL pathway; the reciprocal/EXP constraints and all negative/N/Inf inputs are not specified by this study. Alpha is not investigated.

Native sources from the latter multiply-add are constant-file `C[0].y/x`; reflection calls this uniform `cParam`. Now installed CPU record: `x=field[c50]`, `y=f32(field[c4c]-field[c50])`; constructor defaults of fields about 0.99 and 1.17 respectively. This gives saturation gain in the middle of the q range; current frame values are not yet measured. The exact bit values and addresses are in the CPU chain above. In the final output after saturation, an additional clamp is not detected. There is no separate exposure-multiply or sRGB-transfer in this PS; this does not prove their presence after the passage or the target is constructed.

| ALU Group | Code-relative offsets | Action. |
|---|---|---|
| 0–1 | `0x100..0x158` | sum of RGB and DOT4 with luminance weights |
| 2–7 | `0x158..0x200` | EXP IEEE, reciprocal L, luma and channel exhibitors |
| 8–9 | `0x200..0x228` | mixing by `a²`, clamp RGB |
| 10–11 | `0x228..0x278` | `d`, `1-d²`, maximum RGB and difference |
| 12–13 | `0x278..0x298` | `cParam.x/y` → saturation → RGB |

The semantics of `cBloom`/`cColor` are confirmed by reflection names and texture slot 0/1. The sampler/input texture format, the pre-bloom value and the scale of the HDR still require CPU/render-state verification.

## LUT variant and table generator

For `ENABLE_COLOR_CORRECTION_TABLE=1`, binary 549, code SHA256 `02bd08c2446f0f220cab66b5de3e5407540de3e172a7bf5c513b9ef41b9189e9`:

```text
q = same exponential hybrid and clamp as above
coord.rgb = (7/8) * q + 1/16
out.rgb = sample(cColorCorrection, coord.rgb).rgb
```

Native control flow: `TEX → ALU → TEX → ALU → EXPORT_DONE`The first ALU `0x100..0x278` (13 groups) considers `q` The last ALU, the final ALU. `0x278..0x290` It contains only MOV RGB. TEX code-relative `0x320`: SAMPLE (`opcode 0x10`), resource/sampler 2, source `R0.xyz`, destination `R0.xyz`Normalized coordinates, without TEX offsets. Reflection calls slot 2 `cColorCorrection`Coordinates pass through the 8 counting centers: `1/16` before `15/16`Cemu PS hash with the same input key: `7176c43d918b41e9`Sampler filtering and texture state are not yet extracted: the coordinate formula itself does not prove trilinear filtering. [Cemu ParseTEXClause](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompiler.cpp).

Generator in another container: `Bootup_Graphics.pack → System/Agl/agl_resource.Cafe_Cafe_GX2.release.ssarc` → `agl_technique_pfx.sharcb`, SHA256 `e1ff8e4ee5d0c4d49f891b32816a384f2e16413bdcae9a5cd2f4fbc3b254e9b1`. Program `color_correction_map` by `0x24bbd2..0x24bcdc`, base binary 40, 48 variants Macros: `CORRECT_WITH_HSB=[0,1,2]`, `CORRECT_WITH_CURVE=[0,1]`, `CORRECT_WITH_GAMMA=[0,1]`, `CORRECT_WITH_TOYCAMERA=[0,1]`, `ORDER_TOYCAMERA_HSB=[0,1]`.

The following particular paths, rather than the entire set of 48 options, are confirmed:

1. **Identity, variant 0**, all macros 0. PS binary 41, code
   `0x5500..0x58a0`, SHA256 `483ba16297571fa70b651c3c2eebe0f17208a923994a925f0e2cf06f3432788b`. Native ALU writes `R8..R15.rgb = (clamp(input.x), clamp(input.y), z_i)`. `EXPORT_DONE` has source GPR 8, burst count 8, RGB swizzle xyz: eight results correspond to slices of `z_i ≈ i/7`, `i=0..7`. This is consumer-independent confirmation of a grid of eight slices. The original f32 constants are slightly different from ideal `i/7`; for example, `z_3=0.4285714626312256`.
2. **Saturation/brightness, variant 16**, `CORRECT_WITH_HSB=1`, others
   Macros 0. PS binary 73, code `0x11300..0x117f8`, SHA256 `887eb16f60ae16885f5c1db1ca071138a55d286f20c8dfb31964f78b52f8d22e`. For each slice of `p=(input.x,input.y,z_i)`, `m=max(p)`: `RGB = clamp((m+(p-m)*cHSBG.y)*cHSBG.z,0,1)`. In this embodiment, `cHSBG.x/w` is not used. This saturation is relative to the maximum channel, not desaturation to the weighted luminance. The CPU confirms the transfer of the same AAMP fields unchanged in `cHSBG.y/z`; the exact addresses are in the CPU chain below.
3. **Gamma-only, variant 4**, `CORRECT_WITH_GAMMA=1`, other macros 0.
   PS binary 49, code `0x7700..0x7b68`, SHA256 `cb96b4dff230af6f766fd5e7054152b976f217040c6b7f8088ced35e39e6a953`. `LOG_CLAMPED(x/y)` tested, multiplication by `cHSBG.w`, `EXP_IEEE`, MAX with 0 and MIN with 1; for fixed z-slices log2 is folded into literals. In positive range, this is the path `exp2(log2(channel)*cHSBG.w)`. CPU draws `cHSBG.w = 1/gamma` through `fdivs`. LOG CLAMPED/N semantics are not numerically emulated; probe is able to output these instructions, but does not execute them in evaluator.

Hue (`HSB=2`) and curves remain open. Combined toy-camera variation19 path restored below; other combinations are not yet tested. Master-field AAMP includes toy camera with neutral values; this does not prove that the CPU chose variant16 and optimized the remaining flags. The connection of the AGL generator with KSys is established through sampler `CC+0xf28`, HDR and callback: [CPU chain ](wiiu-render-cpu.md#color-correction-lut-ownership-and-inputs). It is conditional and does not prove that the LUT is enabled in the saved Cemu session.


The startup snapshot `20260928T012415Z-cache-replay` also found exact byte matches of the LUT generator. Macros are taken from the stored manifest; the binary number differs from the variation number:

| Cemu fragment hash | PS binary | Variation | Non-zero macros |
|---|---|---|---|
| `3a11595b2bcc1e76` | 57 | 8 | curve |
| `6b1d41ccee588996` | 89 or 91 | 24 or 25 | HSB=1, curve; order=0 or 1 |
| `cd24daf7cd4b71c6` | 79 | 19 | HSB=1, toy camera, order=1 |

One VS `1c229efc23b39846` matches all 48 variants and doesn't distinguish them by itself. All three PSs have suffix `00000000ffffffff_ps.bin`; PS 89/91 bytes are the same, so the choice between them remains ambiguous. The local `color-correction-map-matches.json` report next to snapshot contains SHA256, all aliases and hashes of the source report/manifest; byte equality is retested only for these four files.

This is the presence of programs in the accumulated cache, not a proof of the order of passes or use of LUT final KSys compose. in particular, found PS79 is investigated separately below.

**Don't mix the two. `hdr_compose`.** V. `agl_technique_pfx` It has its own base660 program with 72 variants. `HDR_BLOOM_COMPOSE`, `HDR_COLOR_CORRECTION`, `HDR_TONEMAPPING`, `HDR_GAMMA`His vertex reflection includes `cParam`, `cTexCoordCoeff0/1` and `cExposureTexture`; fragment — `cHDRImage`sometimes `cColorCorrectionTable`This isn't the KSys program base546 that we looked at above. `agl::pfx::HDRCompose` It does not prove the choice of KSys shader by one name.


## PS79: Toy Camera and HSB

Wii U `color_correction_map`, variation19: HSB=1, toy camera=1, order=1, curve=gamma=0. PS79: archive code `0x13600..0x14878` (4728 bytes), SHA256 `a7edc9ac439b561ec4ee3284d809ca03512d11bdf09001568f69c400214bfb6c`. Native CF0..4 contains 126 ALU groups; CF5 exports eight RGB values from GPR13....20. Cemu cache matches exactly, not just by name/hash file.

Below is the reconstructed algebra for LUT input `p=(x,y,i/7)`, `x,y ∈ [0,1]`, `i=0..7`. Native f32 slice literals and their additions are slightly different from the ideal `i/7`. `clamp` here is component `[0,1]`. Names are loadable uniform, not raw AAMP values:

```text
m = max(p)                         // retain the original input maximum
q = p
for j in {1, 2}:
    q = q + (1 - q) * Offset[j]
    q = exp2(log2(q) * Level[j])    // component-wise, positive-log domain
    L = dot(q, weights)
    q = clamp(L + (q - L) * ToySaturation[j])
q = clamp(((q - 0.5) * Contrast + BrightnessBias) * MulColor)
out = clamp((m + (q - m) * HSBG.y) * HSBG.z)
```

`weights` is native f32 bits `0x3e99096c`, `0x3f162b6b`, `0x3dea4a8c`. Two toy-camera saturation use **weighted brightness**. The final HSB uses the **maximum of the original p** calculated to the toy camera, rather than the maximum already converted q. In particular, with `HSBG.y=0` , the color becomes `max(p)*HSBG.z` before clamp even after a non-neutral toy-camera color. The order of clamp and these operations is substantial.

Reflection and the drawMap `0x03aa47e8` CPU link uniform to the CC object:

| Uniform | Native selector | Source and CPU transformation |
|---|---:|---|
| `cHSBG` | 256 | previously restored hue/60, saturation, brightness, 1/gamma |
| `cToyCam_Brightness` | 257 | `CC+0x2b8`, algebraically brightness/2 |
| `cToyCam_Contrast` | 258 | `CC+0x2c8`, directly. |
| `cToyCam_Level1` | 259 | reciprocal of each RGB after multiplying the RGBA `CC+0x260` by its alpha `+0x26c` |
| `cToyCam_Level2` | 260 | Similar to RGBA `CC+0x27c`, alpha `+0x288` |
| `cToyCam_MulColor` | 261 | RGB from RGBA `CC+0x2d8` multiplied by alpha `+0x2e4` |
| `cToyCam_Offset1` | 262 | `CC+0x228`, directly. |
| `cToyCam_Offset2` | 263 | `CC+0x244`, directly. |
| `cToyCam_Saturation1` | 264 | `CC+0x298`, directly. |
| `cToyCam_Saturation2` | 265 | `CC+0x2a8`, directly. |

Selector is the native constant number, not the `uf_remappedPS` index from GLSL Cemu. CPU helper `0x030c04ec` copies RGBA and calls `0x030c032c`, which multiplies all components by scalar f1; PPC confirms saving f1 before the call. Then drawMap takes reciprocal RGB for Level1/2. So Level cannot be copied from the resource directly as shader exponent. Alpha Offset1/2 and MulColor shader does not use. HSBG.x/w is also not involved in this option, despite the general CPU upload.

Check: color_correction_probe.py (upstream reference: `../../tools/research/color_correction_probe.py`) It only accepts SHA256 of the PS79, decodes all five ALU clauses and compares GPR13..20 to the formula. On 512 deterministic random sets uniform/xy — 4096 RGB outputs, 12288 channels, max absolute error `1.8916195443363648e-7`Report next to snapshot: `color-correction79-probe.json`.This is an f64-test algebra with ideal i/7, not a simulation of native f32, transcendentals, storage R11 G11 B10 FLOAT or sampler filtering. LOG CLAMPED zero/NaN behavior is excluded explicitly: evaluator rejects nonpositive/nonfinite arguments. Synthetic tests check for failure for another shader identity, multiple uniform vectors, PS lane logarithm and register reading before parallel group writing.

Reproduction without re-extracting:

```sh
python3 tools/research/color_correction_probe.py \
  "$REFERENCE/cemu-sessions/20260928T012415Z-cache-replay/shaders/cd24daf7cd4b71c6_00000000ffffffff_ps.bin"
```

The whole formula is about LUT generation, not the screen vignette. Having a program in the cache doesn't prove that KSys selected it for LUT in a particular frame.

## Resources and CPUs: What is not yet connected

`Bootup.pack → Env/env.sgenvb → postfx/master_field.baglccr` is re-read through `aamp_dump`: `enable=true`, `saturation=1.175`, `hue=0`, `brightness=1`, `gamma=1`, `order_toycam_hsb=true`. You can't substitute `1.175` for `cParam.x` just because the word saturation means that the LUT version doesn't declare `cParam` at all.

`master_field.baglblm`: `enable=true`, `finalblend=1`, `enable_clamped_luminance=true`, `clamped_luminance=64`, `intensity=0.1`; `0xd6120a7f=2`, `0xf8c40b97=0.1` correspond to the already restored names `threshhold`, `threshold_range` from FORMATS (archived reference). The enum `finalblend=1` value is not in itself a formula for the composition. Palitre and weather overrides can change parameters.

Local decompilation: `zeldaret/botw`, commit `5b26254bfe69560b0993a50d8ba8f418ae749380`, **Switch 1.5.0**:

- `src/KingSystem/World/worldEnvMgr.cpp`, `initEnvPalette`: Announces
  `Exposure`, Bloom fields, defaults; does not set the found GPU formula. `calc_()` and `worldMgrCalc2()` in the source are empty, CSV marks them `O`, size 4; they can not be impersonated as restored consumer postfx.
- `lib/gsys/src/gsys/gsysModelSceneConfig.cpp`, constructor tagged
  `NON_MATCHING`, CSV `m`. Feature table separately lists `AutoExposure`, `HDR` and `ColorCorrection`. The latest default `should_create_by_default` for auto exposure false, for HDR/color correction true. These are defaults of creation, **is not proof of on/off in Wii U gameplay**.
- Native bodies `ColorCorrection` and `HDRCompose` are not recovered here.
  The following are only CSV symbols with `U` status, not decompiled formulas.

| CPU Target, Switch Address for Navigation Only | Address |
|---|---|
| `agl::pfx::ColorCorrection::drawMap` | `0x71013a5694` |
| `ColorCorrection::updateProgram_` | `0x71013a5be0` |
| `ColorCorrection::updateCurves_` | `0x71013a6380` |
| `agl::pfx::HDRCompose::initialize` | `0x71013b8078` |
| Neighboring unnamed function after HDRCompose init | `0x71013b81b8`, size 1256 |
| `gsys::ModelScenePfx::isHDRComposeEnabled` | `0x7100c35a98` |
| `gsys::ModelScenePfx::isColorCorrectionEnabled` | `0x7100c35abc` |

Switch addresses are not portable to Wii U Ghidra.Wii U priority: links to `hdr_compose`, `ENABLE_COLOR_CORRECTION_TABLE`, `cParam`, write uniform constant 0 and select variation; then generate LUT and link `Exposure`There were no independent references to Ghidra in this sub-task: exact Wii U CPU addresses and function identification captures the data. [CPU study](wiiu-render-cpu.md)It's a local result. `cpu/03aa4378.c.txt` containing a challenge `func_03a7e618(this+0xe88,2,0x1a,8,8,8,1,0,0,1)` and a cycle of eight render-target records, which is consistent with the size of LUT. CPU track then confirmed via native format table: 83 texture has `R11_G11_B10_FLOAT` (exact chain of addresses in the [CPU research](wiiu-render-cpu.md)Therefore, the quantization of LUT should also be considered in future GPU verification. `0x100` Not yet found.

## Output `hdr_compose`: linear light, sRGB at output (Ghidra EU v208)

2026-09-29, matching update RPX (identity - [CPU](wiiu-render-cpu.md)). PS 547 no sRGB conversion; the target format decides how to understand `q`.

- `0x03a1147c` (the only trigger is the initialization of gsys `0x03a11690`)
  Selects the 0 `+0x318` bit (a copy of the `+0x2ce` configuration byte) pair: the default format agl `DAT_1047ec78+0xf0` = internal `0x1d` and TV/DRC scan buffer `0x1a` (RGBA8 UNORM), or the internal `0x22` and scan buffer `0x41a` (RGBA8 **SRGB**). Scan buffer puts `0x030a4a70` → `GX2SetTVBuffer`/`GX2SetDRCBuffer` (`FUN_04004790/040048d0`).
- Table `0x1047ed60` (like LUT): `0x1d` → GX2 `0x01a`, `0x22` → GX2
  `0x019` (R10G10B10A2 UNORM).
- Byte `+0x2ce` for BotW = **1** (2026-10-01, Ghidra EU v208, only)
  reading: KSys `0x03405b04` constructing `gsys::SystemTask` on the stack`r1+0x8`, omission `0x03a14b00`) and writes 1 in his `+0x4a` (`stb r12,0x52(r1)` @`0x03405bb8`r12 = 1); designer `0x03a10dd0` (line) `gsys::SystemTask`) copies the argument in `+0x284…` (`lbz/stb …,0x4a` @`0x03a10f1c`/`0x03a10f20`), i.e. in `+0x2ce`Search. `st* …,0x2ce(` I didn't see the record because the byte comes in a copy of the argument. `0x03a11690` puts the beat 0 `+0x318` and the second branch is taken: the frame is written in R10G10B10A2, a copy in sRGB scan buffer (`0x41a`) encodes it, that is, `q` Linear light, like the output of a Bevy tonmapping before an sRGB surface (consistent with format) `0x019` 1280×720 targets in Cemu graphic pack `BreathOfTheWild/Graphics/rules.txt`Recording by the calculated index after the constructor is not excluded.
- Chain to frame: `0x039b2a88` → `0x039da8bc` → callback → KSys
  `0x03404cdc`; the target is render buffer of the form `+0xa44`, with AA, the temporary color `gsys::RenderBuffer::color(AA)` of the same format (`0x03a07c60`), then the passage of AA (`0x03a07e2c`) into the `+0x24` buffer.

## Bloom `agl::pfx::Bloom` (Ghidra EU v208)

2026-09-29 `agl_technique_pfx` programs are found in the Cemu `20260928T012415Z-cache-replay` image by exact matching bytes: `bloom_mask` PS 141 (=173/397/429, variants 1/17/129/145: only `BLM_LUMINANCE_CLAMP`, depth flags do not fall into the code), `bloom_gaussian` PS 651/653 (VS 650/652), `bloom_compose` PS 655/657, `bloom_reduce` PS 659. Formulas below - Cemu GLSL of these bytes and CPU downloads; native ALU was not separately checked.

Parameters (designer) `0x03a9dcb4`, displacements from the object of parameters: `threshhold` `+0xc` (shut up 0.75), `threshold_range` `+0x1c` (0.1), `intensity` `+0x2c` (1), `finalgather` RGBA `+0x3c`, `expand` `+0x58`second set with step `+0x5c` (for Shaft/ex); `color1..4` RGBA `+0x1b4 + 0x1c·i` ooh `color4` silent. alpha 0); `finalblend` `+0x234` (2), `enable_clamped_luminance` `+0x2a0`, `clamped_luminance` `+0x2d0` (10), `ex_type` `+0x260` (0), `ex_iteration` `+0x270` (5) The luminance weight of the mask`0x03a9d594`): (0x3e990afe, 0x3f162c23, 0x3dea7371≈ (0.29891, 0.58661, 0.11448), divided by the sum of 1.000001 (at `edit_type`≠1).

Mask (`0x03a9e9a0`, `0x03a9e648`), `E` - field of the form `+0xc10` (1 in the constructor of the form `0x03a9dbb0`; late entries are not traced):

```text
r = 1 / (threshold_range·E)
cLuminanceWeight = (w·r, −threshhold·E·r)
cThresholdParam  = (clamped_luminance·r, 0, intensity, const)
c = mean of 4 bilinear samples uv ± (1/width, 1/height) of the source
m   = sat(dot(c.rgb, w)·r − threshhold·E·r)      // alpha HDR = 1
s = sat(clamped luminance/dot(c.rgb, w) // with CLAMP only
out = c · m · s · intensity
```

**What the mask reads** (2026-10-01): PS 141 (=173/397/429) — Cemu `10584c6fc5857351_0000000000000079_ps` The same image (exact bytes, `matches.json`Reflection of the GX2 program: two variables vec4- `cLuminanceWeight` c0 and `cThresholdParam` c1 (names by index) `0x1a0`/`0x1b1`lengths 17 and 16 - rows `0x1b8`/`0x1c9` into `agl_technique_pfx.sharcb`GLSL reads `uf_remappedPS[0].xyzw` and `uf_remappedPS[1]` only `.x` (numerator) `s`and `.z` multiplier `intensity`So, then `cThresholdParam.y` (constant 0, `0x03a9e674`and `.w` global `0x1047eb48`in the data 10000.0, `0x03a9e688`This option is not readable and the mask is not affected. `BLM_LUMINANCE_CLAMP` program-selected `enable_clamped_luminance`; viewer passes this choice to `mask_param.w` (1 with a restriction), this is his flag, not the meaning of the game.

Levels (`0x03aa0714`, `0x03a9f248`): mask - in the target frame size x 0.25 (`+0xc04`) × `+0xc08` (runtime, unread), rounded up to 4; frame less than 64 - bloom skipped. `detect0/1` 5 levels (format) `0x1a` R11G11B10F), level i+1 is half as small. For i = 0.3: level i → level i+ 1 by one bilinear sample (`bloom_compose` STEP 3), then Gauss crossed into the temporary target and back again: 5 samples, offsets ±1.3846 and ±3.23077 texel of this level (`cGaussianParam` = 1/width, 1/height), weights 0.22703, 0.31622, 0.07027.`0x03a9f934`, CB_BLEND_CONTROL `0x0d010d01`: source·1 + receiver·constant), colors - RGB × alpha:

```text
L3 = L4·color4 + L3·color3
L2 = L3·color3 + L2·color2
L1 = L2·finalgather + L1·(finalgather·color1)
```

The result is L1 (1/8 frames); it is `cBloom` in KSys `hdr_compose` (HDR view `+0x184`). The path of `ex_type` ⁇ 0 (`0x03a9fac8`) and the final mixing by `finalblend` (end of `0x03aa0714`, at `param_5 == 0`) are not involved in this path - the assumption is that KSys folds the bloom itself into `hdr_compose`.

Field data: `master_field.baglblm` — `threshhold` 2, `threshold_range` 0.1, `intensity` 0.1, `clamped_luminance` 64, `color1..4` = (1,1,1,0.5), (1,1,1,1), (1,0.48,0.3,1), (1,0.48,0.3,0). `normal.bwinfo` `EnvPaletteStatic`: `BloomLayerColor_8_8` (1,1,1,0.5), `_16_16` (1,1,1,1), `_32_32`/`_64_64` s `NoUse`=1, `BloomComposeColor` (1,1,1,1); palettes- `BloomThreshhold` 0.75–1.5, `BloomIntencity` 0.1–0.15, `BloomClampedLuminance` 2–30; weather – threshold multipliers (1, 0.1, 0.5, 0.25) and force (1, 1, 1.25, 1.5). **Untraceable.**How KSys transmits these `Bloom*` b the object bloom: the viewer substitutes them by name (threshold, force, clump - instead of values) `baglblm`,silhouetteless `NoUse` instead `colorN`, `BloomComposeColor` — `finalgather`Palette fields registration: `0x0363d76c`, `0x0363ef20`, `0x0363f270`, `0x0363f6b4`.

## Reproduction and verification

Local artifacts, outside of Git: `game-data/reference/visual-formulas/shader-archives/uking_pass_shader/` contains manifest and separate `.code`/`.gx2` from the general extractor; `visual-formulas/env.genvb` and `postfx-probe.json` contain local data. Absolute roots are taken from the local configuration.

```sh
python3 tools/research/postfx_probe.py /absolute/uking_pass_shader.sharcb \
  --offset 0xc2800 --size 800 --input-control 0x100 --verify-formula
python3 tools/research/postfx_probe.py /absolute/uking_pass_shader.sharcb \
  --offset 0xc2e00 --size 816 --verify-lut-coordinates
python3 tools/research/postfx_probe.py /absolute/agl_technique_pfx.sharcb \
  --offset 0x5500 --size 928 --clause 0 --verify-identity-map
python3 tools/research/postfx_probe.py /absolute/agl_technique_pfx.sharcb \
  --offset 0x11300 --size 1272 --clause 0 --verify-hsb-map
```

Own probe (upstream reference: `../../tools/research/postfx_probe.py`) It takes an external archive, checks boundaries and limited control flow, displays native ALU groups and hashes. It does not contain game bytes, does not support arbitrary Latte programs, does not execute text from the graphic pack. `--verify-formula` compare **Decoded native operands** with the formula: 3009 RGB/uniform cases, maximum absolute error `3.3306690738754696e-16`This is an algebra test on f64, not a precision comparison of f32 hardware. `1e-5..100`, three uniform pairs; selected uniform - test values, not recovered CPU. In addition: 1003 cases LUT coordinates, max error `2.22e-16`1003 cases of identity-map, max error `3.41e-8`3009 cases of HSB-map, max error `4.09e-8`LUT-map error relative to ideal `i/7` Numerical analysis of the generator compares RGB registers to the formula; vertex mapping, export/render-target state and filtering are not emulated.

The `check-docs` and `check --scope tools` tests were also rejected (17 tooling tests); five synthetic regression probe tests passed, including changes to cache/index/predicate flags and EXPORT source/swizzle/burst; the corresponding mutations of the real PS547 were also rejected. Command: `python3 -B tools/research/postfx_probe_test.py`. Numeric verification checks the shape of texture inputs and output before comparing formulas; the modified export cannot pass as the previous formula. Asstry of existing `aamp_dump`: offline/rel successful. AAMP master-field resources are read; raw file has run, and no gamereaser is controlled by the original GPU / display.

The next verifiable step is to trace the late recordings of the fields found, the source of the LUT and its inclusion flag, then the generation of the LUT and the sampler state. The capture of a particular frame remains a separate future step, the user does not have to repeat the bypass now. Only then combine the palette data with the formula and change the game renderer. Mass editing of `uking_mat` is not necessary for the first proven result.

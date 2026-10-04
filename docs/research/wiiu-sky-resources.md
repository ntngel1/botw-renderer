# Wii U: Cloud resources and the sky table

Date: 2026-09-28 Subsystem: sky, clouds, projection shadows. Context:  accuracy (archived reference),  visual divergences (upstream reference: `../STYLE.md`),  formats (archived reference). Restored the CPU chain → LUT of the sky, its baking and sky dome (`sky_postfx_sky`, options 0, 8, 12) with uniforms sources; in the viewer – `sky_lut.rs` and `sky_haze.wgsl`.

## Question and identity of sources

You need to determine where the textures designated `BaseTextureNo` and `NoiseTextureNo` are, how they relate to cloud parameters, and what can be proved about `sky.skybin` without guessing by shader names.

- Dump: Wii U, update `meta/meta.xml`: `WUP-P-ALZP`, `title_version=208`.
  This is a service pack ID; the game version is not independently defined here. The root is allowed from local `renderer.toml`.
- The `base/content` and `update/content` archives are different, but
  The four internal resources listed below match bytes between the layers. The conclusions are not extended to all the resources in the game.
- Decompilation: local zeldaret/botw, commit
  `5b26254bfe69560b0993a50d8ba8f418ae749380`, the target of **Switch 1.5.0**. This source does not prove the same logic on the Wii U.
- Ghidra: the lead agent gave the results of the spot analysis of the same Wii U
  `code/U-King.rpx`, SHA-256 `ba58da5b95ce929e005d058ceb08b9b2788d1ab2bbc8a6c189bbadca0bb34d30`. Ghidra 12.1.4, RPX Loader 0.9.2 with Espresso language; pre-disassemble/createFunction without full autoanalysis, no all callee signatures. Below are clearly highlighted the conclusions from this CPU source. The original game for pairs observation was not launched.

SHA-256 Source Archives:

| Layer/archive | SHA-256 |
|---|---|
| base `Pack/Bootup.pack` | `3814f6adeb78cae6bb2f44b1ae4f3be229ec4f91c9236ec8704ac77eb7f2929f` |
| update `Pack/Bootup.pack` | `489831ec22e0ee0dfdcbc43ce55d9561471e7c83ff985a3f6cb36bd15a3dbb05` |
| base `Pack/Bootup_Graphics.pack` | `d098ac616ce73bd423bf9cd324658c44343c920b4092125a2f74a1b02c4d0725` |
| update `Pack/Bootup_Graphics.pack` | `dcab53143116996f63ebf9ffd1d6e3145744a24c74945a8338f631b811fee79c` |

SHA-256 internal resources after removal of transport compression:

| Resource | Size, byte. | SHA-256 |
|---|---:|---|
| `Bootup.pack → Env/env.sgenvb → collect.genvres` | 1255424 | `9699dd00e5b1e78826bdb41268da6bdc33586acecd651c8e35e831a5c657a45c` |
| `postfx/master_field.baglclwd` | 2400 | `c0a385c6b5c1415d970f3463a0f28e07ba727eba76964ab44efd16beebe84127` |
| `postfx/master_field.bksky` | 352 | `79fe2c1f6912f377cae8fd88d5c77d44f0fb96079ff87f5e3dc7902935160674` |
| `Bootup_Graphics.pack → System/KSys/sky.skybin` | 1212416 | `c80e06dff16bc22dcd9ae00b7f5cc294988489a5866388030a87fcb3ac918741` |

## Found source of cloud textures

**Dump Fact:** `collect.genvres` - BFRES `0x04050003`, named `genv_res`. It's in the `Env/env.sgenvb` environment archive, not in SystemModel or AGL archives. The existing BFRES reader has parsed five FTEXs:

| Texture | Size | GX2 format | Mip levels | Tile mode / swizzle |
|---|---|---|---:|---|
| `cloudtexture02` | 512 × 512 | `0x34`, BC4 unsigned | 10 | 4 / `0x30000` |
| `cloudtexture03` | 512 × 512 | `0x34`, BC4 unsigned | 10 | 4 / `0x30000` |
| `cloudtexture04` | 512 × 512 | `0x34`, BC4 unsigned | 10 | 4 / `0x30000` |
| `indwp_ia4_00` | 32 × 32 | `0x35`, BC5 unsigned | 6 | 2 / `0x0` |
| `p_shadow_clouds` | 1024 × 1024 | `0x34`, BC4 unsigned | 11 | 4 / `0x40000` |

All `dim=1`, `depth=1`. Additional direct proof of origin: `env.bgenv`, `convert_parts_array/2`, describes the conversion of `nw4f_bin`, `arg=lump`, `name=collect`, `ext=genvres` and lists:

```text
./cloudTexture/CloudTexture02.ftxb
./cloudTexture/CloudTexture03.ftxb
./cloudTexture/CloudTexture04.ftxb
./res/indwp_ia4_00.ftxb
./res/p_shadow_clouds.ftxb
```

It's not just a string match in an executable file: BFRES contains the surfaces themselves with the data, and the record in the former FORMATS section about the undiscovered textures of the sky is now being refined by this source.

**Imaging Check:** level 0 is unpacked through the existing `Texture::level0_slice` (GX2 untiling) and `bc::decode_bc4`; four gray images are viewed. `cloudtexture02` are smooth large spots; `cloudtexture03` and `cloudtexture04` have similar cirrus pattern but different brightness ranges. `p_shadow_clouds` is a separate ready-made cloud patch mask on a white background. These are observations of assets, not game frames.

Measured ranges after BC4 decoding (0-255):

| Texture | Min / max | Average. |
|---|---|---:|
| `cloudtexture02` | 62 / 217 | 97.7203 |
| `cloudtexture03` | 11 / 64 | 30.6822 |
| `cloudtexture04` | 83 / 169 | 125.0354 |
| `p_shadow_clouds` | 0 / 255 | 227.3591 |

Correlation of the corresponding pixels 03 and 04 is 0.99052. This is a measurement of the similarity of the source maps, **is not a formula for obtaining one of the other** and does not replace the original data with procedural noise.

## Resource Chain and Confirmation Boundary

`postfx/master_field.baglclwd` is active in `CloudParam0` and `CloudParam2`. Both `mBaseTextureNo=2`, `mNoiseTextureNo=3`, `mBaseTextureNo_Blend=4`, `mNoiseTextureNo_Blend=3`, `mbCloudTexBlend=true`, `mUseProcedualTexture=false`, `mUseScatter=true`. `CloudParam1` is off; it has `mBaseTextureNo=0`.

**Strong hypothesis:** numbers 2, 3, 4 correspond to the suffixes of `cloudtexture02/03/04` names. In BFRES, these are elements 0, 1, 2, so you can not treat the parameter as the zero index of the entire FTEX table: for example, element 4 is `p_shadow_clouds`. The comparison of "number → name" still requires a CPU registration code / texture selection. The value of 0 in the deactivated layer is not explained.

AGL-archive `Bootup_Graphics.pack → System/Agl/agl_resource.Cafe_Cafe_GX2.release.ssarc → agl_technique.sharcb` contains the strings of the program `cloud`, switches `TYPE_USE_TEX_BLEND`, `TYPE_USE_DEBUG_SUN_DISP`, `TYPE_USE_PROC_TEXTURE`, `TYPE_USE_SCATTER` and sampler-names `cBaseTexture`, `cNoiseTexture`, `cBaseTexture_Blend`, `cNoiseTexture_Blend`, `cScatterTexture`. Metadatadata is consistent with the parameters of the resource, but string search does not prove any specific bindings, GPU formulas, or the choice of executable variation.

**Direct connection of the projection shadow:** into `env.bgenv` set-up `Master_Field`, `refer/26`indicates `file=collect.genvres`, `name=p_shadow_clouds`, `signature=prjshd`, `ext=ftx`. `refer/22` plug-in `postfx/common.bgsdw`his `projection_shadow_0.proj_name=shadowTex_Projector`, `repeat=true`, `density=1`. `envobj/master_field.baglenv` switched on `Projector0` with that name, `view_pos=(0,8000,0)`, `height=1000`, `far=10000`Thus, the environment set clearly contains a separate texture for the projection shadow. `bias_trans` encirclement manager, strength by `CloudShadowOnOff`, a sample in PS 112 - [chipboarding](wiiu-render-cpu.md#cloud-shadows-the-projection-shadow-cloudshadowonoff--proj_shadow_off) (2026-09-29; not verified by frame).

`indwp_ia4_00` in the same set has `signature=doftex` (`refer/2`); it is wrong to consider it as a cloud noise map for one shared container.

The static values of baglclwd are not equal to the proven runtime state: for example, the heights of the two layers included here are 7895.27 and 6836.5225, and `mCloudTexBlendRate` are 1 and 0.417255.

## What's Really Established About Sky.skybin

The resource has 12,12416 bytes; there is no recognizable container header, the first 128 bytes are zero.

| Interpreting | Number of values | Finite | Range of endpoints | Negative |
|---|---:|---:|---|---:|
| little-endian IEEE binary16 | 606208 | 606208 | 0 … 1.9853515625 | 0 |
| big-endian IEEE binary16 | 606208 | 592974 | −59328 … 59328 | 199107 |

This is a significant reason to examine the file as little-endian half-float data, despite the big-endian CPU Wii U. By itself, it **does not prove RGBA16F**, number of tables, dimensions, tiling, stride and channel order. Independent data on the surfaces below are derived from the CPU, not by selecting the deuce decomposition of size.

**Preliminary analysis of native Wii U consumer.** In `FUN_03405f48` , initialization of arguments for `FUN_033f358c` writes `[256,64,64,64,16,32,32,8,256,256,64,64]` numbers in `args[4..15]` fields; Late overwriting of `uStack_3e4=0x10` is taken into account. `FUN_033f358c` transfers them to `this+0x198..0x1c4`, calls `FUN_033f2980` to create surfaces, and when `FUN_033f3558(this, data, size)` is available for download.

In `FUN_033f2980` , three initial `FUN_03a7e618` calls are visible:

| Object in this | Dimension enum | Format enum | Width × height × depth |
|---|---:|---:|---|
| `+0x1f8` | 1 | `0x2b` | 256 × 64 × 1 |
| `+0x374` | 2 | `0x2b` | (8 × 32) × 32 × 16 |
| `+0x4f0` | 1 | `0x2b` | 64 × 64 × 1 |

`0x2b` here **internal enum AGL**GX2 format ID is the native Wii U chain. `FUN_03a7e618 → FUN_03b4abb0 → FUN_03b485b8` transduce it through a table `0x1047ed60`: entry 43 by `0x1047ee0c` It contains a large-endian u32. **`0x820`**, verified by the lead agent in `.data` The original RPX. This is GX2. `R16_G16_B16_A16_FLOAT`RGBA16F and 8 bytes on texel. `FUN_03b4abb0` This is recorded in the surface format field.`param_1[5]`), not just stored next to an opaque enum.Local Switch AGL `lib/agl/include/common/aglTextureEnum.h` It also calls itself enum 43. `cTextureFormat_R16_G16_B16_A16_float`But the proof for Wii U now relies on its own conversion table: the Mips count in surface init is 1, the initial tile mode field is 0; the final layout after calculating the GX2 surface still needs to be checked.

`FUN_033f3558` reads the `this+0x2d8`, `+0x454`, `+0x5d0` dimensions, fails to match their sum with the resource size, then makes three consecutive copies from `data`, `data+size0`, `data+size0+size1`. Thus, the CPU really expects the **to have three consecutive images of surfaces without a separate** header. At eight bytes on texel, these dimensions accurately give the entire file:

| Sky.skybin range | Size | Surface. |
|---|---:|---|
| `[0, 0x20000)` | 131072 | 256 × 64 × 8 |
| `[0x20000, 0x120000)` | 1048576 | 256 × 32 × 16 × 8 |
| `[0x120000, 0x128000)` | 32768 | 64 × 64 × 8 |

This is a **layout by size of native surfaces,**, backed up by their dimensions, RGBA16F and exact matching of the total file size. The images turned out to be tile: see " `sky.skybin` layout" in the [ section of the LUT](#cpu-chain--lut-of-the-sky-matching-wii-u-2026-09-28) chain. When reading these ranges as `<4e` on tables 0 and 2, the fourth component is everywhere zero; Table 0 has RGB at 0...0.999512, Table 1 - RGB to 1.985, and Tabletr2, this must be linked via texturetaskater 214, but it must be added via the tabulated taglined object transferragonalter 2112.

`master_field.bksky` has density/scattering parameters: `rayleigh_base_height=24`, `mie_base_height=2`, `mie_scattering_coeff=0.00183`, `mie_symmetrical_prop=0.8`, separate rendering parameters (`mie_symmetrical_prop_rendering=0.85`, `mie_amplifier_rendering=8`) and the sun color `(1,0.86,0.68,18)`. Units of height and the role of alpha color from the AAMP itself do not follow.

In `agl_technique_pfx.sharcb` and `uking_pass_shader.sharcb` are the names `sky_transmittance`, `sky_irradiance`, `sky_inscatter`, `sky_delta_inscatter`, `sky_copy_inscatter`, `sky_copy_irradiance`, `sky_bake_inscatter`, `sky_bake_irradiance`, `sky_bake_range_transmittance`, `sky_postfx_sky`, `sky_postfx_ground`; also parameter/textural names `cHeightMie`, `cHeightRayleigh`, `cScatteringCoeffMie`, `cScatteringCoeffRayleigh`, `cSizeBakedInscatter`, `cSizeBakedRangeTransmittance`, `cTexInscatter`, `cTexBakedInscatter`, `cTexTransmittance`.

**Conclusion:** , a collection of half-like data, physical parameters, and shader interfaces, supports the hypothesis of pre-calculated atmospheric tables. It does not yet link specific ranges of `sky.skybin` to specific samplers. The statement that “the game uses the exact Bruneton algorithm” is not confirmed here: integration formulas, LUT coordinate mapping, and number/order of passes, not just similar names.

## Restored formula sky postfx sky, option 0

Source: `Bootup_Graphics.pack → System/KSys/ U-King.Cafe_Cafe_GX2.release.ssarc → uking_pass_shader.sharcb`. A structural reader of `shader_archive` highlighted the program `sky_postfx_sky`, binary **419**, pixel shader. All four switches are 0: `BAKED_SUNVIEW_NON_LINEAR`, `USE_ADHOC_FOG`, `RENDER_SUN`, `RENDER_CLOUD`. This is an existing version of the archive; its actual use in the selected frame has not yet been observed.

- The code range inside the unpacked SHARCB is `[0xa7400, 0xa7610)`, 528 bytes.
- SHA-256 code:
  `fb5bf63ddeeda4929f94be7e087a5d7b0a65bb3492f112de9101f870d2b28f46`.
- Reflection: `cAmplifierAdhoc` offset 0, `cGroundColor` offset 4,
  `cSunDir` offset 8 in scalar uniform register file; sampler `cTexBakedInscatter`, slot 0, 2D.
- CF: ALU `[0x100,0x190)`, TEX `0x200`, ALU `[0x190,0x1c8)`,
  `EXPORT_DONE` from `R1.xyzw` to pixel target 0. All offsets are here relative to the beginning of the code.

The analysis of ISA fields was compared with the primary Cemu sources: [LatteInstructions.h](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/ISA/LatteInstructions.h), [LatteDecompiler.cpp](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompiler.cpp), [opcode definitions](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompilerInstructions.h)This is the description of the ISA, not the source of the game formula: the formula is derived from its own shader bytes, taking into account the parallelity of the ALU groups, PV/PS, the output multiplier. `DIV2` and clamp.

For the non-zero input direction `D = R0.xyz`:

```text
d = normalize(D)
u = 0.5 + 0.5 * dot(cSunDir, d)
v = 0.5 + 0.5 * d.y
s = sample(cTexBakedInscatter, (u, v))
a = clamp(s.a + cGroundColor.a, 0, 1)
out.rgb = cGroundColor.rgb
        + (s.rgb * cAmplifierAdhoc - cGroundColor.rgb) * a
out.a = 1
```

The first ALU instructions contain `DOT4_IEEE` and `RECIPSQRT_IEEE` for normalization, then `DOT4 / 2` with `cSunDir` and `MOV / 2` of normalized Y. The term 0.5 comes from the constant selector 252. TEX: opcode `0x10`, texture/sampler 0, normalized coordinates, identity destination swizzle, zero offsets. The second ALU-clause literally implements the shown interpolation with saturated alpha. The name `cAmplifierAdhoc` does not change the fact: this multiplier reads even in this variant of `USE_ADHOC_FOG=0`.

This is how both **coordinates of this baked 2D LUT** are established: the cosine of the angle between the sun and the direction of the gaze and the vertical component of the gaze, linearly transferred to the range of 0-1. A separate azimuth coordinate is not needed here. This does not prove the format of the original `sky.skybin`: the baked table can be the result of a separate passage from preloaded tables.

The research sky probe.py (upstream reference: `../../tools/research/sky_probe.py`) decodes a limited set of CF/ALU/TEX, calculates the result from decoded operands and compares it to a formula per 1,000 deterministic random inputs. The maximum absolute error of `2.220446049250313e-16` for UV/RGBA. In addition, probe rejects 10 mutations of supported code that add predication, change execution uniform, relative 2, cache/index flags, waterfall/whole-quad or conditional execution of TEX. So unsupported sample pixel GXQ2 is not yet validated by pixel interfiltr, this means that the pixel filtering is not actually checked in the nucleargemicular direction of the other pixels, fure / square, and fbrating pixels are not tested in fact vestimages.

```sh
python3 tools/research/sky_probe.py /path/to/00419-stage1.code
```

Additional narrow result: `sky_copy_inscatter`, `LOCAL_STEP=1`, binary 401, code `[0xa2100,0xa22a0)`It contains two TEX instructions for the same XYZ coordinates.`+0x180`) reads sampler 1, `cTexDeltaSR`in `R1.rgb` and masks the alpha; the second`+0x190`) reads sampler 0, `cTexDeltaSM`, recording only him **R in R1.a**The subsequent export is giving away. `R1.xyzw`This is a direct swizzle/write mask of bytes: packed result has `(DeltaSR.rgb, DeltaSM.r)`It's not the sum of the RGB of the two textures. DeltaSR/DeltaSM filters and formulas are not recovered.

## CPU chain → LUT of the sky (matching Wii U, 2026-09-28)

Sources: Ghidra of the same `U-King.rpx` (function names saved in the project), Cemu GLSL from [ cache session ](wiiu-cemu-sessions.md) (`250cb5a0…_ps` = PS 413, `2e5c98a2…_vs` = VS 406-412, exact match of bytes), `normal.bwinfo` v208. Sample: sky bake probe.py (upstream reference: `../../tools/research/sky_bake_probe.py`) and example `sky_skybin` (`the original format parser`).

**Model: Bruneton 2008 (code fact)** Shaders use Rg = 6360, Rt = 6420 km, H = √(Rt2 - Rg2) = 875.6712, mapping coordinates `texture4D` Bruneton (μ, μs through 1 − e^(−3μs − 0.6), ν - slices with linear interpolation). `SKY_CalcAltitudeTextureParams` (`0x033f099c`, 356 bytes; Ghidra, 2026-10-01) is not this mapping but the parameters of the Bruneton layer (Brunetton layer).`getLayer` + `dhdH`): by the proportion of height x r = √(63602 + (x·H)2) (H2 = 766 800), at the edges r + 0.001 at x ≤ 0 and r - 0.001 for x ≥ 1; output (Rt − r, ρ + H, r − Rg, ρ), ρ = √ (r2 − R g2). `SKY_PrecomputeAtmosphereTables` (twice) `0x033f8b4c`, `0x033f927c` three-seat `SKY_BakeInscatterLut` (PPC `0x033f62cc`, `0x033f65e8`, `0x033f68d4`): there x = `cAltitude`The result is uniform 15 of the 6/7/8 programs. Whether PS 413 reads version 3 has not been checked (the formula below takes r = 6360 + 60·).`cAltitude` VS); the viewer does not build this uniform. `SKY_CalcRayleighCoefficients` (`0x033f12f8`): β R over wavelengths 0.7 / 0.546 / 0.436 μm (`0x102bfb08`), refractive index according to the formula 0.05792105/(238.0185 - λ−2) + 0.00167917/(57.362 - λ-2) (+1 is a constant in uninitiated data, accepted 1), β = 8π3(n2 - 1)2/(76.5 λ4)·1000. `SKY_PrecomputeAtmosphereTables` (`0x033f3d44`) - complete predation in the game ("Single Irradiance", "Single Scattering Rayleigh / Mie", "delta J", "Delta J", `+0x680` = 8 orders of magnitude); after loading `sky.skybin` It's off (flag) `+0x88c` bit 16 takes off `SKY_LoadSkybinSurfaces`, `0x033f3558`), and static palette parameters (`EnvPaletteStatic`15/1/0.0018/0.8) come through `SKY_SetStaticScatteringParams` No flag counting, so the tables `sky.skybin` The frame is not recalculated; each frame is baked only by LUT.

**Layout `sky.skybin` (observation).** File - images **tiles** GX2 surfaces (CPU copies their memcpy). Linear reading gives mixed blocks 128×32. Tile mode bruteness by the existing untiler: inscatter 256×32×16 - mode 7 (2D tiled thick), roughness 0.026 vs. 0.041 in mode 4; after it, slices are a clean table (8 ν × 32 μs by x, 32 μ by y, 16 altitudes by z). Transmittance 256× 64 - mode 4 (0.0042; 7 - 0.44), then the empirical solution for Gile mode is not validated by 3x64. `botw_formats::sky` (`parse_inscatter`/`load_inscatter`) decodes only inscatter (mode 7), the only table read by a PS 413 baked product; on a dump v208 via `Bootup_Graphics.pack` coincidental `sky_skybin` bit-to-bit (example) `sky_inscatter --compare`Transmittance and irradiance are not decoded until their mode is confirmed.

**Program Table** (`0x102bf050`, 11 number-to-name pairs): 0 `sky_transmittance`, 1 `sky_irradiance`, 2 `sky_inscatter`, 3 `sky_delta_inscatter`, 4 `sky_copy_irradiance`, 5 `sky_copy_inscatter`, 6 `sky_bake_inscatter`, 7 `sky_bake_irradiance`, 8 `sky_bake_range_transmittance`, 9 `sky_postfx_sky`, 10 `sky_postfx_ground`. Uniforms names - `SKY_BindProgramUniformNames` (`0x033f7c88`).

**Frame Baking** - `SKY_BakeInscatterLut` (`0x033f5a98`), program 6 for the purpose of recording LUT (`+0x670`, record 0xd6c; 256 × 256, `+0x1b8/+0x1bc`). Option choose bits `+0x88c` 19 and 24; constructor (`SKY_ConstructSkyObject`, `0x033f164c`) puts `0x1090000` → option 3 (`BAKED_SUNVIEW_NON_LINEAR=1, ADHOC_PROC=1`, PS 413). In the dump Cemu of four options is only him (dump incomplete, so this is consent, not proof).

| Uniform | Source (field of the object of the sky) |
|---|---|
| `cAltitude` | Y Camera / 60,000 (km / 60) |
| `cSunZenithAngle` | Y normalized −`+0x72c…0x734` (to the sun) |
| `cAmplifierReyleigh`, `cAmplifierMie`, `cSymmetricalPropertyMie` | `+0x708`, `+0x728`, `+0x718` |
| `cSunColor` | `dynamic_color` `+0x74c`: rgb × a |
| `cFade` | `+0x760` |
| `cScatteringCoeffRayleigh` | `+0x6c0` (β R above) |
| `cEffectiveBelowHorizon` | byte 3 of the LUT record (initialized 1) |

In the same place, `+0xd64` = 6360 + Y/1000 (camera radius, km) and `+0xd68` = −√(1 − (6360 / r)2) are written in the record. This is the **e27.w** of the fog PS 140 ([razbekz](wiiu-deferred-shading.md#pre-shading-fog-ps-140-read-in-cemu-glsl)). The flag 18 additionally bakes irradiance (program 7) and range transmittance (program 8, `cNearFar`).

**Formula PS 413/VS** (Cemu GLSL; compliance; `uf_remappedPS[i]` Uniforms is based on the role in the formula, because `cemu_uniform_map.py` Not applicable to this shader -- interpretation. For Texel (u, v) LUT:

```text
r    = 6360 + 60·clamp(cAltitude, 1e-4, 0.9999)          (VS)
μh   = −√sat(1 − (6360/r)²)                              (VS)
μv   = 2v − 1;  μ = max(μv, cEffectiveBelowHorizon ? −1 : μh + 1/256)
ν    = 2·sin(u·π/2) − 1                                  (NON_LINEAR)
g'   = g·max(sat(2u − 0.25), sat(μ + 0.75))              (ADHOC_PROC)
S    = texture4D(inscatter, r, μ, μs = cSunZenithAngle, ν)   (Bruneton, max 0)
M    = S.rgb·S.a / max(S.r, 1e-4) · (β_R.r / β_R)
out  = cSunColor·[ ampR·3/(16π)(1+ν²)·S.rgb
                 + ampM·3/(8π)(1−g'²)(1+ν²) / ((2+g'²)(1+g'²−2g'ν)^1.5)·M ]
out = lerp(out, same for ν = 0 and g without ADHOC, cFade) (if cFade > 0)
alpha = μv < μh ? 0 : 1
```

**Parameters from the weather update** (`ENV_UpdateWeatherPalettes`, `0x036425b8`; Wii U palette fields in step `agl::utl::Parameter`): `SkySunColor` (+0xb0; `SkySunColorNoUse` +0xa0 ⁇  0 color - `BgDifColor` palettes +0x0cThe intensity is alpha. `SkySunColor` +0xbc always; PPC; `0x036455c8..0x03645a70`, reread 2026-09-29) `dynamic_color` (on the line 0 × `F`how `BgDifColor`, `0x036460f4`traced 2026-09-29); `SkyRParam_rayleigh_amplifier` (+0x12c), `_mie_amplifier` (+0x14c), `_mie_symmetricalProperty` (+0x13c) → `SKY_SetDynamicScatteringParams` (`0x033f7a98`, valves ≥ 0, g in [0, 1]Each is a mixture of four palettes (division of day × sky) and two climates, like fog. For the line, the palette of field (0) is multiplied by climate factors ().`ClimateDefines`: `FeatureColor` +0x250, `CalcRayleigh` +0x26c, `CalcMieSymmetrical` +0x27c, `CalcMie` +0x28c) and weather (`WeatherInfluence_N` +0xc, +0x44, +0x54, +0x64; mix of two weathers at the transition). Weather index: 2 and 4 (rain, snow) → 2; 3, 5, 7 (strong) → 3; 8 (rain in the sun) → 1; otherwise 0. Mi-amplitude additionally stretches to 1 by `(1 − SkyMgr+0x2178)`; field = 1 (record only when reset) `0x0364f324`), the same step ([scalar](wiiu-render-cpu.md#main-light-env5-and-the-palettes-exposure)In the Monster dungeons (`StageType` (4) to this is added `Remains_N`. `cFade` -No `SkyIsotropicfade` palettes, and `SKY_CalcNightFadeByTime` (`0x03642468`), time d in degrees of day (15° = 1 h): f = 1 − (330 − d)/15 at (315, 330), (345 − d),/15 at [330, 345), 1 - (45 - d)/15 at (30, 45)], (52.5 - d)/7.5 on (45, 52.5), otherwise 0; result min(2f, 1) - shelf 1 from 21:30 to 22:30 and 2:30 to 3:15. `SkyIsotropicfade`No, no, no, not a direction writer. `+0x72c` for `cSunZenithAngle`- sky calculation `0x03656be0` (2026-09-29): at night (22:00-03:00) is an arc of night light, during the day - the path of the sun, without cutting. `SunDirYStop` ([analysis](wiiu-render-cpu.md#main-light-direction-and-the-skys-sun-0x03656be0)); shelves `cFade` And the ones above are on his shifts (22:00, 3:00). SKY-LUT-001 (archived reference).

Values v208: `WeatherInfluence_2` (rain) - `FeatureColor` (0.525, 0.614, 0.729), Raleigh 0.5, Mi 0.75, g ×1; `_1` - (0.6, 0.6, 0.6), 1/2/0.8; `_3` - (0.329, 0.434, 0.567), 0.5/0.75/1. Midday palette of field 3: `SkySunColor` (1, 0.92, 0.85, 18), Raleigh 1, Mi 12, g.75; cloudy 11: (0.73, 0.8), 0.8, 0.

**Offline baking (observation, not a frame of the game).** Sample on the untiled table (height 134 m, μs = 0.9): clear (palette 3) - zenith (0.11, 0.26, 0.61), horizon away from the sun (1.38, 1.71, 1.67); cloud (11) - (0.016, 0.052, 0.092) / (0.24, 0.36, 0. 26); rain (11 × weather 2) - (0.005, 0.017, 0.034) / (0.074, 0.121, 0.097). Horizon shade in cloudiness and rain gray gray-gray (11) - as a clear R/233 - as a frame of R-237 - 5-115 - a clear game;`cAmplifierAdhoc`, palette exposure, untraceable. `visual-formulas/sky/bake/` (`compare-lut-game.jpg`Sampler filtering (clamp, trilinear) accepted without verification.

Not installed: tile mode 2D tables; path LUT to `gsys_user0` ( `+0x670` → sampler PS 140 – by name, not by code).

## Sky Dome: sky postfx sky, options 8 and 12 (matching Wii U, 2026-09-28)

**Shaders (Cemu GLSL, cache session, exact match of bytes).** PS 435 (Variant 8), `BAKED_SUNVIEW_NON_LINEAR=1`) = `4e21d093c6136a15_…79_ps`PS 443 (Variant 12, more) `USE_ADHOC_FOG=1`) = `53422d4f43914447_…79_ps`, VS 442 = `a8a2f58c13258ade_vs`Correspondence. `uf_remapped*` Uniforms - by reflection archive and order of first use (as Cemu numbers; in PS 435 order) `cSunDir`, `cAmplifierAdhoc`, `cGroundColor` For the direction of the view d (normalized):

```text
c  = 0.5 + 0.5·dot(cSunDir, d)
u = 1 − (2/π)·acos(c) (acos - polynomial √(1−x)·(1.5707288 − 0.2121144x)
                                  + 0.0742610x2 − 0.0187293x3); inverse to ν baking
v  = 0.5 + 0.5·d.y
s  = cTexBakedInscatter(u, v)
a  = sat(s.a + cGroundColor.a)
sky = cGroundColor.rgb + (s.rgb·cAmplifierAdhoc − cGroundColor.rgb)·a (as option 0)
- only option 12 (P = cNormalFogCoeff):
f = √sat(4·P.w) · lerp(P.w, P.z, sat(d.y)^P.y) (√ - in VS 442, the rest in PS)
out = lerp(sky, cNormalFogColor.rgb, f)
```

**CPU (Ghidra, names saved).** Draws `SKY_DrawPostfxSkyDome` (`0x033f6a08`, program 9). Option: `BAKED_SUNVIEW_NON_LINEAR` - bit 19 `+0x88c`, `USE_ADHOC_FOG` - `+0x81c > 0 and +0x7f0 > 0`, `RENDER_SUN` - bit 7, `RENDER_CLOUD` - clouds in the LUT record (`+0x478`). Uniforms:

| Uniform | Source |
|---|---|
| `cAmplifierAdhoc` | argument: **1.0** from the main view (`FUN_034030c4`, step 3), `+0x878` (`amplifier_for_envmap`) from the second call (`FUN_0340b6cc`, with the sun off; in meaning - env map) |
| `cSunDir` | −`+0x72c…0x734` (same vector as `cSunZenithAngle` baking) |
| `cGroundColor` | `ground_color` bksky (`+0x85c…0x864`), a = 1 − sat(`+0x868`) |
| `cNormalFogCoeff` | (`+0x7e0`, `+0x7f0`, `+0x800`, `+0x81c`) with `+0x81c > 0`, otherwise (0, 0, 1, `+0x81c`) |
| `cNormalFogColor` | (`+0x810…0x818`, 1) |

Fields of the object of the sky - parameters bksky (designer) `0x033f164c`): `adhoc_fog_near/far` (+0x7c0/+0x7d0), `_atten_grd` (+0x7e0), `_atten_sky` (+0x7f0, ≥ 0.5), `_atten_minscale_sky` (+0x800), `adhoc_fog_color` (+0x810, rgba), `ground_color` (+0x850 object, value +0x85c), `amplifier_for_envmap` (+0x878) `master_field.bksky` v208 `ground_color` = (0.5, 0.4, 0.3, **1**), so `cGroundColor.a` = 0: above the horizon - LUT (s.a = 1), below is the color of the earth. `+0x85c` Not in the weather update.

**Ad hoc fog every frame** (`ENV_UpdateWeatherPalettes` →  `SKY_SetAdhocFogParams` `0x033f7bd8`; lamps: atten ≥ 0, atten sky ≥ 0.5: color and force - object `fog_scatter` (KSys+0xa4: +0xe8...+0xf4), near/far – its Start/End (+0xb8, +0xc8), atten grd – palette `afParam_attenuationForGrd` (+0x10c), atten_sky — `afParam_attenuationForSky` (+0x11c), minscale is the constant **0.3**. `fog_scatter` It's also written in ibid., rgb. `FogColor` palette `FeatureFogColor` weather `FeatureColor` climate (only on the line palette of field 0, `PaletteSetSelect`weather index as in `TempMgr`; parsing 2026-09-29 in [CPU](wiiu-render-cpu.md#main-light-env5-and-the-palettes-exposure)), **alpha** - in field sets (`WEATHER_IsFieldPaletteSet`) **humidity `TempMgr`/100**no `FogColor.a`In sets 0 and 1 (clearly overcast fields), atten sky is replaced by humidity /100·4.8. Total in the field option 12 works always when humidities > 0 - in clear weather too; f near the horizon = humidification (at ≥ 0.25), at the zenith 0.3·√sat(4·humidity).`StageType` 4, `+0x3cf0c` s `+0x3ceb0`, smooth transitions `+0x3ce04…`, `+0x3ce8c`⁶).

**Bottom line on the brightness of bad weather.** `cAmplifierAdhoc` The frame is 1 - the dark cloudy LUT game does not amplify. The bright sky and the rainfall give the ad hoc fog of the dome and the same fog on the ground (B in PS 140, ed.). [analysis](wiiu-deferred-shading.md#pre-shading-fog-ps-140-read-in-cemu-glsl)): with rain humidity (climate base +`AddMoisture`20) it closes further `FogEnd` (400m in overcast palette) most of the scene is in color `FogColor` × `FeatureFogColor`Previous conclusion: "B off in clear weather" (by `FogColor.a` = 0) wrong for the field.

**Rain on this chain — decomposition in the viewer** (2026-09-30, Wii U v208, `HyrulePlainClimate` field, `weather/fieldcam_rain_1000` camera, 10:00; time log values, not included in the code) chain ad hoc fog and LUT in the viewer — games, compared its inputs:

| | `Rain` (set 1, overcast) | `BlueskyRain` (set 0, clear) |
|---|---|---|
| `FogColor` palettes 10:00 | (0.383, 0.443, 0.205) | (0.585, 1.0, 0.806) |
| `FeatureFogColor` weather | entry 2: (0.768, 0.88, 1.0) | record 1: (1.3, 1.2, 1.1) |
| `cNormalFogColor` | (0.294, 0.390, 0.205), olive | (0.760, 1.200, 0.887), turquoise |
| humidity | `MoistureMin/Max` 2.5–20 + 20 (22.8% in frame) | 2.5–20 + 15 (17.8 %) |
| LUT at the horizon from the sun | (0.114, 0.147, 0.097) | (1.49, 1.31, 1.02) |
| LUT at zenith (v = 250/256) | (0.005, 0.016, 0.031) | (0.076, 0.163, 0.344) |
| sky of the frame (`style_stats`) | L* 38.6, C 13.3, #505e4c | L* 84.9, C 6.2, #ccd6d1 |

R/712, R/717 (rain; weather not set): sky L* 78.8, C 17.3, #a3cbbc; linear fractions r/g 0.61, b/g 0.8 `cNormalFogColor` weather-pack `BlueskyRain` - 0.63/0.74, in overcast c `Rain` Conclusion: according to the game in ordinary rain, the field is olive and dark (the fog force ≤ 0.4 - humidity up to 40%, the LUT of an overcast set is 10-15 times darker than clear); the light turquoise sky of standards corresponds to a clear set, i.e., the shade of the bright turquosine sky of the standards corresponds with a clear set. `BlueskyRain` (The state of the sky is clear, `SKY_UpdateWeatherStateBlend`) or another climate. The choice of a reference RAIN-REF-001 (archived reference)The state of the sky in bad weather awaits clouds `SkyMgr+0x2120` > 0.2 ([writer](wiiu-deferred-shading.md)).

**Clouds and the same fog.** Ad hoc fog also falls on the clouds (end of PS 449). [parsing](#cloud-cloud-program-option-9-matching-wii-u-2026-09-28).

## Cloud: cloud program, option 9 (matching Wii U, 2026-09-28)

**Shaders and uniforms.** Programme `cloud` `agl_technique`option 9 (`TYPE_USE_TEX_BLEND=1`, `TYPE_USE_SCATTER=1`how `mbCloudTexBlend`, `mUseScatter` into `master_field.baglclwd`): VS 448 = `69ce21ae784ffad2_vs`, PS 449 = `1e5b65a56cff348a_…f249_ps` (Cemu GLSL, exact match). `uf_remapped*` And the blocks -- `tools/research/cemu_uniform_map.py` (bisection, 96 PS calls, all VS calls), names - reflection of option 9 (see below).`manifest.txt`block `Common`, shift uniform to float; block `View` — `cViewMat`, `cProjMat`, `cZOffsetParam`Roles are no longer guessed.

**CPU (Ghidra `U-King.rpx` v208).** Designer `agl::fx::Cloud` `0x03a57340` (`aglclwd`: `Cloud` + `CloudParam`, at 0x1070 bytes `+0x15b0`) designer `CloudParam` `0x03a55724` (the value of the parameter is by object displacement + 0xc). `FUN_03a59cf4`: `FUN_03a83f6c(block, …, i, significance)`, the number i coincides with the displacement in the float block `Common` (0x29 → `mCloudColorScale` 41, 0x2b → `mScatterHeight` 43 = `+0x224` Special cases: `mAlphaMul` = `mAlphaMul/(1 − mAlphaThreshold)` (at threshold  ⁇  1); `mScatterAmb` = **1 − `mScatterAmb`**; `mCloudColorScale` value `Cloud+0x400` (baglclwd **2.75**or 1 if `Cloud+0x4a4` ⁇  0 (designer sets 0); `mCloudColorScaleInv` = 1/`Cloud+0x400` always; `sysColor0Vary` = `mBaseColor`·`mBaseColorIntensity`, `sysColor1Vary` = `mHilightColor`·`…Intensity`, `cShadowCol` = `mShadowColor`·`…Intensity`, `cBackLightCol` = `mBacklightColor`; `cScaleMat` = rotation x scale (`mSkyScale`, `mSkyHeight`, `mSkyScale`)·`Cloud+0x4cbc`Every shot. `FUN_033ff8cc` copy `Cloud`: `+0x410` (→ `cNormalFogColor`) = ad hoc fog (`SKY_GetAdhocFogParams`: color, **a = force**, i.e. the humidity in the field, `+0x44c` (→ `cNormalFogCoeff.y`) = his `atten_sky`, `+0x42c/+0x43c` - his near/far; `cScatterFogDistance` = (1/(far − near), near/(far ‒ near) scatter-fog bksky (`SKY_GetScatterFogParams`), `cScatterFogCoeff` (atten, horz, density, ...) are the same as A PS 140. `AGL_ConstructCloudObject`, `AGL_ConstructCloudLayerParams`, `AGL_WriteCloudLayerUniforms`, `AGL_BuildCloudDomeVertices`, `ENV_CopySkyFogsToCloudObject`.) `ENV_UpdateWeatherPalettes` write `CloudParam` palette`Cloud*_Color*`, a mixture of time and weather, on the line of sets palettes 0 × `F` = `FeatureColor` weather × climate. [factor-book](wiiu-render-cpu.md#main-light-env5-and-the-palettes-exposure); `CloudParam` 0 and 2. `CloudParam` 1 cycle skips - take a line `Cloud{i}_*` Each of the eight palettes, with its `Cloud2NoUse` ≠ 0 — `Cloud2_*`; PPC `0x03647ef0..0x03648110`, reread 2026-09-29): base, hilight·(1 − n), shadow = lerp(shadow, base, n), backlight·( 1 − n) `mBacklightPower` = palette·(1 - n)·`SkyMgr+0x2154` (to 0 in the new moon, see "Other" below), where n - `SKY_CalcNightFadeByTime`Drawing. `FUN_039d1eac` and `FUN_0340b6cc` for env map); grid- `FUN_03a57cc0`.

**Grid.** Hemisphere 24 × 12 + top: ring j (0...11), r = 1 − (j/12)3, y = √(1 − r2) (below 0.1 compressed: 0.1 − 0.3·(0.1 − y)) − 0.07, (x, z) = r·(cos θ, sin θ); vertex - (0, 0, 3·(0, 1 − y). **1**, 0), without -0.07 (reread 2026-09-29). `mSkyScale` (26,500 m) × `mSkyHeight` (~7,900 m), x `Cloud+0x4cbc`VS shifts the vertices at the horizon (y grid < 0.1, i.e. rings 0-2): xz·1.05, y−0.007; the lower ring extends to −0.007 height under the center of the dome.

**Formula VS 448** (p is the top of the grid, n = normalize()`cScaleMat`p) is the direction in the world, z is the depth of the vertex in the form of:

```text
tA  = sat(z·Dist.x − Dist.y)                       (Dist = cScatterFogDistance)
S   = sat(Coeff.z·(1 − (1 − tA)^Coeff.x))·mScatterHeight   (Coeff = cScatterFogCoeff)
u = 1 − (2/π)·acos(0.5 + 0.5·dot(n, −cLightDir0World)) (as dome)
v = 0.5 + 0.5·(1 − tA)^Coeff.y (like fog A, without the cosine of the horizon)
sky = cScatterTexture(u, v).rgb (LUT sky; sem1 = sky·mCloudColorScaleInv)
uv0 = mBaseTexScale·(scroll + p.xz − 0.5) + 0.035·wave (flat grid projection)
sun = mBaseTexScale·(scroll + 1 − 2·mSunPos − 0.5) (the sun in the same uv)
m   = |p.xz − mPosDensityChg| < Range ? 1 + Power·(1 − d/Range)² : 1
```

**Formula PS 449** (T0/T1 – basic textures and mixtures thereof, T2/T3 – noise):

```text
r = |uv0 − centre|/mBaseTexScale; s = |uv0 − sun| (distance to the sun in uv)
uv = uv0 + mFarUVMul·(uv0 - center)/Scale·r^(2·mFarUVPow) (stretching to the horizon)
uvE = uv − (uv0 − sun)·mEmbossWidth (shift to the sun)
N = noise 1 and 2 (mNoiseScale*, mNoiseSpeed*, mNoiseDensity*) in uv and uvE, a mixture of T2/T3
far  = sat((r − mFarDensityChgStart)/mFarDensityChgEnd)·mFarDensityChgPower
a = sat(m·(base(uv + distortion N) + mDensity + far) - density here
e = sat(m·(base(uvE + ...) + mDensity + far − 4·mEmbossDensity)) - to the sun
LS   = 1 + (0.5·D − 0.4)·mLightSideNoiseParam;  D = 1 + (N' − 1)·mDarkSideNoiseParam
bl   = mBacklightPower·min(max(mBacklightRange − s, 0), 0.5)²
hl   = sat(mHighlightRange − s)² + mHighlightAmbient
α    = sat(mAlphaMul'·(a − thr)) + 0.2·min(max(e − thr, 0), 0.95)
col  = lerp(a·C0·LS, a·cShadowCol, sat(9·D·(e + mShadowPower − a)))
h    = sat(10·(a + mHilightPower − e))
col  = lerp(col, max(col, hl·h·a·C1), sat(10·h))
col  = (col + 55·bl·min(max(mBacklightParam1·(0.25 − mBacklightParam0·α), 0), 100)·cBackLightCol)
       ·clamp(1 − 5·bl, 0.8, 1)
mix  = sat(0.2·bl + min(mScatterAmb', max(S·n.y, 0)^0.25))         (mScatterAmb' = 1 − mScatterAmb)
col  = lerp(sky/scale, col, mix)
col  = scale·lerp(col, cNormalFogColor.rgb/scale, cNormalFogColor.a·(1 − sat(n.y)^cNormalFogCoeff.y))
α    = sat(α·(1 + 6·bl) + mFarAlphaChgPower·sat((r − mFarAlphaChgStart)/mFarAlphaChgEnd))
```

C0 = `sysColor0Vary`, C1 = `sysColor1Vary`, scale = `mCloudColorScale`. Bottom line: cloud color - palette × 2.75, mixed with **sky color from LUT at the distance of the cloud**: the horizon (n.y → 0) cloud - clear sky, higher - up to 90% cloud color (`mScatterAmb` 0.1 → 0.9). Then ad hoc fog.

**CPU Clouds: Sun, Scrolling, Silence** (Wii U v208, Ghidra Reading Only, 2026-09-29):

- `mIsSyncSunPosition ` is the name **with the** space (crc32 = `0xdd74d2cb`,
  `master_field.baglclwd` = true; `Cloud+0x3d4` object, `+0x3e0` value. `AGL_WriteCloudLayerUniforms` gives `mSunPosX/Y` not parameters, but `CloudParam+0x1034/+0x1038`, which writes `FUN_03a5b8a0` (called `ENV_CopySkyFogsToCloudObject`, `0x034016bc`): d = `+0x72c..+0x734` of the object of the sky (the direction where the light goes is [CPU](wiiu-render-cpu.md#main-light-direction-and-the-skys-sun-0x03656be0)).
  + (0, −0.07, 0) (constant `0x105530e0`, x and z = 0.0 of `0x102bfd24`);
  v = normalize(d.x, d.y·`mSkyScale`/`mSkyHeight`, d.z); `mSunPos` = 0.5 + 0.5·v.xz. In VS, the sun on the grid is 1 − 2·`mSunPos` = −v.xz, i.e., the point to light is normalize(s.x, (s.y + 0.07)·S/H, s.z.xz, s = −d.
- `mBaseTexScrollSpdX/Y` - **speed**; the shader goes sums
  `CloudParam+0x1040/+0x1048` (double): `FUN_03a59734` adds velocity × `Cloud+0x4c9c` each step, with |sum | > 300 - 0. Same noise: `+0x1050..+0x1068` += `mNoiseSpeed*`·`mNoiseSpeedMaster`·step·0.002. What `SkyMgr` writes at speed is [below ](#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01); `baglclwd` snapshot: (−0.000035, 0.000088) and (−0.000065, 0.000178).
- `cScaleMat`: scale (`mSkyScale`, `mSkyHeight`, `mSkyScale`)×
  `Cloud+0x4cbc` and port from `KSys+0x868..0x870` (`FUN_03c6fb94`; in appearance - camera position, not checked). `Cloud+0x4cbc` (and `+0x4cb8`) designer puts 1.0; other entries in `0x03a50000..0x03a60000` no (the rest of the code is not viewed).
- `CloudParam` (`0x03a55724`) omissions: `mFarUVPow` 8, `mFarUVMul` 0.8,
  `mFarDensityChg` 0.9/0.1/−0.2, `mFarAlphaChg` 0.8/0.5/−0.4, `mEmbossWidth` 0.1, `mEmbossDensity` 0, `mSkyScale` 12 000, `mSkyHeight` 4 000, `mSunPos` 0.5, `mPosDensityChgRange` 0.5, `…Power` 0, `mBaseTexScrollSpdX/Y` 0.0001/0.001. `SkyMgr::SkyMgr` (`0x0364f620`and `initEnvPalette` (`0x0363d76c`) Wii U matched Switch `worldSkyMgr.cpp`/`worldEnvMgr.cpp`Silence. `SkyMgr`: `BacklightParam0/1` 0.2/0.45 for all `PrCloudV`; `SkyHeight` 15,000, y `PrCloudV1_0` 13 000, `PrCloudV0_2` 14 500, `PrCloudV1_2` 10 000; `EmbossWidth` 1. `PrCloudV1_0` 0.06, at layer 2 0.08904; `ScrollSpd`/`WindVecAdd`: `PrCloud_0` −0.35/0.262, `PrCloud_1/2` −0.5/0; `SunMoonDispDist` -24,000 (in the dump -23,000). `initEnvPalette`: `Cloud{0,1,2}_BacklightPower` 1.2 in palettes 8-15 (otherwise 1.6), `SkySunColorNoUse`/`Cloud2NoUse` 1, his own line `Cloud2_*`In v208, all of these fields are set -- silences only apply to incomplete documents.
- `mSkyHeight`, `mDensity`, `mBaseTexScale` image by `baglclwd` (7895, 0.37,
  2.07) lie inside `Min`/`Max` `PrCloudV0_0` - `SkyMgr` writes fluctuating layer values - `FUN_0365867c`, [ below ](#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01).

**Cloud Textures: List, numbers, samples PS 449** (Wii U v208, Ghidra read only, dump; 2026-10-01):

- Samplers* (reflection version 9, PS 449): `cBaseTexture` [1, 0]
  `cBaseTexture_Blend` `Common` , 1], `cNoiseTexture` ] , 2], `cNoiseTexture_Blend` [1, 3] — `textureUnitPS0..3` Cemu, biography `uf_remappedPS[i]` → `Common` (`cemu_uniform_map.py`): [0] = floats 20–23 (`mBacklightRange`, `Param0`, `Param1`, `mBaseTexScale`), [1] = 8–11 (`mNoiseScale2`, `mNoiseDensity1`, `2`, `mEmbossWidth`), [3] = 44–47 (`mScatterAmb`, `mSunOccChkSize`, `mFarDistotionChgStart`, `End`), [6].x = 48 (`mFarDistotionChgPower`), [7] = 4–7 (`mNoiseSpeed1Y`, `2X`, `2Y`, `mNoiseScale1`), [8 ] = 0–3 (…, `mDistotion`, `mDensity`, `mNoiseSpeed1X`), [10].x = 40 (`mCloudTexBlendRate`), [11] = 112–113 (`mDarkSideNoiseParam`, `mLightSideNoiseParam`).
- *Samples PS 449* (line by GLSL): Noise reads `.xw` from T2 and T3 and
  mix `T2 + (T3 − T2)·mCloudTexBlendRate`noise 1 - in `mNoiseScale1·p + (S1X, S1Y)`Noise 2 - in `(mNoiseScale2·p + (S2X, S2Y)).yx` (u and v rearranged), p = uv (stretched to the edge) and uvE; N = (d1·n1 + d2·n2)/(d1 + d2) by x and w; distortion uv′ = uv + f·`mDistotion` ·(N.x − 0.5, N.w − 0.5), f = 1 + sat((r − `mFarDistotionChgStart`)/ `mFarDistotionChgEnd`)·`mFarDistotionChgPower`; base- `.x` T0 and T1, same mix. Side noise - only in uvE and **without** divisions by d1 + d2: N' = 6·(d1·n1.w + d2·n2.x) + 2·(d 1·n1.1.x + d2•n2.w); D = 1+ (N′ - 1)·Dark; LS = 1+(0.5·D − 0.4·Light; color - lerp(a·C0·LS, a·cShadowCol, sat(9·D·(e + mShadawPower − a)) Uniforms). `mNoiseSpeed*` sums `CloudParam+0x1050..+0x1068` (`AGL_WriteCloudLayerUniforms`, reread), step-by-step increase `mNoiseSpeed*·mNoiseSpeedMaster·0.002· Cloud+0x4c9c` (`FUN_03a59734`; `+0x4c9c` = 1.0 from `0x1035f520` in constructor), a 0 drop per ± 300.
- * Texture list. * `FUN_039cea0c` resets the `Cloud+0x4c98` counter
  (`FUN_03a5b76c`) and for each texture of each environment resource, calls in order `FUN_03a5b778`Name whose first 12 characters are equal `cloudTexture` (according to the register), passed, the rest are written in `Cloud+0x4bd8` (by 0xc, to 16). `collect.genvres` lowercase names`cloudtexture02`), so that the list of the field is all five FTEX in the order of the file: 0 `cloudtexture02`, 1 `cloudtexture03`, 2 `cloudtexture04`, 3 `indwp_ia4_00`, 4 `p_shadow_clouds`What are the resources of the environment`+0x910`/ `+0x914`) no others with textures - output by content `env.sgenvb` (one BFRES), the writer of the array is untraceable.
- *Number → texture.* `AGL_WriteCloudLayerUniforms` takes the record
  `FUN_03a59bec(Cloud, number)`: the number (without a sign) < count − 1, or 0 is the last record (`p_shadow_clouds`) is unattainable, 4 gives `cloudtexture02`. Numbers are `CloudParam+0x1c4` (`mBaseTextureNo`) → `FUN_03a59ba0` (textura `+0x644`, sampler `+0x788`), `+0x1d4` (`mNoiseTextureNo`) `FUN_03a59c10` (`+0x6e4`, `+0x904`), `+0x204` (`mBaseTextureNo_Blend`) → `FUN_03a59c5c`, `+0x214` (`mNoiseTextureNo_Blend`) `FUN_03a59ca8`. Which of the four samplers is the name of the shader.
- * Who writes the numbers.* `FUN_03659fa8` (`SkyMgr`) in the first step copies the
  `CloudParam0` significance `SkyMgr+0x104/+0x114/+0x144/+0x154`in `CloudParam2` — `+0x334/+0x344/+0x374/+0x384`puts `mbCloudTexBlend` = 1 and `mCloudTexBlendRate` = 0. Parameter `BaseTextureNo` (`0x10300fcc`) registers the designer `SkyMgr` `0x0364f620`; step 0x230 = 2×0x118 (object) `PrCloud`15 parameters of 0x10 + 0x28 are `PrCloud_0` and `PrCloud_2`I mean, **`PrCloud_N` layer parameters `CloudParamN`**, not "patterns" (confirmed) `FUN_0365867c`, [below](#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01)Dump: `PrCloud_0` and `PrCloud_2` — 0/1/2/1 (`PrCloud_1` 2/3/4/4), i.e. in the field of base `cloudtexture02`her mixture `cloudtexture04`noise `cloudtexture03` (and his mixture) Numbers. `baglclwd` (2/3/4/3) and silence `CloudParam` (0/1/2/3) overlap.
- *Channels.* compSel FTEX `cloudtexture02–04` - (R, R, R, R): `.x` = `.w`
  = R; `indwp_ia4_00` (BC5) - (R, G, G, G); `p_shadow_clouds` - (R, R, R, 1). Wewer reads `.x`/`.w` by compSel (that runtime does not change it - not checked).
- *Mixture over time* (`FUN_03659fa8`, reread in PPC 2026-10-01).
  The challenge is every frame: `FUN_0367920c` → `FUN_0365b0b4` (same sunshine) `FUN_03656be0`) → `FUN_0365acb0` (if the type of scene) `WorldMgr+0x530` not 0, 2, 5 `FUN_03659fa8`Reset. `FUN_0364f324` (constructor and loading of the scene) `FUN_03673f6c`condition `SkyMgr+0x217c` = 0, speed `+0x2138` = 0.001, `+0x2188` = 0, `+0x218c` = 0, `+0x2048` = −1, `+0x204c` The cycle of the function follows layers r20 = 0 and 2, and **pass-by** The machine is re-executed (two steps per frame):
  - 0: Texture numbers `PrCloud_0/2` → `CloudParam0/2`, `mbCloudTexBlend`
    = 1, both lobes of 0, s = 0.0002 + 0.0008·r (r from `sead::Random` `DAT_1046c948`, [0, 1) through the mantisse), state 1;
  - 1: the share of the upper → 1; 2: the lower → 1; 3: the upper → 0; 4: the lower → 0,
    Step - `VFR::lerp(share, goal, 0.01, s, s)`: multiplier 1 - 0.99^F, the smallest and largest step s·F (F - frame scale `*(DAT_1047c258+0xc0)[nucleus]`, 1 at 30 f/s), i.e. exactly s·F;
    |share| ≤ s·F is the target. Once you reach the target (≥ 1 or ≤ 0), the machine
    It goes further and pulls the new s – except the transition 4 → 1.
  - Then, if `+0x2188`  ⁇  0, fractions = `TexBlendRatio` `SkyPalette0/2_{n−1}`
    (`+0x12ec`/`+0x17cc`, step 0x9c, `+0x7c`). `+0x2188` only writes reset and timer `FUN_0365b0f8` (`+0x218c` → 0 ⇒ `+0x2188` = 0); no non-zero records (search for all `0x2188(`, `0x218c(` in U-King: the rest are other objects). The branch in v208 is dead.
  - Then, if the weight of `+0x204c` > 0: upper (pass 0) — lerp to
    `CloudPat0_p.TexBlendRatio` (`+0x1d08 + p·0x7c`), object 6 parameters
    + 0x1c), lower (pass 2) - to `CloudPat2_p` (`+0x1e7c + p·0x7c`),
    Weight and p write `FUN_03655de8`: set of palettes `EnvMgr::getPaletteSet()` (`+0x3cec4`, otherwise `+0x3cecc`) = 16 → p = 0, = 17 → p = 1, the goal is `EnvMgr+0x198`; otherwise, with `EnvMgr::getBloodMoonProgress()` > 0 → p = 2, the goal is this fraction; otherwise, the goal is 0. Weight is `VFR::lerp(weight, goal, 0.05, 0.005, 0.005)`, at ≤ 0 - p = −1. The rows of sets 16/17, no climate chooses (`PaletteSetSelect` dump: 0, 1, 7, 10) - `EnvMgr::setPaletteSet` puts them (events, places); in the normal field, weight is 0.
  - The result for the field: the shares go 0 → 1 → 0 in turn layers, segment -
    1/(2·s) frames (≈ 17–83 s at 30 fps), both 0s at the beginning of the scene ( `cloudtexture02` only). The state of the `sead::Random` generator of the game is not recoverable (it is general, seeded in bars when booting - " `sead::Random` Seed" below).
- `CloudParam` (`AGL_ConstructCloudLayerParams`) Silences: numbers
  0/1/2/3, `mDarkSideNoiseParam` 0.5, `mLightSideNoiseParam` 0.25,
  `mDistotion` 0.4, `mNoiseSpeedMaster` 1, `mNoiseSpeed1X/1Y/2X/2Y` −1/
  −0.6/−1.2/−1.8, `mNoiseScale1/2` 4/6, `mNoiseDensity1/2` 1/0.5,
  `mFarDistotionChg` 0.4/0.9/3.5.

**Mixing and ordering of layers (`FUN_03a5adf8`, 2026-10-01).** The state is `sead` GX2 (`FUN_030c0928` omissions, `FUN_030c0a80` applies); field `+0x20` register `CB_BLEND_CONTROL` 0 (by 5 bits per multiplier: color src) [4:0]function [7:5]dst [12:8], alpha src [20:16]alpha dst [28:24], separate alpha [29]); silence `0x25040504` (ADD, separate alpha) Sky`FUN_039cf948` → `FUN_03a5b498`, `param_5` = 0) and the cube map (`FUN_0340b6cc`): colour = src·**SRC_ALPHA** + dst·**(1 − SRC_ALPHA)** (4/5), alpha - src·0 + dst·(1 − α) with `mIsDrawReduceBuffer` (`+0x3b0`or `+0x4a4`, otherwise src·α + dst·(1−α). `param_5` = 1 (only) `FUN_034030c4`, not disassembled) - color src·α + dst·1 (additively). The layers are drawn in separate passageways in order `DAT_1047e668[mDrawOrder·3 + i]` (table {0.1,2}, {0.2,1}, 2.4,2} and {1.2,0}); `mDrawOrder` (`+0x3f0`) = 0 in `master_field`, i.e. upper (0) to lower (2) Flags of the object in order `baglclwd`: `IsEnable` `+0x3a0`, `mIsDrawReduceBuffer` `+0x3b0`, `mIsDisableFarClip` `+0x3c0`, `mIsDisableDepthTest` `+0x3d0`, `0xdd74d2cb` `+0x3e0`, `mDrawOrder` `+0x3f0`In the vuere, one pass "bottom over top" with a multiplied output (`clouds.wgsl`): same for colour; reduced buffer (`mIsDrawReduceBuffer`) - like the game from 2026-10-01, [below](#reduced-cloud-buffer-gsys-wii-u-v208-2026-10-01).

**Cloud Samplers (2026-10-01, Wii U v208, Ghidra read only).** Four samplers of the layer. `agl::TextureSampler` (designer) `0x03a82f34`,line `"agl::TextureSampler"`): `CloudParam+0x788` (text) `mBaseTextureNo`, `FUN_03a59ba0`), `+0x904` (`mNoiseTextureNo`, `FUN_03a59c10`), `+0xbc0` (`mBaseTextureNo_Blend`, `FUN_03a59c5c`), `+0xd3c` (`mNoiseTextureNo_Blend`, `FUN_03a59ca8`); `FUN_03a833ec` (`activate`) only reassemble the GX2Sampler by flag `+0x170` And he puts it in a slot.`0x03b4ad34`, sampler fields `+0x148…+0x16b`GX2 names are based on relocations `0x03a83244…0x03a832d8`): frame (0, 0, 0, 1), LOD 0...14, offset 0, mag/min `+0x164/+0x165` = 1 (LINEAR), mip `+0x166` = 2, wrapper X/Y/Z `+0x167…+0x169` = 2 (CLAMP), anisotropy `+0x16a` = 0 (1:1), depth comparison 0. `FUN_03a59734` (scrolling noise) for three layers rewrites: the X/Y wrapper of the base and its mixtures = 0 (**WRAP**), noise and mixtures = 1 (**MIRROR**); mip of all four = 2 (LINEAR) if `Cloud+0x4bd4` ⁇  0, otherwise 1 (**POINT**) — `+0x4bd4` It is written only by the designer. `AGL_ConstructCloudObject` (`0x03a57c74`, 0), i.e. in the game mip - POINT. Slots when drawing (`FUN_03a5adf8`): without mixture or at a fraction < 1 - `+0x788`, `+0x904` (+ `+0xbc0`, `+0xd3c` for mixtures; for a fraction of ≥ 1, only `+0xbc0`, `+0xd3c` in the first two slots (the option without a mixture; the result is the same as the mixture with a fraction of 1).`clouds.rs` `cloud_sampler`, `CloudTextures`Each texture is twice repeated and mirrored. `clouds.wgsl` `noise_mirror_sampler`), 2026-10-01.

**Sun Obscurity Test (`FUN_03a5b4a4`).** The second cloud pass (`param_5` = 1: color src·α + dst·1) is not part of the frame: `FUN_03a5ace4` sets a small target (double buffer by `Cloud+0x4ca4`), the layers are drawn into it, then the texels are read on the CPU (`FUN_03a80568`, `(int)Cloud+0x4b0`2 grid), and `Cloud+0x4ca8` = 1 − the middle fraction (sightness of the sun through the clouds; spot size is `mSunOccChkSize`). Consumer `+0x4ca8` is not tracked (problocked, while the sun is not affected by the shots);

**Reduced buffer (`mIsDrawReduceBuffer` = true in `master_field`).** Scene.`FUN_039aa628`The Xlu Pass only draws clouds when `IsEnable`, **without** buffer-less `+0x4a4`; otherwise they are drawn by stage. `FUN_03a15764` (Drawing layer for gsys "ReducedBuffer"): the same `FUN_039cf948` → `FUN_03a5adf8`, the alpha channel of the target stores the bandwidth (src·0 + dst·(1 − α)) and collects with the frame in depth. [below](#reduced-cloud-buffer-gsys-wii-u-v208-2026-10-01).

Not established: `cLightDir0World`/`cSunColor` (`+0x490`/`+0x460`, env `+0x11c`The parameters of the layers write. `SkyMgr` (`FUN_0365867c`, [below](#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01)), `mFar*`, `mNoiseScale*`, `mNoiseDensity*`, `mFarDistotionChg*`, `mNoiseSpeedMaster` from `baglclwd` (`SkyMgr` In a dump veer, the formula is executed on a grid and in the game's dome. **texture** (list and numbers above, `clouds.rs` `CloudTextures`, `clouds.wgsl` `game_textures`); without the dump, viewer pattern. `mCloudTexBlendRate` in the viewer - machine above (`clouds.rs` `CloudBlend`Two passes per frame, reset on a new stage, no branch `CloudPat` (there are no 16/17 sets and no Blood Moon in the viewer), generator - `sead::Random` with a constant seed of vuere: CLOUD-LIGHT-001 (archived reference), CLOUD-UV-001 (archived reference).

## Cloud layers: what SkyMgr writes (fun_0365867c, Wii U v208, 2026-10-01)

Ghidra `U-King.rpx` EU v208, read only; matched with Switch `worldSkyMgr.h/.cpp` (clamation and omissions) and `master_field.baglclwd` image.

**Challenge and objects.** Every frame. `FUN_0365acb0` (Scene type not 0, 2, 5) calls `FUN_03657fac` (shadow of clouds) `prjshd`, see. [CPU](wiiu-render-cpu.md#cloud-shadows-the-projection-shadow-cloudshadowonoff--proj_shadow_off)It has nothing to do with the layers, then **`FUN_0365867c`** (layer parameters) `FUN_03659fa8` (texture numbers and mixture), `FUN_0365aa10`. `FUN_0365867c` It's in layers 0 and 2.`CloudParamN` = `Cloud+0x15b0 + N·0x1070`; parameter value - object + 0xcnames- `AGL_ConstructCloudLayerParams`- The layout `SkyMgr` Wii U: `PrCloud[3]` s `+0x5c`step 0x118 (`ScrollSpd` +0xc, `NoiseAdd1` +0x1c, `NoiseAdd2` +0x2c, `NoiseAdd1_side` +0x3c, `NoiseAdd2_side` +0x4c, `PosDensityChgRange` +0x5c, `…Power` +0x6c, `…Speed` +0x7c, the strength of the spot +0x80condition +0x84, `WindVecAdd` +0x94multiplier0x98texture numbers +0xa8…); `PrCloudV[3][2]` s `+0x3a4`stride 0x518sort 0x28cNames of objects `PrCloudV0_%d`, `PrCloudV1_%d` (line) `0x10303b14`, `0x10303b24`), i.e. **`PrCloudV{species}_{layer}`**0 is a clear sky, 1 is an overcast (see below). `PrCloud_1`/`PrCloudV*_1` in the dump - omissions of the designer (layer 1 in the field is turned off).

**Wobble.** For `SkyHeight`, `Distotion`, `Density`, `AlphaMul`, `AlphaThreshold`, `BaseTexScale` each type: s = (sin φ + 1)/2 (φ - before adding), value = `Min` + (`Max` − `Min`)·s; then φ += `SinSeedAdd`· `SkyMgr+0x2148`·F (F is the frame scale), with φ > 2π - φ − 2π.`SkyHeight` And so on. It's only taken when the flag is next to the phase (the designer drops it, you can't see any records in the game). `GlobalRandom` in the constructor, one per parameter, common to all layers and types (Switch) `SkyMgr::SkyMgr`, the Wii U matched earlier.

**View of the clouds.** Each parameter = lerp(view 0, view 1,) `SkyMgr+0x2120`) - the cloudiness of the world (writer- `FUN_03655de8`in vuere `climate::Weather::cloudiness`That's how you spell it. `mSkyHeight`, `mDistotion`, `mDensity`, `mAlphaMul`, `mAlphaThreshold`, `mBaseTexScale` (vociferous) and without wobbling `mEmbossWidth`, `mEmbossDensity`, `mHilightPower`, `mShadowPower`, `mHighlightRange`, `mHighlightAmbient`, `mBacklightRange`, `mBacklightParam0/1`, `mDark/LightSideNoiseParam`. `BacklightPowe` It's not written here. `mAlphaMul` ×= `SkyMgr+0x2178`, `mAlphaThreshold` += (1 - threshold)·(1 -) `+0x2178`); `+0x2178` = 1 in the field (reset only) - unchanged.

**Palettes of the sky and `CloudPat`.** When `SkyMgr+0x2188`  ⁇  0 values are taken from `SkyPalette0/2_{n−1}` (height, distortion, density, alpha, threshold, scale, `PosDensityChgPower`); `+0x2188` writes only reset (0), see "Mixture over time" - ** in v208 `SkyPalette*` in the field do not act **. At the weight of `+0x204c` > 0 height, density, alpha and threshold stretch to `CloudPat0/2_p` (sets palette 16/17, Blood Moon).**Speed.** Layer angle: a0 = atan2(w.x, w.z) + `WindVecAdd₀`·r0, where w = `SkyMgr+0x20c8/+0x20d0`; layer 2 - a2 = atan2(sin a0, cos a0) + `WindVecAdd₂`r2 (the angle accumulates from the top layer). r = `PrCloud+0x98` - at each discharge `FUN_0364f324` randomly [−1, 1). s = sin a, c = cos a (`FUN_04210514` = sin, `FUN_04210d48` = cos, `FUN_04210e90` = atan2f):

```text
mBaseTexScrollSpdX = s·k₁·ScrollSpd      mBaseTexScrollSpdY = c·k₁·ScrollSpd
mNoiseSpeed1X = k₂·(s·NoiseAdd1 + c·NoiseAdd1_side)
mNoiseSpeed1Y = k₂·(c·NoiseAdd1 + s·NoiseAdd1_side)
mNoiseSpeed2X = k₂·(s·NoiseAdd2 + c·NoiseAdd2_side)
mNoiseSpeed2Y = k2·(c·NoiseAdd2 + c·NoiseAdd2 side) (both cos are so in PPC, 0x03658d74, 0x03658db8)
k₁ = SkyMgr+0x2144, k₂ = SkyMgr+0x2148
```

`baglclwd` (layers 0 and 2): |`mBaseTexScrollSpd`|/|`ScrollSpd`| = 1.05·10−4 for both, angles 158° and 160°; `mNoiseSpeed*` converge with k2 ≈ 0.105 at the same angles, including asymmetric `2Y` (0.293 vs 0.293; symmetric formula would give 0.70).

**Writer k1, k2 and wind `FUN_03655de8`** (2026-10-01; the same one that writes cloudiness `+0x2120`The previous scans didn't see him: the recordings are from the base. `r28 = SkyMgr+0x1cac` (`CloudPat0`): `stfs …,0x41c/0x424(r28)` = `+0x20c8/+0x20d0` (0x03656604, 0x03656624), `0x498/0x49c(r28)` = `+0x2144/+0x2148` (0x036566c8, 0x036566c0), `0x480/0x484` = `+0x212c/+0x2130`, `0x458/0x45c` = `+0x2104/+0x2108`The scan of everyone. `addi`-base 0x1000–0x2158 on the program (and any bases in 0x0363–0x0369) gives other writers of these fields SkyMgr only in reset `FUN_0364f324` and designer `FUN_0364f620` corner `+0x212c` = 0, force `+0x2130` = 0.2; they're not affected by resetting. Each frame (t is the scale of the frame) `DAT_1047c258+0xc0`1 at 30 k/s):

```text
θ* = atan2(d.x, d.z), d = world::Manager::getWindDirection  (FUN_03672ec8 = 0x03672ec8;
     (sin, 0, cos) climate angle WorldMgr+0x548[ climate], Switch "O"
v* = world:::Manager::getWindSpeed (FUN 03672fe8: WindPower climate x)
     multiplier, Aoc substitutions/card edge/hand wind; out of field 0)
+0x212c (angle) → θ*: fraction 1 − 0.5^t, step in [0.01t, 0.05t] (directly, without
     transition through ±π
+0x2130 (force) → v*: exactly 0.1t
WorldMgr+0x53c – both of which are on target
w = +0x20c8/+0x20d0 = (sin, cos) angle
b = min(+0x2130/10, 1)·0.3·m,  m = TimeMgr+0xb0 (Switch `_d0` =
    max(mTimeStep/Default, 1) only in the ordinary course of time, otherwise 1;
    v208 is always 1 - see below "Time Step"
k₂ = +0x2148 = b,  k₁ = +0x2144 = 0.001·b
at CloudPat weight c = +0x204c > 0 (sets of palette 16/17):
    k₂ = b + (CLOUDPAT_noisePow[p] − 0.001·b)·c
    k1 = 0.001·(b + (CLOUDPAT_windPow[p] − 0.001·b)·c) (so in PPC 0x03656650–0x036566ac)
+0x2104/+0x2108 (cloud shadow shift) += w·k1·1.75·t
    (with the counter +0x2184 > 0 - += +0x210c/+0x2110·t; in v208 the counter
     Always 0, see Cloud Shadows CPU.
```

The constants are `.rodata` `0x10300db0` 0.5, `0x10300e48` 0.01, `0x10300e44` 0.05, `0x10300e14` 0.1, `0x103013ac` 10, `0x10300e08` 0.3, `0x10300dc4` 0.001, `0x103013b0` 1.75. The wind w is normalized, so the wind force enters only through k (picture: k2 = 0.105 → force 3.5 at m = 1). Oscillation: φ += `SinSeedAdd`·k2·t, i.e. with wind ≥ 10 - 0.3·`SinSeedAdd` rad per frame.

**Time step `TimeMgr+0xb0` (2026-10-01).** `mTimeStep` (`TimeMgr+0xa4`) only the designer is written `FUN_0365e9fc` (`0x0365eb98`) and discharge `FUN_03661870` (`0x036618a8`- both `0x3c088889` (1/120, `.rodata` `0x10301844` = `DefaultTimeStep`); scan of records `+0xa4`/`+0xb0` coded `TimeMgr` (`0x0365c000…0x03664000`) no other information, in the source Switch (`worldTimeMgr.{h,cpp}`) No setter. Update `FUN_0365f558` writer `+0xb0` = 1, then in the normal course of time max(`+0xa4`/`0x10301844`, 1) (`0x0365ffe4…0x03660018`) - i.e. **m = 1 always**; constant viewer `TIME_STEP_MULTIPLE` = 1 is the value of the game. It's possible to write by pointer from someone else's code with a bias scan.

**The density spot (`mPosDensityChg*`).**Automatic. `PrCloud+0x84`: 0 - X, Y = 2r - 1 (two calls) `sead::Random` `DAT_1046c948`) force `+0x80` = 0, transition to 1 in the same frame; 1 is force > 1, 2 is force > 0, then 0. `VFR::lerp` with base 0 (multiplifier 1 − 0^F = 1), step exactly `PosDensityChgSpeed`·F. `mPosDensityChgRange` = `PosDensityChgRange`, `mPosDensityChgPower` = `PosDensityChgPower`-force. Then at q = `SkyMgr+0x2134` < 1 (in field 1) is the attraction to (−0.168, −0.28, 0.45, −1.0) the fractions of 1 − q and at w = `FUN_0364bdac(EnvMgr)` (share of palette 2 by set) `+0x184/+0x188/+0x18c`) - k (−0.19,−0.286, 0.18,−0.8); in the field both 0. VS 448 (`uf_remappedVS[1]` = floats 36-39 in `cemu_uniform_map.py`): d = |p.xz - (X, Y)| on a shifted grid (xz·1.05 at the horizon), m = 1 + Power·(1 − d/Range)2 at d < Range, otherwise 1; PS: a = sat(m·(...)).**Other.** From the top of the `SkyMgr+0x2154` function ( `mBacklightPower` multiplier in `ENV_UpdateWeatherPalettes`) extends to 1 (or to `+0x2158`, 0 after reset, when `FUN_0365e34c(TimeMgr)` = 4 is in the fields of the moon phase), step 1 − 0.9^F within [0.01F, 0.01F]; with `WorldMgr+0x53c` - immediately. `FUN_0365e34c` - `TimeMgr::getMoonType` (Switch "O": (days + (time > 12:00)
+ (1) mod 8, forced phase `+0x12c`, Blood Moon 0, 4-
`NewMoon`; `+0x2158` writes only reset (scan above). In the viewer - `clouds.rs` `MoonBacklight` (from 2026-10-01; phase of the moon - `TimeOfDay::moon_phase`, without the Blood Moon and forced phase).

**`WorldMgr+0x53c` (2026-10-01).** This is `world::Manager::mTimer` (Switch `worldManager.{h,cpp}`: 30 in `onStageInit`): constructor `FUN_03671e5c` (`0x036721b8`) and the beginning of the `FUN_03673f6c` scene (`0x03674070`, next to `+0x530` = scene type, `+0x538` = 0 - `mTicks`) put 30; update of the world `FUN_03677ef0` (`0x03677f3c…0x03677f60`), while `FUN_036414d8` = 0, counts `+0x538` up and `+0x53c` down to 0 (at zero - flag `+0x544`) Two more (combind 2026-1001):
- Scene order (`FUN_03414c80`): `FUN_03678c38` - Managers' Tasks
  (`FUN_03677d58`, type 1), then count `FUN_03677ef0` (called if there is no `FUN_031ca674(DAT_1046d3ac)` or `+0x53c`  ⁇  0 event), then `FUN_036781fc`; then `FUN_03678d78` (type 2 tasks, positions, climate) both require scene type `+0x530`  ⁇  0, `FUN_03678d78` - another `+0x648` = 1 (puts the end of `FUN_03673f6c`).
- `FUN_036781fc` (after manager update, `+0x540`++) puts 30,
  if `FUN_036414d8` 0 – UI 0x4e or 0x45 screen is open (list) `DAT_1047e650+0x18`; `FUN_03a3e880`: `screen+0x96` = 2 and `+0x95` ≥ 0; states write `FUN_03a40f40` (2), `FUN_03a40f94` (3, «close»), `FUN_03a411a0` Screen numbers - name table indexes `0x1047b24c` (99 names, she's searched for a number by name) `FUN_036097ec`): **0x4e = `Fade`, 0x45 = `FadeDemo_00`** - dimming (while the screen is open, the countdown is set and the timer is kept at 30). `FUN_0367811c(20.0)` — Switch `Manager::hasCameraOrPlayerMoved`: camera (`+0x500` backward `+0x50c`or player`+0x518`/`+0x524`) moved by ≥ 20 m (no frame scale) - except "players ≤ 100 m and the event is happening". `FUN_03678d78` The same frame is checked after the scan, so you check the previous frame. `FUN_03673f6c` Puts the previous camera 200m above the current one (like Switch) `onStageInit`), so in the first frame of the scene, the timer is again 30 (31 frames in total).
- `FUN_03678d78`: once per frame former ← current, current ← camera
  `FUN_03191b70(DAT_1046cf88)`+0x34 (`getLookAtCamera()->getPos()`) and, if there is a player, `FUN_030e3b74` If the share of climate change `+0x5cc` <1 – it catches up with 1 (1 − 0.99^t, step 0.005t...0.01t), when `+0x53c` ⁇  0 at once 1, and climate change is not being looked for in this frame. `FUN_036728c0` (Switch `getClimate`) in the player's position, without him - the camera; when changing: `+0x5fc` The old one, `+0x5f8` ← New, `+0x5cc` = 0, and if it's new, **15 = `DarkWoodsClimat`** (Switch `worldDefines.h`and `FUN_0364be3c(EnvMgr)` < 1 — `+0x5cc` = 1 and **`+0x53c` = 90**. `FUN_0364be3c` — `EnvMgr::getConcentrationDarkness`: share of the transition set palette `+0x198`if active set `+0x194` = 7, or 0. Set of -- `getPaletteSet` (`+0x3cec4`otherwise `+0x3cecc`? `+0x3cecc` writer `FUN_03641114` from `FUN_03641140` — `PaletteSetSelect` climate `WorldMgr+0x5f8` (`FUN_0367742c` → `FUN_036773d4`In the field only; it is read by the update `EnvMgr` (`FUN_036425b8`, manager's assignment) before `FUN_03678d78`So the shift frame is still in the same climate. Set 7 in v208 dump, only for the `DarkWoodsClimat`, i.e. when entering the Forest Wildlands from another climate, a timer 90 is set if the set 7 is not specified by the map (`ChangeWeatherTag` `PaletteSel`).

Until it is 0, managers take the targets at once: wind SkyMgr. `+0x2154`cloudiness `+0x2120`, weather transitions, palette sets (`EnvMgr+0x198`), LightAnalyzer ([CPU](wiiu-render-cpu.md)In the vuere, a shared resource `climate.rs` `StageTimer`: 30 fps, 30 from a new scene (with 200 m higher in the first frame), 30 when the camera or player jumps ≥ 20 m per viewer frame, climate change `+0x5cc` and 90 for climate change `DarkWoodsClimat` by the percentage of the set palette 7 (`PaletteRows::darkness`No: screens `Fade`/`FadeDemo_00` At this time, the skymgr wind is on target. `MoonBacklight`Cloudiness, weather transition, sky condition `+0x18c`, palette sets `+0x198`, climate change.

**Who reads climate and how sets of palettes mix (2026-10-01, Wii U v208, only reading).** getters `WorldMgr`: `FUN_036723a0` = `+0x5f8` (current climate), `FUN_03672eb8` = `+0x5fc` (former), `FUN_03672ec0` = `+0x5cc` (transition share) References to `+0x5fc` and `+0x5cc` - just `ENV_UpdateWeatherPalettes` (`0x03642df4`, `0x03642e00`); straight `lwz/lfs/stw/stfs …,0x5cc/0x5fc(rX)` coded `WorldMgr` designer `FUN_03671e5c`beginning `FUN_03673f6c` (`+0x5f8` = climate under the camera, `+0x5cc` = 1, `+0x5fc` = `+0x5f8`and `FUN_03678d78`The current climate is read by 20 more places (weather). `FUN_03672890`wind `FUN_03672fe8`, `TEMPMGR_UpdateMoisture`, `PaletteSetSelect` `FUN_0367742c` And so on. All without a transition. **Climate change is only for climate multipliers in palettes**: `ENV_UpdateWeatherPalettes` (`0x03642e04…0x036431b4`) `FeatureColor` (`+0x250`), `CalcRayleigh` (`+0x26c`), `CalcMieSymmetrical` (`+0x27c`), `CalcMie` (`+0x28c`), `CalcSfParamNear` (`+0x29c`), `CalcSfParamAttenuation` (`+0x2ac`), `CalcVolumeMaskIntencity` (`+0x2cc`) taken as `(current − former)·+0x5cc + former` (displacements in order `ClimateInfo` Switch, `PaletteSetSelect` = `+0x2dc`The palettes themselves are mixed not by climate, but by the lines of the palette sets:

```text
Climate row = PaletteSetSelect of current climate (FUN 036773d4: only
    in the field, WorldMgr+0x530 = 1, +0x648, and not +0x638 = +0x63c = 1
FUN 03641140 (at the beginning of the EnvMgr update): line 0 → FUN 03641114:
    +0x3cecc = string, +0x3ced0 = 4, +0x3ced4 = +0x3ced8 = 1, +0x3ce5c = 1
FUN_0364becc (from FUN 036781fc, after managers): +0x3ced0 = 0 →
    +0x3cecc = −1, otherwise +0x3ced0 −=1 - after leaving the climate
    It is supported by 4 more EnvMgr updates.
string r = +0x3cec4 (card mark) ≥ 0? +0x3cec4 : +0x3cecc
if +0x198 ≥ 1 (FUN 036425b8 ~ decompilation lines 390-450):
    r < 0: +0x194 ≠ 0 → +0x190 = +0x194, +0x194 = 0, +0x198 = 0
           (Scene type 2 – 5); at +0x3ced8 = 0 at once 1;
    r ≠ +0x194: +0x190 = +0x194, +0x194 = r, +0x198 = 0
           (at +0x3ced4 = 0 at once 1; all writers put 1)
step s = 0.005 (0x10300220) at +0x3cef8 = 0; 0.0025 (0x10300210)
    in mode 1 or at the previous line +0x190 = 7; 0.1 at 2; 0.025 at 3;
    × +0x3ce5c (1) × frame scale t
+0x198 → 1: share 1 − 0.9^t, step exactly s·t (and in the shift frame);
    WorldMgr+0x53c – 1 at once
```

`+0x3cef8` mode writes `FUN_0364c2bc` (holds 4 frames, then 0), `+0x3cec4` tag - `FUN_0364bde8`; both are called only `FUN_02136504` and `FUN_02538120` (card/event tags, not disassembled) - in the normal game mode
0. Reset EnvMgr `FUN_0363d104`: lines 0/0 (5/5 for scene type 2),
`+0x198` = 1, `+0x3cecc` The palette of each field is `lerp(P(+0x190), P(+0x194), +0x198)`where P- `ENV_BilerpPaletteSetValues` The sky and the sky are divided by the colors of the day. `F` and climate and weather multiplier scalars are multiplied separately in each row only if its number is 0 ()[CPU](wiiu-render-cpu.md), tables “Where `F`Total: from the field to the Forest Wildlands (line 7) - immediately (timer 90, line 7 under timer), back - for 400 frames (13 s), among other lines (Lost forest 1, Korokov Forest 10, field 0) - for 200 frames (6.7 s); Climate multipliers within row 0 - for 169 frames of transition `+0x5cc` (5.6 s: 1% residue per frame to half, then 0.005). `FUN_030c023c`color-mixed `FeatureColor`In the Ghidra database, it is disassembled incorrectly (similar to a color designer); the direction of the mixture is taken along scalars next to it.

**Sowing `sead::Random` (2026-10-01).** All SkyMgr random numbers are from `sead::GlobalRandom` (`DAT_1046c948`, `getU32` — `FUN_030c499c`, 0,1) - senior 23 bits in mantissu [1, 2) - 1). It's created by `FUN_030c47b4` iz `FUN_0309ecb4`) once: object 0x20 (generator + `IDisposer` `+0x10`) designer `FUN_030c4938` = `Random::init()`: importation `0x040050a8` (64-bit time) `TickTime`) and the smallest word (r4) in `Random::init(seed)` `FUN_030c48dc` (x = 0x6C078965·(s^s>30) + 1, etc. as sead `seadRandom.cpp`Other challenges `init(seed)` (`FUN_030c48dc`15 seats) seed their generators on the stack / in objects, global - no (crossing with references to `DAT_1046c948` - Just yourself. `createInstance`; `FUN_0364e730` It's local. `DAT_1046c948` ~670 links throughout the program, resulting in grain being a boot clock, no recovery at SkyMgr call time. `FUN_0364f620` - u, v shadow displacements (`+0x2104/+0x2108`, `0x0364faa0`, `0x0364facc`), then six phases 2π·u (`0x036543ec…0x036544c0`, 2π — `0x10300e78`) in the order of fields `+0xf0, +0x128, +0x160, +0x198, +0x1d0, +0x208` (Height, distortion, density, multiplier α, threshold α, scale); `FUN_0364f324` r three layers, u, v shadows (5 calls); `FUN_03655de8` - once in 32 times peace (`WorldMgr+0x538` & 31 = 0) u in `SkyMgr+0x2124` (`0x0365673c`the consumer is not tracked; `FUN_0365867c` - spot X, Y on layers 0 and 2 (layer 1 cycle passes, `0x03658a50`); `FUN_03659fa8` - the speed of the mixture (7 seats). `clouds.rs` `GlobalRandom`: one generator, sown for hours at start-up, these calls in this order (without calling every 32 bars and without other consumers); pictures and shooting - constant `CAPTURE_SEED` Again on each scene[CLOUD-SEED-001 (archived reference)).

**In the voier** (`the original format parser` `env.rs` `SkyClouds`/`SkyCloudLayer`, `clouds.rs` `layers`, `blend_looks`, `DensitySpots`, `CloudScroll`, `clouds.wgsl` `dome_vertex`): views of cloudiness, the vibration of Min-Max with two sinuses (views 0 and 1) in a shader, the spot and m - as in the game; `SkyPalette*` From 2026-10-01, scrolling and noise speeds and wobbling are formulae higher than the SkyMgr wind.`clouds.rs` `SkyWind`, `layer_speeds`, `CloudSway`; the world wind -- `grass::GrassWind::world`The initial phases of the wobbling, r layers, spots, shadow and speeds of the mixture are from a common generator seeded in hours like that of the game (above); branch `CloudPat` It is not executed (SI-SKY-09). `WorldMgr+0x53c` wind-wise `+0x2154` Like the game (above), except for the screens of darkness.

## Reduced cloud buffer (gsys, Wii U v208, 2026-10-01)

Ghidra `U-King.rpx` EU v208 (read only), dump v208 (update), dump shaders Cemu `20260928T012415Z-cache-replay`; layout - with Switch `lib/gsys/gsysModelSceneConfig.{h,cpp}`.

**A view record.** `GSYS_CreateViewGBufferRecords` (`0x03a058a8`) creates records of 0x3444 per view; in each `+0x3e0` three sets of goals in 0xbd0 (`FUN_03a0572c`At the beginning, `sead::FrameBuffer`: virtual size `+0`physical field `BoundBox2f` `+8`Set 1 (Set 1)`+0xfb0`) — «half»: `+0xfb8…+0xfc4` - his area. `FUN_03a05de0` (`psq_st`..so the scan `stw/stfs` It was not found: for sets 1 and `+0x2750` (ReducedBuffer buffer) area = (0, 0, **W·0.5, H·0.5**), for set 2 (`+0x1b80`and `+0x2a80` - 0.25, where W×H is the physical size of the species (`FUN_030c4448`); minimum - static zero `.bss` (`0x10549db0`loaded as `Vector2f`). `FUN_03a071f8` cuts out `int`: **ReducedBuffer target -  ⁇ W/2 ⁇  ×  ⁇ H/2 ⁇**as well `color(half)` (`FUN_03a06ef4`).

**Gsys settings (`gmsconf`).** `FUN_039b4a64` designer `gsys::ModelSceneConfig` (0x1940), 32 in `ModelSceneConfigList` (`FUN_039b95b4`, IParameterIO `"gmsconf"`objects `config_%02d`. File- **`gsys.bgmsconf`** (`FUN_03a19520`: `"%s.%s"` from `gsys`, `gmsconf` → `"b%s"`), takes `FUN_03a16f8c` gsys KSys package `/KSys/U-King.Cafe_Cafe_GX2.release.sarc` (`FUN_03410c24`- in the dump `update/content/Pack/Bootup_Graphics.pack` → `System/KSys/U-King.Cafe_Cafe_GX2.release.ssarc` near `gsys.bgapkginfo`, `gsys.bptclconf`Configuration names: `config_00` = `Main`, `config_01` = `UI`In. `Main`: `reduced_buffer_16bit` = **true**, `reduced_buffer_edge_adjust` = **true**, `reduced_buffer_edge_adjust_coeff` = **2**, `depth_reduce` = false, `nld_enable` = true, `nld_32bit` = false, `nld_half_32bit` = true (`aamp_dump` The configuration of the field view is not traced (`ModelSceneConfigList::setup` by name; `Main` - in meaning. `FUN_039b0848` transfers values to the gsys object (`renderer+0x1f0`):
- `+0x20` (ReducedBuffer format) = `16bit` ? agl 0x2b : 0x1d → GX2 `0x820`
  **R16G16B16A16_FLOAT** (Table `0x1047ed60`);
- `+0xc` bit 11 = `edge_adjust`, `+0xa4` = `edge_adjust_coeff`
- `+0x30`/`+0x34` (NLD and half NLD formats) = `nld_32bit`/
  `nld_half_32bit` ? agl 0x14 : 9 → GX2 `0x80e` **R32_FLOAT** / `0x806` **R16_FLOAT**: complete NLD is R16F, half is R32F;
- `+0xc` bit 18 = `depth_reduce`.

**Depth passages (`FUN_03a08e38`).** The gsys programs. `update/…/Bootup_Graphics.pack` → `System/GSys/gsys_resource…ssarc` → `common.sharcb` (in update 14 programs, 162 binaries; base differs) The option number is considered as Σ (macros value · step), the choice is according to the option table with 1 (`+0x10`, index - 1), 0 - default option (`+8`Byte-byte matches with Cemu dump ()`cemu_shader_matches.py`):
- **complete NLD** (`FUN_03a086d0`, `"gsys::RenderBufferContext::NLD"`,
  set size 0): `render_buffer_depth` variant 16 (`DRAW_NORMALIZED_VIEW_DEPTH`=1), PS 93 = Cemu `971cb335c9ba44c0`: `NLD = cParam0.x / (1 − d·cParam0.y) − cParam0.x` depth `d` (`+0xb18`) = (z − n)/(f − n);
- **half NLD and depth** (`FUN_03a080b0`,
  `"NLD(half)"`, `"Depth(half)"`, set size 1): option 24 (`DRAW_NORMALIZED_VIEW_DEPTH`=1, `COPY_DEPTH`=1; `DEPTH_MAX` = 0 at `depth_reduce` = false), PS 109 = Cemu `45b98d3ef1a08ace`: the same formula for one sample of total depth in uv pixel half of the target, and `gl_FragDepth` = `d` (copy of depth in) `Depth(half)`, agl 0x3c format) Variant `DEPTH_WITH_MODIFY_OFFSET` (17: PS 95 with second sampler, targets) `EdgeAdjust` RG8 and `BlurAdjust`) only goes when `rec+4` bit 13 and `rec+8` bit 11; it is not in the dump Cemu, and the assembly below is taken without him - in frames of the game this way **failed**, `rec+4` bit 13 = 0.

**Purpose and Drawing (`FUN_03a071f8`, stage `FUN_03a15764`).** `+0x2750` Buffer: ReducedBuffer color (RGBA16F) and depth `Depth(half)` (`+0x13f8`, set 1). Cleaning `FUN_03a76b2c` only colors (flag 1) in `gsys+0x94` = **(0, 0, 1)** ( `FUN_03a0501c` designer); depth - from PS 109. Clouds (`FUN_039cf948`) are drawn into it with a depth test: color s src·1 - alpha, α α α α α α, α α α α α α α α α α α α α α α α α α.

**Assembly (`FUN_03a078fc`).** Programme `render_buffer_color` (Index 4 of the gsys programme tables) `DAT_1047e550+0x34`), option in `gsys+0xc` bit 11`edge_adjust`): `rec+8` bit 13? 5: 4 → **4 = `DRAW_COLOR`=5** (without bit 11 – the default option) `DRAW_COLOR`Samplers by name (=1)`FUN_03a1c44c`: [0] `cDepth`, [1] `cColor`, [2] `cHalfNormalizedLinearDepth`): `cColor` - ReducedBuffer color`+0x28ec`), `cDepth` - complete NLD`+0xc94`), `cHalfNormalizedLinearDepth` - half NLD (half-NLD)`+0x1864`; `+0x2fec` = `EdgeAdjust` at bat 13). Mixing `0x04010401`: Color ONE + dst·SRC ALPHA. VS 132 = Cemu `d1cf6920c3d5b194`, PS 133 = Cemu `b4a729584b6188ea` (S uniform card - `cemu_uniform_map.py`: `uf_remapped[0..2]` = `Context` vec4 0..2 = `cParam0..2`):
```
VS: uv; t = uv·cParam2.xy + 0.5 (cParam2.xy is target size)
PS: D = cDepth(uv).x;  g = gather(cHalfNLD, uv + 0.0001)   // x=(0,1) y=(1,1) z=(1,0) w=(0,0)
    e_ij = |g_ij − D|;  k = cParam1.w / (D + cParam0.x)
    fu = fract(t.x), fv = fract(t.y) // bilinear weights
    u' = sat(fu + k·(e00 + e01 − e10 − e11)),  v' = sat(fv + k·(e00 + e10 − e01 − e11))
    out = cColor(uv + ((u' − fu)·cParam2.z, (v' − fv)·cParam2.w))
```
That is, one bilinear sample of reduced color with a shift of weights to the texels of the half-target, whose depth is closer to the depth of the pixel. `Context` writes `FUN_03a05de0` (`P` - projection of the view, `+0xe0` near, `+0xe4` far, `+0xf4` tan(fovy/2) - the same three `(P+0xf4·P+0xec, P+0xf4, P+0xe8)` = (tan·aspect, tan, fovy) writes `FUN_03a0ade8` in `context[15]` models):
- `cParam0` = (n/(f−n), 1 − n/f, (f−n)/f, (f−n)/n);
- `cParam1` = (f − n, n, f, `coeff`/tan(fovy/2) · (rec+4 bit 13? (f−n)/n : 1));
- `cParam2` = (W/2, H/2, 2/W, 2/H).
The total for bit 13 = 0: k·e = `coeff`·|Δz|/(tan(fovy/2)·z) - does not depend on near/far (except for the accuracy of R16F full NLD).

**In the voier** (from 2026-10-01), `clouds/reduced.rs`, `reduced_depth.wgsl`, `compose.wgsl`, `clouds.wgsl` `reduced`): After the opaque passage of the main camera - full NLD (R16F) and half (R32F, texel  ⁇ uv·W ⁇  depth frame), clouds with the same formula in the target  ⁇ W/2 ⁇ H/2 ⁇  RGBA16F with cleaning (0, 0, 0, 1) and mixing the game; build PS 133 - material on the place of clouds in a transparent phase (after the sky and haze, before other things), ONE/SRC ALPHA without a depth test. Kubocard draws clouds directly (as the boundary of the scene 2 ⁇ iV = 2ndr) - not affecting the camera at the edge of the game (AA - 0 - ) - the entire range of the game - novtektextextecularity - after the game (nx - 0 - the game - vetextm (figue - 0 - 0 - vs - the game). RENDER-002 (archived reference).

## CPU source and the following checks

In the `src/KingSystem/World/worldSkyMgr.cpp` Switch decompilation, the designer sets the numbers 2/3/4/3 for the three `PrCloud` s. In the `data/uking_functions.csv` , the designer is marked with `O`: `0x71010dd91c`, size 16892. This confirms the original values of **on the Switch 1.5.0**, but does not restore runtime. `wm::SkyMgr::doCalc` (`0x71010d62c0`, 19952 bytes) and `SkyMgr::onTimeUpdate` (`0x71010e37f8`, 116 bytes) are marked with `U`. `SkyMgr::init_` in the source is empty and marked with `O`, size 4; the voidness of this system does not mean the absence of the entire cloud function.

Priority questions for the sole owner of the Ghidra project Wii U:

1. ~~Find xrefs to `cloudtexture`/`CloudTexture`, the boot location of `collect.genvres`
   `mBaseTextureNo`~~~- [ is disassembled by ](#cloud-cloud-program-option-9-matching-wii-u-2026-09-28) ("Cloud Textures"): the number is an index in the list of environmental textures, the numbers are written by `SkyMgr` from `PrCloud_0`/`PrCloud_2`, the mixture of bases is a `FUN_03659fa8` automaton ("Mixture by time").
2. Bring the found bootloader `sky.skybin`: the final native imageSize/tiling;
   Then link three texture objects to shader samplers, and the RGBA16F format is already supported by native tables, dimensionality by init arguments.
3. ~~Establish the World/SkyMgr path → agl::fx:::Cloud~~ - `FUN_0365867c`
   ([analysis](#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01)); writers `SkyMgr+0x2144/+0x2148` and the wind `FUN_03655de8` It was: which fields are overwritten by weather, time and palette; where are speeds and altitudes translated.
4. Attach `TYPE_USE_*` to baglclwd switches and restore formulas
   density/alpha/emboss/backlight on the shader executables.
5. ~~ `prjshd` Passage, UV Transformation, Shadow Density in Weather~~~
   [ is disassembled by ](wiiu-render-cpu.md#cloud-shadows-the-projection-shadow-cloudshadowonoff--proj_shadow_off): the shadow is a separate mask, not connected to the clouds of the sky; the pattern floats along the SkyMgr wind (`FUN_03655de8`, amendment 2026-10-01: it was previously thought not to move).

## Reproduction and limitations

Local extracts, native disposable scripts, and previews are stored in `game-data/reference/visual-formulas/sky` outside Git. Relative `game-data` here denotes the permitted local root of the data, not the assumption of the worktree neighborhood. In it, `extract_local.py` stores the sources and nested SARC, `preview_local.rs` uses the existing BFRES/GX2/BC4 code; the paths in the disposable extractor refer to this machine.

For independent re-checking, extract the two SARC levels from the above archives (between levels remove Yaz0 if present), then:

```sh
cargo run --offline -p the original format parser --example bfres_info -- /path/to/collect.genvres
cargo run --offline -p the original format parser --example aamp_dump -- /path/to/env.bgenv
cargo run --offline -p the original format parser --example aamp_dump -- /path/to/master_field.baglclwd
cargo run --offline -p the original format parser --example aamp_dump -- /path/to/master_field.bksky
```

`struct.iter_unpack('<e', bytes)` and `struct.iter_unpack('>e', bytes)` of standard Python are sufficient for half-testing; `math.isfinite` separates NaN/Inf. Hashes are calculated via `hashlib.sha256`.

This work actually uses the previously collected release examples of `bfres_info` and `aamp_dump`, rather than reassembling the unchanged viewer. It analyzes the original resources, compares the base/update, decodes and visualizes four textures, restricts the ALU/TEX parsing of sky #419 to check the formula. It does not compare with Cemu, paired game captures, runtime-connection of these assets, and checks the synthetic renderer: runtime has not changed. Consequently, the find eliminates the unknown location of assets, but does not cover the atmospherential divergence of the cloud.

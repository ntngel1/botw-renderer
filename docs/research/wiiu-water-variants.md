# Wii U: terrain water options and water type extraction

Date: 2026-09-28 Restored a limited part of the vertex shader: transition from the packed surface channel to the water type and parameter indices. Restored CPU packaging and parameter table binding, gbuffer PS connected formula and part of the CPU uniform. Full water pipeline not yet installed. Transfer to the viewer: [below](#realization-in-the-viewer)Context: FIDELITY (archived reference), water-heuristic (archived reference), [BFSHA](wiiu-shader-containers.md), [CPU-choice](wiiu-render-cpu.md).

## Sources and reproducibility

Local Wii U update, `WUP-P-ALZP`, `title_version=208`, root of `renderer.toml`. The conclusions below do not use Switch decompilation as proof of Wii U. Resources extracted outside Git in `game-data/reference/visual-formulas/water/` and `shader-archives/uking_terrain_water/`.

| Resource after Yaz0 | Size | SHA-256 |
|---|---:|---|
| update `Pack/TitleBG.pack → Model/Terrain.sbfres` | 351232 | `64a47a4bbcbc9905c01647a4f1c029377dc62efbf3b532d596b964d72334d982` |
| `Model/Terrain.Tex2.sbfres` | 33282816 | `a7754ea4093f7bb17bc34e462c8e602a3fd6fec3d141433ecdb15a215d5a5be4` |
| update `Pack/Bootup_Graphics.pack → Shader/uking_terrain_water.product.sbfsha` | 279040 | `9a6f4002ea5ed67b302153d177295c955bc9a42800d1241359a3970d9cb28688` |
| BFSHA model 0, program 0, VS code | 2240 | `53ba8e73b15720e45971ba1e5b2a6825dcdce4a738c231dafb3159e0de557ee7` |

BFSHA version `0x04050004`. VS program 0: header `0x30298`, code `[0x36000,0x368c0)` in unpacked BFSHA. Instruction shifts are further relative to this code blob, not RPX.

To repeat after retrieving the archive with the example of `shader_bfsha`:

```sh
ALL_OPTIONS=1 target/release/examples/model_info /path/to/Terrain.sbfres TeraWater
python3 tools/research/water_kind_probe.py /path/to/model000-program00000-vs.code
```

The script reads the original instructions from the transferred file, does not contain game bytecode and is not a full shader emulator.ISA fields are verified with [Cemu LatteInstructions.h](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/ISA/LatteInstructions.h), [opcodes](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompilerInstructions.h) and [ParseTEXClause, including a separate VTX markup VFETCH](https://github.com/cemu-project/Cemu/blob/c717fcab1ccc3e0b0b97499a4d9b04a77084e347/src/Cafe/HW/Latte/LegacyShaderDecompiler/LatteDecompiler.cpp).

## Archive material and options

**Dump Fact: The** model `TeraWater` contains three forms of `Normal`, `DegeneracyX`, `DegeneracyY`, one material `Translucent`; shader archive and shader model both `uking_terrain_water`. The material stores 406 shader options. Its pairs sampler → texture:

| Sampler material | Texture |
|---|---|
| `_a0` | `WaterAlb` |
| `_n0` | `WaterNrm` |
| `emission` | `WaterEmm` |
| `tera_water` | `WaterAlb` |
| `tera_height` | `WaterAlb` |

Assignments shader sampler: `_s0`, `_t0`, `_a1` → `_n0`; `_e0` → `emission`; the other names listed are the same. These are connections in the author's FMAT; they do not prove that the dynamic terrain texture during the draw remains `WaterAlb`.

In BFSHA 407 static and 4 dynamic options, 15 programs. All 407 static choices are the same in all programs. `gsys_weight=0` and `system_id=0` are also constant; only two dynamic options change:

| Program | `assign_type` | `tera_render_type` |
|---|---|---|
| 0, 1, 2 | `gsys_assign_visualize` | 0, 1, 2 |
| 3, 4, 5 | `gsys_assign_material` | 0, 1, 2 |
| 6, 7, 8 | `gsys_assign_zonly` | 0, 1, 2 |
| 9, 10, 11 | `gsys_assign_gbuffer` | 0, 1, 2 |
| 12, 13, 14 | `gsys_assign_cubemap` | 0, 1, 2 |

The PS bytecode is the same within each three. It's five passes with three vertex processing options, not fifteen visual waters. `tera_render_type` fits the three forms in their order so far.

Of the 406 author choices, 404 match each BFSHA key. Two differences are: `gsys_renderstate`: FMAT option `0`, BFSHA `3`; `gsys_alpha_test_func`: FMAT option `0`, BFSHA `6`. The only optional static option in the archive is `uking_lumberjack_side_texcoord=0`.

**Communication with the restored CPU:** `TeraWater` FMDL `0x4be38` Refers to the Material Index Group `0x4bfac`The only FMAT `0x4c234`, RenderState `0x4c348`His raw BE words: flags `0x0`, polygon `0x280242`, depth `0x49749736`, alpha control `0x6`. Native initialization `0x0399d74c` and late substitution of options in `0x039e2780` Mode 0 gives shader renderstate 3, alpha control gives func 6, enable 0. `gsys_pass="no_setting"` This gives the initial pass 0. The detailed chain and RPX identity are written in [CPU research](wiiu-render-cpu.md)After this substitution, known choices are consistent with the archive; a missing static option requires its default 0. This is not yet proof of a specific draw: callback and dynamic options can change the final key.

## Native Formula: Packed Type and Flow

**program 0, `gsys_assign_visualize`, `tera_render_type=0`**. Transferring the formula to other programs requires separate verification. In the sampler locations table for the first two entries `tera_water`, `tera_height` are `[02,ff,ff,ff]`, `[01,ff,ff,ff]`. Native VS does read resource/sampler 2 and 1; the first location is interpreted as VS slot.

CF `0x20` sets two Samples in `0x800`, `0x810`: resource 2, sampler 2, results `R4.xyzw` and `R2.xyzw`The order of four locations bytes VS/GS/PS/CS is also confirmed. [ShaderLibrary ReadLocationList](https://github.com/KillzXGaming/ShaderLibrary/blob/main/ShaderLibrary/WiiU/BfshaLoaderWiiU.cs)Let's call the results A and B, and the input. `R7.x` Calculation of t and coordinates, filtering and real texture bindings are not included in the numerical check below. `0x28`, code `0x210`45 slots, cache banks 12/13, modes 2/1, addresses 0/0 calculates:

```text
S = A + (B - A) * t
q = S.a * (65535 / 256) + f32(0.5 / 65535)
fraction = fract(q)
kind = q - fraction
flow = 2 * S.gb - 1
base = int(kind * 6)
```

Here, `kind` for finite nonnegative q is floor(q). Machine code uses `FRACT`, subtraction and `FLT_TO_INT`, not a separate floor. The direction of the world axes and the units of `flow` have not yet been established; the following clauses scale it with uniform parameters. You cannot replace the current m/s with this expression without restoring these uniforms.

The `0x30` CF specifies seven TEX/VTX instructions: SAMPLE resource 1, followed by six **VFETCH** from buffer resource 132 (`0x84`), fetch type 2, offset 0, `USE_CONST_FIELDS=1`. Their indexes are the outputs of the disassembled ALU clause:

| Instruction | Index of element | Receiver. |
|---|---|---|
| `0x830` | `base+5` | `R1.xy` |
| `0x840` | `base+0` | `R8.xyzw` |
| `0x850` | `base+1` | `R9.xyzw` |
| `0x860` | `base+2` | `R10.xyzw` |
| `0x870` | `base+3` | `R11.xyzw` |
| `0x880` | `base+4` | `R12.xyz` |

So the GPU really selects six vector entries per water type. The CPU chain below links it to the WaterAlb RGB reading results repackaged from seven texels into six vec4. These are not the first six RGBA texture texels.

**Hypothesis with exact numerical compatibility:** if alpha is UNORM16 of two bytes of `.water.extm`, `alpha=(256*kindByte+lowByte)/65535`, then q becomes `kindByte + lowByte/256 + epsilon`. For the kind 0....7 and all 256 values of the junior byte, the formula retains the original kind; the lower byte falls into. When interpolating different types between texels fractions, the alpha kind is already interpolated.

**Tile format and sampler (CPU, 2026-09-29, Ghidra read only).** The file is read into the cache block without conversion (`0x036d91f4`then `0x036d39bc` → `0x03693eb4` +0x5c of an extm object (vtable) `0x10304664`) `0x036a0270` → `0x036a00c0`: `0x03a7e618` (0x036a019c) creates a 2D surface, the agl format. `0x036f226c`Type 3, Type 1 (water) → agl `0x27` ..a table `DAT_1047ed60` (`0x03b485b8`) **GX2 `0x1f`, R16G16B16A16 UNORM** (grass) `0x1a` RGBA8); size `DAT_10309f5c[1]` = **64×64**1 mip, tile mode 1 (linear aligned), swizzle 0, channel selection identical`0x105966c8..d4` = 0.1,2,3). Image is the memory of a block (`0x036a01c4`Own sampler (`0x036a0204..0x036a0258`Translated to GX2. `0x03a830d4`): clamp by x/y/z, **bilinear** mag/min, mip is point. So R = height, G/B = flow, A = `unknown + 256·kind` (u16 LE: GPU reads bytes of the file), and `floor(q)` Filtered A is a species. **Untraceable:** code that binds this texture to a variable `tera_water` shader `Translucent` (Sampler index) `0x036ee270`, +0x5d4); it is possible that the VS reads a derivative texture of the GPU passage (`tera::update_grid` `0x036e87d4`, `0x036f46a4`Wewer takes the tile as is (`WaterTile::packed_kind`, SI-WAT-08).

## Options from accumulated Cemu-cache

The `20260928T012415Z-cache-replay` image contains exact matches:

| Cemu base hash | Stage | BFSHA candidates |
|---|---|---|
| `7b5b9f2fd4a624a6` | VS | program 9, gbuffer, tera_render_type 0 |
| `4ba8531637df52f4` | VS | program 10, gbuffer, tera_render_type 1 |
| `4ba954d077db66b9` | VS | program 11, gbuffer, tera_render_type 2 |
| `e57a8ad2ff5cb0bc` | PS | programs 9/10/11, common code |
| `2364006f2b86ab25` | PS | programs 6/7/8, zonly; three Cemu context hash |

For the last rows, all aliases are saved; the common PS match does not select a single material or draw. PS G-buffer writes four outputs with locations 0, 1, 3, 5; it cannot be interpreted as the finished water color. The shot does not establish the order of these passes or the conditions of the scene.

Program 9 VS has SHA256 `9adcd55f0fad9670dbaaa01dbb6a6017ece694c70b5827e47d7001b0c6a06eef`, 2208 bytes. In it, kind-clause remains on CF `0x28`, code `0x210`, 45 slots, with the next VFETCH `0x830..0x880`. Register distribution changes: blend is taken from `R9.x`, fraction remains in `R1.z`; kind, flow and order of indexes are stored. `water_kind_probe.py --program 9` checks the **original instructions of this variant of**, instead of substituting a result of 0. 3,048 cases with zero f64 error and rejection of 17 unsupported Programs are retested by this number 10/be tested.

Reflection program 9 and its Cemu GLSL specify the following bindings:

- `tera_water` → native VS slot 1, `tera_height` → VS slot 0; both
  Cemu declarations are `sampler2DArray`. FMAT names do not prove the original runtime texture or format.
- Buffer resource 132 becomes `uf_blockVS4` and block table
  Locations associates VS 4 with **`gsys_shape`** (archive index 11).The previous mapping to `gsys_skeleton_ex` was wrong: this block has archive index 10 and VS slot 3. Runtime enum and archive index are different; the CPU chain below independently confirms the name `gsys_shape`.
- Flow after `2*S.gb-1` is multiplied by `uf_blockVS6[38].w`.
  `uf_blockVS6[38].x`.VS 6 corresponds to `gsys_environment`; the physical units and meaning of these two components have not yet been restored.

Damped texts and numerical reports remain outside Git. Local reports: `reference/water/kind-probe-program{0,9}-observed.json`. The findings do not require a new user-to-user bypass.

## WaterAlb CPU Table: Reading, Packaging and VS4

**Matching Wii U RPX, static chain:** water object is in the terrain core of `+0x2ac4`; its buffer is `Water+0x6c4 = Core+0x3188`. The `0x036e62fc` constructor and the `0x036e8090` destructor confirm nesting. The names are conditional, the addresses refer to RPX with SHA from [CPU evidence](wiiu-render-cpu.md). The Switch `Water::setUpAttributeTable` is used only for navigation; its local declaration does not contain the recovered body.

Source and preparation path:

1. `0x036edbd0` loads `WaterAlb`, `WaterNrm`, and `WaterEmm` into three samplers
   `0x17c` increments, starting with `Water+0x20`. Virtual getter `0x036efef4` in the `0x17` argument returns `Water+0x20`: `this + 0x20 + index*0x17c - 0x2224`.
2. `0x036ee638` finds userdata WaterAlb. `name` → `+0x5f8`
   `file` → `+0x600`, `array_index` → `+0x604`; `attribute`/`attribute_sub` → `+0x608/+0x60c` refer to a separate name resolution path, not six vec4. `0x036e7154` takes the number of types from the u16 count of `name` (`record+4`).
3. `0x036e6f7c` takes width from sampler `+0xc4` (copyed from Surface)
   `+0x1c` function `0x03a82dc8`), limits the bottom to one and allocates the CPU array of RGBA f32: `width*kindCount` elements of 16 bytes, `Water+0x6c0` pointer, count `+0x6bc`. Initial values `(0.25,0.25,0.25,1)` - initialization, **not proven parameters of normal frame**.
4. `0x036e9954` is a step-by-step GPU reader through the state of `+0x618` and
   temporal `+0x61c`For the element `j` Source - sampler WaterAlb, layer is taken from `array_index[j/width]`; coordinates are specified `u=((j%width)+0.5)/width`, `v=0.5`In the next step, four 32-bit words of the result are read, each one is rearranged by helper bytes. `0x030ac4a4`It's written to a CPU array. It's not a direct reading of the halves of the RGBA16F by the processor. `*(System+0x2c0)+0x23c`This is ResourceHolder and the program. `draw_texture_partial`, disassembled below.
5. After filling, `0x036e979c` is called, ready byte is set.
   `Water+0x6f0`. It packs RGB, loads the buffer, and executes flush. Details of atomic cycles in decompilation are unreliable; the packaging scheme is verified by PPC listing, not by restored pointer types.

Local resource fact: `Terrain.Tex2.sbfres`, FTEX `0xd350`, userdata `file` `0xd498` contains 8 names Water, HotWater, Poison, Lava, IceWater, Mud, Clear01, Sea. `array_index` `0xd4f4` - `[0,1,2,3,4,5,6,7]`. So for this resource, the choice of layer retains the order of the file table. Local report: `reference/water/wateralb-userdata.json`.

### Seven RGBs in six vec4

Let `T0..T6` be the RGBA f32 **results of the specified reading of** of one layer. `0x036e979c` zeroes the scratch buffer, then for each texel transfers only RGB indices from BE u32-table `0x1047be90`:

| texel | Float indexes for RGB |
|---|---|
| T0 | 12, 13, 14 |
| T1 | 0, 1, 2 |
| T2 | 4, 5, 6 |
| T3 | 8, 9, 10 |
| T4 | 16, 17, 18 |
| T5 | 15, 3, 7 |
| T6 | 11, 20, 21 |

Hence the vector layout that reads VFETCH with `base=6*kind`:

```text
V[base+0] = (T1.r, T1.g, T1.b, T5.g)
V[base+1] = (T2.r, T2.g, T2.b, T5.b)
V[base+2] = (T3.r, T3.g, T3.b, T6.r)
V[base+3] = (T0.r, T0.g, T0.b, T5.r)
V[base+4] = (T4.r, T4.g, T4.b, 0)
V[base+5] = (T6.g, T6.b, 0, 0)
```

Stride is computed as `(3*width+3)>>2` vec4, here 6; eight types give 48 vec4 / 768 bytes. Uniform member type is 6 in `0x03a83d34`; `0x10362a70` table and stride getter `0x03a83f58` give 4 components and 4 words per element. `0x03a83f6c → 0x03a83cf0` records these f32 with a change of order of bytes for the GPU. The original alpha is not portable; slot floats 19, 22, 23 remain zero. Readback shader below does not add color conversion at zero `cColorOffset`; exact sampler/tar execution and frame values are not measured by this.

### Linking to native VS

`0x036cb6cc` registers `0x036ce898`'s callback. That gets `Core+0x3188`, its data pointer `+4` and byte size `+8`, then via virtual getter `0x036eff28` selects uniform enum `0x30` from Water: getter reads `this+0x530+enum*4`, that is, `Water+0x5f0`. `0x036ee270` initialization writes **4**. It's a time block enum, not a BFSHA index.

`0x03973344` fills runtime location table `+0x94` with names from `0x039732d4`, a string array of `0x1047daac`; enum 4 with **`gsys_shape`**. In program 9 BFSHA, this block has archive index 11 and locations `[04,ff,ff,ff]`: VS slot 4, the remaining stages are inactive. Unlike `gsys_skeleton_ex` has runtime enum 3, archive index 10, VS slot 3.

PPC callback `0x036cead8` calls **GX2SetVertexUniformBlock** with location, size and data buffer. Name confirmed by ELF relocation on import-symbol, not by decompilation of the erroneous `0x04004918` address: Ghidra can show a foreign body on it because of the unresolved import stubs. Similar conditional PS/GS calls are `0x036ceabc/0x036ceafc`; native relocations are stored in `reference/cpu/water-binding-relocations.json`.

Verified by the original PPC instructions: RGB `0x036e9858..0x036e9888` cycle, transition to the next type of `0x036e98cc..0x036e98d8`, upload `0x036e98dc..0x036e98f4`, bind `0x036cea60..0x036ceafc`. `reference/water/cpu-table-layout.json` saves permutation, corrected reflection and SHA256 27 local proof files. This is a static chain of CPU→buffer→VS, without starting a game or capturing frame values.

## Intermediate shader reading WaterAlb

Native `0x036f2cb4 → 0x036e7bb4` installs `System+0x2c0` as **ksys::tera::ResourceHolder**, rather than the general graphics manager. Its `0x036e22c8` downloads four terrain SHARCBs and registers 55 program slots starting with `Holder+0x238`. The `0x10307544` table, stride 8, contains the archive index and name; slot 1 (`+0x23c`) - archive 0, **`draw_texture_partial`** from `tera_common.sharcb`.

The resource is verified directly in update `Pack/TitleBG.pack` → `Terrain/System/tera_resource.Cafe_Cafe_GX2.release.ssarc` → `tera_common.sharcb`. This terrain archive was not part of the previous set from Bootup Graphics. It contains 14 programs / 128 binaries; size 115190, SHA256 `044d6d38f4f68dfc0466280a6c2c9b15cf97e325eb0fa9b19d869e80db0ea2e1`. Extracted outside Git in `reference/shader-archives/tera_common`; previously stored main archives were not retrieved.

### Choice of option and CPU inputs

Program: base binary 8, four variants of two macros: `SOURCE_TYPE=[2D,2DARRAY]`, `SPECIFY_MIP_LEVEL=[0,1]`. `0x036e9954` takes the stride of the first macro (`macro+0x18`) and selects the option with this index. Native AGL `0x03a79ed8` sets the stride 1; `0x03a7a04c` multiplies it by the number of next macros. Here stride=2, so **variant 2: 2DARRAY, SPECIFY MIP LEVEL=0**, VS12/PS13. Structure comes from parsed via `0x03a7d3dc → 0x03a7a4d0 → 0x03a7a204`; archives offsets in RPXXX match.

The `0x036e22c8` registration sets uniform record 2 as `cColorOffset`, 3 as `cTargetInfo`, 4 as `cTextureInfo`, sampler 0 as `cTexColor`. The `0x036e9954 → 0x036f40e4` call loads:

```text
cPosMtx     = (1, 1, 0, 0)
cTexMtx     = (0, 0, (column + 0.5)/width, 0.5)
cColorOffset= (0, 0, 0, 0)
cTargetInfo = (0, 0, 0, 0)
cTextureInfo= (array_index[kind], 0, 0, 0)
```

Zero offset is confirmed by PPC `0x036e9d48..0x036e9d90`: the r9 argument indicates four zero floats on the stack+0x40; the helper loads it into record 2. These are the values of a particular CPU path, not the assumption from the name `draw_texture_partial` , and not captured by runtime uniform.

### Code and surveillance in Cemu

VS12: 408 bytes, SHA256 `82c8fc6ddb788a0d9e124276a1e2e6ed571a7e0af956909634440b7f19a658c4`; PS13: 400 bytes, SHA256 `ea51b8859d8ad5c479fba9613fe6be0fba2736931a772ca078a9860e25be30b0`. Precise bytes are in the saved Cemu snapshot: VS `5b50acaf84322172_0000000000000000`, PS `7dcfa6d3defffd9d_000000000000007d`. The total VS has aliases in other variants, so one hash does not prove the selected draw. All four PS of this family are also present; the local report `reference/water/readback-cemu-matches.json` retains the correspondence.

The Cemu GLSL VS transmits the UVs derived from `cTexMtx.xy * vertexUV + zw` and the layer from `cTextureInfo.x`. When given a zero xy CPU, this is the constant center of the selected texel. PS rounds the array layer to the nearest even integer, makes SAMPLE, and outputs:

```text
output = texture(cTexColor, (u, v, roundEven(layer))) + cColorOffset
```

Native PS13: ALU CF `0x10`, code `0x108..0x128` - four independent ADDs for xyzw, sources `R0` and uniform selector 256 (`cColorOffset`), without clamp; CF `0x18` exports R0.xyzw. Clause is verified by the existing `postfx_probe.decode_groups(code, 2)`. First clause, SAMPLE and VS were not executed by this limited ALU probe; they used GLSL Cemu, accurately matched with native bytes. The full GPU emulator is not claimed.

Thus, **in the selected pixel shader does not have gamma, tone-map, saturation or restriction of RGB with the range [0,1]**, and the CPU offset is zero. The temporal surface is created with the internal format `0x2e`; the native table `0x1047ee18` translates it into GX2 `0x823` (RGBA32F), retaining the possibility of values above one. The CPU then reads f32 and repackages the RGB in the manner described above. Sampleaster/B accuracy, raster/blend, synchronization of the actual frame, and the result is not read separately.

`reference/water/readback-shader-evidence.json` contains decoded ADD-clause, selected macros, CPU inputs, and SHA256 20 proof files. This refines the contents of the table, but does not restore the final material-pass of the water.

## What is WaterAlb and what else is needed?

`WaterAlb` is 7×1×8 RGBA16F in Terrain.Tex2; the previous comparison of its texels with material constants remains a useful resource fact. However, the VS studied receives six vectors through VFETCH, and the sampler `tera_water` uses alpha as a packed type and G/B as a stream. The current textual communication “water shader reads WaterAlb through tera water” cannot be considered restored runtime semantics only by FMAT. In `gsys_assign_material` , `_a0` is inactive, whereas in `gsys_assign_visualize` it occupies alpha as a packed type and G/B as a stream.

The following exact CPU/GPU questions:

1. Where does terrain tie the texture of tiles to `tera_water`/`tera_height` ?
   The format, swizzle and sampler of the `.water.extm` tile itself are set ([ above ](#native-formula-packed-type-and-flow)); the binding itself is not.
2. For a full bit chain check sampler/target state and readback
   Shader `draw_texture_partial`, CPU inputs, packaging, and binding buffer 132 have already been restored; no retrieval is required.
3. What is the physical scale of G/B and what are the values?
   Is `tera_render_type` available for three different terrains?
4. How PS material-pass combines this table with scene color/depth
   Normal maps, absorption and Fresnel? The common graph of `uking_colorN_*` is not reconstructed by this parsing.

## Inspections and boundaries

`water_kind_probe.py`: 3,048 numerical cases, including 2,048 kind/junior byte combinations and 1,000 random four-channel interpolations; error on the recorded formula is 0 in the f64 model used. 17 mutations of unsupported cache/control/predicate/relative/format flags rejected. This is a test of limited decoder and algebra, no texture filtering, accurate GPU f32 rounding, execution of a full VS/PS or frame of the original game. Unknown input cache values are given arbitrarily; verifiables/kind/actions/kines are dependent on them.

Raw assets, extracted bytecode and JSON parsing remained outside Git. There were no paired visual water checks; current viewer ratios are not declared game formulas.

## The outputs of the observed G-buffer PS

Program 9/10/11 uses a single PS: 2768 bytes, SHA256 `85529578fde6015b16d0f792c942721ec68128cfa9ba079373d4611d507c8c49`. The exact match in the saved Cemu snapshot: `e57a8ad2ff5cb0bc_0000f0f0ff34db6d_ps`. Mapping confirms the presence of this variant in the cache, but does not draw a specific frame.

Native CF `0x40` — EXPORT_DONE, burst 4, source GPR6..9, xyzw. Cemu GLSL directs them to output locations **0, 1, 3, 5**. Native serial export slots and these locations are different levels of presentation; specific render target attachments, formats and subsequent blend are not yet restored here.

### Verified final packaging

`tools/research/water_export_probe.py` reads native ALU slice `0x898..0x928` (the last three CF6) and `0x928..0x9c0` (the entire CF7). It accepts only the specified shader identity, requires explicitly specified initial registers, and takes into account simultaneous readings within the VLIW group. This is a limited check of the final eight groups; the previous ALU and texture fetch are not executed.

At the entrance of the site, we will mark:

| Designation | Native boundary source | Meaning. |
|---|---|---|
| `q` | R127.y | The magnitude before Floor |
| `g` | R19.x | Color multiplier |
| `C` | R124.w, PV.y, R126.w | Three Components to Multiply by `g` |
| `S` | R16.xyz | Previously obtained RGB |
| `a` | R126.z | Weight of removing `S` from a separate output |
| `H` | R126.x, R3.z, R124.z | Half of the transformed vector |
| `F` | R127.w | Flags already converted to float |
| `m`, `d` | R2.z, R0.z | Tag and parameter difference |

For `k = f32(1/255)`, literal `0x3b808081`, the result is:

```text
GPR6 → Cemu output0 = (m, F*k, d, 1)
GPR7 → Cemu output1 = (g*C.r, g*C.g, g*C.b, k)
GPR8 → Cemu output3 = (H.x+0.5, H.y+0.5, H.z+0.5, 4*floor(q)*k)
GPR9 → Cemu output5 = (S.r*(1-a), S.g*(1-a), S.b*(1-a), 1)
```

The 703 numerical cases include each internal FLOOR boundary and adjacent f64 values, as well as 512 random inputs. The error below `2e-14`; this is a **f64 algebra with native f32 literals**, not checking native f32/FMA rounding. Synthetic tests separately check output modifier ×4, negative FLOOR, simultaneous readings, explicit boundary inputs and failure on unsupported instructions. The report with disassembled instructions is stored outside Git: `reference/water/gbuffer-export-probe.json`.

### Links to the previous GLSL

The following links are read in the **exactly matched Cemu GLSL**, but are not yet included in the executed native slice. The `U[i]` numbers stand for Cemu `uf_remappedPS[i]`, not the original block/vec4 uniform numbers:

- `m = f32(22/255)`, literal `0x3db0b0b1`.
- `F = float(uint(texturePS3(...).y * 255) | uint(U[16].z * 255))`.
  This is the OR of the two flag sources. Bit names and the CPU source of `U[16].z` are not yet known; the conversion behavior of invalid values has not been declared.
- `d = passParameterSem10.w - passParameterSem7.w` Physical units
  until they're appointed.
- `q = 63.75 * saturate(g * (Sem7.w + d*C0))`, where `C0` is clamped
  This is **ne** boundary `C.r` from the table above. Therefore, for final normal inputs, output3.w has 64 levels of `0,4,...,252` multiplied by `k`; does not reach 1.
- Let `N` be a normalized vector from the basic clause, group 12-18.
  Then `2*H = U[19].xyz*N.x + (0,1,0)*N.y + U[10].xyz*N.z`. This allows you to describe normal encoding, but not the coordinate space without the CPU linking the base vectors.
- `S` comes from `texturePS2`. `a` is obtained via LOG/EXP.
  The result of `S*(1-a)` alone does not establish the composition of the frame.

The next step is to link PS sampler slots and remapped uniforms to BFSHA/GX2 reflection and VS exports, then proceed back from `g`, `C`, `a`, `N`. FMAT names and a similar kind of formula are not enough to call channels "transparency," "depth," or the final color of water. CPUs and the state of the real render environment remain separate issues.

## PS bindings and WaterAlb transmission via VS

Program9 reflection from the stored BFSHA manifest gives the following **PS slots**. Names refer to shader bindings; real image/sampler state draw is not established by this.

| PS slot | Name of shader sampler | Type in matching Cemu GLSL |
|---|---|---|
| 0 | `gsys_normalized_linear_depth` | 2D |
| 2 | `gsys_light_prepass` | 2DArray |
| 3 | `gsys_gbuffer_material_id` | 2D |
| 7 | `_s0` | 2DArray |
| 8 | `_n0` | 2DArray |
| 9 | `_e0` | 2DArray |
| 10 | `_t0` | 2DArray |
| 11 | `_a1` | 2DArray |

FMAT binds `_s0`, `_n0`, `_t0`, `_a1` s `WaterNrm`, `_e0` s `WaterEmm`. `_a0/WaterAlb` is not an active PS sampler of this program: its parameters are already transmitted through the CPU table and VS. `S` from the previous section comes from **light-prepass binding**not from `gsys_color_buffer` The source of the added OR flags now has a name `gsys_gbuffer_material_id`.

### Cemu remapped uniform → native block

The new `tools/research/water_uniform_map.py` maps requests by ALU group, order, and channel. It only accepts SHA specific native bytes and stored GLSL; all 49 calls in 53 groups give a consistent bijection for 20 vec4. This is scanner of sources, **does not execute ALU**. Cash addresses are obtained from CF KCACHE bank/base and source selector; `base = KCACHE_ADDR*16`, selector128..159/160..191 specifies the relative index in the corresponding cache. ISA fields are checked against Cemu specified at the beginning of the document.

| Cemu `U[i]` | Native bank | Vec4 index | Block by program9 reflection |
|---:|---:|---:|---|
| 0 | 6 | 45 | `gsys_environment` |
| 1 | 6 | 43 | `gsys_environment` |
| 2 | 1 | 17 | `gsys_context` |
| 3 | 6 | 37 | `gsys_environment` |
| 4 | 1 | 16 | `gsys_context` |
| 5 | 1 | 14 | `gsys_context` |
| 6 | 1 | 15 | `gsys_context` |
| 7 | 8 | 16 | `gsys_material` |
| 8 | 8 | 18 | `gsys_material` |
| 9 | 1 | 18 | `gsys_context` |
| 10 | 1 | 12 | `gsys_context` |
| 11 | 8 | 27 | `gsys_material` |
| 12 | 8 | 22 | `gsys_material` |
| 13 | 8 | 25 | `gsys_material` |
| 14 | 8 | 28 | `gsys_material` |
| 15 | 8 | 23 | `gsys_material` |
| 16 | 8 | 32 | `gsys_material` |
| 17 | 8 | 29 | `gsys_material` |
| 18 | 8 | 26 | `gsys_material` |
| 19 | 1 | 11 | `gsys_context` |

Two apparently similar addresses `U[7]`/`U[8]` differ in CF `0x10`, group4: `0x358/0x370` instructions read bank8[16].y/x, `0x360/0x368` - bank8[18].y/x. The same sets of channels are not used as sufficient proof of compliance. `gsys_scene_material` is tied to PS10 in reflection, but this bytecode does not address it: the presence of binding does not equal active reading.

Thus, the basis of the coded normal uses the context.[11]/[12]OR-flags - material[32].z, degree for `a` — material[28].w. CPU context semantics refined in the last sections; real frame values have not yet been measured. `gsys_material` Now linked via matching CPU offset binding in the next section; previously assumed layout verified.

### Table of parameters in interpolants

In a matching VS9, six VFETCHs read `base=6*kind`; once downloaded, the listed components are not overwritten until the respective exports:

| VS varying | Read vector | WaterAlb readback components |
|---|---|---|
| Sem7 | V0 | `(T1.rgb, T5.g)` |
| Sem10 | V1 | `(T2.rgb, T5.b)` |
| Sem2 | V2 | `(T3.rgb, T6.r)` |
| Sem3 | V3 | `(T0.rgb, T5.r)` |
| Sem5.xyz | V4.xyz | `T4.rgb` |

`T` has the same meaning: the CPU readback result. The table describes **vertex outputs to raster interpolation**, does not assign a fixed kind to one pixel without checking geometry and interpolation. Sem5.w is not included here: VFETCH writes only xyz. Two V5 components are used in vertex displacement calculations before; Sem8 cannot be considered to export V5 - GPR11.xyz already contains the converted position.

From here, `d` to output0.z is the difference between the interpolated T5.b and T5.g. The color inputs of PS Sem7.rgb, Sem10.rgb and Sem3.rgb lead to T1, T2 and T0, respectively. These are established component paths; physical names (e.g., transparency/absorption/emission) still require the full formula.

Local proof: `reference/water/gbuffer-uniform-map.json` (49 native offsets bindings), `reference/water/gbuffer-bindings.json` (location tables and source hashes) No retrieval required. The following independent questions: check material uniform offsets; restore `a` and mix T0/T1/T2/T4 with depth/light-prepass; link environment/context to CPU upload.

## Material: Matching CPU offset binding

Matching RPX `0x03bf9d9c` sets the FMAT parameters by reflection shader model. Call goes through `0x039e24e4` (call `0x039e251c`); name lookup - `0x03c06fd4`.

Verification of PPC and current resources:

1. `shaderModel+0x1c` contains index material block. Here's model `0x60`.
   Byte is equal to **5**. The relative pointer `+0x4c` leads to the 16-byte block records array `0x2838`; the record 5 is `gsys_material` `0x2888`.
2. Block `+2` sets the size **0x238 = 568 bytes**. CPU records the size
   FMAT `+0x18`. Dictionary pointer block `+8` It is used to lookup each parameter name read through FMAT. `+0x34`, stride `0x14`.
3. `0x03bf9e6c` reads `u16(uniformRecord+8)`, `0x03bf9e70` subtracts 1,
   `0x03bf9e7c` records the result in an FMAT param record `+4`. If the name is missing, `-1` is written. It's a native confirmation of the one-based offset field, not just a guess from the figure of numbers.

Verifying names in the current FMAT gives 58 matches out of 61. `uk_kari_chemical_fire_ratio`, `uk_kari_chemical_wet_ratio`, `uk_special_effect_D` are missing. Of the 59 reflection members, one is not represented in FMAT: `gsys_alpha_test_ref_value`. This calculation does not mean that all parameters are actively read by the PS data.

### Readable names and values of the resource

The table links only proven PS calls to destination offsets. Values - **FMAT of the original Terrain.sbfres**, not captured by the GPU buffer. Subsequent CPUs override are not yet excluded.

| Native material input | Name. | Value in FMAT |
|---|---|---|
| [16].xy | `indirect_scale2` | (0.15, 0.15) |
| [18].xy | `indirect_scale4` | (0.25, 0.25) |
| [22].xyz | `const_color2.rgb` | (0, 0, 1) |
| [23].w | `const_color3.a` | 6 |
| [25].w | `const_color5.a` | 0 |
| [26].x | `const_vector0.x` | 0.5 |
| [27].x | `const_vector1.x` | 60 |
| [28].z | `const_value2` | 1.75 |
| [28].w | `const_value3` | 0.2 |
| [29].z | `const_value6` | -4.5 |
| [32].z | `uk_object_attribute` | 0 |

For example, `const_value3` record `0x4768` stores `0x01cd`; CPU-derived offset `0x1cc` — vec4[28].w. `uk_object_attribute` record `0x4828` stores `0x0209`; offset `0x208` — vec4[32].z. Typed filling of the entire material buffer (especially `tex_srt`) is not implemented here. The initial values are read from FMAT parameter attribute data; zero object in the resource does not prove a zero frame flag.

The local `reference/water/material-uniform-offsets.json` report contains 61 parameters with source/destination offsets, values, and seven hashed sources. The PPC navigation search is saved as `reference/cpu/uniform-offset-minus-one-navigation.json`; you don't need to repeat it. The irrelevant `0x03c57664` candidate is texture-matrix code, not part of that chain.

### Depth and angle: Reconstructed formula

Originally read in the matching GLSL, the link is now tested on native ALU in the bounded probe below. Let's say `P=Sem8.xyz`, `z = context[16].x * depth(uv).x + context[14].x`, where depth is selected from the final light-prepass lookup coordinates; `C=context[12].xyz`. For non-zero length, `P`:

```text
h = saturate(dot(C, normalize(P)) + 1)
D = (P.z + z) * (1 + const_value2 * (1 - h))
g = saturate(const_color3.a * (const_color5.a + D))
b = saturate(interpolated(T2.r) * (interpolated(T3.r) + D))
a = saturate(exp2(const_value3 * log2(b)))
```

The last line is limited to **b > 0**: native LOG_CLAMPED and Cemu replacing infinity with minimum final float does not equal unconditional `pow` for all extreme cases. `P.z+z` is not renamed to physical thickness: this requires CPU context and comparison with the original frame. Positive domain and mixing connection are checked in the next section.

## Native Mixing Testing After Texture Fetch

`tools/research/water_color_probe.py` performer **CF6 and CF7 in their entirety**: code `0x540..0x928` and `0x928..0x9c0`, 28 ALU groups after the last texture fetch. Exact SHA PS matches the previous sections. This extends the verification of the eight final groups: now compared `h`, `D`, `g`, `b`, `a`output1.rgb, output3.w and output5.rgb with independently written abbreviated formula. **border-line**They're not emulated.

Inputs: `T0..T5` interpolated components, `P` view-vector, sample normalized depth selected, two `_e0` RGB samples (FMAT: WaterEmm), weight between them, flow length, RGB sample `gsys_light_prepass`, uniform values and already prepared normal inputs. All test values are synthetic; they are not issued as frames or a new dump.

### Abbreviated color formula

Let `h,D,g,b,a` be defined above; `T` here already denotes **interpolated** components of the readback table. `E0/E1` - two RGB samples of `_e0`, `phase` - their weight, `L` sample - light-prepass, `flowLength` - length of Sem9.zw:

```text
j = saturate(T2.g * (T3.g + D))
k = saturate(T2.b * (T3.b + D))
v = saturate(T4.r * (10 * flowLength + const_value6 * b))
w = saturate(T5.r * (1 - k) + T4.g * v)
E = E0 + (E1 - E0) * phase
C = saturate(E * (v + 1 - j) + w)         // componentwise RGB

output1.rgb = g * (T1.rgb*a + (T0.rgb - T1.rgb*a) * T4.b * C)
output5.rgb = L * (1 - a)
output3.w = 4 * floor(63.75 * saturate(g * (T5.g + (T5.b-T5.g)*C.r)))
            * f32(1/255)
```

Thus, WaterEmm samples participate in the **mixing modifier C**; the texture name does not mean simply adding sampled RGB to the final color. After external multiplication by `g` , the native tail does not enter another clamp RGB. `output5` is a separate output; the state-target blending does not yet allow the sum of these outputs to be declared the final color of the screen.

Native trace checkpoints: CF6 group3 → `h`, group5 → `D`, group7 → `b`, group8 → `g`, group14 → `a`; groups 13-19 build `C` and color to the final package. These are **group numbers inside CF6**, not global GLSL line numbers.

### Verification and boundaries

Of the 1024 deterministically generated cases, 1023 have `b>0` and have been compared; one with zero after clamp is excluded by **explicitly**, rather than being replaced by a similar formula. Proven range b: approximately `0.00942067..1`. The maximum absolute error is `3.81e-15` ( `1e-12` verification threshold). Native f32 literals are used, but operations f64: GPU FMA/EXP/LOG precision, native integer edge cases and quantization are not tested.

Synthetic tests check DOT4 and simultaneous VLIW readings, cache bank/base addressing, output modifiers, scalar LOG/EXP path, failure in non-positive LOG and preservation of integer-type in OR flags. In the initial report below output3.xyz has not yet been compared, and normal inputs have been fixed. Advanced normal check and separate UV selection check are now described in the next section.

Local report: `reference/water/gbuffer-color-probe.json`.

```sh
python3 tools/research/water_color_probe.py /path/to/model000-program00009-ps.code
```

It remains to trace the generation of candidate UV, `phase` and early normal samples, as well as the context/environment CPU origin. UV selection and final normal encoding are checked below. Zero LOG, actual sampler/target states, and matched-scene A/B remain open.

## Verification of UV selection and final normal

### Selection of depth/light-prepass UV

`tools/research/water_sampling_probe.py` executes native **CF4**, code `0x480..0x540`, four ALU groups. At its edge, the original `uv0=R14.xy`, the `delta=R12.yz` offset, the compared value of `zc=R6.z` , and the result of the depth lookup in candidate texel `zs=R2.z` are already known:

```text
uv = (zc > zs) ? uv0 : uv0 + delta
```

This is a strict comparison: **equality saves candidate UV**. The next TEX clause uses the obtained R16.xy simultaneously for `gsys_normalized_linear_depth` and `gsys_light_prepass`. Array layer light-prepass - 1 (originally specified in CF0 R16.z, static GLSL link; CF4-probe this early MOV/RNDNE does not execute).

1024 cases include equality, adjacent f64 numbers on both sides and random comparisons: 379 choose the original UV, 645 choose candidate UV. Maximum error `1.12e-16`. Also checked intermediate normal components, their square and three normal multipliers. This is the **selection algebra at these inputs**: candidate UV calculation, sampling/filtering and native f32 comparisons are not yet executed by this probe.

A precisely matched GLSL specifies the following section for verification:

```text
zs = depth((floor(candidateUV * context[18].xy) + 0.5) * context[18].zw).x
zc = -(P.z + context[14].x) * context[15].x
```

Candidate depth is taken at texels centers, but after selection, both final lookups use **uv** rather than these rounded coordinates. The actual contents of context[18] (sizes/reverse dimensions) should be confirmed by the CPU upload; here is the formula without assuming their values.

### The ultimate normal encoding

Extended `water_color_probe.py` retains color check and adds an independent comparison of **output3.xyz**. Two normal inputs, a third normal sample, three weights and the strength of the extra normal vary. For finite nondegenerate inputs, the formula is:

```text
A = primary decoded normal XY
B = secondary decoded normal XY
V = normalize((A.x+B.x, A.y+B.y, sqrt(1-saturate(dot(B,B)))))
W = saturate(inverseFactor * const_vector1.x + const_color2.rgb)
N = normalize(W * V + const_vector0.x * (decode(third.r), decode(third.g), 0))
output3.xyz = 0.5 + 0.5 * (context[11].xyz*N.x + (0,1,0)*N.y + context[12].xyz*N.z)
```

`decode(x) = x*f32(0x40008102) + f32(0xbf810204)`; it's about `2.007874*x - 1.007874`, **ne** , the normal `2*x-1`. `W` is tested separately by the CF4-probe; it's transmitted explicitly at the CF6-probe boundary. The original `inverseFactor=R15.x` and the early samples themselves remain inputs, not newly calculated textural operations. You can't replace the second normalization of the first: there are component weights between them and another normal. After the context-transformation of the third normalization, there is no.

1023 positive LOGs of the case are again validated for both color and normal; maximum error of all `3.81e-15` values compared. Here the mathematical inputs vary; sampler precision, degenerate normals/NaN, full texture path and context base space are not separately tested. Critically, W weights may be zero in the real resource; zero vector normalization behavior is not declared restored.

Local reports of `reference/water/gbuffer-sampling-probe.json` and `reference/water/gbuffer-color-normal-probe.json` are kept separate from the previous color-only checkpoint. CF0/CF2 is now checked below. Do not repeat already checked areas without a new change or specific discrepancy.

## Native Flow Phase and Early Coordinates

`tools/research/water_flow_probe.py` executes CF0 (code `0x100..0x2b0`, 10 groups) and CF2 (`0x2b0..0x480`, 11 groups). The results of the first series of SAMPLE are clearly presented between them. 1024 cases compare phases, primary/secondary normal coordinates and emission, inverse factor, layer, candidate and snapped depth UV, source depth comparison. Maximum error `1.12e-15`, validation allows `1e-11`; computations f64, literals f32.

### Two phases and flow displacement

Let's say `P=Sem8.xyz`, `F=Sem9.zw`, `Q=Sem11` and `env=gsys_environment`. For non-zero `length(F)` and positive tested denominators:

```text
q = (dot(P,env[45].xyz)+env[45].w) * f32(0x3cbe82fa)
  + (dot(P,env[43].xyz)+env[43].w) * f32(0x3cdd67c9)
t0 = fract(q + env[37].y)
t1 = fract(q + env[37].z)
Fscaled = F / length(F) * saturate(length(F)*f32(0x3fb6db6e)) * f32(0x3f333333)
o0 = Fscaled*t0
o1 = Fscaled*t1
phase = abs(2*t0-1)
uv0 = Q.xy / Q.w
layer = roundEven(Sem9.y)
inverseFactor = 1 / (-P.z * context[17].y)
```

The four literals are approximately equal to `1/43`, `1/37`, `1/0.7`, `0.7`; probe retains their actual f32 bit patterns, does not replace them with exact rational numbers. The semantics of env[43]/[45] and env[37].y/z on the CPU have not yet been restored — values are not called time or world speed based on one type of formula. Individual synthetic checks check `fract` negative values and `roundEven` at half values.

### Texture inputs coordinates

The following pairs are listed in the order of phase t0/t1, not in the order of native TEX:

| Binding | Coordinators |
|---|---|
| PS8 `_n0` primary normal | `Sem1.xy + o0`, `Sem1.xy + o1`, layer |
| PS7 `_s0` secondary normal | `Sem0.zw + o0`, `Sem0.zw + o1`, layer |
| PS10 `_t0` indirect | `Sem4.zw`, layer |
| PS11 `_a1` third normal | `Sem4.xy`, layer |
| PS9 `_e0` emission | `Sem1.zw + indirect.xy * indirect_scale4 + o0/o1`, layer |
| PS0 initial depth; PS3 material flags | `uv0` |

Native SAMPLE order and GPR swizzles are now also disassembled by the existing `postfx_probe.texture_instructions`: total **13 SAMPLE**, resource/sampler ids `[8,8,0,10,3,0,7,7,11,9,9,0,2]`. XY normalized, Z array layers are not normalized; 2D inputs use zero z selector. Destination selector7 retains the former component. Raw decoding is stored in `reference/water/gbuffer-native-textures.json`; this is instruction decoding, **is not reading text contents or sampler inputs state**.

### Candidate UV and depth protection

`n0/n1` is XY samples primary normal in t0/t1 phases, `depth0` is sample in uv0. `decode` is defined in the previous section, `mix(x,y,t)=x+(y-x)*t`:

```text
A = decode(mix(n0,n1,phase))
z0 = depth0 * context[16].x + context[14].x
depthWeight = saturate((P.z + z0) / 2)
candidateUV = uv0 + depthWeight * indirect_scale2 * inverseFactor * A
snappedUV = (floor(candidateUV * context[18].xy) + 0.5) * context[18].zw
zc = -(P.z + context[14].x) * context[15].x
```

These formulas are now native-checked. CF3 reads depth by `snappedUV`; CF4 rigorously tests source or candidate UVs; CF5 reads depth and light-prepass by selected UVs. Primary normal A continues to be used in the final normal, not just for distortion. The depth value, filtering, and texture size itself are still unmeasured.

### What's covered now

All five ALU clauses PS — 53 groups — **were executed in limited checks on the sections of**, with explicit inputs between them. This is not running the entire shader pipeline with live textures, nor is it proof of pixel identity of the frame. Color/normal probe, CF4-probe and tail-probe re-passed after the general evaluator expansion; tolerances remained the same.

Local report: `reference/water/gbuffer-flow-probe.json`. Register transfers between these regions are now verified by the synthetic probe link below. Real sampling, zero flux/vector, zero LOG, CPU env/context and blend/target states remain separate issues; no new user bypasses are required.


## Linked PS verification with synthetic textures

`tools/research/water_pipeline_probe.py` connects all five ALU clauses of the observed PS9/10/11 via **13 decoded SAMPLE instructions**. Exact-bytecode guard remains the same. Native path executes 53 ALU groups and reads texture source swizzles before writing destination lanes; selector7 retains the same value. Comparable formula path builds the coordinate sequence and four RGBA outputs from the formulas above. Both paths receive one analytical synthetic sampler, depending on slot and XYZ: this checks the transfer of coordinates and registers between clauses.

Of the 512 deterministic sets, 438 are in the verifiable region; 74 with a non-positive LOG are explicitly excluded. **7008 of the output components of** are compared, all 13 sampling coordinates for each case; 308 cases select the original UV, 130 candidates UV. Maximum error of `1.59e-14` outputs, `1.78e-15` coordinates, `1e-11` tolerance. Report: `reference/water/gbuffer-pipeline-probe.json`.

This closes the old gap between individual ALU checks, but does not add evidence for actual textures, filtering, mip/LOD, GPU f32/NaN/zero domains, VS→PS interpolation, CPU frame uniforms, attachments or blending formats. The synthetic sampler itself does not emulate hardware sampling. Individual no-dump tests check source/destination swizzle, retention of masked lanes and failure in case of missing coordinates or incorrect sample.

The next step is to go from the native runtime uniform block names and callback registration to the owner of `gsys_context`, then trace the CPU filling context[14..18] used by the depth/UV formula. First, reuse `0x03973344`, `0x039732d4` and `0x036cb6cc`; do not repeat the general offset search. If this chain does not make progress, the independent reserve is the curve-map PS57/89 from the existing cache.

## CPU-filling gsys context for depth and UV

Matching Wii U EU update v208: general binding is installed independently of the specialized WaterAlb table. `0x03973344` fills in binding records by names from `0x039732d4`; runtime enum1 - `gsys_context`, `shader_info+0x94+1*4 = +0x98` entry. In `0x0399ae28` , it receives buffer from `draw_context+0x1c`; buffer +4/+8 - data /size. ELF relocations from `0x0399aef0`, `0x0399af0c`, `0x0399af30` confirm the calls to GX2SetPixel / Vertex / GeometryUniformBlock, rather than bodies mistakenly recognized by Shablock-specifics.

`0x0399a9b8` record-holder `0x03a0b218`: owner is located on `*(*(draw_context+0x10)+0x1a8)`Records start with owner+4, count with owner+0, stride `0x30`, uniform buffer inside record+4. The index from the argument is saved by draw context+0x29Outside the range, getter uses the first record; the selected one is marked bit0. It's a CPU selection of the record, not a shader binding number. `0x0399c16c` Updates these records from the scene records array with stride `0x31c`passing in `0x03a0ade8` record, record+0xc0 record+0x60 like three sources.

### Layout and coefficients

`0x0396db74` Creates a context template through `0x03a0c918`; `0x03a0a260` copy it metadata to each buffer through `0x03a83890`The initializer contains 35 members. The first four are type6 arrays of 3,4,4,3 elements; the next seven are type6 one at a time. `0x10362a70`, entry 6 = `[4,4,4,0]`and accessor `0x03a83f58` They give a size/alignment/array stride of four words. Consequently, members4..8 have **byte offsets 0xe0,0xf0,0x100,0x110,0x120**. . . the context.[14..18]This is a native layout test, not an identification of member ID with a vec4 index.

We denote `qE0/qE4/qE8/qEC/qF4` float fields by the corresponding offsets of the fifth `0x03a0ade8` argument (in the caller above it is scene record + 0x60). Let `delta=qE4-qE0`, `r=1/delta`; PPC calculates them once through `fsubs/fdivs`, then uses `fmuls qE0*r`. PPC `0x03a0afe8..0x03a0b0dc` and upload helper `0x03a83f6c` set:

| Vector | The value of CPU to endian conversion |
|---|---|
| context[14] | `(qE0, qE4, qE0/qE4, 1-qE0/qE4)` |
| context[15] | `(r, qE0*r, qEC, 1/qEC)` |
| context[16] | `(qE4-qE0, 0, 0, 0)` |
| context[17] | `(qF4*qEC, qF4, qE8, 0)` |
| context[18] | `(W, H, W>0 ? 1/W : 0, H>0 ? 1/H : 0)` |

The last line is loaded separately in **first** record by the `0x03a0a80c` function (PPC upload `0x03a0a914`). W/H is the results of `0x030c4448` for transmitted viewport/framebuffer inputs. An important decompilation error: it shows a non-initialized `local_30` at upload member8; PPC explicitly records W,H, invW, invH in stack+8.0x14 and transmits stack+8. Member9 separately receives the original dimensions and their guarded reciprocals. For other entries, the direction is confirmed in the following section 8.

So the previously tested water formulas now bind to the CPU like this: `depthReconstruction = depth*(qE4-qE0)+qE0`, `depthCompare = -(P.z+qE0)/(qE4-qE0)`, `inverseFactor = 1/(-P.z*qF4)`; snapped UV uses W/H and invW/invH. It's an algebraic substitution, not a change in the order of the GPU f32 operations. The names qE0/qE4 as near/far and FOV/aspect for the remaining fields are now confirmed by the native chain in the next section. Switch offsets are not transferred here. The actual frame values and texture dimensions are not measured.

Local report `reference/water/context-cpu-layout.json` stores 35-member layout, relocations, formulas and SHA-256 21 of the original evidence file; raw source table/stride remains matching RPX. Sources of projection fields and matrix ordering are traced below. Non-standard/zero domains and real sampling/target states remain open. Save `0x036c9810`, `0x036c79d4`, `0x03a0ade8` do not decompile again.


## Native-semantics of projection and context transfer

`0x0399bcf0` selects a scene record (stride `0x31c`) and calls `0x03b26d08(record+0x60, camera, projection)`. The latter copies the camera matrix/inverse and transmits to `0x03b26b84`. This function calls four virtual projections over slots `+0x24/+0x2c/+0x34/+0x3c`, stores the results and transmits to `0x03b262b4`. PPC `0x03b26c5c..0x03b26cc4` confirms the order of the arguments. `0x03b262b4` writes them to the geometry view `+0xe0/+0xe4/+0xe8/+0xec` - exactly the fields of the previous CPU chain.

For native perspective vtable `0x1027b54c`:

| Slot | Getter | Field of projection | Field view geometry | Semantics |
|---|---|---|---|---|
| +0x24 | 0x030c195c | +0x94 | +0xe0 | near |
| +0x2c | 0x030c1964 | +0x98 | +0xe4 | far |
| +0x34 | 0x030c196c | +0x9c | +0xe8 | vertical FOV, radians |
| +0x3c | 0x030c1974 | +0xac | +0xec | aspect |

These names are confirmed not only by the order of Switch declarations: Wii U `0x030c1990` builds a perspective matrix where for zero offset `M00=1/(tan(fovy/2)*aspect)`, `M11=1/tan(fovy/2)`, `M22=-(far+near)/(far-near)`, `M23=-2*far*near/(far-near)`, `M32=-1`. It translates camera z=-near/-far into NDC z=-1/+1. This is a **matrix to projection conversion** device, not a statement about the range of stored depth texture. 64-number real sanity cases check these planes and frustum edges; this is not an emulation of CPU float precision.

Setter `0x030c16d0` saves FOV and causes trigonometric functions for half an angle. `0x04210340` has a tanf shape: reduction by pi/2, rational approximation and `-reciprocal` for odd quadrants. In view geometry at **fovy>0** PPC `0x03b266d8..0x03b266f8` calculates `+0xf0=tanf(fovy)`, **`+0xf4=tanf(f32(fovy*0.5))`**, `+0xf8=1/+0xf4`. When fovy<=0 there is a separate branch with reconstructed angles, the perspective rule is not equal to Rustab.

Thus, for positive perspective FOV, the previous CPU formulas are called `qE0=near`, `qE4=far`, `qE8=fovy`, `qEC=aspect`, `qF4=tan(fovy/2)`. Water PS uses the interpolation of `near + depth*(far-near)` and factor `1/(-P.z*tan(fovy/2))`. This does not allow you to substitute new camera defaults: real near/far/FOV, active camera, producer normalized-depth and device conversion are not yet matched to a particular frame.

### Copy Direction and Matrix Boundaries

`0x03a83ac4` copies **the first buffer in the second**. Source range is given by member indices arguments3..4 **inclusive**, destination origin is argument5; argument6 is not involved in the calculation of size. PPC and ELF relocations in `0x03a83ba8`/`0x03a83c68` confirm OSBlockMove (destination, source, size).

Therefore, `0x03a0ade8` for non-zero entry carries from the zero entry members8..10 (`0x120..0x14f`, context[18..20]), including viewport; separately members15.25 `0x03a0adc8` The same helper copies members0..3 to members11.14. **inside**. These are different sets of matrix members, not vec4 indices11..14. With update argument7=0, the copy is executed before the current matrix is written, otherwise after; the meaning of controlling flag is still open.[11..13] These refer to member3 (inverse source matrix), and their order is checked in the next section separately from this copy of the story.

Local proof: `reference/water/context-projection-semantics.json` with hashes 19 evidence files. Two missing leaf functions `0x030c1990`/`0x030c2dd0` are identified from vtable in Ghidra, the program is saved; the binary has not changed. Negative results before determination are stored separately. The matrix ordering/source link to P=Sem8 is verified below. No re-bypass is required.

## Context Matrix and Position from VS9

The Matching Wii U chain specifies the source without the assumption of transposition: `0x0399bcf0` Copy argument3 to selected scene record+0 `0x0399c16c` Transfers this address to argument3 `0x03a0ade8`Let's denote this affine 3×4 matrix. **A**Context member0 contains three lines, meaning context.[0..2]. Member3, context[11..13], gets three lines **inverse(A)** from `0x03c6ff74`.

Inverse Helper Cofactor/Determinant Computing and Stores `0x03c7001c..0x03c70060` set the row-major order; translation is equal to `-inverse(linear(A))*translation(A)`This is an interpretation of the float mode GQR0; the live register value is not removed here. `fres` and refinement, so real-number inverse is not declared bit-by-bit equivalent. At zero determinant, the function returns 0 to stores; caller `0x03a0ade8` Does not check the result - the behavior of the singular matrix can not be replaced by a fictional identity fallback. `0x03a83cf0`caused by `0x03a83f6c`, copies three lines of four words with endian conversion. There's no additional transpose.

In the exact observed VS9 final clause CF `0x68`, code `0x5a0..0x7d0`, the first ten ALU groups give the following formula: `V` is the position of **after** of the previous displacement; its algorithm is not tested by this probe:

```text
P[i] = dot(context[i], (V.xyz,1)), i=0..2
C[i] = dot(context[7+i], (P.xyz,1)), i=0..3
C.z += bias if predicate != 0
Q = ((C.x+C.w)/2, (C.w-C.y)/2, C.z, C.w)
```

`P.xyz` Exported from GPR11 to Sem8, clip C from GPR1 to position, Q from GPR4 to Sem11. Native export registers/swizzles checked; semantic mapping taken from previously stored matching Cemu VS/reflection.[7..10] projection member2, not combined member1. `P` I have already passed through the selected A. `Q.xy/Q.w` Bias comes in clause as GPR0.y from VS11[7].y; predicate -- GPR0.x. They're boundary inputs here, their CPU sense and previous logic are still open. Adding clip Z does not change the exported P.z.

`tools/research/water_vertex_probe.py` decodes all 17 groups of this clause and performs the first ten: 512 asymmetric synthetic matrix cases, 5632 P/C/Q comparisons, 256 cases of each bias branch, max error **0**. The last seven UV/material groups are not included in this test. This is the final f64 algebra with post-displacement inputs, non-GPU precision, interpolation or measurement of the real camera. Identity guard VS is separated from the PS guard; unknown bytecode guard is still rejected.

The previously restored PS output3 can now be written more accurately: `0.5+0.5*(inverse(A).row0.xyz*N.x + (0,1,0)*N.y + inverse(A).row1.xyz*N.z)`. Here it is row0/row1, not automatically selected camera right/forward. This is **not proven common orthonormal normal transform**: in synthetic A=identity, the expression inside encoding is `(Nx, Ny+Nz, 0)`. So the name of the world/chamber base and the substitution of the standard normal matrix would be premature. The expression itself is verified by PS ultimate native probe; separately, you need to separate A and the actual coordinate producer.

Proof: `reference/water/gbuffer-vertex-transform-probe.json` and `reference/water/context-matrix-semantics.json` (13 source hashes).The next independent step is the environment flow inputs [37]/[38]/[43]/[45]; ultimate camera selection, ultimate-bias semantics and normal-basis interpretation remain clear issues.

### The basis of normality: A - matrix of the species (2026-09-29)

Matching Wii U v208, Ghidra (read-only) and Cemu GLSL program 24. Three independent signs that **A is moving the world into the space of the** species (sead camera, `Camera::mMatrix`):

- VS9 above: `P = A·(V,1)`, then `C = context[7..10]·P` - projection;
  The PS recovers the same `P` from the depth as the position in the form (`z = depth·(far−near)+near`, `P = z·uv`).
- Final `field_water` (program 24, `2e2543216c04766d`)
  normal and reflected beam from the G-buffer in the direction of the cube map with three scalar products with the strings `u7, u8, u6` = context[11], [12], [13] (`inverse(A)`): the world = `inverse(A)·v_species`, the z axis with a minus.
- CPU: `0x033e8a48` builds a camera matrix (vtable +0xe0, slot +0x24, etc.)
  Writes it in +0xb0) and transmits it to `0x0399bcf0` as A; `0x03c6fe5c` copies it into a record, `0x03c6ff74` reverses it into +0x30.

For rotation `inverse(A) = Aᵀ`, so line i `inverse(A)` World axis i, seen from the camera:[11] - World x, context[12] - world y (up) The result of the normal coding: `N_species = x_peace_into_form·N.x + (0,1,0)·N.y + y_peace_into_form·N.z`So in the world, `N.x` along x, `N.z` up `N.y` upwards **camera** (not the world z) The final pass normalizes the read normal.`dot`, `inversesqrt`This is performed by the viewer (SI-WAT-02). It's not explained why the third vector is constant. `(0,1,0)` form, not context[13]: so in the native code (probe above), the author's idea is not restored.

## CPU origin of the environment for the flow

The new chain uses the same matching Wii U RPX. In `0x0399ae28` shader binding record `+0xac` (runtime enum6 `gsys_environment`) is stored in draw context `+0xd0`. `0x0399b3b4` when changing signed byte draw-item `+0x1d` causes `0x03a0b78c(scene->owner, scene->env_manager, index)`: owner and manager - pointers scene `+0x1a8/+0x1ac`. PPC `0x0399b700..b73c` keeps index in r5, although decompilation of the call concealed the third argument. ELF relocations from `0x0399b774/b790/b7b4` confirm the transfer of the received UBO to GX2 pixel/vertex/geometry uniform blocks.

Owner `+0x30/+0x34` contains count/pointer array records with stride `0x50`. Index=-1 returns default singleton+0x3c; index outside count selects the base record. Record+0x4c bit0 prevents reassembly of the already prepared record. Specific draw index and moment of invalidation are not yet measured.

### Layout and KSys extension

`0x03a0cc94` defines 35 members. Native allocator `0x03a83d34` and the `0x10362a70` type table give **member34 = custom payload with offset0x190**, alignment16 and reserved size0x1000.

| Shader vector | Offset UBO | Offset custom payload |
|---|---|---|
| environment[37] | 0x250 | 0xc0 |
| environment[38] | 0x260 | 0xd0 |
| environment[43] | 0x2b0 | 0x120 |
| environment[45] | 0x2d0 | 0x140 |

`0x03405f48` allocates KSys per-view records stride0xf7c via count/pointer `+0x8d4/+0x8d8`. Through `0x039a86b4` → `0x03a0a224` , it registers the first **0x250 bytes** record as custom data for the environment **index0** of each view. A separate scene also gets registered if the main-view_index0 and the corresponding pointer exist. This sets the draw0 path, but does not prove that the arbitrary one uses it.

Before upload, `0x03a0b78c` calls callback record+0x3c, then `0x03a84008(..., member34, data, 0, 1, size)`. Default callback vtable `0x10356d20`, slot+c → `0x03a0d2a0` contains a single `blr` instruction (raw RPX `4e800020`). Helper copies 148 words from endian conversion, without transpose: payload takes UBO `0x190..0x3df`. This is the size of the filled custom data, **not** redundancy/ful allocation. Uniform virtual size hook `0x03a840a4` also `blr`; function is determined by verified vable and Ghitable upload is still installed.

### Phase and scale

Writer `0x033ff8cc`, PPC `0x03400750..0x034008c8`, fills in custom `+0xc0/+0xd0`.

- `T` — double `*(DAT_1047bf14)+0x938`, getter `0x036f53c4` = `lfd; blr`;
- `S` - float `DAT_1046f47c`, image initializer **0.5**, no writers
  ([ below ](#frame-rate-s-and-r-restored-statically));
- `R` — float `*(DAT_1047bf14)+0x340`.

Native constant `c30` has bits **0x3d088888** (0.03333333134651184), that is, it cannot be thoughtlessly replaced by rounding the expression `1.0f/30.0f`. `c02` = **0x3ca3d70a**, 50 = **0x42480000**. For the usual final signed-conversion domain, the order of calculating the phase:

```text
t = f32(T * f32(S * c30))
u = fract(t)
v = fract(f32(t + 0.5))
environment[37] = (t, u, v, abs(2*u - 1))

a = f32(50 / S)
b = f32(R / S)
environment[38] = (f32(S*c02), a, b, -f32(a*b))
```

The `fract` formula here reduces native truncation with a separate correction of negative values to floor; it is not an emulation of overflow/NaN/Inf. The CPU calculates the triangle through the `2*u > 1` branch and then `1-triangle`; the abbreviation through abs is real-number algebra. PS reads [37].y/z as two phase offsets and itself calculates its spatially shifted mixture. The T and upstreams are restored in the next section. The meaning of the S/R gate and the specific states remain open; elapsed cannot be substituted.

### Spatial strings

The same writer view geometry **B** Selected via scene record, stride0x31c, plus0x60 and byte KSys+9. The B matrix comes from a separate camera argument5. `0x0399bcf0`; `0x03b26d08` Store it inverse at +0x30. PPC `0x034010a4` or `0x0340114c` It copies this inverse to custom+0x120.[43] — row0 inverse(B), [45] - row2 inverse(B) Next copy `0x03401158` Separately saves forward B in custom+0x150.

For the already tested PS spatial phase, you can now define `H = inverse(B) * (P.xyz,1)` and write `q = H.z * f32bits(0x3cbe82fa) + H.x * f32bits(0x3cdd67c9)`. Previously, VS gave `P=A*(V.xyz,1)`. Only **at B=A** reduces the original x/z positions after displacement. Equality of the selected A and B for real water draw is not set here; frame selection is kept as a separate issue. In the source, you should keep the order of operations of the native shader, rather than promising bitwise equality of contraction.

Local report. `reference/water/environment-cpu-layout.json` Contains 35-member layout, imports, exact constant bits and SHA-256 24 evidence files; a generator is stored nearby.0x8d8 preserved `cpu/environment-record-navigation.json`: this is another, proven chain, the old unsuccessful search for immediate size/offset do not need to be repeated. `cpu/uniform-offset-minus-one-navigation.json` and `cpu/water-environment-{offset,size}-navigation.json` limited and contain extraneous structures: no direct result `li 0x3e0` does not prove no downloads; do not repeat (visual-formulas card, 2026-10-02). `manager+0x938` The actual selection of the environment record remains open (S/R below), and neither new game launch nor renderer changes are required.

## Rechargeable battery and invalidation environment

Source T from the previous section is now identified as **ksys::tera::System**: matching Wii U `0x036f2cb4` contains this name; constructor `0x036f23e0` highlights 0x948 bytes and sets vtable `0x1030a130`. Slot+0x2c points to `0x036f2184`.

In `0x036f2184` , T = double System+0x938 is increased by double input only if `(flags[+0x944] & 6) == 6`. At bit0x40000, `0x036f210c` is first called, then bit is cleared. The accumulation itself is `lfd`, `fadd`, `stfd` in `0x036f21d4..21e0`. Constructor and setup `0x036f2cb4` reset T to zero. No game state names are assigned for these bits.

### Delta - personnel units, not seconds

The login chain is installed on the Wii U, including the difference between the four VFR arrays (Switch was only used to find the corresponding algorithm):

1. `0x03416590` causes **OSGetCoreId** (ELF relocation `0x034165bc`).
   It gets a logical core through `0x1054aba0` and reads a pointer from `DAT_1047c258 + 0xc0 + 4*core`. With logical core>=3, it takes the base slot.
2. `0x034098b8` → `0x03a122f8` is stored in graphics system+0x354.
3. `0x03a12fdc` → `0x03993d5c` → `0x039a8e70` transmits it to active scenes.
4. Native `fmuls` in `0x039a8ebc` records scene+0x4700 =
   `f32(delta * scene[+0x46fc])`, if scene+0x46dc bit0 is cleared by **or** scene+0x46e0 bit5 is set. Otherwise, the `0x039a8ed0` branch multiplies delta by zero. This is a raw gate, not a proven name for a particular pause mode.
5. `0x03414690` selects the first scene of KSys, reads +0x4700 and calls
   The terrain virtual slot+0x2c with its optional gate, in particular, the call is excluded when `DAT_1047aff0 != 0` and its field+0x1c!=0.

Wii U VFR constructor `0x03792578` and setter `0x03792960` confirm:

| Pointer array | Formula of meaning |
|---|---|
| +0xb4 | `d=max(input, f32(0.01))` |
| +0xc0 | `f32(d * interval_ratio)` |
| +0xcc | `f32(d * frame_time)` |
| +0xd8 | `f32(value_at_c0 * frame_time)` |

Interval-ratio pointers are on +0x3c, frame time on +0xec. **Terrain receives an array +0xc0** where the frame time multiplier has not yet been applied. Thus, T is the accumulated interval-adjusted delta frames after the multiplier and the gates of the scene. It cannot be replaced by elapsed seconds or game clocks. External call frequency, actual interval/multiplier, and response to specific/menu states are not yet measured. Default time constructor does not prove the current frequency of the game.

In addition, the native constructor system defines R (field + 0x340) with the value of **20.0**.

### Frame rate, S and R (restored statically)

Wii U v208, Ghidra `U-King.rpx`, names retained:

- `VFR_InitializeBaseVsyncInterval` `0x03792d20(vfr, base, …)` writes
  `+0xe4 = base`, `+0xe8 = 60/base`, `+0xec = 1/(60/base)`. The only call is KSys init `0x034124e0`, `li r4,2` in `0x03413cf4`: **base 2 vsync, 30 Hz, frame time 1/30**.
- `VFR_UpdateVsyncIntervalRatio` `0x037932f0` puts the
  `interval_ratio (+0x28) = interval / base`, where interval is the +0x124 slot of the framework (`0x03416658`) or the fixed value of `+0x18` if the `+0x15` flag is enabled.
- `VFR_ApplyMinimumSlowFactor` `0x03792a44` takes minimum active
  deceleration factor (1.0 without deceleration) and transmits it to `VFR_SetDeltaFramesForCore` `0x03792960`: `+0xc0 = coeff.·interval_ratio`.

The total delta that terrain receives is 30 Hz: 1.0 per frame at regular 2 vsync, less in deceleration. T grows at **30 per second** in normal play. Whether the slot +0x124 measures the actual interval (then T holds real time and during drawdowns) or gives a given one is not established.

S: `0x1046f47c` has only readings from `0x033ff8cc`; no writing, no taking of the address (neighboring `0x1046f488/48c` with Ghidra records allows), so **S = 0.5 is the actual constant**. R: in the entire `System+0x340` program, only the `0x036f256c` constructor (`stfs`) writes, reads only the writer environment `0x034008a8`; **R = 20.0**. Indirect writing through the calculated pointer is not statically excluded (byte search in MCP returns only the first match).

### When the environment is reassembled

`0x03a0a5a0(owner, view_index, backing_buffer)` goes through the entire array of records stride0x50 and clears ready byte+0x4c. Simultaneously shorts+0x2c/+0x2e get -1, float+0x30 - 0, byte+0x21 selects backing buffer, and data pointer+4 is recalculated. Owner+0x38 gets index. Then the former `0x03a0b78c` can reassemble the record and set0. This is lazy after explicit reset, not an indefinite bit ready cache.

Main scene preparation `0x039bcca8` calls reset under its camera-state conditions; KSys special-pass `0x033fc3d4` calls it with view_index0 and backing-buffer selector KSys+0xde0. The exact cadence and matching each water draw require further verification - the fact of reset does not replace it.

Local proof `reference/water/environment-clock-lifecycle.json` contains 27 source hashes, proven terrain vtable and OSGetCoreId import. Searches for T, delta-field writers and readiness writers are saved separately; they do not need to be repeated. Static environment and A/B selection conditions are disassembled by [ below ](#selection-of-environment-and-two-camera-matrices); live selection, unusual normal basis and clip-bias semantics are still open.

## Selection of environment and two camera matrices

**Matching Wii U CPU and resource, no observation frame.** Local proof. `reference/water/draw-selectors-camera.json` Stores 26 source hashes, PPC instruction checks, and direct reading of FMAT `0x4c234` extracted `Terrain.sbfres`. RenderInfo entry `0x4c308` line `gsys_env_obj_set="Default"` (count1, type2).

### Environment index in draw-item

The `0x0399e5d8` constructor puts draw-item+0x1d=0, but this is not an immutable value. `0x03996260` sorts through the draw-items of the models, maps the material index item+0xa and calls the resolver of `0x03a23654`. PPC `0x03996384` reads the signed short result+4, and `0x03996394` writes the junior byte into item+0x1d. Then the previously installed `0x0399b3b4` reads it as a signed index for the environment `0x03a0b78c`.

Resolver `0x03a23654`:

- `0x03a235e0` first sets the result +4=-1 if RenderInfo is missing.
  or has a zero count, this value is retained.
- `0x0396dd90` is the `gsys_env_obj_set` key. Lookup works through
  FMAT RenderInfo dictionary, not shader option or float parameter.
- If there is a record with a non-zero count, the index first becomes 0
  (`sth` in `0x03a23820`) The string is searched in collection renderer+0x452c: `0x03a96f78` compares the name object+0x54, `0x03a97078` returns the index of the found object pointer. Successful result replaces 0 (`0x03a23868`).

So the original TeraWater chooses `index("Default")` or 0 if the lookup fails. This is stronger than constructor default knowledge, but **doesn't prove that an object named Default is the first**, or that this updater has already been applied to a particular draw. -1 has a separate default-UBO path in the builder; it can't be mixed with index0 KSys custom payload.

Tracked callback chain: `0x036a85d0` establishes `0x036b65d4`; that translates draw-context+0x18 through `0x036f36d0` In the terrain record and transmits the input packet to `0x0368f5e0`The latter checks the material index against the WaterCore kind0x18 and the model pointer, then calls `0x036cb6cc`It saves the draw packet input and installs it. `0x036ce898`which causes `0x0399b3b4(context,item)`This chain doesn't reset item + 0x1d. `0x0399a918` After draw resets binding cache, not the environment index of the item itself. `stb +0x1d` into `0x03900000..0x03a00000` Save as navigation; it includes different structures and is not proof of the absence of other writers.

### Context selector and A/B

Common draw pass `0x0399be9c` creates context and calls `0x0399a9b8` with index from pass-parameter+0x18. There are KSys overrides: `0x03404b98` for event2..7 with KSys+0xc1a ⁇ {3,6,7} selects KSys+9; event8 selects the result `0x033ef460`. `0x03405ec8` temporarily selects `0x033ef468`, then restores `0x033ef460`. The presence of these features does not set the schedule for a specific draw water.

For the matrices from [VS9 and CPU environment](#cpu-origin-of-the-environment-for-the-flow) , the exact static condition is now set. Getter `0x03ae4aa0(camera)` returns:

```text
base = camera+0x48 pointer, or fallback address 0x105978c4
flags = u16(camera+0x52)
if (flags & 0x21) != 0 and (flags & 2) != 0 and camera+0x19c pointer != 0:
    return camera+0x19c pointer + 4
return base
```

In `0x039a8e70`, for each scene record, the `0x039a946c..0x039a94e8` PPC transmits to `0x0399bcf0(scene,0,A,projection,B)`:

- A = getter(camera);
- when renderer+0x46e8 bit11=0, B = the same getter of the same camera
- at bit11=1, B=base, bypassing the alternate-branch getter.

With stable input fields A=B in the first branch, and in the second if getter also selects base. Different pointers do not by themselves prove different values of the matrices. Individual context2 in `0x039a9514` gets A=B from renderer+0x4704→+0x34, projection from +0x98 if `0x039a8a38` is non-zero.

For a reversible B, this gives a condition where `inverse(B) * A * V = V` and phase-coordinate return to post-displacement V (in exact algebra). For a particular water, you also need to link the selected context, KSys+9 for inverse-B payload and current camera flags; equality for all passes is not stated. The value of the fallback matrix, the meaning of the alternate camera, the unusual PS normal base and clip-Z bias are not renamed here by guess.

## The only environment record and updater terrain model

**Save Configuration and Matching RPX.** `reference/water/environment-collection-model-update.json` saves 21 source hash, disassembled resource templates, native format string and virtual slot. Used the resources already extracted from the [sky archive inventory](wiiu-sky-resources.md), re-extracting the archives was not:

- `System/KSys/U-King.Cafe_Cafe_GX2.release.ssarc → gsys.bgmsconf`:
  `env_obj_set_template_00` is turned on, `num=1` is called `UKingDefault`; the other seven templates are turned off.
- `Env/env.sgenvb → envobj/common.baglenvset`: group 0/record0,
  `setting.name="Default"`: Slots are written for two Directional Lights, one HemisphereLight, and four Fogs; some reference names are empty. The mere presence of a resource does not determine when it will be used.

Native config constructor `0x039b95b4` registers eight `env_obj_set_template_%02d` objects. Constructor template `0x039b8f60` binds `enable` with +0x28, `num` with +0x38 and string `template_name` with +0x98. Accessor `0x039b8f40` takes the config-list via config+0x9a4 and selects `list+0x330d4+index*0xc4`. These offsets are independently installed on the Wii U; Switch `gsysModelSceneConfig.cpp` was used only for navigation.

Renderer initializer `0x039a608c` (cache query `039a6090`) creates manager renderer+0x452c via `0x03a96804`, then slices templates 0..7. Only `enable!=0 && num!=0` calls `0x03a96d28`. `0x03a968d0` creates stride0xcc records and sets initial names for native format string `0x10364920="%s%d"`: template name + record index. `0x03a96d28` adds pointers to the total array of manager+0x1a4 and enlarges +0x19c in order of templates, then records.

When you use the saved `gsys.bgmsconf` and successfully allocate memory, you get **one entry, index0, with the initial name `UKingDefault0`**. For material `gsys_env_obj_set="Default"` resolver from the previous section, therefore gives 0 both before renaming (lookup fallback) and after renaming this record to `Default` (successful lookup). This eliminates the need to guess the `common.baglenvset` load timing specifically for the numerical selector. It does not prove that subsequent config or direct overrides draw-m changes are impossible.

The updater connection to the terrain model is also established:

1. `0x036b4488` is building six resource-core models, including WaterCore.
   owner+0x2ac4, and writes model+0x74=`0x10304e8c`.
2. BE word `0x103050e0` in this vtable, slot+0x254 is equal to `0x03996260` .
   updater, which records the result of the environment resolver in item + 0x1d.
3. `0x036b4488 → 0x03984eb0 → 0x03984958` Registration Adds Model
   model-unit array stride0x24.
4. `0x039853b8` with non-zero model-unit+0xb4 causes this slot to be used for
   Registered models, transferring renderer from +0xb4. `0x03985468` Assign renderer and immediately start the update; `0x039948ac` Repeats the update for model-units of the corresponding renderer.

So the default constructor draw-item is not the only basis: there is a path from the resource configuration via model virtual update to selector0, which the water callback then uses for the previously found KSys custom payload. This is a **static chain for saved inputs**, not a snapshot of executed draw. The next unclosed dock is the context index of a particular water and its match with KSys+9, from which the inverse-Bload is selected.

## Context GBuffer and KSys service events

Matching Wii U evidence `reference/water/pass-context-callbacks.json` contains 32 source hashes, three vtable slots, adjusting thunk, and PPC context index transfers. This clarifies the static choice of matrices, but does not give the initial value as the value of any water draw.

`0x0399c408` writes the sixth argument in draw-parameter+0x18: PPC saves r8→r28 (`0x0399c428`), then r28→+0x18 (`0x0399c4b4`). These five ordinary GBuffer passes pass **0** there:

| Function | Native name of the passageway |
|---|---|
| `0x039b16d0` | `Model(GBuffer/Opa+AlphaMask)` |
| `0x039b17b4` | `Model(GBuffer/XluSeal)` |
| `0x039b19e4` | `Model(GBuffer/Opa+AlphaMask/Blend)` |
| `0x039b1bc4` | `Model(GBuffer/XluOpa)` |
| `0x039b1ce8` | `Model(GBuffer/Xlu)` |

`0x0399be9c` then invokes `0x0399a9b8(context, parameter+0x18)`.This conclusion does not apply to special/capture passes and does not establish which of these lists a particular instance of water has landed on.

**Context substitution occurs within the draw list.** KSys constructor `0x034051f0` creates a service model at KSys+8 and puts it vtable in KSys+0x7c=`0x102c0aac`. Setup `0x03405f48` Creates nine custom items through `0x03999488`It gives them names and sort inputs, then registers that model in a model-unit. Item constructor. `0x039993e4` uses vtable `0x1034b1a8`, stride0x20.

Eight dispatcher loops from the table `0x1047dc0c` (`0x0399c4dc` … `0x0399ca98`) pass the selected list and call item vtable+0x6c. `0x039999e4`: it loads the owner from item+0, the event number from u16(item+0xa), saves the transmitted context and makes an indirect tail-call in the owner model vtable+0x29c. `0x03404cd4`who perform `r3 -= 8` branch in `0x03404b98`. Both thunk instructions and the transition address are verified by the original RPX words. This is callback draw, not destructor slot+0x1c (its neighboring thunk leads to the `0x034047a4`).

| Event | Name in setup | Action `0x03404b98` |
|---|---|---|
| 0, 1 | `Memory Barrior(Xlu/OpaOpaBlend)` | barrier, the context does not change |
| 2 | `Change ViewProj(G-Buffer/Opa)` | conditionally KSys+9 |
| 3 | `Change ViewProj(G-Buffer/OpaOpaBlend)` | conditionally KSys+9 |
| 4 | `Change ViewProj(G-Buffer/Xlu)` | conditionally KSys+9 |
| 5, 6 | `Change ViewProj(XluDepthWrite)` | conditionally KSys+9 |
| 7 | `Change ViewProj(Xlu)` | conditionally KSys+9 |
| 8 | `Change ViewProj(XluResetForParticle)` | conditionally |

The condition for events2..8 is byte KSys+0xc1a ⁇ {3,6,7}; otherwise, the handler does not change the context. KSys+9 is the same selector by which the CPU environment selects a previously studied inverse-B payload. If the desired event is performed before the water and the selector is not later changed, these two paths use the same index. The order for a particular water draw has not yet been proven.

Leaf `0x033ef460` returns 0; `0x033ef468` returns 0x16. So the individual particle-hook `0x03405ec8` temporarily selects context0x16, performs callback kind0x1a, and returns context0. In contrast, `0x033ef450` and `0x033ef458` returns -7 and -5: setup transfers them to `0x039995e0`, which packs sort inputs into metadata+4, obtained via record vtable+0x1d4. They are not to be confused with context indices.

Ghidra is supplemented only by the vtable leaf-tested `0x039999e4` function (32 bytes), the project is saved. Writers KSys+9 and +0xc1a are disassembled in the next section. The comparison of pass masks/sort keys of service events and terrain water remains open, as are live flags and real frames.

## KSys and context 22 matrix

Matching Wii U `reference/water/mode-camera-selection.json` contains 23 source hashes, verifications of two vtable slots, native predicate and PPC arguments of both branches of context22. Continues checking state writers from the previous section; the order of events regarding water is not yet established.

**The regime is applied on a deferred basis.** `0x034050a0` (cache query `034050a4`) writes byte arguments in KSys+0xc19 between calls `0x030bb668/030bb69c`. `0x03409478`caused by `0x03415cd0` address `0x03416500`checks c19!=0xff, transfers it to c1a, calls `0x03408b48`, then drops c19 to 0xff. Constructor `0x034051f0` First puts c1a=5 and c19=0xff, but later **requesting mode0** Therefore, constructor value5 is not an established game mode.

`0x03408b48` performs the setting only at non-zero KSys+0xb70 and +0xb74.

| KSys+0xc1a | KSys+9 | Events2..8 |
|---|---|---|
| 3, 6, 7 | 22 (`0x16`) | active |
| others | 0 | idle |

PPC stores KSys+9 are in `0x0340919c/0x034091b8`, values obtained through previously verified leaves `0x033ef468/0x033ef460`. Loop before this calls model vtable+0x1b4 for seven items with indices2..8. Slot `0x102c0aac+0x1b4 → 0x039996ec` sets/removes bit0 in +0x1c and calls `0x039e592c`, reassembling the array of active items. Item predicate slot `0x1034b1a8+0x2c → 0x039999d8` consists of three instructions and returns this bit0. For analysis, the vtable-confirmed function `0x039996ec` (60 bytes), Ghidra is saved.

Callers of requests were also found, without assigning the numerical modes of the alleged game names:

- `0x0364b890` reads manager `DAT_1047be88`: +0x530=2 querys2
  +0x530=4 queries6 at byte+0x653!=0, otherwise7 at +0x634=4, otherwise3. +0x530=5 leaves the request unchanged; the remaining values are requested1 at +0x638=1, otherwise0.
- `0x03673b60` similarly translates the transmitted argument: 2→2,
  4→6/7/3; here 5 explicitly requests 5, the rest →1/0 on +0x638.
- A `0x02c5cc8c` containing native `ViewerStage` strings queries 5.

This establishes write paths, but does not show what mode was in the saved Cemu session. A full search for immediate `stb ...,0xc19/0xc1a` is stored in `cpu/ksys-mode-byte-writer-navigation.json`; it does not exclude writing via alias, a calculated pointer, or another type of instruction.

**Context22 fills in the same environment updater `0x033ff8cc`.** Two branches call `0x0399bcf0` with index22:

| Conditionalities | A | B | Projection |
|---|---|---|---|
| KSys+0xaec!=0 and +0xaf0!=0 | KSys+0xaf4 | same as KSys+0xaf4 | pointer +0xaf0 |
| otherwise | First Context Record+0 | first record+0x60 | `0x03ae4aec(view+0x208)` |

PPC `0x03401074/0x03401080` transmits one address to r5/r7 for the first branch; `0x034010b0..0x034010e0` transmits the first record and record+0x60 for the second. In the previously restored layout, this is respectively the A0 and B0 matrix. Therefore, if there is a valid record, 22 fallback inherits the difference or equality of A0/B0, and override transmits the same A22/B22. `0x0399bcf0` itself selects the first record when out of the index, so inferring a self-record 22 requires sufficient capacity.

For the flow formula, this clarifies the condition `inv(B) * A * V = V`: in addition to selecting one context for the CPU payload and VS, you need the match A/B and the reversibility of B. Override-branch gives the matching of input matrices; fallback retains the previously described conditions of camera flags. This is while **conditional static communication**, without proof of the current mode, order of callbacks, validity inverse and immutability of state between update and draw.

Pass masks, capacity context and writers KSys+0xaec/+0xaf0/+0xaf4 remain open.

## Sort draw and terrain range between camera events

Matching Wii U `reference/water/draw-order-priority.json` saves 34 source hashes and native key checks, vtable/thunk and unsigned compare. Restored **conditional order on key**; getting events and water into one pass still requires checking masks. You can't replace this check with event names `G-Buffer/Xlu`.

`0x039bd5cc(view,1)` returns `*(view+0x214)+0x28` to list1. It's used by the previously found GBuffer Xlu/XluOpa passes. `0x0399be9c` takes the array of pointers from list+0x18 and count from +0x24; the dispatcher goes forward. The draw record contains item pointer+0, pass mask+4, key+0xc and float depth+0x10. **is a different structure of** than item with key+4.

The `0x039f14c4` build and its `0x039f194c/0x039f1e04/0x039f22e8` variants carry item+4 to draw+0xc. In the usual branch, OR is added with the word per-view workrecord+0x10. On the `0x039e81c4` path, this word is initialized0; this is not a universal statement about all build variants.

`0x03a24bbc` Assigns sort mode lists from ModelJobQueue+0xe0, then calls virtual slot+0x14 directly or puts jobs `ModelJobQueue::preSort`. Slot `0x1034b490+0x14` point `0x0399d684`The only branch in the `0x0399d544`The latter selects the sorter by list+0xc. Default table `0x1035ae38` It contains five BE words. `[0,1,0,0,1]`; initializer `0x03a23d40` reads each word and writes it down to the lower byte, so default list 1 is mode1. `0x0399cf3c`It's a heap sort by **Unsigned key by ascending, then finite depth by ascending**Checked native. `cmplw` The heap bypass playback coincided with an independent tuple-sort test in 512 synthetic sets, totaling 32518 entries. Equal pairs are not required to maintain order; NaN/depth infinity and late overrides sort mode have not been investigated.

**Terrain receives an intermediate range of keys when changing mode.** Previously found `0x03408b48`The current terrain system and bit1 System+0x944 cause `0x033ee8d0` for model-unit `0x036faa10`:

- modes 0/1/5 transmit 1;
- The 2/3/4/6/7 modes transmit 0. Of these, camera events are only active 3/6/7.

`0x033ee8d0` passes model-unit+0x24/count+0x1c, passes each model to `0x033ee768`. With argument0, it puts model+1=22 and writes `key=(old & 0xc3ffffff) | 0x08000000` for each item. With argument1, it puts model+1=0 and clears bits26...29 keys. A limited search for the corresponding native mask is stored in `cpu/draw-key-priority-mask-navigation.json`; the matches found do not prove the absence of other writers.

This is the same unit that WaterCore registers with: thunk `0x036f3450` loads System+0x2b8 and goes to `0x03942b78`. The latter transfers the owner+0x90 as a model-unit to `0x036b4488`; `0x036faa10` returns exactly System→+0x2b8→+0x90. `0x036b4488` creates resource models in this unit, including WaterCore, through the already restored registration of `0x03984eb0`. Late additions of models after updating mode require separate accounting.

Comparison with `0x039995e0` gives:

| Element. | Bits26..29 key | Range at top2=0 |
|---|---|---|
| Change ViewProj events2..7, input -7 | 1 | `0x04000000..0x07ffffff` |
| Terrain after `0x033ee768(...,0)` | 2 | `0x08000000..0x0bffffff` |
| Reset event8, input -5 | 3 | `0x0c000000..0x0fffffff` |

When top2 bits are the same, these ranges are saved in the draw key, and you get into the same list with the key **sorting, the terrain is preceded by the reset,** follows, regardless of the lower26 bits and depth. The boundaries are checked for all four top2 values. This explains the purpose of the intermediate priority terrain, but does not yet prove the execution of both events in a particular water pass.

The path of saving the resource item priorities is tested: model slot + 0x2a4 leads through `0x036bafa8 → 0x036bafac` to `0x03998180/0x03998278`, which, when recreated, transfer the old item + 4. Resource item slot + 0x5c is `0x0399e9d4`; its update bits16...25 does not erase bits26...29. This does not exclude direct overrides in other callbacks.

The masks and bucket for the regular GBuffer Xlu are restored in the next section, `0x10353eac/0x10353e6c` assembler tables and their variants are saved for further testing of alternative routes.

## GBuffer Xlu mask and general list of water with camera event

Matching Wii U `reference/water/pass-mask-bucket.json` contains 24 source hashes, resource options, native type table verification and PPC bit tests. The common **static route** event4 and TeraWater via list1 and pass bit0x20 is established. This clarifies the previous section: reset event8 should not be automatically included in the sequence of GBuffer Xlu.

`0x03971c70` calls `0x0399d74c(material+0xbc, material, callback)`, then `0x039e2780(material+0x10, ..., material, ...)`. The TeraWater saved has RenderState mode0, and RenderInfo `gsys_pass="no_setting"`: table `[5,0,1,2]` gives type5 to +0xbc, without swapping type for `seal/xlu_water`. This is not shader choice3 derived from a separate +0xbd field; these values cannot be mixed.

| Author's Option | Meaning. | material+0x70 | Model-material flag |
|---|---|---|---|
| `gsys_deferred_shading_material` | 1 | bit10 | bit17 |
| `gsys_gbuffer_xlu` | 1 | bit11 | bit19 |
| `gsys_gbuffer_xlu_opa` | 0 | bit12 is not being put | bit20 |
| `gsys_enable_color_buffer` | 1 | bit2 | bit8 |

Option lookup `0x039e2544/0x039e2658` checks for name in authored and shader dictionaries, then choice string with the first character not `0`. `0x039e2780` puts the corresponding material bits. Proven model+0x214 → `0x03997f30` slots them to indices17/19/20/8; `0x039e56c8` collects 23 flags in model+0x44, stride0xc. `0x03997f30` was determined by vtable, computed branch table restored by Ghidra; project saved. Saved resource does not set the state after arbitrary runtime override or initial callizer.

Dirty update goes through `0x039e6c54 → 0x039e6a24 → 0x039e6218`. Save parsings for reuse: `0x039e6218`, `0x039e6a24`, `0x039e61dc`, `0x039f14c4`. For type2 or 5, with flag6=0 and base bit29, check `flag17 && flag19` puts mask0x20. Flag20 adds 0x40, flag8 -0x80. Type3/4 go on other branches. Native tests `0x039e6548/0x039e6554` and OR on `0x039e6608` are confirmed by PPC, not just pseudocode.

Event4 has type2 through `0x039e4ca0`, flag17 from the setup loop of the first five items, and flag19 from OR0x80000. So it passes the same test as water type5 with two options=1. For water, stored values also give bit0x80 and do not add 0x40; this is not a statement about the full final mask.

**Per-view Recording Conditions:** for classification1 and getter model slot+0x194 ≥ 1 function sets record flags=1. The presence of mask0x20 removes base bit29, but does not add record bit3. Source flags1/3 or per-view flag1 can later remove bits0/2, eliminating the usual. Getter<1 uses another path; this is not yet disassembled fade state, it can not be replaced by a conventional record.

`0x039f14c4` computes bucket as `(record.flags >> 3 & 1) ^ 1`. For this ordinary record, bit3=0, which means bucket1, the list address of `view-work+0xe8+0x28`. Draw mask is taken from record+8 AND workrecord+0x18. On the path of `0x039e81c4` , the last filter changes only bit27, saving 0x20. When visibility is saved, both elements therefore pass `Model(GBuffer/Xlu)` (`0x039b1ce8`): list1, mask0x20, initial context0. The pass itself also has a renderer/config gate, it is not called unconditionally.

Together with priority bands from the previous section, this leads to the following conclusion: with active event4, matching top2 key bits, saved bands, regular list1 and passed visibility/fade/filter gates **event4 is executed before terrain water**. In modes 3/6/7, it selects 22, coinciding with KSys+9 for the CPU inverse-B payload. Other callbacks between them, late registration of models and change of mode still require accounting; this is not proof of selector in the saved frame.

Events7/8 over setup doesn't get flags17/19 and don't give mask0x20 along the way. So reset8 **is filtered out in GBuffer Xlu**, even if it's on the same sorted list. Its order relative to terrain is important for the other pass; the overall key range doesn't prove a callback call.

Next step: capacity context22 and writers override matrix KSys+0xaec/+0xaf0/+0xaf4. Then separate check timing/other callbacks and alternative draw-list builders, without re-examining this mask.

## Realization in the viewer

2026-09-28, branch `claude/water-native`. `water_material.wgsl` executes formulas from the sections above to f32 (order of operations and native literals saved, bitwise equivalence is not stated). Parameters - FMAT `TeraWater` from the dump (`load_water_material`), table - `WaterAlb`; without dump - values v208 and hand-held table stub.

**Read in exactly the same Cemu GLSL VS9** (`7b5b9f2fd4a624a6`), natively not yet executed probe. Block VS8 - `gsys_material`; `tex_srtN` lie in bytes of `32+32N` (from `material-uniform-offsets.json`), that is vec4 [2...13] as matrices 2×3 in columns. VS builds `base = srt3·(x,z)` of the world (R3 - the position of the vertex, the same as multiplied by A), then:

| Varying | Matrix | Sampler in PS |
|---|---|---|
| Sem0.zw | srt0·base | `_s0` Secondary Normal |
| Sem1.xy | srt1·base | `_n0` Basic Normal |
| Sem1.zw | srt2·base | `_e0` foam |
| Sem4.xy | srt4·base | `_a1` third normal |
| Sem4.zw | srt5·base | `_t0` foam distortion |

I also have vertical displacement: `h = 0,1·T6.g`, `k = T6.b/max(T6.g,1e-5)`, `y += (h·sin(k(0,35z + 2t))·cos(k(0,35x + 1,4t)) − 0,025)·fade`, `fade = saturate((water-water − relief − 0,025)/h + 0,5)`, `t = environment[37].x`, corners wrapped in -π, π) to SIN/COS. The relief of VS reads from `tera_height` It's geomorph; it's baked at the top of the viewer. `roundEven(kind)`. `WaterEmm` has a select RRRR component, `WaterNrm` - RG11. `T6.g` = 0 (species without waves) `h` = 0 and `fade` games divided by zero; the result on the GPU Wii U is not restored (output from the formula VS9); the viewer decides the input / output by sign `water-water − relief − 0,025`eh `log2(0)` into `a` (LOG_CLAMPED, above) - full transparency.

The assumptions of the viewer (register - [RENDER-002 (archived reference)): A=B (phase on world x/z), `T` = 30 frames / s without slowing down (S=0.5 and R=20 restored, [ see above ](#frame-rate-s-and-r-restored-statically)); matrix SRT - Maya rotation `u' = sx(cos·u + sin·v)`, without pivot; normal - based on the game ([ higher ](#the-basis-of-normality-a---matrix-of-the-species-2026-09-29), from 2026-09-29; before that x/y/z / z / ed → world x/ z / water was restored, ZxQ3 c. above the first image is a lighted in the game with an open display of 201 / 2000 sc1 s, compared to the image of the game, respectively.

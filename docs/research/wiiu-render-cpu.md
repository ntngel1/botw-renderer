# Wii U render CPU evidence and binary identity

Date: 2026-09-28. Subsystem: rendering. Related gap: DATA-001.

## Question and sources

Trace the effective Wii U environment resources into render uniforms, cloud
texture selection, atmosphere lookup tables and postfx selection. Switch
addresses and layouts are navigation aids, not proof for Wii U.

Local roots are configured by `renderer.toml`; binaries and Ghidra databases
remain outside this repository. Verified local identities:

| Layer / relative file | Bytes | SHA256 |
|---|---:|---|
| `base/code/U-King.rpx` | 22029888 | `cfdcf9f85535a09e82a863792e96d8fec81888d06c47fe5dd33db8f1f3b315e1` |
| `update/code/U-King.rpx` | 23268032 | `ba58da5b95ce929e005d058ceb08b9b2788d1ab2bbc8a6c189bbadca0bb34d30` |

`update/code/app.xml` identifies title `0005000E101C9500`, title version
`0x00d0`; `update/meta/meta.xml` agrees with decimal title version 208.
These metadata values are verified; a retail version label has not yet been
independently read from the running game. The update RPX is the analysis target.
Its ELF header identifies 32-bit big-endian PowerPC, Cafe ABI, entry
`0x03c70404`. Resource/GPU payload endianness must be established separately.

Supporting decompilation: `zeldaret/botw` commit
`5b26254bfe69560b0993a50d8ba8f418ae749380`, targeting Switch 1.5.0.
The project's [platform rationale](https://botw.link/about) explains its
choice and the value of cross-checking Wii U. The locally available Switch
packages have not yet been identified by executable build/version.

## Direct binary observations

The following are virtual addresses of strings in the decompressed `.rodata`
section of the update RPX. They are **search anchors**, not recovered callers:

| Address | String |
|---|---|
| `0x102c0660` | `System/KSys/sky.skybin` |
| `0x102c0710` | `hdr_compose` |
| `0x102ffd8c` | `Exposure` |
| `0x103318a8` | `cExposure` |
| `0x1034d520` | `AutoExposure` |
| `0x1034d56c` | `color_correction` |
| `0x10376fb8` | `color_correction_map` |
| `0x1037736c` | `hdr_compose` |

Reproduction: read ELF32 big-endian section headers; for a section carrying
`SHF_RPL_ZLIB=0x08000000`, its first four bytes give the uncompressed size
and the remainder is zlib. Validate that size after decompression, locate
the NUL-terminated string and add its section virtual address. This layout
is also described by the original
[RPX loader converter](https://github.com/Maschell/GhidraRPXLoader/blob/v0.9.2/src/main/java/cafeloader/RplConverter.java).
No relocation processing or function recovery is implied by this inventory.

## Ghidra setup

The user opened project `botw-wiiu-analysis`, initially empty. The integrator
is its only writer. Ghidra 12.1.4 and GhidraMCP 7.0.0 are installed locally.
The RPX loader was absent. With the user's import/setup authorization,
Maschell/GhidraRPXLoader v0.9.2 (`d496d43`) was downloaded from its official
repository and built against the installed Ghidra jars using JDK 21.
Its Espresso SLEIGH specification was also compiled with Ghidra 12.1.4.
Both compilation steps succeeded. No third-party code was added to this repo.

The locally built extension is installed under the user's Ghidra 12.1.4
`Extensions/GhidraRPXLoader` directory. Its jar SHA256 is
`75a1cbde3a770d70dc5b0185b35ae653c34d726109a2c9934e526ec3637c892d`.
The published prebuilt zip targets 12.0; this installation uses the locally
compiled jar and language definition instead. After the user restarted Ghidra,
the update RPX was imported into the main project. The import request exceeded
the MCP client's 300-second timeout, but server analysis continued and reported
completion in 579 seconds. After the user accepted the native plugin dialog,
API readback confirmed `/U-King.rpx`, the update executable path, Espresso
language, 97,804 functions, `analyzed=true` and `analyzing=false`.
`save_program` then returned success. The main project is ready for research.

A separate temporary headless project verified loader selection
`Wii U / CafeOS Binary (RPX/RPL)`, language
`PowerPC:BE:32:Gekko_Broadway_Espresso`, the SHA256 above and original section
addresses. Import warned about six unsupported Havok TLS relocations (types
68/78) at `0x1045cc14..0x1045cc28`, plus absent external RPL libraries.
Autoanalysis also reported some failed thunk/function creation and unresolved
import stubs. These limitations must not be interpreted as recovered behavior.

## Targeted native consumers

While the main project analyzed, a separate headless project was used for
targeted disassembly/function creation/decompilation, without full automatic
analysis. All addresses below refer to the exact update RPX above. Provisional
function names describe observed roles; they are not original symbols.

- `0x03405f48` prepares sky initialization arguments and loads `sky.skybin`.
  ELF relocations at `0x03406eb2`/`0x03406eb6` reference its path string.
- `0x033f358c` receives the initialization arguments; `0x033f2980` creates
  texture surfaces; `0x033f3558` verifies the sum of their image sizes equals
  the input file length before three consecutive copies. Layout and limitations
  are recorded in [the sky investigation](wiiu-sky-resources.md).
- `0x03aa4378` initializes color-correction resources and creates an 8×8×8
  surface using internal format `0x1a`. Its role is supported by the native
  `agl::pfx::ColorCorrection` string, creation calls and eight render-target
  iterations, not just by proximity to a Switch function.
- `0x03abb644` initializes AGL HDRCompose resources (native `HDRCompose`
  string reference); `0x03abb79c` selects AGL variants and uploads parameters.
  This is not proof of the KSys `uking_pass_shader` consumer: the two archives
  contain different programs with the same name. See
  [the postfx investigation](wiiu-postfx-materials.md).

### Internal AGL format → GX2 format

Native call chain `0x03a7e618 → 0x03b4abb0 → 0x03b485b8` converts the internal
format before writing the GX2 surface format field. The final function reads
a big-endian u32 lookup table at `0x1047ed60`. The following original `.data`
entries were checked directly, independent of the Switch enum definitions:

| Internal value | Table address | GX2 value | Format |
|---|---|---|---|
| `0x1a` | `0x1047edc8` | `0x816` | R11_G11_B10_FLOAT (color-correction LUT) |
| `0x2b` | `0x1047ee0c` | `0x820` | R16_G16_B16_A16_FLOAT (sky tables) |
| `0x2e` | `0x1047ee18` | `0x823` | R32_G32_B32_A32_FLOAT ([WaterAlb readback](wiiu-water-variants.md#intermediate-shader-reading-wateralb)) |

The GX2 value meanings are documented by
[WUT enum.h](https://github.com/devkitPro/wut/blob/master/include/gx2/enum.h).
This establishes the format conversion, not sampler filtering or the final
GPU pitch/tiling. Function prototypes inferred by Ghidra remain provisional;
missing call arguments must be checked against instructions before reuse.

Local raw pseudocode, original scripts and logs are outside Git under
`game-data/reference/visual-formulas/cpu` and the temporary import-check
directory. The main project should receive only reviewed names and types;
no bulk Switch symbol transfer was performed.

### Material shader option overrides

The autoanalyzed main project identifies `0x039e2780` as the consumer of
`gsys_renderstate` (`0x10353af8`), `gsys_alpha_test_func` (`0x10353b0c`) and
`gsys_alpha_test_enable` (`0x10353b24`). Its only recovered direct caller is
`0x03971c70`. After iterating the authored material option dictionary, it
overwrites four options using the third argument's runtime material state:

| Shader option | Native source / transformation |
|---|---|
| `gsys_renderstate` | byte at `+0xbd`: 0/1/2 map to themselves, 5 maps to 3, other values to 0 |
| `gsys_alpha_test_func` | big-endian u32 at `+0xd8`, low three bits |
| `gsys_alpha_test_enable` | same u32, bit 3 |
| `gsys_pass` | byte at `+0xc1` |

These are runtime object offsets, **not BFRES offsets**. Native tables at
`0x103539a4` and `0x103539ac` contain the alpha-function identity mapping and
the first three render-state results. A pointer table at `0x1047e340` selects
the decimal choice strings `"0"` through `"9"` at `0x103535e8` onward.

Helper `0x039e26a4` looks up the option and requested choice and invokes
`0x03bf8d38`. That function selects a key buffer using option flag bit 0,
then `0x03c09ed8` writes
`key[word] = (key[word] & ~mask) | (choice_index << shift)`.
Word, shift and mask come from option-record offsets `+6`, `+7` and `+8`.
Ghidra omits the third argument in the intermediate function's inferred
prototype; its PPC listing confirms that `r5` is forwarded unchanged to
the key writer. This is a concrete reason why matching every authored option
literally can produce no archived program match.

The caller `0x03971c70` first invokes `0x0399d74c(material + 0xbc, material, callback)`, then the shader option consumer. The initializer resolves the
FMAT relative pointer at `+0x20` to its RenderState. Its low two flag bits
select table `0x1034b550 = [5, 0, 1, 2]`, whose result is stored at runtime
material `+0xbd`. Combined with the shader-option mapping above, the initial
`gsys_renderstate` choice for BFRES modes 0/1/2/3 is respectively 3/0/1/2.

The initializer also reads alpha control at RenderState `+0x0c`:
`0x03bf6b70` extracts bit 3 (enable), and `0x03bf6b7c` extracts the low three
bits (function). Identity table `0x1034b4c8` preserves the function enum;
both fields are written to runtime material `+0xd8`. These native accesses
establish the initial values without choosing a nearest archived key.

For runtime `+0xc1`, the same initializer first writes zero, then reads the
FMAT RenderInfo named `gsys_pass` (`0x0396de34`). Helper `0x0396df70` uses
table `0x1047d9b0`: `no_setting`, `seal`, `xlu_water`, `reduced_buffer`.
The latter three strings select pass 1, 2 and 3 respectively; absent or
unrecognized values retain zero. This RenderInfo is separate from the
similarly named authored shader option.

An optional callback at the initializer's end can alter runtime state, and
later changes remain untraced. Therefore an initial static candidate is not
proof of the variant selected during a real frame.
Raw decompilation, tables and the intermediate PPC listing remain local in
the CPU evidence directory, with address and endpoint in their filenames.

### KSys HDR compose uniforms and LUT gate

The startup Cemu snapshot now contains exact KSys PS 547 bytes; see
[capture evidence](wiiu-cemu-sessions.md). Following the registration at
`0x03407bcc` establishes the KSys consumer independently of AGL HDRCompose:

- The `hdr_compose` registration stores its program handle at object `+0x8c8`.
  It registers uniform index 0 as `cParam` and sampler indices 0/1/2 as
  `cColor`, `cBloom`, `cColorCorrection`. These are named binding records,
  not necessarily native texture-unit numbers.
- `0x03404cdc` reads that handle (`lwz` at `0x03404d88`). Callback
  `0x0340c240` forwards the singleton `DAT_1046f428` and draw arguments to
  it; the callback is installed by both `0x03405f48` and `0x034077e0`.
- The draw function clears the supplied LUT pointer when object `+0x164`
  lacks mask `0x100`. It selects the first variant when the resulting
  pointer is null, otherwise the next variant and the third sampler record.
  Together with archive variation order/reflection this establishes the
  local no-LUT/LUT choice. The supplied LUT is traced below; ownership of
  the KSys enable mask remains unresolved.

PPC instructions `0x03404dc8..0x03404e40` (including the alternate branch)
construct a four-float upload at stack `+8`, then call `0x03a788f8` with
count 4 and uniform record 0, unless its location word is `0xffffffff`:

```text
cParam.x = object.float[0xc50]
cParam.y = f32(object.float[0xc4c] - object.float[0xc50])
cParam.zw = two floats from the draw argument
```

The subtraction is native `fsubs`. In PS 547 only x/y are consumed; the
LUT variant has no `cParam` reflection. The conditional upload is therefore
significant. Helper `0x03a788f8` checks two signed-short stage locations and
forwards the count/data pointer to GPU import wrappers. Ghidra incorrectly
marks one wrapper non-returning; the caller listing, rather than that
inference, is the basis for following later bindings.

Constructor `0x034051f0` writes the two fields at `0x0340586c` and
`0x034058ac`. Their original constants were read from program memory:

| Object field | Constant address | f32 bits | Exact decoded float |
|---|---|---|---:|
| `+0xc4c` | `0x102c04ec` | `0x3f95c28f` | 1.1699999570846558 |
| `+0xc50` | `0x102c04f0` | `0x3f7d70a4` | 0.9900000095367432 |

These are constructor defaults, not captured frame uniforms. They must not
be replaced with `master_field.baglccr` saturation 1.175. Combining the
upload with the recovered shader formula gives, algebraically,
`s = hi - (hi - lo) * d*d`, with `hi=field[c4c]`, `lo=field[c50]` and
`d=(2/3)*sum(q)-1` using the native f32 coefficient. Thus the constructor
curve peaks near 1.17 at `sum(q)=1.5` and approaches 0.99 at black/white.
This reduction does not preserve all intermediate f32 rounding.

Raw answers are saved under the local `reference/cpu` root as
`03404cdc-{decompile_function,disassemble_function}.json`,
`0340c240-decompile_function.json`, `034051f0-*.json`,
`102c04ec-read_memory.json` and binding-helper decompilations. No program
types, names or instructions were changed. Custom-script execution is
disabled in the current bridge; supported read-only endpoints still work.
The saved search `cpu/ksys-0xc4c-all-instructions.json` (and its `+0xc50`
equivalent) mixes unrelated objects and negative offsets; later writes of the
saturation fields remain unresolved (visual-formulas card, 2026-10-02).

## Next evidence

Continue tracing the string anchors above to their consumers. Recover
later writers of the KSys saturation fields, the source of its supplied LUT
and the owner of enable mask `0x100`, plus callback/dynamic changes to material state.
The texture-format conversion and loader chain above are native CPU evidence;
the full render graph and cross-platform equivalence remain unverified.


### Color-correction LUT ownership and inputs

The matching Wii U RPX establishes a conditional path from the AGL
ColorCorrection object to the KSys callback. This is static pointer and
control-flow evidence, not a captured LUT-enabled frame. Let `Pfx` denote
the scene postfx object, `CC = *(Pfx+0x224)`, and `HDR = *(Pfx+0x230)`.
All offsets below are bytes; fields belong to different objects.

1. `0x039d9b70` allocates/initializes CC and HDR when their creation config
   permits it. CC calls constructor `0x03aa3898` and initializer
   `0x03aa4378`; the latter creates the previously identified 8³
   `R11_G11_B10_FLOAT` surface at `CC+0xe88`. It passes that surface to
   `0x03a82dc8` with destination `CC+0xf28`, connecting the sampler to the
   generated texture. It stores coordinate coefficients 0.875 and 0.0625
   at `CC+0x380/+0x384` (bits `0x3f600000/0x3d800000`).
2. `0x039da034` prepares each HDR view record (array `HDR+8`, count `HDR+4`,
   stride `0x1a8`, falling back to record zero for an out-of-range view).
   It first clears the bloom and LUT pointers. With HDR enabled, it stores
   `CC+0xf28` into `HDRview+0x180` only when CC is scene-enabled, its
   parameter `enable` byte at `CC+0x1b8` is nonzero, and its selected
   generator variation at `CC+0x10b8` is nonzero. It also copies the two
   coordinate coefficients to `HDRview+0x188/+0x18c`. The pointer store is
   PPC `0x039da1b4`; the surrounding gate is `0x039da164..0x039da1e0`.
3. `0x039da8bc` dispatches through the scene callback when `Pfx+0x238 != 1`
   and bit 0 of `Pfx+0x210` is clear; its other path calls the separate AGL
   HDRCompose at `0x03abb79c`. In the callback path, context word 8 receives
   `HDRview+0x180` only if HDR flags at `+0x1c` have mask `0x2`, otherwise
   zero. Word 7 similarly receives bloom (`HDRview+0x184`) under mask `0x1`.
   Words 9/10 contain the coordinate coefficients.
4. The already identified callback `0x0340c240` passes context word 8 to
   KSys draw `0x03404cdc`, which applies its additional object `+0x164`
   mask `0x100` gate. This closes the supplied-LUT chain without assuming
   that AGL and KSys `hdr_compose` programs are interchangeable.

Scene-enabled checks are separate from resource creation: `0x039d99ac`
requires CC to exist and byte `*(Pfx+4)+0x6e0` to be nonzero;
`0x039d9a44` requires HDR to exist and config byte `+0x6d0` to be nonzero.
`0x039b2a88` includes a call to CC drawMap `0x03aa47e8` before its later
HDR dispatch branches. Actual frame flags, resource overrides and the
KSys mask's owner are still open.

The generator's library slot is also identified independently of names:
`0x03b4049c` builds the program array from 8-byte records at `0x103768a0`.
Record 8 points to `color_correction_map` at `0x10376fb8`; the registration
assigns uniform indices 0/1/2/3 to `c3DTexCoordOffset`, `cHSBG`,
`cCurveA[0]`, `cCurveB[0]`. DrawMap `0x03aa47e8` reads library slot 8 and
uploads uniform record 1 as:

| Component | CC value field | Named parameter | CPU operation |
|---|---|---|---|
| x | `+0x1c8` | `hue` | divide by 60 |
| y | `+0x1d8` | `saturation` | copy |
| z | `+0x1e8` | `brightness` | copy |
| w | `+0x1f8` | `gamma` | reciprocal |

Names and value offsets come from constructor `0x03aa3898`; `hue` is also
read directly at `0x10366eec`. PPC `0x03aa495c..0x03aa49c4` confirms
single-precision `fdivs`, stack stores and count-4 upload. Thus the recovered
gamma shader's exponent is `1/gamma`, not `gamma`. This connects named
parameters to the GPU but does not prove which file's values survive scene
and weather overrides in a particular frame.

Variation flags at `CC+0x1394` are rebuilt by `0x03aa352c`:

| Bit | Condition | Selector effect in `0x03aa46b0` |
|---|---|---|
| 2 | hue differs from 0 (`0x03aa34ac`) | HSB option 2 |
| 3 | saturation differs from 1 (`0x03aa34cc`) | HSB option 1 if bit 2 is clear |
| 4 | brightness differs from 1 (`0x03aa34ec`) | HSB option 1 if bit 2 is clear |
| 5 | gamma differs from 1 (`0x03aa350c`) | add macro-2 stride |
| 6 | nonidentity curve samples (`0x03aa332c`) | add macro-1 stride |
| 7 | `toycam_enable` byte `CC+0x218` | add macro-3 stride |
| 8 | `order_toycam_hsb` byte `CC+0x208` | add macro-4 stride |

The macro ordering agrees with the archive's HSB/curve/gamma/toycam/order
axes. Hue takes precedence over saturation/brightness in the HSB axis;
neutral toy-camera numeric values do not clear its explicit enable bit in
this flag builder. The curve updater compares RGB at eight `i/7` samples
against identity with tolerance `2^-23` and sets bit 6 if any differs.
The internal curve evaluator itself is not yet recovered.

Bits 0/1 mean map/program dirty in these consumers: flag rebuilding sets
both; the variation selector clears bit 1, and drawMap clears bit 0 after
rendering. DrawMap requires `enable` and either bit 0 or bit 16, plus its
render-context gate. The additional bit-16 policy remains unresolved.
The setter at `0x03aa5064` provides a concrete brightness-update example:
it writes `CC+0x1e8`, updates bit 4, marks the program dirty if the bit
changes, and marks the map dirty. These details avoid treating variation
zero, an allocated texture, or `enable=true` as proof of active LUT use.

Saved evidence uses `reference/cpu/<address>-<endpoint>.json`, indexed and
hashed in the research registry. Some request addresses lie inside the
resolved function (`039da038` → `039da034`, `03aa3544` → `03aa352c`,
`03aa344c` → `03aa332c`); the response's function entry is authoritative.
PPC listings verify the pointer transfer and uniform upload. Ghidra's
atomic-loop decompilation in texture initialization remains imperfect;
the earlier surface-format and eight-slice evidence is retained.


### HDR LUT flag and constructor defaults

`0x039d9e54` owns the HDR mask `0x2` used in the callback dispatch above.
With HDR scene-enabled, it sets this bit exactly when CC is scene-enabled,
`CC+0x1b8` is nonzero and selected variation `CC+0x10b8` is nonzero;
otherwise it clears the bit. PPC `0x039d9f04..0x039d9f30` confirms the
set/clear stores. This agrees with the separately recovered per-view LUT
pointer gate. Constructor `0x03abb488` initializes HDR flags to `0x20`,
so mask `0x2` is not an allocation default.

KSys constructor `0x034051f0` initializes its separate flags word `+0x164`
to zero (the field address is formed by `addic.` at `0x03405378`, followed
by the zero initialization). Consequently its LUT permission mask `0x100`
is not a constructor default either. Its later writer remains unresolved.
A scoped instruction search found only the constructor's direct field
address among the immediate stores/address calculations in the KSys
region; this cannot exclude indexed access, aliases, bulk copies or
external/debug code. Do not conclude that LUT is never used.

Search navigation is saved locally as `ksys-164-writer-navigation.json`,
`ksys-singleton-relocations.txt` and `ksys-164-singleton-near-navigation.json`.
The last report is empty: no immediate field access occurred within the
chosen 0x180-byte window of a singleton relocation. This is only a bounded
negative search result. The base constructor `0x03999278` covers a separate
0x84-byte subobject at KSys+8; it does not itself explain this flags word.
Prefer another independent rendering question before widening this search.

## Main light direction and the sky's sun (`0x03656be0`)

Date: 2026-09-29. Same RPX (EU v208), Ghidra read-only decompilation plus
PPC listings. `0x0365b084` copies `SkyMgr+0x20ec..+0x20f4` into
`dir_main`'s `Direction` (`KSys+0xa0`, value `+0x11c..+0x124`; KSys is
`DAT_1046f428`). The vector is the direction the light travels (from the
light to the ground). `0x03656be0`, the SkyMgr calculation, fills it from
the time angle `a` (`TimeMgr+0x98`, hours × 15°, `0x03661b00`; the Switch
`timeToFloat` confirms the unit). For world mode `+0x530` ∈ {1, 3, 4}:

```text
night, a < 45 or a > 330 (22:00–03:00):
    t = (a (+360 if a < 60) − 330) / 75
    P = −R(axis, −tπ)·(1, 0, 0),  axis = normalize(0, −sin 47.5°, −cos 47.5°)
      = (−cos tπ, −cos 47.5°·sin tπ, sin 47.5°·sin tπ)
day, otherwise:
    t = 0 if a < 60, (a − 60)/255 if a ≤ 315, else 1
    P = (−cos tπ, −sin tπ, −SunSlope·sin tπ)
dir_main = normalize(P.x·L, min(P.y·L, SunDirYStop), P.z·L),  L = 80000
```

`R` is the quaternion rotation `0x201766c` → `0x203d48c` (right-handed,
standard matrix). The axis is built in the constructor `0x0364f620`
(`0x036543ac..0x036543d0`): angle constant `0x10300e74` = 0.829031
(47.5°), x = −cos·0 (`0x10300dbc` = 0). `SunParam` in
`WorldMgr/normal.bwinfo`: `SunSlope` −1.1, `SunDirYStop` −42000 (built-in
defaults 1.1 and −55000, `0x10300e64`, `0x10300e70`; registration
`0x036541f0..0x0365438c`, values at `SkyMgr+0x205c` and `+0x209c`).

For the field this gives: the sun rises in the east at 04:00 and sets in
the west at 21:00, culminating at 12:30 in the **north** (`P` has +Z,
towards the south), 42.3° up; the night light rises in the east at 22:00,
culminates at 00:30 in the north 42.5° up and sets in the west at 03:00;
in between, 21:00–22:00 and 03:00–04:00, the light waits on the horizon.
The cut at `SunDirYStop` keeps the shading light at least ≈27° up
(`0.525/|(…)|`). The uncut vector, followed per component with a step
limit (`+0x10..+0x18`, about 0.05 a frame), is normalized into the sky
object's `+0x72c..+0x734` (`KSys+0x854`, the end of `0x03656be0`): the sky's
`cSunDir`/`cSunZenithAngle` — at night, the night light's arc.
`EnvAttribute_N.DifUse`/`DifXang`/`DifYang` (`EnvMgr+0x39418/0x39428/ 0x39438`, stride 0xcc by palette set) replace the direction by fixed angles
for sets that enable it (15 sets; not 0 and 1).

**Switch fade.** Entering the night branch or leaving it (flag `+0x2190`) sets `+0x2118` = 1; `+0x2114` follows `+0x2118` by `+0x211c`·`TimeMgr+0xb0` a frame (0.0065, constant `0x10300dc0`; `+0xb0` is the Switch `_d0`, always 1 in v208: [time step](wiiu-sky-resources.md#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01)): the chase helper shape with that times the frame factor `t` as both the least and the most step (`0x03657b1c.. 0x03657c74`, re-read 2026-09-29), so a constant step. The target is set only when the palette set's `DifUse` byte (`EnvAttribute` `+0xc`) is 0 (`0x03656f2c..0x03656f60`). Right after setting it, the byte `WorldMgr+0x64a` is tested (`0x03656f64`; r27 is `WorldMgr` by its `mMgrs` load, inferred), 0 branching away; that branch is not traced (Ghidra, 2026-10-02). While `+0x2118` > 0 the direction is not updated; once `+0x2114` reaches 1, `+0x2118` returns to 0 and `+0x2114` falls back. `0x03408770` receives `min(1, +0x2114 + [+0x2f0 of the fourth world subobject])` and sets `base_light_change_ratio = 1 − x`, which the deferred shaders multiply the light direction by (`L = env4·ratio`). About 154 frames each way.

**Limits.** Modes other than 1/3/4 (type 2: fixed 65° elevation around an
angle from `0x03679d80`; others: a constant direction) are not mapped to
places. The `+0x2f0` term is not read. The viewer repeats the fade by
frames (`daynight::LightSwitch`, 2026-09-29). No captured
frame confirms the direction. Observation (a screenshot, not a matched
frame): in reference shot 443 (Great Plateau by day; the viewer's camera
matched to it, `vista_ne_1100`, looks north-east, and the far peak reads as
Death Mountain) the characters' shadows fall towards the camera, so the sun
stands ahead, in the northern sky, as the formula says; with a southern sun
they would fall away from it. An interpretation of the view's direction. The viewer applies the
path day and night since 2026-09-29 (`daynight::game_main_light`).

## Main light `env5` and the palettes' `Exposure`

Date: 2026-09-29. Same update RPX (EU v208), read-only: Ghidra decompilation
plus PPC listings from the decompressed sections (local helper scripts and
listings under `reference/visual-formulas/cpu/env5-exposure/`, with
SHA256 sums). The field and character pixel shaders light with
`C = gsys_environment[5]` ([field PS 32](wiiu-field-shading.md), PS 112 and
[characters](wiiu-character-shading.md)); this section traces `C` to the
palettes.

**Palette fields.** `0x0363d76c` registers palette `i` at
`EnvMgr+0x19c+i*0x468` (207 palettes). Value offsets inside a palette:
`BgDifColor` `+0x0c..+0x18`, `BgDifIntencity` `+0x28`, `AmbientIntencity`
`+0x408`, `Exposure` `+0x418` (constructor default 0). The field names and
defaults agree with the Switch `EnvMgr::initEnvPalette`; the offsets are
Wii U's.

**Blend into `dir_main`.** `ENV_UpdateWeatherPalettes` `0x036425b8` (the
EnvMgr calculation; name already in the project) blends each field over
four palettes with `0x03642418` (weights from `EnvMgr+0x180` and the time
transition), once for the previous and once for the active palette set,
then lerps the two by the set transition. For `BgDifColor`:

- when a row of palette sets (`EnvMgr+0x190` previous, `+0x194` active;
  see below) is 0, its colour is multiplied per channel (RGB) by the
  feature colour `F` (stack `0x74`; PPC `0x03644290..0x036442c8`,
  `0x036442e0..0x03644318`): `FeatureColor` of the weather's
  `WeatherInfluence` entry (`EnvMgr+0x391b4+0x315c+w*0xc4`, value `+0xc`;
  `w` is chosen from the weather state at `0x036432a0..0x0364331c`) times
  the climate's `FeatureColor` (below);
- an override lerps it towards the colour at `EnvMgr+0x3cdf4` by a factor
  `f22` when `f21 != 0` (`0x03644330..0x03644370`; not identified,
  possibly lightning);
- the result is stored to `dir_main`'s `DiffuseColor` (`+0xb8..+0xc4`,
  `0x03644374..0x03644390`).

`BgDifIntencity` is blended the same way (`0x03644428..0x03644518`); under
the same override a random term is added (`0x03644554..0x03644588`); the
result is clamped to `>= 0` (`fsel`) and stored to `dir_main`'s
`Intensity` (`+0x10c`, `0x03644594`).

**The row: `+0x190`/`+0x194` are `PaletteSetSelect`.** (2026-09-29,
second pass; this corrects the first reading, "palette set 0".) The
update copies the static table of palette-set rows (`0x1030024c`, 57 rows
× 3) to the stack and picks the sets as `rows[+0x190 * 3 + +0x184]` and
`rows[+0x194 * 3 + +0x188]` (`+0x184`/`+0x188`: the sky states clear,
overcast, change of day), so `+0x190`/`+0x194` index rows, not sets. The
active row comes from `EnvMgr+0x3cec4` when set (≥ 0), else from
`+0x3cecc`; `0x03641140` stores there the current climate's
`PaletteSetSelect` (`0x036773d4`: climate `+0x2dc`, the value of the
parameter registered at `+0x2d0` under `PaletteSetSelect` by
`0x03673f6c`, in the open world, `WorldMgr+0x530` = 1) when it is not 0;
without one the row falls back to 0 (or 5 when `WorldMgr+0x530` = 2).
So `F` tints `BgDifColor` in every climate on the field's row 0 (sets 0, 1,
2: clear, overcast and the change of day, weather included), and in no
other row: of the 20 climates only the woods (`DarkWoodsClimat` 7,
`LostWoodClimate` 1, `KorogForest` 10) are off row 0.

**The climate's `FeatureColor`.** The climates (`ClimateDefines_N`) are
the world manager's array at `WorldMgr+0x1e4` (count, pointer), stride
`0x310` (`0x0367be40`); `FeatureColor` is registered at `+0x244` (value
`+0x250`, `0x03675d28`). `ENV_UpdateWeatherPalettes` reads it for the
current and the next climate (`0x036723a0`, `0x03672eb8`) and lerps by
the climate transition (`0x03672ec0`) at `0x03642e1c..0x03642e94`
(stack `0x320`). At `0x03643b40..0x03643b64` it scales that colour by a
factor `k` (`0x030c032c`) and multiplies it per channel into the
weather's `FeatureColor` (stack `0x74`, `0x030c0480` → `0x030c02e8`) and
into the weather's `FeatureFogColor` (stack `0xe8`, loaded from the
influence entry `+0x28` at `0x0364343c`). So with the row 0:

```text
F = FeatureColor(weather) × FeatureColor(climate) × k
```

`k` (`EnvMgr+0x3ce64`) moves each frame towards 1 by a time-based step,
or towards `EnvMgr+0x3ce68` while the word `EnvMgr+0x3cee0` is set (writer
not identified), and passes a soft knee below 0.3 (`k' = 0.3·(1 − (1 − k/0.3)^4)`, constant `0x102ff7cc`; `0x036437f0..0x03643b3c`): in ordinary
play it is 1. The desert (`GerudoDesertClimate`, row 0) has
`FeatureColor` (1.25, 1.31, 1.09): its light is yellow-green and brighter
than the field's; Hebra (0.79, 0.97, 1.32) bluer, Eldin (0.85–0.93, 1.0,
0.65–0.75) olive.

**Where `F` and the fog product go** (2026-09-29, third pass: every
row-gated multiply in `ENV_UpdateWeatherPalettes`, traced to its palette
field and to the object written; decompilation of the whole function and
PPC listing). Each colour is blended per set like `BgDifColor`, the
previous set's blend multiplied when `EnvMgr+0x190` is 0 and the active
set's when `+0x194` is 0, then the two lerped:

| PPC (gate) | Palette field (value offset) | Factor | Written to |
|---|---|---|---|
| `0x03644290` | `BgDifColor` (`+0x0c`) | `F` | `dir_main` `DiffuseColor` (above) |
| `0x036449a8` | `FogColor` (`+0x38`) | fog product | `fog_scatter` (`KSys+0xa4`) colour `+0xe8` |
| `0x036451e0` | `YFogColor` (`+0x74`) | fog product | `fog_world` (`KSys+0xa8`) colour `+0xe8` |
| `0x036460f4` | `SkySunColor` (`+0xb0`) | `F` | sky object (`KSys+0x854`) `+0x74c` |
| `0x036483ec` | `CloudN_ColorBase` (layer `+0x0c`) | `F` | `CloudParam` `+0x440` |
| `0x036489b0` | `CloudN_ColorHilight` (layer `+0x38`) | `F` | `CloudParam` `+0x45c` |
| `0x03649024` | `CloudN_ColorShadow` (layer `+0x64`) | `F` | `CloudParam` `+0x478` (by the night fade, see [clouds](wiiu-sky-resources.md#cloud-cloud-program-option-9-matching-wii-u-2026-09-28)) |
| `0x036496a4` | `CloudN_ColorBackLight` (layer `+0x90`) | `F` | `CloudParam` `+0x3e4` (× a scalar, `0x030c04ec`) |
| `0x0364b260` | `VolumeMaskColor` (`+0x3dc`) | `F` | not followed |

The fog product is `FeatureFogColor` of the weather × `FeatureColor` of the climate (× `k`; stack `0xe8`); `F` is stack `0x74`. So `SkySunColor` is the sky's `dynamic_color` `+0x74c`, the bake's `cSunColor` ([sky resources](wiiu-sky-resources.md#cpu-chain--lut-of-the-sky-matching-wii-u-2026-09-28)): the sky table is tinted by the climate and the weather on row 0 only, like the main light. The objects: `KSYS_FindEnvLightAndFogObjects` (`0x033fe1cc`) looks the environment set up by name — `+0xa0` `dir_main` (`0x102c025c`), `+0xa4` `fog_scatter` (`0x102c0050`), `+0xa8` `fog_world` (`0x102c00c0`), `+0xac` `fog_inner` (`0x102c00cc`), `+0xb4` `hemi_inner` (`0x102c0268`); `+0xb0` takes the name returned by `0x033fe048` (not read). The clouds: `CloudParam` `i` of the `Cloud` object at environment set `+0x2b08` (`+0x15b0 + i·0x1070`), `i` = 0 and 2 (the loop skips 1, `0x03647ef0..0x03647f24`), takes the palette's layer `i` (`+0x160 + i·0xa0`), or layer 2 while the palette's `+0x34c` (by the field order `Cloud2NoUse`) is not 0, per palette before the blend (`0x03647f24..0x03648110`). The climates have no `FeatureFogColor` of their own (`ClimateDefines` in the dump), so the fog's tint is the weather's fog colour times the climate's light colour. **The scalar factors** (2026-09-29, fourth pass: every load of `0x190(r24)`/`0x194(r24)` in the PPC listing of the whole function, each followed to the palette field blended before it and to the factor applied after it). They are gated by the same row. A gate register is loaded once per field pair and reused across the calls to the blend helper `0x03642418` (its register use lets the compiler keep `r0`/`r10`), so Mie, g and `SfParam_attenuation` have no load of their own:

| PPC (gate, previous / active) | Palette field (value offset) | Row 0 | Written to |
|---|---|---|---|
| `0x03646328` / `0x03646400` | `SfParam_near` (`+0xdc`) | `+` climate `CalcSfParamNear` | stack `0x2b4` → `0x033f7b2c` |
| `0x036464f0` / `0x036465c0` (`r10`, `r0` reused) | `SfParam_attenuation` (`+0xec`) | `+` climate `CalcSfParamAttenuation` | stack `0x2c0` → same |
| — | `SfParam_horizontal` (`+0xfc`) | no factor, no gate | stack `0x2c4` → same |
| `0x036475a0` / `0x03647684` | `SkyRParam_rayleigh_amplifier` (`+0x12c`) | `×` climate `CalcRayleigh` `×` weather `CalcRayleigh` | stack `0x188` → `0x033f7a98` |
| `0x0364776c` / `0x03647844` (reused) | `SkyRParam_mie_amplifier` (`+0x14c`) | `×` climate `CalcMie` `×` weather `CalcMie` | stack `0x18c` → same |
| `0x03647950` / `0x03647a2c` (reused) | `SkyRParam_mie_symmetricalProperty` (`+0x13c`, g) | `×` climate `CalcMieSymmetrical` `×` weather `CalcMieSymmetrical` | stack `0x190` → same |
| `0x03649d04` / `0x03649ddc` | `BloomThreshhold` (`+0x3ac`) | `×` `TempMgr` bloom factor (below) | bloom object `+0xc` |
| `0x03649f58` / `0x0364a030` | `BloomIntencity` (`+0x3bc`) | `×` `TempMgr` bloom factor (below) | bloom object `+0x2c` |
| `0x0364ac24` / `0x0364b144` | `VolumeMaskIntencity` (`+0x3f8`) | `×` climate `CalcVolumeMaskIntencity` `×` weather `CalcVolumeMaskIntencity` | light shafts (not followed) |

The other `0x190`/`0x194` loads are the colour gates of the table above
(twice each, for the two branches of the blend) and the row bookkeeping at
the top (`0x036427d4..0x03642c20`). On other rows every field in both
tables is the palette's value as is.

- **Climate factors** (`ClimateDefines`, Wii U value offsets: `CalcRayleigh`
  `+0x26c`, `CalcMieSymmetrical` `+0x27c`, `CalcMie` `+0x28c`,
  `CalcSfParamNear` `+0x29c`, `CalcSfParamAttenuation` `+0x2ac`,
  `CalcVolumeMaskIntencity` `+0x2cc`) are lerped between the current and
  the next climate at `0x03642ebc..0x03643200` (stack `0x10`, `0xc`,
  `0x8`, `f24`, `f25`, `f26`). Only while `WorldMgr+0x530` = 4 (not the open
  world) the `Remains` entry's offsets are added to `f24`/`f25` and its
  factors multiplied into the weather's; the weathers have no
  `CalcSfParam*`. `CalcAmbientIntencity` (`+0x2bc` by the Switch
  `ClimateInfo` order, inferred) is not among these loads; the Switch code
  only registers it, and no reader was found (the viewer reads it by name).
- **Weather factors** (`WeatherInfluence`, value offsets `CalcRayleigh`
  `+0x44`, `CalcMieSymmetrical` `+0x54`, `CalcMie` `+0x64`,
  `CalcVolumeMaskIntencity` `+0x74`) are lerped between the previous and
  the active weather's entry by the weather transition
  (`0x03643560..0x03643828`; stack `0x18`, `0x14`, `f28`, `f31`).
- **Mie** gets one more step after the set lerp (`0x03647884..0x036478b8`):
  `mie' = 1 − x + x·mie` with `x` = `SkyMgr+0x2178` (manager slot 1 by the
  Switch order). The only store found is the sky manager's reset
  (`0x0364f324`, 2026-09-29: `x` = 1.0, next to `+0x2170` = 1.0 and the
  cloud rates `+0x2160..+0x216c`); a scan of every PPC store with the
  immediate offset `0x2178` (`0x02000000..0x04800000`) finds only unrelated
  UI objects, and the two other readers (the cloud update `0x0365867c`)
  only read it. So in play `mie' = mie`: nothing to repeat. (Not excluded:
  a store through a pointer with another base.)
- **Bloom's factor is not the weathers' blend.** `TEMPMGR_UpdateMoisture`
  (`0x0365d3c4`, manager slot 4) picks one `WeatherInfluence` entry by the
  weather type (`WeatherMgr+0x18`, getter `0x0366ad14`; the Switch
  `WeatherType` order) once the weather transition `WeatherMgr+0x14` ≥ 0.9,
  and sets the wet timer `TempMgr+0x64`:

  | Weather (type) | Entry | Timer |
  |---|---|---|
  | Bluesky (0) | none | — |
  | Cloudy (1), ThunderStorm (6) | 0 | 0.5 |
  | Rain (2) | 2 | 15 |
  | Snow (4) | 2 | 0.5 |
  | HeavyRain (3), ThunderRain (7) | 3 | 15 |
  | HeavySnow (5) | 3 | 0.5 |
  | BlueskyRain (8) | 1 | 15 |

  The timer is in the game's time units (`TimeMgr` `timeToFloat`: 15 per
  hour), so 15 is one game hour and 0.5 two game minutes. It is set every
  frame such a weather holds; otherwise it runs down by the time step
  `TimeMgr+0xa4` × `t` (`mTimeStep`, reset `0x03661870` to `0x3c088889` =
  0.25/30 per frame, one game minute per second) while `TimeMgr+0x12f` is
  set (`0x03661cb4`, the Switch `mIsTimeFlowingNormally`), not below 0.
  `t` is the frame-rate scale (`*(DAT_1047c258+0xc0)[core]`, 1 at 30 fps).
  `TempMgr+0x74` chases the entry's `BloomThreshhold` (`EnvMgr+0x3c310 + i·0xc4 + 0x84`) and `+0x78` its `BloomIntencity` (`+0x94`), or 1 and 1
  without such a weather, but only while the weather holds or the timer is
  0 (in between the values hold). The chase: `d` = target − value; within
  `0.005·t` it snaps, else it steps by `d·(1 − 0.99^t)` clamped to
  `0.005·t..0.01·t` (so for `d` ≤ 1 always `0.005·t`: 0.5 in 100 frames,
  3.3 s). The weather update copies `+0x74`/`+0x78` to
  `EnvMgr+0x3ce98`/`+0x3ce9c` (`r14+0x3ce4`/`+0x3ce8`, `r14` = `EnvMgr + 0x391b4`) while `TempMgr+0x64` > 0, else chases those towards 1 by the
  same helper (`0x0364e27c`: rate 0.01, steps `0.005·t..0.01·t`; `f30` =
  0.01, `0x10300220` = 0.005), so both hold the same value; it multiplies
  them into the palette's bloom on row 0. In rain the factor goes down to
  the rain entry's values in about 3 s (for the field: threshold × 0.5,
  intensity × 1.25; heavy rain 0.25 and 1.5; entry 0 is 1 and 1), stays
  there a game hour after the rain (two minutes after snow), then returns
  to 1 in about 3 s. The viewer repeats it (`climate::Moisture`) with its
  own weather blend as the transition.
- **The weather transition** `WeatherMgr+0x14` (weather calc `0x03667d20`,
  2026-09-29; constants re-read from the PPC listing
  `0x03668fc4..0x03669430` the same day, correcting the first reading):
  the wanted weather is `0x03672890` (the current climate's weather,
  `0x036723a0` → `0x036723a8`). It is taken into `+0x18` (the previous one
  kept in `+0x19`) only when it differs and the transition is at least
  0.99 (`0x1030215c`); then `+0x14` = 0. Otherwise `+0x14` chases 1 by the
  helper shape above: while `WorldMgr+0x610` = −1, rate `1 − 0.99^t` with
  the step both at least and at most `0.002·t` (`0x103021d0`) when
  `WorldMgr+0x649` (`0x03672318`, the world's set weather, the Switch
  `Manager::mWeatherType`, 0xff unset) is 0xff, else `0.0025·t`
  (`0x103021cc`); while `+0x610` ≠ −1, rate `1 − 0.9^t` and step
  `WeatherMgr+0x300·t`. The rate stays below the step for a gap ≤ 1, so the
  step is constant: a new weather takes 500 frames (about 17 s at 30 fps)
  in ordinary play, 400 (13 s) with a set weather. It is 1 at once when
  `WorldMgr+0x53c` is set, and when a weather is set (`+0x649` ≠ 0xff)
  without `+0x64d` (the Switch `setWeatherType` argument `x`) and
  `0x0366bff0` is 0 or `+0x610` = −1 (`0x036697a0..0x03669804`). (The first reading
  gave `0.0005·t`/`0.00005·t` and 67 s: those are `0x103021c0`/`0x103021c4`,
  the steps of `+0x2c0..+0x2c8`.)
- **`hemi_inner` and `fog_inner`** (`0x03645524..0x036455b8`): the weather
  update sets the hemisphere light's two colours (`+0xb8` and `+0xd4`, `SkyColor` and `GroundColor` by the object's order) to `EnvMgr+0x3cd48` every frame, and `fog_inner`'s colour to `fog_scatter`'s times the same colour, its start, end and ratio to `+0x3cdb4`, `+0x3cdc4`, `+0x3cdd4`. By the Wii U layout (4 `WeatherInfluence` of 0xc4 from `0x3c310`, 7 `Remains` of 0x104) `+0x3cd3c` is `IndoorPalette`, so these are its `FeatureColor` (dump: 0.5, 0.275, 0.2325, alpha 0.7), `IndoorFogStart` (1), `IndoorFogEnd` (12) and `attenuationForGrd` (1): no climate or weather `F`, no palette, no time of day. The field's deferred fill does not read `hemi_inner` at all: it takes the cube map through `LightAnalyzer` (`amb` from `cube(N)`, far `T[2,3]` = the cube up and down; [field shading](wiiu-field-shading.md#lightanalyzer-field-texes-gsys_user4-13)), so `F` reaches the fill only through what the cube map sees: the sky (`SkySunColor` × `F`) and the lit land (`dir_main` × `F`), both on row 0 only.

**Palette value offsets** (Wii U; value = parameter + 0xc, a `Color4f`
parameter 0x1c bytes, a float/u32/bool one 0x10, a cloud layer 0xa0), in
the Switch `worldEnvMgr.h` `EnvPalette` order and checked at each field
the code reads: `BgDifColor` `+0x0c`, `BgDifIntencity` `+0x28`,
`FogColor` `+0x38`, `FogStart` `+0x54`, `FogEnd` `+0x64`, `YFogColor`
`+0x74`, `YFogStart` `+0x90`, `SkySunColorNoUse` `+0xa0`, `SkySunColor`
`+0xb0`, `SfParam_near` `+0xdc`, `SfParam_attenuation` `+0xec`,
`SfParam_horizontal` `+0xfc`, `SkyRParam_rayleigh_amplifier` `+0x12c`,
`SkyRParam_mie_symmetricalProperty` `+0x13c`, `SkyRParam_mie_amplifier`
`+0x14c`, clouds `+0x160` (per layer: `ColorBase` `+0x0c`,
`IntencityBase` `+0x28`, `ColorHilight` `+0x38`, `IntencityHilight`
`+0x54`, `ColorShadow` `+0x64`, `IntencityShadow` `+0x80`,
`ColorBackLight` `+0x90`), `Cloud2NoUse` `+0x34c`, `BloomThreshhold` `+0x3ac`, `BloomIntencity` `+0x3bc`,
`VolumeMaskColorNoUse` `+0x3cc`, `VolumeMaskColor` `+0x3dc`,
`VolumeMaskIntencity` `+0x3f8`, `AmbientIntencity` `+0x408`, `Exposure`
`+0x418`.

**`dir_main`.** KSys `+0xa0` is found by `0x033fe1cc`: in the environment
object set (`*(KSys+0xc4)+0x41f4`) the object of type `DirectionalLight`
named `dir_main` (string `0x102c025c`, store `0x033fe32c`). The
`agl::env::DirectionalLight` constructor `0x03a989d0` lays out
`DiffuseColor` (parameter `+0xac`, value `+0xb8`), `SpecularColor`
(`+0xd4`), `BacksideColor` (`+0xf0`), `Intensity` (`+0x10c`, default 1,
`Min=0 ,Max=8`), `Direction` (`+0x11c`) and `ViewCoordinate` (`+0x128`).

**Into `gsys_environment`.** The record builder `0x03a0b78c` (see [environment layout](wiiu-water-variants.md#layout-and-ksys-extension)) takes the first enabled `DirectionalLight` of the scene's set (type check through vtable `+0x4c`, enable byte `+0x30`) and writes, PPC `0x03a0b95c..0x03a0ba8c`:

| Member | Vector | Value |
|---|---|---|
| 4 | `env[4].xyz` | per-view direction, `light+0x13c` array (stride 0xc, view byte `record+0x38`) |
| 5 | `env[4].w` | `Intensity` |
| 6 | `env[5]` | `DiffuseColor × Intensity` (all four components) |
| 7 | `env[6]` | `SpecularColor × Intensity` |
| 32 | `env[23].xyz` | a second per-view vector, `light+0x144` |

The product is helper `0x030c04ec` → `0x030c032c` (four `fmuls` by `f1`).
`BacksideColor × Intensity` is computed as well; its member was not
followed. Without an enabled light, members 6 and 7 take the static colour
at `0x1054a4fc` (in `.bss`; value not read). `master_field.baglenv` of
`Env/env.sgenvb` has a single `DirectionalLight`, `dir_main`, so for the
field the first light is `dir_main`. KSys itself reads the same product for
the `LightAnalyzer`'s `cMainLightColor` (`0x03401988`).

**Result.** In the game's units, on the field's row of palette sets
(`PaletteSetSelect` 0), in ordinary play (`k` = 1) and without the
override:

```text
env5 = blend(BgDifColor) × FeatureColor(weather) × FeatureColor(climate)
       × blend(BgDifIntencity)
```

No exposure factor enters it: the field's diffuse light of a white surface
facing the midday sun is `BgDifColor × 9` (no division by π).

**`Exposure`.** It is blended like the other fields
(`0x03647afc..0x03647c2c`) into `EnvMgr+0x3ce90`. Readers found by the
access pattern `WorldMgr(0x1047be88)→+0x464→[+0x18]→+0x3ce90`:

- `0x039644fc` and `0x03966ec0`: virtual getters (vtable slots at
  `0x10343b80`, `0x10344b84`; no RTTI) returning 0 without a world
  manager; their callers are not identified;
- `0x0383b6bc`: passes it as property 3 to `0x0383a31c`, which forwards it
  to `0x03b9b05c` of two globals (`0x1047f018`, `0x1047f028`) — by shape a
  pair of global property sets, possibly the effect and sound links (not
  verified);
- `0x0294fb7c`: a threshold comparison returning a bool;
- `0x02f4e890`, `0x03607100`, `0x033a2c0c` (called from `0x029f2740`):
  not classified.

None of them is on the light chain above. 2026-09-29, second pass: the
two virtual getters sit in vtables (`0x10343a38`…, `0x10344a1c`…) whose
sead RTTI chains share two base classes (`0x10495dd4` → `0x10495dac`) with
container methods that pick among children by a property value and a
range table (`0x03963fe8`, stride `0x3c`): by shape the effect/sound link
containers reading `Exposure` as a property, not a render pass (the name
itself is not in the ELink/SLink databases' strings; not verified further).
The only shader input named after it, `uking_dynamic_exposure`, is
`gsys_scene_material` byte 0x24 (manifest offset 0x25 − 1: vec4[2].y).
A scan of every dumped program (`block_locations` of the manifests, the
native ALU sources through `cemu_uniform_map.native_groups`) finds it read
by 180 of the 11 790 `uking_mat` pixel shaders that bind the block, all
with `uking_enable_emission` = 1, and by no `uking_sys` (deferred shading),
`uking_terrain`, `uking_grass`, `uking_tree` or `uking_flower` program. So
if the palettes' `Exposure` (1 by day, 0 in the night, dawn and dusk
palettes of sets 0 and 1) reaches it, it only changes some emissive
materials; the setter of `uking_dynamic_exposure` was not followed.
`AmbientIntencity` is 1 in every palette of sets 0 and 1, so whatever its
path, it does not darken the field's night or overcast. The `LightAnalyzer`'s `cExposure`
is unrelated: `0x038b410c` registers it as sampler 2 of program
`light_analyzer` (with `cGBuffer`, `cShadingBuffer`, `cCubeMap`,
`cCubeMap2D`), the frame's share in sunlight.

**`cAmbientMasterIntensity`** (2026-09-29, the `LightAnalyzer`'s `Master`,
record `+0x6e8`, uniform 0x16 in `0x038b48a4`). Its only setter
`0x038b5600` is reached through the KSys tail call `0x0340a70c`
(`KSys+0x1e0` = the analyzer, view 0), called only from
`ENV_UpdateWeatherPalettes` (`0x03644720`, `0x03644740`). The value is
`EnvMgr+0x3cea0` (`r14+0x3cec`). Right after `dir_main`'s `Intensity` is
stored (`0x03644594`, clamped to `>= 0`), while that intensity is above 0
the value chases 1.0 (`f27`, constant `0x102ff7a8`) through the step helper
`0x0364e27c` (rate `f14`), or is set to 1.0 at once when `WorldMgr+0x53c`
is set; only when the intensity is exactly 0 does another branch
(`0x0364459c..0x036446e4`, `0x0421067c` of the constant `0x103001bc` and
the frame factor) move it by a step. No palette field is read. Every field
palette has a positive `BgDifIntencity` (night 1.8, overcast 4.5), so in
play `Master` = 1: the ambient is not darkened at night or under clouds.
The sibling setter `0x038b5620` (`+0x6ec`, `cForceShadowRatio`) has one
caller, `0x034089a0`, not followed; `0x0340a718` → `0x038b55c4`
(`cAmbientScaleChara`) is called from `0x03643c0c`/`0x03643c3c`, also not
followed.

**Limits.** Static reading only: no captured frame confirms the uniform.
Other writers of `dir_main`'s fields are not excluded (scans of immediate
`+0x10c` stores found no other in the KSys/EnvMgr region, which does not
cover indexed or copied writes). The override (`f21`, `f22`), the writer of
`k`'s other target (`EnvMgr+0x3cee0`, `+0x3ce68`), the weather index
mapping and the unclassified `Exposure` readers remain open; a render use of `Exposure` through the
virtual getters is not excluded.

## Cloud shadows: the projection shadow (`CloudShadowOnOff` → `proj_shadow_off`)

2026-09-29, Wii U EU v208 `U-King.rpx`, static reading (Ghidra, read only; methods of the `aglprojsdw` class are not defined as functions in the project and were read with a pseudo-disassembler script). Shader side: PS 112 `bec68ec6f40a864f` ([field shading](wiiu-field-shading.md#ps-112-pre-shading-relief)).

**Shader.** `gsys_projection0` (unit 5 of PS 112) is not a screen target:
it is sampled at `uv = (W·ctx[35], W·ctx[36]) / (W·ctx[37])`, `W` = the
world position rebuilt through the inverse view (`env43..45`), at LOD
`0.005·z` (z = view depth, metres), channel `x`; then
`Shadow.x = sat(proj0 + uking_dynamic_proj_shadow_off)·…`. The option
`proj_shadow_matrix_view_coordinate` (registered by `0x039b4a64` at gsys
config `+0x10b8`, default false; read at `0x039df3ec`) keeps the matrix in
world coordinates.

**The matrix** (`0x039ddb98`, per projection-shadow entry of the shadow
manager `KSys+0x1240`, entries `+0x1440`, stride `0x704`): the entry's
texture matrix (`+0x30c`, 3×4) times the projector's view-projection
(`Projector+0x1c4`, 4×4 row-major; `0x03a9b0c8` = projection `+0x184` ×
view `+0x154`) → `+0x4c0`; `0x039dee9c` copies its rows 0, 1 and 3 to
`gsys_context[35..37]`. The translation column of the texture matrix
multiplies the clip `w`, so after the divide
`u = 0.5·x_ndc/bias_scale.x + 0.5 + t_x`, `v = −0.5·y_ndc/bias_scale.y + 0.5 + t_y` (rotation `bias_rotate` + animation, 0 here).

- Texture matrix `0x03b112b0`: rotation about z by `bias_rotate` (`+0x404`)
  + animated angle (`+0x344`), scaled by (`0.5/bias_scale.x`,
  `−0.5/bias_scale.y`, 1) (`0x02022744` scales the columns), translation
  `(bias_trans + 0.5 + anim + swing)` added (`0x0255ef3c`).
- `aglprojsdw` parameters (constructor `0x03b114b4`, value = parameter
  `+0xc`): `bias_scale` `+0x384`, `anim_trans_vel` `+0x398`,
  `anim_swing_cyc_x/y` `+0x3ac`/`+0x3bc`, `anim_swing_amp` `+0x3cc`,
  `bias_trans` `+0x3e0`, `anim_rot_speed` `+0x3f4`, `bias_rotate` `+0x404`,
  `repeat` `+0x414` (sampler wrap 0 = repeat, else 2 = clamp, `0x03b11414`).
  The sampler (`agl::TextureSampler` at `+0x190`) filters linearly within a
  level (mag/min `+0x2f4/+0x2f5` = 1) and picks the nearest level: mip
  `+0x2f6` = 1 (point) unless the byte `+0x418` is set (then 2, linear);
  `+0x418` is 0 from the constructor and nothing else of this class writes
  it (constructor `0x03b11a24…0x03b11a80`; the code at `0x03b11414`, not
  disassembled in the database, read from the raw words).
  Animation `0x03b11afc`: offset `+= anim_trans_vel·t/60`, wrapped to
  [0, 1]; angle `+= anim_rot_speed·t/60`.
- `Projector` (`agl` env object, constructor `0x03a9a6c0`): `proj_type`
  `+0xb8`, `view_pos` `+0xc8`, `view_at` `+0xe0`, `view_up` `+0xf8`,
  `near` `+0x110`, `far` `+0x120`, `aspect` `+0x130`, `fovy` `+0x140`
  (degrees), `height` `+0x150`. `0x03a9afd8`: type 0 → sead perspective
  (near, far, `fovy·π/180`, aspect), type 1 → orthographic of height
  `aspect·height`. `0x03a9ac08`: look-at, `dir = norm(view_pos − view_at)`,
  `right = norm(view_up × dir)`, `up = dir × right`.
- Dump (`master_field.baglenv` `Projector0` `shadowTex_Projector`): type 0,
  pos (0, 8000, 0), at (0, 0, 0.1), up (0, 0, 1), near 1, far 10000,
  aspect 1, fovy 5. So (derived) `x_ndc = −X·k/(8000 − Y)`,
  `y_ndc ≈ Z·k/(8000 − Y)`, `k = cot 2.5° = 22.904`: one repeat of the
  texture (`repeat` = true, `bias_scale` 1) spans 698.6 m at Y = 0,
  shrinking with height (`(8000 − Y)/11.45` m). `common.bgsdw`
  `projection_shadow_0`: `proj_name` `shadowTex_Projector`, `repeat` true,
  `bias_scale` (1, 1), all animation 0, `bias_trans` (−0.0389, 0.1479) —
  overwritten every frame (below). Texture: `p_shadow_clouds`
  (`collect.genvres`, `env.bgenv` `refer/26`, signature `prjshd`).

**`bias_trans` and animation are set by the environment manager.** `0x03657fac` (EnvMgr sub-object, `this+0x21xx`) each frame writes `(+0x2104, +0x2108)` (wrapped to [−1, 1]) to the first entry's `bias_trans` and 0 to its `anim_trans_vel`. The constructor/reset `0x0364f620` (`0x0364fac4`, `0x0364faf8`) and `0x0364f324` draw each a uniform random number in [0, 1) (`0x030c499c`, mantissa trick). **Correction (2026-10-01): the pattern does scroll.** The earlier scan (immediates `0x20fc..0x210c`) missed `FUN_03655de8`, which writes from the base `r28 = this+0x1cac` (`stfs …,0x458/0x45c(r28)`, 0x036566f4, 0x036566f8): every frame `+0x2104/+0x2108 += w·k₁·1.75·t`, w the sky manager's wind direction `+0x20c8/+0x20d0` and k₁ `+0x2144` (see [sky resources](wiiu-sky-resources.md#cloud-layers-what-skymgr-writes-fun_0365867c-wii-u-v208-2026-10-01)Writer K1, K2 and the Wind; while the counter `+0x2184` > 0 it adds `+0x210c/+0x2110·t` instead (both 0 from the reset, no other writer found). With the field's wind of about 5, k₁ = 1.5·10⁻⁴: 2.6·10⁻⁴ of a repeat a frame, some 5.5 m/s on the ground at the projector's 698.6 m repeat — the shadows sail with the clouds. The viewer follows it (`clouds.rs` `ShadowDrift`).

**Strength.** `proj_shadow_off = 1 − x` (`0x03408724` stores `x` at
`KSys+0xb5c` and sets the uniform through `0x034085e8`). In the frame
update `0x03657fac`:

```text
x = F · s · q · (1 − L)
  F = EnvMgr+0x214c: CloudShadowOnOff of the palettes (value byte +0x35c of
      the parameter registered at +0x350, 0x0363ea6c), 0 or 1 per palette,
      bilerped like every palette field (0x03642418, both palette sets)
      and lerped by the set transition (f19); ENV_UpdateWeatherPalettes
      0x03647c9c..0x03647e44
  s = +0x2150 chases 1, or 0 while the player is indoors (0x03677224,
      below): snap if |Δ| ≤ 0.01t, else step |Δ|(1 − 0.8^t) clamped to
      0.01t (t = frame factor, 1 at 30 fps), i.e. 0.01 per frame
  q = +0x2134 chases 1, or 0 while the counter +0x2191 > 0, which it counts
      down (0x0365b0f8): factor 1 − 0.9^t, step in [0.01t, 0.1t]; while
      WorldMgr+0x53c (stage timer) runs q takes its target at once
  L = lightning flash: +0x2f0 of WorldMgr.mMgrs[3] (Switch: WeatherMgr),
      read while its +0x34c ≠ 0 (0x03658260…0x03658288); written by
      0x03666714 in the thunderstorm weathers 6/7; else the term is absent
```

(Here the object is `SkyMgr` = `WorldMgr.mMgrs[1]`, the manager array at
`WorldMgr+0x464`, count `+0x45c`; `FUN_0365acb0` runs the update each frame
unless `WorldMgr+0x530` is 0, 2 or 5.) While `+0x2180 > 0` the update
sets `x` = 0 and counts it down (0x03658604…0x03658628).

**What drives the factors (2026-10-01, Wii U v208, Ghidra read only).**

- **`s`: the player indoors.** `0x03677224` = `if (p = *(DAT_1046cd00 + 0x90)) return p->vtable[+0x17c]()`, else 0. `DAT_1046cd00` (singleton,
  created by `0x0314fa3c`, `0x1bc` bytes) gets `+0x90` from `0x03417cd0`,
  which hands the pointer to several singletons; its callers `0x02d49730`
  / `0x02d49794` are `PlayerInfo::setAndAcquirePlayer` / `resetPlayer`
  (`PlayerInfo+0x30` = `mPlayerActor`, the Switch layout shrunk to 32
  bits) and pass `player + 0x838`, a secondary base of the player. Its
  vtable (`0x101e6c1c` in the player's constructor `0x02d56290`,
  `0x101e3ad0` in `PlayerBase`'s `0x02d31920`; entries are {adjust, function}
  pairs after an 8-byte head, so `+0x17c` is a function) gives the thunk
  `0x02d595f8` → `0x02d788fc`: `return (player+0x844 >> 2) & 1`. Bit 0x4 of
  `player+0x844` is cleared and set once a frame in `FUN_02d5b7dc`
  (`0x02d5e490…0x02d5e5b4`): the player's contact-point list `player+0x3a0`
  is searched by name for **`"InDoor"`** (string `0x101e554c`, lookup
  `0x034bc874`), and the bit is set when that list has a valid contact
  (iterator `0x034cefac`/`0x034cf0b4`, bit 0 of a point's `+0x44` marks it
  invalid); bits 0x8/0x10/0x20 then record the contacts' kinds. The layers
  `SensorInDoor` / `SensorHitOnlyInDoor` exist in the Switch decompilation
  (`physDefines.h`, static compounds `physStaticCompoundInfo.cpp`). So the
  cloud shadow fades out (0.01 a frame, 3.3 s) while the player stands in
  an indoor sensor volume and back when they leave. The player's
  `.bphysics` contact list was not read from the dump.
- **`q`: `ChangeWeatherTag` with `CloudShadowOff`.** The only writer of
  `+0x2191` besides the reset is `FUN_02538120` (`0x0253849c`):
  `WorldMgr.mMgrs[1]+0x2191 = 10` when the tag's bool parameter at
  `this[0x11]` is set. The function reads the map-unit parameters
  `Weather`, `WeatherEff`, `PaletteSel` by name and the rest by slot in the
  order of Switch `ChangeWeatherTagRoot::loadParams_` (`PSelSpeed`
  `this[0xb]`, `IgnitedLevel` `[0xc]`, temperatures `[0xd]…[0x10]`,
  `CloudShadowOff` `[0x11]`, `BluffThunderOff` `[0x12]` → `mMgrs[3]+0x35c = 10`, `FogMinusCorrection` `[0x13]`); it is
  `ChangeWeatherTagRoot::calc_` (Switch 960 bytes, `U`). It applies only
  when `FUN_0379f1dc(actor, 0)` (the actor's links `+0x4fc`, likely its
  basic signal) is on, so each frame of an active tag keeps the counter at
  10: q falls while the tag is on and recovers 10 frames after. MainField
  v208 (`actors_near`-style census of all 80 cells, static and dynamic
  units): 120 `ChangeWeatherTag`, 7 with `CloudShadowOff` true — six in
  B-6…B-8 (x −3840…−3240, z 1900…3130, Gerudo) and one in D-8 (−1390,
  3500); 3 with `BluffThunderOff`.
- **`+0x2180`, `+0x2184` are dead in v208.** Scans of every store with
  displacement `0x2180`/`0x2184` (and of `addi` bases 0x100…0x2191 with the
  matching displacement) find, for `SkyMgr`, only the reset `0x0364f324`
  (0), the countdowns (`0x03658628`, `0x0365b2ec`) and unrelated objects
  (static globals at `0x104a…`/`0x104c…`, UI classes `0x02f67508`,
  `0x02f67bc4`). Both stay 0: the shadow is never forced off and the drift
  never takes `+0x210c/+0x2110` (also only reset-written). A write through
  a computed pointer is not excluded by this scan.
- **Scene setup `0x03408b48`** sets the strength once (`0x03408724`: 1 for
  render scene type `+0xc1a` 0 or 3, else 0); in the field the frame update
  overwrites it every frame (callers of `0x03408724`: only these two and
  `0x03657fac`), so it matters only where `FUN_0365acb0` does not run.
- **`density`.** `0x039dede8` (entry, variation) = sat(`+0x46c`, or `+0x47c`
  for variation 1 when `+0x48c`) × sat(`+0x50c`/`+0x510`); `0x039dee9c`
  collects it per projection-shadow entry (two) and `0x03a0ab00` uploads
  the pair as `(d₀, d₁, 0, 0)` to uniform slot 0x10 of the shadow block,
  next to the matrices (slot 0xf). PS 112 and PS 108 read no density term
  (their cloud term is `sat(proj0 + proj_shadow_off)`), so it does not
  reach the field's lighting.
- **Grass and far trees take it per pixel.** Blades and tufts are G-buffer
  class 4 (`field_hybrid`) and far trees class 7 (`field_leaf`)
  ([field shading](wiiu-field-shading.md)); their pre-shading (PS 112,
  PS 108) samples `gsys_projection0` at each pixel's rebuilt world
  position, like the ground.

In field play outdoors, away from the 7 tags and without thunderstorms,
`x = F`, and the ground term is `sat(proj0 + 1 − F)`.

**Limits.** Static only; not compared with a Cemu frame. The contact
list's contents (which bodies carry `SensorInDoor`) are not read from the
dump; the tag's activation (`FUN_0379f1dc`) is read only as far as the
call; the lightning term `+0x2f0` (`0x03666714`) is not recovered beyond
its place in the product.

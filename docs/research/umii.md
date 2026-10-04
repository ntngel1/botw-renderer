# Recovered UMii work: implementation and gaps

Recovered on 2026-10-03 from the uncommitted files in Claude's
`agent-a4c8fc984d648a389` worktree. The research document referenced by
those files was missing. This note describes the available implementation;
it does not replace independent verification of the original game.

## Available implementation

- `bake/src/umii.rs` reads `.bumii` as AAMP, selects Hylian body, face,
  nose, hair, glasses, beard, backpack and hat models and colour-animation
  frames. Source comments cite `FUN_03444294`, `FUN_03433360` and
  `FUN_03433a94` in Wii U v208.
- `bake/src/material_anims.rs` reads shader-parameter animations from FRES.
- `bake/src/npcs.rs` samples colours, folds supported albedo recipes into
  per-villager textures, records bone aliases and idle clips, and writes
  placement descriptors.
- `character.rs` merges aliased joints and `cast.rs` starts each idle at
  a placement-dependent phase.

## Current limits

- Hylian assembly only; other races and Mii-based ffsd type 1 are rejected.
- Height/weight index is parsed but not applied. Facial feature position,
  scale, aspect and rotation, and other UMii adjustments are not fully used.
- The colour-recipe reader supports the recovered face colour operators,
  scalar constants and inverse channel variants. Multi-UV face rendering
  uses a separate GPU material; its recipe is excluded from single-UV baking.
  Missing animations or other unsupported recipes can leave original colours.
- Named characters without a supported UMii descriptor are not covered.
- NPC loading currently loads the baked cast, without recovered actor-life
  streaming rules.
- Layered face colour/depth/shadow composition and the separate nose attachment
  are connected and checked in local GPU captures. NPC016 now has its nose,
  but still lacks its mouth; full part assembly, proportions and facial animation remain
  unfinished. No pixel-for-pixel game comparison has been performed.

Until these gaps are resolved, villagers require explicit `render --villagers`
(SI-CHR-03); normal rendering do not load them automatically.

## Checks

The recovered code passes its CPU tests. A real bake into a separate copy of assets produced 58 placements
from 184 candidate placements, 57 actor definitions and 249 model units.
Successful loading and baking do not establish correct appearance.

The following investigation sections record the earlier checkpoints in order;
the latest GPU integration is described at the end.

## Bloom defect investigation

Bisecting the cast identified `Npc_HatenoVillage016` as a reproducer of the
large black regions. The defect also reproduces with that villager alone;
it is not caused by crowd size. Removing its face or disabling bloom makes
the world frame correct.

Temporary shader diagnostics using float exponent bits found non-finite
highlighted albedo on achromatic pixels (`sat <= 0`), while the incoming
normal/albedo and lighting were finite. Simple floating-point `x == x`
diagnostics did not catch the defect on this Metal adapter. These temporary
diagnostics are removed from the final shader.

The final shader evaluates the achromatic limit directly (`paled = M`)
and the black limit as zero, retaining the recovered highlight formula for
chromatic, positive albedo. Bloom remains enabled. Verified exports:
`captures/rendering-completion/npc-016-achromatic` (close-up) and
`npc-all-achromatic` (the full 58 placements, world camera). The blank face
is a separate material-composition defect and remains open.

## Recovered layered face shader

Platform: Wii U v208, `UMii_Hylia_Face_M_000 / Mt_Face`, `uking_mat`.
The material's static options match programs 11184–11187 with three runtime
render-state overrides (render state, alpha-test function and enable).
Original code and Cemu dumps are under the external visual-formulas reference:

- PS 11187: `model000-program11187-ps.code` is byte-identical to
  `e81702a641e3ed4f_00000f5249549249_ps.bin`, 5024 bytes,
  SHA-256 `e02b22b91ef53a74f5bc0833630f3e0eb251781a817246c7a61c18014257f69c`.
- VS 11184: `model000-program11184-vs.code` is byte-identical to
  `4f5a74221c021ff5_0000000000000000_vs.bin`, 2240 bytes,
  SHA-256 `7f545fd27c48ae9d4e8f7db67f8517d358f70f679b53d788f10dfcfb32aa0d6e`.
- Snapshot: `cemu-sessions/20260928T012415Z-cache-replay/shaders`.
  Reading copies with resolved uniform and sampler names are ignored files
  `captures/rendering-completion/face-{pixel,vertex}.glsl`. They are research
  aids; the original GLSL retains non-IEEE multiply and integer operations.

Sampler assignment is crucial: the shader's `_a0` is the material's `_a4`
(skin), `_s0` is `_a2` (line), `_n0` is `_ao0`, `_e0` is `_a0` (lash),
`_t0` is `_a1` (brow), and `_a1` is `_a3` (makeup). Picking the material's
first `_a0` as its albedo therefore picks the lash mask rather than skin.

Let `S`, `L`, `B`, `P`, `D` be samples of skin, lash, brow, makeup and line;
`cN` is `const_colorN`, and `sat` clamps each component to 0–1. The native
PS's face colour is:

```text
shade  = sat(c3 + (1 - c3) * sat(L.rgb + 1 - B.g))
skin   = sat(c1 * S.rgb * shade)
mask   = sat(L.g * P.a)
paint  = sat(mix(c0 * P.rgb, skin, 1 - mask))
brow   = sat(mix(c4 * B.r, paint, 1 - B.a))
albedo = sat(brow * D.r)
alpha  = gsys_xlu_zprepass_alpha * sat(B.a + L.a + 1 - B.g)
```

This proves colour calculation type 1 is A+B, type 17 is A+B+C, and
type 27 is `mix(A*D, B, C)`. Channel variants 11/21/41 invert the selected
channel; input 112 reads `const_color4`. These operators, per-register clamps
and computed-colour channel selection now exist in the NPC recipe evaluator.
Input 104–111 scalar meanings come from the previously recovered emission
programs; 113–115 remain the recorded SI-EMI-01 interpretation.

The VS takes four vertex UV attributes. Its exports demonstrate these paths:

| Samples | Vertex coordinates before interpolation |
|---|---|
| Skin, AO | `_u0` |
| Line, makeup | `tex_srt2(_u1)` |
| Brow | `tex_srt4(tex_srt1(_u3))` |
| Lash | `tex_srt5(tex_srt0(_u2))` |

Here `tex_srtN` means the game's uploaded 2×3 matrix, not an assertion that
the raw six-component BFRES parameter is already that matrix. The upload modes are now traced below; UMii feature-animation values still
need verification. Samplers
also differ: lashes/brows clamp both axes; line/makeup/skin mirror U and
clamp V. The next rendering step must preserve these UV paths, filtering,
alpha and feature transforms. A common-UV texture bake would lose them.

Validation: the complete eight-register colour graph is tested with
synthetic samples for skin, makeup and brow/line coverage. Scalar constants,
constant alpha/inversion, and cyclic graphs have separate tests. These
checks establish the recipe math, not finished rendering of the face.

## Four-UV asset transport

The skinned bake now preserves `_u1` even when `SecondUv` has no ordinary
normal/specular/map flags. It also copies `_u2` and `_u3` verbatim. GLB carries
these as optional `TEXCOORD_2` and `TEXCOORD_3`; old files without them remain
readable. The renderer uploads them as separate `Face_Uv2` / `Face_Uv3` mesh
attributes, alongside Bevy's UV0/UV1. No face SRT or layer composition is
applied yet; the single-UV recipe guard remains in place.

A real NPC bake verified all 57 layered `Mt_Face` primitives have four UV
accessors with equal vertex counts. The skinned GLB round-trip fixture also
uses distinct UV2/UV3 values so loss or swapping of those sets is detected.

## Layer inputs in baked assets

`MaterialInfo::layered_face` now records six actual KTX2 stems in shader-slot
order, each layer's three original GX2 sampler words, `const_color0..4`,
`gsys_xlu_zprepass_alpha` and six typed raw TexSrt parameters. The bake runs
after sampled colour animations, so these are per-villager colours rather
than the source model's defaults. Missing required face inputs fail baking
instead of silently writing a partially described face.

The Wii U parser currently exposes kind 30's mode word as an f32 bit pattern.
The face exporter reads `to_bits()` (e.g. float bits 1 become mode 1), not a
numeric float-to-integer cast. This avoids changing the shared source parser
while retaining the original word. Tests exercise modes 0/1/2 and all six
components, plus the GLB round trip of the complete layer descriptor.

Before tracing the native upload, the formulas in
[Cafe-Shader-Studio's BfshaRenderer](https://github.com/KillzXGaming/Cafe-Shader-Studio/blob/main/BfresEditor/Bfres/Render/BfshaRenderer/BfshaRenderer.cs)
were a research lead, not native-game verification. The local Ghidra bridge
reported no running instances. A read-only scan/disassembly of v208's RPX
rejected candidates `0x03be8a4c` (unrelated scaled float upload) and
`0x03c271f8` / `0x03c2723c` (codec/quantization helpers); neither proves the
TexSrt upload. No Ghidra database was changed. The layered GPU shader and feature transforms remain unfinished.

## Native TexSrt upload recovered

A read-only v208 RPX scan found the actual shader-parameter conversion path:
`0x03c07eec` installs built-in callbacks from table `0x103d31f4` when
parameter kind >= 28 and its custom callback is zero. Table entry 30 is
`0x03c07eb4`, whose mode-indexed dispatch table at `0x103d3274` contains:

| Mode | Kind-30 callback | Size / SHA-256 of instructions |
|---|---|---|
| 0 | `0x03c081b0` | 268 / `3461f0a4f265839699943fe66f0c186da28c2d98d8f575aac2c27a5ea878f54e` |
| 1 | `0x03c082bc` | 252 / `3839fcb81d370345696657f5f0bea67f34395877f9dbe543347fea051ab056a0` |
| 2 | `0x03c083b8` | 232 / `896fb25423589296b5e593200b38a4342ec73e78f78373e2d8a6faf2ebb7dda1` |

Each writes six floats and returns 24 bytes. With `xc=sx*cos`, `xs=sx*sin`,
`yc=sy*cos`, `ys=sy*sin`, the column-major matrix is:

```text
mode 0: [xc, -ys, xs, yc,
         sx*(-.5*cos - (.5*sin-.5) - tx),
         1 + sy*(-.5*cos + (.5*sin-.5) + ty)]
mode 1: [xc, -ys, xs, yc,
         xs*(ty-.5) - xc*(tx+.5) + .5,
         ys*(tx+.5) + yc*(ty-.5) + .5]
mode 2: [xc, ys, -xs, yc,
         xs - xc*tx - xs*ty,
         1 - yc - ys*tx + yc*ty]
```

Constants `0x103d3290/94/98` are 1, .5, -.5. The converter uses a native
256-entry trig table at `0x103d2140`, each row `[sin, cos, deltaSin, deltaCos]`.
Angle quantization takes the low word of the f64 expression
`f64(rotation) * 0.3183098861837907 + 3145728.0`; its high byte selects
the row and low 24 bits times `2^-24` interpolate it. The multiplies/adds
use the CPU's fused instructions where shown in the original disassembly.
This differs from the effects' sead table layout and angle quantization.

`TextureSrt::matrix_from_sin_cos` ports the recovered matrix arithmetic
with fused operations. Tests cover non-unit scale, translation and cardinal
rotations in all three modes, and reject unknown modes. It takes a native
table sine/cosine pair explicitly: host trig has not been substituted for it.
Remaining integration: bake this native table / final matrices, carry any
UMii feature animation, and implement the layered GPU material in colour,
depth and shadow passes. The raw descriptors alone do not fix blank faces.

## Native matrices now baked and UV paths connected

`bake::texture_srt` reads the native table from v208's RPX once for the NPC
bake. It checks kind-30's built-in callback and all three mode-table entries
before interpreting the table. It uses the original f64 fused angle-index
operation and f32 fused interpolation, then `TextureSrt`'s recovered matrix
arithmetic. Each face GLB carries all six resulting matrices. Old descriptors
without matrices remain readable, but cannot prepare layered UVs: the renderer
reports that the NPCs need rebaking.

The renderer's skinned mesh upload leaves UV0 unchanged, transforms UV1 by
matrix 2, UV2 by matrices 0 then 5, and UV3 by matrices 1 then 4. These are
fixed per-villager source parameters; animated facial feature transforms
remain a separate unfinished task. A noncommuting scale/translation fixture
checks selection of the separate UV sets and their composition order.

A read-only instruction interpreter ran the three original PowerPC callbacks
against the actual RPX constants/table and compared the real baked matrices.
All 342 matrices from 57 faces are bit-identical after the JSON numbers are
read as f32. Coverage: 228 mode-1 and 114 mode-0 matrices; 285 zero rotations
and 57 rotations of 0.6981317 radians. Mode 2 remains covered by the synthetic
cardinal-angle arithmetic test rather than this NPC sample. The comparison
does not establish finished layer sampling or facial geometry.

The GPU material still uses the ordinary character shader. Next: add explicit
layer varyings and texture/sampler bindings, the recovered face composition,
and its alpha test in colour, depth and shadow passes. The raw UV transport
and prepared matrices alone do not change the blank-face appearance.

## Layered GPU face material connected

`render::face_material` wraps the existing character material with six source
textures and separate lash/brow varyings. It composes the recovered native
11187 colour/alpha graph and calls the existing character lighting helper.
It preserves KTX channel selectors and native 2D sampler wrap, XY/mip filters,
LOD clamps and signed bias. The register field decoding follows Decaf's
[`SQ_TEX_SAMPLER_WORD0/1`](https://github.com/decaf-emu/decaf-emu/blob/master/src/libgpu/latte/latte_registers_sq.h).
Unknown wrap/filter modes fail the NPC load rather than silently selecting one.

Forward, depth and shadow shaders share the face alpha expression and cutoff.
The vertex interfaces and skinning/motion paths follow Bevy 0.19.1, with UV2
and UV3 appended to the existing vertex layout. Field names avoid trailing
numbers: naga_oil's WGSL header writer renamed `uv2` to `uv2_`, causing imported
accesses to fail. Shadow/depth variants without `PREPASS_FRAGMENT` return no
fragment outputs while still evaluating the same alpha discard.

Real exports completed for NPC016, NPC014 indoors, NPC024 outdoors and the
full placed set's world view. The frames show the face layer/brow composition without the black bloom
artefact. A tighter NPC016 capture still lacks its nose/mouth: the GPU
material hookup does not establish a finished face. Individual feature placement,
animated transforms, proportions and other races remain unfinished. Deferred
material output is implemented but has not been validated by a separate
GPU capture; the project currently uses the character forward lighting path.

## Next geometry investigation: authored tracks and nose attachment

Read-only inspection of v208's source files found the following FSKA tracks
in `UMii_Hylia_Face_Edit.sbfres`: `Face_{B,M,W}_Jaw_Pattern`,
`Face_{B,M,W}_Mouth_Scale`, `_Mouth_Scale_V`, `_Mouth_Trans_V`, `_Nose_Scale`,
`_Nose_Trans_V`, and `Face_{B,M,W,X}_HeightWeight`. Each Hylian body file also
has its own `<unit>_HeightWeight`. These authored tracks must be composed with
normal animation using the game's rules; changing inverse bind matrices to
match a sampled shape would cancel the deformation and is not a fix.

RPX call sites identified by their format strings/tables:

- `0x034362d8`: the two nose feature tracks from `0x10554204`.
- `0x0343677c`: the three mouth feature tracks from `0x10554214`.
- `0x03436c34`: `Face_%s_Jaw_Pattern`, sampled from the parameter at +0x40c.
- `0x034375ec`: `Face_%s_HeightWeight` / `Face_X_HeightWeight`, using the
  existing height/weight selector `0x0342fba0`.

The separate nose model's two bones, `Nose_Root` and `Nose_D`, do not currently
follow face joints in the baked NPC descriptor. NPC016's 140 nonzero vertex
weight contributions include 58 on the root and 82 on `Nose_D`, so attaching
only the latter would stretch part of the mesh. The native path has separate
setup and per-frame follow operations:

- `0x03437e54` finds `Nose_Root` in the nose model and `Head_Controled` in
  the face, transforms its local matrix using `0x10554274`, and applies
  scale rules. This is not simply a root-to-head world-matrix copy.
- `0x0343ce24` finds face `Nose_Base` and nose `Nose_D`, reads the former's
  matrix through vtable +0x74 and writes it through +0x6c to the latter.

These sites are identified from the original bytes and string references;
their full model attachment order is not yet ported or independently validated.
The initial Capstone dump omitted `CS_MODE_PS`, causing paired-single opcodes
to be decoded as other instruction families. Re-reading with that mode enabled
recovers the paired-single operations described below. No Ghidra project was
modified; no instance was running.

## Native additive nose and mouth settings

The v208 helper `0x03436204` samples an authored bone twice using
`0x03c0b48c` and `0x03c0b608`, then subtracts the reference scale and
translation from the selected pose. The nose and mouth callers add those
differences to the current local pose. Paired-single decoding of
`0x03c700b0` confirms direct XYZ translation addition, without rotating the
difference by the bone orientation; scale is also added, not multiplied.

The tracks and reference frames, in native order, are:

| Track | UMii selection | Reference frame |
| --- | --- | --- |
| Nose_Scale | nose.scale (+0x6d4) | 4 |
| Nose_Trans_V | nose.trans_v (+0x6c4) | 9 |
| Mouth_Scale | mouth.scale (+0x730) | 4 |
| Mouth_Trans_V | mouth.trans_v (+0x720) | 13 |
| Mouth_Scale_V | mouth.aspect (+0x740) | 3 |
| Jaw_Pattern | shape.jaw (+0x40c) | 0 |

The baker now saves these differences in `CharacterDef::joint_adjustments`.
The renderer applies them after body/facial animation, leaving source inverse
bind matrices intact. Missing target bones are skipped, as in the native
callers. Tests cover reference subtraction, additive scale, preservation through
face overrides and repeated frames without accumulation. Older descriptors
remain readable with empty adjustments.

A real Hateno rebake produced the same 58 placements / 57 actors / 249 units;
50 actor descriptors contain nonzero differences. GPU exports of NPC014,
NPC024, NPC016's face detail and the world view validate loading and rendering,
but do not establish finished nose/mouth geometry. Root placement, jaw,
height/weight and other feature settings remain separate work.

Native `0x03439814` multiplies the face root's local matrix by a rotation
constructed from the vector at `0x10554274`. Re-reading the initializer
corrects the earlier X-only interpretation: `0x0343e330` writes X and
`0x0343e358` writes Y from the same `-pi/2` constant; Z is zero. This uses
math-library sine/cosine, rather than the UV transform table. Both temporary
asset-only Face_Root rotation probes deformed the face and were reverted.

More importantly, the tail-call dispatcher `0x0343a784` selects that Face_Root
operation only for UMii mode +0x240 = 1. Ordinary mode 0 takes `0x03438be4`,
whose face part (slot 1) calls `0x03437d10`: it finds **Neck_Root**, left
multiplies its local RT by the same rotation and replaces the translation
with zero before storing the existing scale. A universal Face_Root correction
would therefore apply the wrong native operation to the placed villagers.
Porting normal setup also requires preserving its relation to the subsequent
controlled-bone local copies and model attachments; it is not implemented yet.

The face feature dispatcher `0x03437c7c` applies nose, mouth, jaw and face
height/weight in that order. The jaw helper `0x03436c34` uses the same sampled
scale/translation difference as the nose and mouth, with selected jaw frame
and reference zero (`0x03436fd0`, constant `0x102c59a0`). The baker now includes
that jaw difference for matching face bones. Native forwarding into a separate
beard model, and the remaining height/weight step, are still unfinished.
Release bake tests pass (25 passed, 1 ignored), the rebake preserves 58
placements and fresh NPC016/NPC014/NPC024/world exports complete without GPU
errors. NPC016's jaw outline changes, but nose/mouth placement remains open.

An independent read-only check multiplied the source bind-world matrices by
the FSKL-authored inverse matrices: residuals are at most `8.75e-8` across the
face's 113 smooth matrices and `4.38e-8` across the nose's two. This rules out
a large mismatch between authored inverse binds and the source bind pose in
those two models; it does not prove the assembled runtime skeleton is correct.

The nose follow helper `0x0343ce24` has a per-frame caller at `0x037a6d14`,
after model calculation `0x039859a4`, with model slots 2 (nose) and 1 (face).
That confirms the follow is a runtime operation, not an inverse-bind edit.

## Separate nose model attached

The normal model constructor dispatcher `0x03432520` selects `0x034313bc`
for mode +0x240 = 0. Its attachment names come from `0x10554990`, initialized
at `0x0344723c`: slot 2 is `Nose_U` (`0x102c7608`, written at `0x03447280`).
The constructor resolves that bone through `0x03985f28`, then attaches the
nose unit via `0x039869b4` / `0x039868b0`. Nose attachment uses flag zero;
`0x03985578` reads the parent bone's world matrix and passes it to the unit's
world calculation without stripping scale. This accounts for **all** nose
root influences, not just the vertices influenced by Nose_D.

The baker now records `Nose_Root -> Nose_U` in
`CharacterDef::joint_attachments` and `Nose_D -> Nose_Base` in aliases.
The renderer attaches that root before creating animation clips and joint
entities; inverse binds remain the original source ones. It rejects missing
targets, non-root attachments and parents that violate traversal order.

Root setup follows `0x03437e54`: Euler rotation `(-pi/2,-pi/2,0)`, then
root scale multiplied by Head_Controled's local scale. The extra `0.93`
factor (`0x102c6038`) is selected by **face class W** (0x0342fcc4 == 2),
including adult women and old characters; it is not an old-age-only factor.
All 18 source Hylian nose roots have identity RT, independently checked from
the original FSKL bytes. For these roots, the existing post-animation rotation
offset is equivalent to the native left multiplication; the baker checks that
precondition. This is a formula port, not a claim of bit-identical native
math-library sine/cosine output.

The full test suite passes (242 passed, 8 ignored); release bake/render build
and real rebake retain 58 placements / 57 actors / 249 units. All 57 actors
load in fresh GPU exports. NPC016's close-up now shows the nose, and
NPC014/NPC024/world captures complete without pipeline or load errors.
NPC016's mouth remains missing. The nose's correctness under the still
unfinished full face/body proportions and facial animation is not established
by these static captures.

This investigation also corrects an assumption in the recovered alias code:
`0x0343aa4c` copies **local** matrices, not world matrices, from body to face.
Head_Controled receives RT only (+0x54 setter), preserving its own scale;
Neck_Controled, clavicles and Spine_2_Controled receive local RT and scale
(+0x4c setter). Spine_SkinHelper and Neck_Root have additional rules.
The current shared-joint approximation therefore cannot represent independent
face head scale. Full normal face model attachment and these copies remain
SI-CHR-03 work, alongside height/weight, beard forwarding and other races.

## Mouth isolation and lip material investigation

Read-back of NPC016's baked face confirms all three mouth meshes are present:
`Mouth__Mt_Lip`, `Mouth__Mt_Mouth` and `Tongue__Mt_Mouth`. Their nonzero skin
influences include the authored upper/lower lip and teeth bones, rather than
falling back to an unbound root. Both materials are opaque. The probe below
preceded the dedicated layered lip material.

An asset-only capture with `hidden: ["Face__Mt_Face"]` leaves the mouth/lip
surface visible below the separate nose. With the face restored, that surface
is covered and no mouth line is visible. This establishes that missing baked
geometry is not the explanation. It does not establish whether the wrong
face/lip pose, material composition, or both cause the occlusion. The descriptor
was restored immediately after the probe. An earlier probe using `"Face"`
did not hide anything: hidden names must match the baked shape's full name.

The original `Mt_Lip` assignment has five texture slots:

| Shader slot | Source sampler | Source image |
| --- | --- | --- |
| 0 (`_a0`) | `_a2` | Lip albedo |
| 1 (`_s0`) | `_a0` | Line |
| 3 (`_e0`) | `_ao0` | Lip AO |
| 4 (`_t0`) | `_a1` | Makeup |
| 5 (`_a1`) | `_cm0` | Lip colour mask |

Slots 1 and 4 select texcoord 1; slots 0, 3 and 5 select texcoord 0.
Texcoord 0 has SRT -1, while texcoord 1 selects SRT 2. Both report mapping 1;
the matched original vertex program now proves this means raw **UV1** for
texcoord 0 and **SRT2(UV1)** for texcoord 1, rather than raw UV0.
The common-UV colour bake loses this distinction.

Using the already recovered calc types 2 (product) and 27
(`mix(A*D,B,C)`), the selected `uking_albedo_color = 203` graph is:

```
c200 = mix(makeup.rgb * const_color0.rgb,
           const_color1.rgb, 1 - makeup.a)
c201 = colour_mask.rgb * const_color3.a
c202 = mix(c200 * line.r, const_color3.rgb, c201)
albedo = c202 * lip_albedo.rgb
```

These four colour nodes have clamp disabled in the source assignment.

### Matched native programs and GPU lip composition

All 407 material options select the original `uking_mat` programs 8520..8531
(the archived alpha-test function differs, 6 versus the material's 0).
Program 8523's vertex and pixel bytecode are byte-identical to the readable
Cemu shaders `650b053c168503d0_0000000000000000_vs` and
`2e2ead7088ee9b39_0000079a492a9249_ps`. Their SHA-256 hashes are respectively
`5db7401195ed8cd45eeac2494b3dbbd99c3e76333458437f2003ee5f50516556` and
`4e6f44c4c847529ed250345446e54a66e1fe990ac21d60e8560bd883c66d4379`.
Reflection maps `_u1` to semantic 9. The VS exports that input as raw XY and
SRT2-transformed ZW in `passParameterSem0`. The PS reads makeup and line at ZW,
and lip albedo, colour mask and AO at XY. Its first colour operations implement
the four unclamped nodes above and its opaque alpha is one.

The baker now preserves these five source images, sampler words, swizzles and
native SRT matrices. `FaceComposition::Lip` reuses the layered material with
bindings `[slot0, slot1, slot3, slot3, slot4, slot5]`; the duplicate AO binding
fills the six-layer face interface. The renderer supplies raw UV1 and
SRT2(UV1), and evaluates the native albedo graph per pixel. Original UV0 stays
in the extra vertex attribute. Ordinary character lighting (SI-LGT-17) still
approximates the native lighting, including the UV0/SRT3 toon-specular path;
this change does not claim to recover that path or fix mouth pose/occlusion.

Validation: a fresh release bake of 58 placed villagers and GPU captures of
NPC016, NPC014, NPC024 and the world completed without shader or GPU validation
errors. In the identical NPC016 front-view scene, the mouth line and lips now
appear where the previous common-UV material produced no visible mouth. This
resolves that observed material defect; it does not prove all mouth poses or
facial animation. The release workspace suite passes 242 tests (8 ignored),
including separate UV1/SRT2 lip paths without UV2/UV3 source requirements.

## Body height/weight: native construction and per-frame application

Read-only v208 instruction inspection establishes a different operation from
the additive face feature settings. Constructor `0x03432754` resolves
`%s_HeightWeight` (`0x102c5d1c`) in the body's resource file. Its selected frame
is `getHeightWeightIndex` (`0x0342fba0`, called at `0x03432c04`), and its
reference frame is **1.0** (`0x102c5d10`). Calls at `0x03432e40/50` initialize
and sample the reference; `0x03432e60/70` initialize and sample the selected
frame. For each matched track the 0x38-byte cached record receives:

- **Absolute selected local scale**, written at `0x03432e8c..0x03432ed0`.
- **Selected minus reference local translation**, accumulated at
  `0x03432eec..0x03432f60`.
- A source bone name and runtime binding resolved by `0x0398863c`.

Records without an authored animation channel keep the corresponding source
bind scale and zero translation difference. The native record is not the
current `JointAdjustment` (which adds scale as well as translation).
`Skl_Root` additionally records selected/reference Y translation ratio when
it is finite and outside the +/-0.01 interval; that value has further consumers
and must not be replaced by an invented actor-wide scale.

Runtime `0x0343dcc0` reads the current local matrix with the +0x64 getter,
adds the cached translation, reads the cached **absolute** scale, runs
`0x0343cfa0`, then writes local RT/scale with the +0x4c setter
(`0x0343e020..3c`). The separately handled **Skl_Root** name at `0x102c6418` also
multiplies its Y translation by the supplied factor. `0x0343cfa0` includes
additional model/bone-specific position rules and optional runtime inputs;
these have not yet been fully recovered. This pipeline runs over the animated
pose, leaving authored inverse bind matrices intact.

A source-file probe sampled all nine selector frames of 50 Hylian body units
with a matching height/weight animation. The tracks alter scale and translation;
this is not an unused setting. Animation order matches model bone order in 48
units, but **BodySK_W_000** (57 tracks/61 model bones) and **BodyS_M_010**
(63 tracks/84 model bones) differ. **BodyS_M_001** has no own height/weight
animation; the constructor explicitly recognizes the `BodyS_M_001` family
string (`0x102c5d3c`) in its fallback path. Animation-only files with a
`Skeleton` placeholder are not missing body models. Any implementation must
resolve animation/bind skeleton channels and names, handle the native fallback,
and validate these exceptional units, rather than assuming an index-for-index
match or silently selecting a generic proportional scale.

Next implementation requires a separate body pose operation (absolute scale
plus translation difference), recovery of `0x0343cfa0` and the special-bone
factor's inputs, and independent local body-to-face copies. The existing shared
head alias loses face head scale, so implementing face height/weight by adding
to that shared joint would also deform the body. Body/face height and weight
remain unfinished; no proportions are being guessed from the captures.

### Absolute-scale runtime preparation and angular corrections

`JointAdjustment::scale_override` now represents the native body operation:
set authored local scale, then apply any additive feature difference. The
renderer evaluates it after the body/face animation layers and when spawning a
bind pose, without changing inverse binds. Older descriptors default to no
override and preserve the existing additive feature behavior. No NPC bake
emits this field yet: the full native body pipeline below must be connected.
The animation-layer regression checks absolute body scale and independent
additive face settings over repeated updates.

Further read-only recovery of `0x0343cfa0` resolves the initializer at
`0x0343e894`: table `0x105541dc` contains **Clavicle_**, **Arm_1_**, and
**Leg_1_** prefixes (strings `0x102c64d8/64b4/64bc`). Their branches read
construction parameters with `0x03441ea4`: index 0 for clavicles, 1 for arms,
and 2/3 for legs. Angles multiply degrees-to-radians `0x102c5f90` and the
optional runtime factor returned by `0x0370f248`; matching `_R` reverses signs.
The native helper composes those rotations with the current local matrix.
`Assist` is treated separately in the arm path. Therefore an absolute-scale
plus translation-only bake would still omit authored angular proportions.
Recover the construction parameter table and the runtime factor before wiring
this entire operation into NPC output.

Validation of runtime preparation: the release workspace suite passes 243 tests
(8 ignored). The animation-layer test checks the absolute body override during
repeated ECS updates while the face layer and its additive settings run. No
new GPU capture is claimed for this step: NPC bakes still emit no absolute
scale override, so this is runtime support, not an enabled proportions fix.

### Authored angular table resource and selectors

The angular table is AAMP **Mii/umii.bnetfp** in **Pack/Bootup.pack**,
not UMiiConstructionInfo.byml. Native construction (`0x03440ce4` onward)
registers `Hylia` object parameters using four class names from
`0x105547f8`: `child`, `man`, `woman`, `old`; four part names from
`0x105547d8`: `clavicle`, `arm`, `leg`, `crotch`. Formats for the six
selected frame values are `%s_%s_slim`, `%s_%s_normal`, `%s_%s_fat`,
`%s_%s_tall_slim`, `%s_%s_tall_normal`, `%s_%s_tall_fat`. The short strings
and format strings were read from v208, then all **96 named Hylia entries**
were found as numbers in the source AAMP. `standard` is not this table's
parameter suffix.

The baker now parses this resource into `BodyAngles` while loading Bootup.
Normal Hylian class selection (`0x0342fc14`) maps both children to `child`,
adult men/women separately, and both old age variants to `old`.
`0x03441d14` selects frame 0..5 and returns zero outside those branches;
its indexing must not be clamped to the last valid row. The native constructor
initializes absent table values to zero. The alternate `0x03441ea4` path's
check at UMii +0x250 == 4 concerns **race**, not the Hylian `body.type == SK`;
it is not a reason to switch a Hylian outfit to another race's table.

The optional factor getter `0x0370f248` resolves the supplied bone through
`0x03985f28`, searches 0x34-byte binding records using `0x0371844c`, and
returns the matched record's float at +0x2c. With no match it returns **0.0**
(`0x1030af74`), while `0x0343cfa0` uses **1.0** only when that optional
controller pointer is absent. Therefore unconditionally using one for a
present controller would apply angular correction that the native renderer
might suppress. The controller's record construction/update still needs
recovery before connecting angular proportions to actual NPC poses.

Validation: all 96 expected keys exist in the v208 resource; the release bake
still exports 58 placements / 57 actors / 249 units successfully. The release
workspace suite passes 244 tests (8 ignored), including age/sex selection and
out-of-range frame behavior. NPC pose output is unchanged at this step;
parsing the source table does not complete the body proportion pipeline.

### Bound FSKA pose initialization

Wii U v208 `0x03c0b48c` initializes each pose channel from the FSKA base
when its flag is present, otherwise from the bound FSKL bone: scale +0x14,
rotation +0x20, translation +0x30. With no bone it uses native identity
constants. `0x03c0b608` subsequently samples curves and writes only the
individual target float. The shared formats sampler now exposes
`BoneAnim::sample_with_bind`; callers must supply the correct bound bone and
rotation representation. Existing `sample` callers retain their sparse pose
contract. The change was made in the original format parser first and copied
byte-identically here.

Synthetic verification covers non-unit bind scales, non-zero translations,
authored-base priority and untouched rotation. The real-data probe sampled
frames 0..5 of all 50 matching Hylian body units. Their scale/translation
channels have complete authored bases, so bind initialization does not change
those values; it must not be described as a visible body-proportion fix.
Two tracks lack a same-name bone in their model, in addition to the previously
recorded ordering exceptions. Native FSKA binding and the BodyS_M_001 fallback
remain required before production use. No bake output changes at this step.
The original format checks and copied format crate tests pass (139 tests).

A further constructor trace separates **initialization** from **runtime binding**.
At `0x03432c90..a8`, FSKL +0x14 yields the bone array, indexed by the
loop's 0x40-byte bone stride (`0x0343303c`). The FSKA track array at +0x20
uses that same ordinal with a 0x18-byte stride (`0x03433030`). The native
initializer receives this ordinal FSKL bone at `0x03432e34/58`; it does not
resolve an initialization bone by track name or use FSKA +0x28 here.
Separately, the **track name** is resolved across runtime model parts through
`0x0398863c` at `0x03432de4`; invalid part/bone bindings skip sampling
(`0x03432e00..18`). Thus ordinal bind initialization and named output binding
must remain distinct. The two same-name misses in the probe are a diagnostic
of its model-only lookup, not proof that the game lacks those runtime bones.
The next producer must preserve this distinction rather than reorder tracks
or substitute same-name FSKL initialization.

### Independent face head and local RT copying

Native body-to-face helper `0x0343aa4c` resolves body `Head` and face
`Head_Controled` using strings `0x102c61e0/6210`. At
`0x0343b0e4..100`, the source +0x64 getter returns local RT and separate
scale. The destination call `0x0343b104..11c` uses +0x54, passing only RT;
it deliberately does **not** use the +0x4c RT/scale setter used for the
subsequent neck/clavicle/spine copies. Consequently sharing the head joint
also shared its scale and prevented independent face proportions.

NPC bakes now emit `joint_rt_copies: [("Head_Controled", "Head")]` and
omit that head alias. The renderer retains a separate face joint, copies
source local rotation/translation after animation, then applies destination
feature settings; destination scale and source inverse binds stay intact.
Bind-pose spawning follows the same ordering. Missing, duplicate or forward
copy bindings are rejected at load. Older descriptors default to no copies.
The remaining controlled aliases are still SI-CHR-03: this change does not
port the complete local-copy helper or face height/weight sampling.

A synthetic ECS regression checks independent non-unit head/body scales,
rotation and translation forwarding, additive destination placement, spawn
and three animation updates without accumulation. The release workspace
suite passes 245 tests (8 ignored), and the release bake still exports
58 placements / 57 actors / 249 units. All 65 source face models inspected
have unit head bind scale; the change alone does not introduce authored
height/weight scale, which still needs the native face proportion producer.

GPU validation exported NPC016's front-view detail plus NPC014 indoors,
NPC024 outdoors and the world overview. All 57 actors loaded with the new
copy bindings, with no shader/load errors. The inspected captures preserve
face placement and rendering. They verify integration, not completion of
height/weight, the remaining part-copy rules or native facial animation.

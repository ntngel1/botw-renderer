# Wii U: BFRES materials → uking mat keys

Date: 2026-09-28 Resource and native CPU research; runtime unchanged Context: FIDELITY (archived reference), [native material state](wiiu-render-cpu.md#material-shader-option-overrides), [postfx](wiiu-postfx-materials.md).

## What's established

For sword, Link and Bokoblin materials, foliage and lava, a match was found between all 407 authored static options BFRES and specific `uking_mat` software families. A direct comparison of authored gave **nol** coincidences: the native CPU replaces four state fields after reading the dictionary. The initial state/alpha values are restored from the real RenderState of each material and the native handler; `gsys_pass=0` from the actual RenderInfo `gsys_pass=no_setting` options and the native table. The remaining 403 options are compared accurately, without substituting the values by similar default or searching material.

This is not yet the chosen shader of a particular frame. Callback and further state changes, dynamic pass and skinning selector must be associated with the real draw. The formulas material combiner, emission and fresnel are not yet restored ****; their names and options values below serve as the index of the study.

## Sources and reproduction

The roots are from `renderer.toml`. Resources are Wii U update; local Cemu previously reported EU title `00050000-101c9500`, title version 208. Retail label update is not independently confirmed. Switch decompilation was not used for this matching.

| Resource in update/content | SHA256 of the original file |
|---|---|
| `Pack/TitleBG.pack` → `Model/Link.sbfres` | `469f6162a53e179654f2bf43182ac41b0c45a26c0ab797f85a183d134563d38f` (pack) |
| `Model/Weapon_Sword_001.sbfres` | `7dd64ac3e4b57fa7476fe836225398593c27d0f25290903bad872f28d3f8e0c9` |
| `Model/Enemy_Bokoblin.sbfres` | `009b71666b36c6841e5a3aab61ceb957194381b8e2ba450a33cd377ae6b3148d` |
| `Model/Obj_TreeBroadleaf_A.sbfres` | `96ec721b9f819189ee07ed4e39fb8f723046439d9e9a1716da490ec3fecdbebc` |
| `Model/FldObj_LavaPlane_A-00.sbfres` | `85918199cda07c1e10f6a4e79d7bad6d9e7e66b2be6eaad12e2aa4f2f04f8605` |

Extracted `uking_mat/archive.bfsha`: SHA256 `408db069eb3139b05da9b99c4400f5a87137e2a77ce9858301d36817ccfb3c93`, FSHA 4.5.0.4, one shader model, 14148 programs. Its manifest and native VS/PS code are obtained by the example of `shader_bfsha` from `the original format parser`. All source and extracted data remain outside Git, in `game-data/reference/visual-formulas/`.

`model_info` must be run with `ALL_OPTIONS=1`: the filtered output loses significant strings of `"0"` and `"-1"`. Local results: `material-{sword,link,bokoblin,tree,lava}.txt`.

```sh
R=/absolute/path/to/game-data/reference/visual-formulas
ALL_OPTIONS=1 target/release/examples/model_info /path/to/model.sbfres > "$R/material.txt"
python3 -B tools/research/material_variants.py \
  "$R/shader-archives/uking_mat/manifest.txt" "$R/material.txt" \
  --ignore-option gsys_renderstate --ignore-option gsys_alpha_test_func \
  --ignore-option gsys_alpha_test_enable --ignore-option gsys_pass \
  > "$R/material-variants.json"
```

Probe saves authored options, the full key of each program, dynamic choices, source hashes, code offsets and **all** differences in allowed fields. Without `--ignore-option` , the comparison is strict. It doesn't apply CPU overrides automatically or select the nearest option. Incomplete vocabulary, unknown value and overlapping fields are rejected. Archive identity and shader are checked simultaneously; someone else's archive is unsupported. Synthetic checks: `python3 -B tools/research/material_variants_test.py`.

## Native Overrides and Real RenderState

The CPU chain, function addresses, and RPX identity are described in the [canonical ](wiiu-render-cpu.md#material-shader-option-overrides) entry.

```text
gsys_renderstate       = [3, 0, 1, 2][RenderState.flags & 3]
gsys_alpha_test_func   = RenderState.alpha_control & 7
gsys_alpha_test_enable = (RenderState.alpha_control >> 3) & 1
gsys_pass = index of RenderInfo.gsys_pass in
            [no_setting, seal, xlu_water, reduced_buffer]
```

In all seven table materials below, RenderInfo contains `gsys_pass=no_setting`; native initializer translates it to `0`. The missing RenderInfo also leaves the initial `0`.

Checked big-endian u32 for specific offsets below in unpacked BFRES. RenderState is on signed self-relative pointer FMAT+0x20; flags - RenderState+0, alpha control - RenderState+0x0c. Materials found through FRES model dictionary → FMDL material dictionary, not looking for magic throughout the file. Existing `RenderState` reader shows enable/ref, but does not save off alpha function: so here read raw word.

| Model/material | FMAT | RenderState | flags | alpha_control | Initial shader choices: state / func / enable |
|---|---|---|---|---|---|
| `Weapon_Sword_001 / Mt_Sword_001` | `0x3f4` | `0x514` | `1` | `6` | `0 / 6 / 0` |
| `Link / Mt_Earring` | `0xa6fc` | `0xa81c` | `1` | `6` | `0 / 6 / 0` |
| `Link / Mt_Face` | `0x14a2c` | `0x14b4c` | `1` | `6` | `0 / 6 / 0` |
| `Bokoblin_Red / Mt_Skin` | `0xd6d0` | `0xd7f0` | `1` | `6` | `0 / 6 / 0` |
| `Obj_TreeBroadleaf_A_L / Mt_Treeleaf_00` | `0x36c68` | `0x36d88` | `2` | `0xe` | `1 / 6 / 1` |
| Same model, `Mt_Treeleaf_01` | `0x39508` | `0x39628` | `2` | `0xe` | `1 / 6 / 1` |
| `FldObj_LavaPlane_A_07 / Mt_Lava_C_Slow` | `0x3c8` | `0x4e8` | `0` | `6` | `3 / 6 / 0` |

SHA256 unpacked BFRES for address verification:

| File. | SHA256 |
|---|---|
| `Link.bfres` | `133573d93ce3325c1b19d219daae559dfaeaf87a80d38786fd7c477857897576` |
| `Weapon_Sword_001.bfres` | `bdead7ef8e4ae7e1a4743371de80d887d8365057a7d8ee53edb7a388ef7672e3` |
| `Enemy_Bokoblin.bfres` | `ee418ed11125c859f39208c9c806720b4fad01df4dba61cbe1b0b67e71006c6c` |
| `Obj_TreeBroadleaf_A.bfres` | `4c08ec38e9189c2fe75b4167a6d7e949a9c3b11e474aaad548dabdfa977ab2ec` |
| `FldObj_LavaPlane_A-00.bfres` | `7c22e8101c31bae60e5ce089ce38cade4d0376482235b656e1aa688192f00032` |

For example, `struct.unpack_from('>I', data, 0x36d88 + 12)` returns `14` for the specified tree BFRES. This is a testable base alpha enable `1`, rather than the inference from the word `leaf` or the appearance of the texture.

## Keys and queue of native programs

Each program key consists of 52 u32. The option is encoded by the **index choice**, not the numerical value of its string: `key[word] = (key[word] & ~mask) | (choice_index << shift)`. The CPU writer confirms this operation. 407 static options end in word 50; four dynamic options are in word 51:

| Dynamic option | shift / mask | Choices in order |
|---|---|---|
| `gsys_weight` | `28 / 0xf0000000` | `-1,0,1,2,3,4,5,6,7,8` |
| `gsys_assign_type` | `26 / 0x0c000000` | `gsys_assign_visualize, gsys_assign_material, gsys_assign_zonly, gsys_assign_gbuffer` |
| `gsys_assign_variation` | `24 / 0x03000000` | `0,1,2` |
| `system_id` | `23 / 0x00800000` | `0` |

The following table shows families with **initial Z** state, func, enable and `gsys_pass=0` restored. All 407 static choices after these substitutions match. For a small list of decode targets, `gsys_assign_type=gsys_assign_material`, `gsys_assign_variation=0` are selected separately, and the `gsys_weight` string is equated with BFRES shape skin count. The latter is an explicit condition of the research query; the native setting of this dynamic selector has not yet been verified. The indexes are zero, as in manifest.

| Materials | Family of program indices | skin_count | Decode target | PS code range, end exclusive |
|---|---|---|---|---|
| Sword | `3600..3659` | `0` | `3603` | `0x9ffc00..0xa00e70` |
| Link earring | `84..143` | `1` | `99` | `0x7a3500..0x7a4570` |
| Link face | `10596..10607` | `4` | `10599` | `0x1196100..0x11974a0` |
| Bokoblin skin | `252..311` | `4` | `303` | `0x7dbc00..0x7dcd70` |
| Tree leaves 00/01 | `8052..8111` | `0` | `8055` | `0xf5fb00..0xf60e20` |
| Lava | `11136..11159` | `0` | `11139` | `0x1258400..0x1259c70` |

An example of a retrieved file name is `model000-program11139-ps.code`. SHA256 lava PS: `6667cba5db30cae28810baefd2851ebde50e346c76375a946515c24a1a585f60`. SHA256 sword PS: `becbd464ee7b4500e0654889c4a05a40c934a1ddcd9ea5a5726d208b140a306d`. The full keys and binding tables are in the local manifest and JSON probe; they are not copied to the game's source or runtime.

## What to decode next

The lava `uking_enable_emission=1`, `uking_emission_color=206`; the opaque materials listed have the emission turned off; the sword/earring/face/Bokoblin skin has `uking_enable_fresnel_cheat` on; the leaves and lava are off; these lines set meaningful goals, but do not prove that `206` is the texture, constant, or result of a particular combiner.

1. Remove shader ID / dynamic choices real draw in Cemu for one
   and check out the callback, `gsys_pass` So far, Cemu has not been launched for this test; a family match does not prove an active option.
2. Link `gsys_res_material` reflection, its per-program block location
   And the Latte KCACHE operand to the actual field of BFRES. Without this chain, `const_colorN` and `uking_emission_color` cannot be substituted by name.
3. Decode lava `11139` for emission. Compare output isolation
   With `gsys_assign_gbuffer`, program `11145`, PS `0x125ed00..0x125f320` (1,568 bytes vs. 6256). Sword has a similar small variant of `3609`, PS `0xa01b00..0xa01eb0` (944 bytes). The shorter version does not guarantee that the emission is written there.

Existing bounded `postfx_probe.py` does not decode the control flow and cache modes of these material shaders; its limitations were not removed for the sake of a plausible formula. The next step is a small extension with explicit support for the right instructions and validation of operands, or an exact match with the Cemu-generated shader.

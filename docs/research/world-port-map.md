# Port map: objects, models, far trees, grass, water

Survey of `the original renderer` (2026-10-02, static reading + read-only parsing of
the Wii U dump) for stage 1 steps 2–4. Paths: `hf` = the original format parser (our
`botw-formats`), modules = the original renderer source tree.

## Decisions

- **DLC static units — bake the DLC's (user, 2026-10-02).** The viewer
  reads `<cell>_Static.smubin` only from `Pack/TitleBG.pack`; the DLC has
  loose statics that differ (H-6: 1244 vs 1176 objects, I-7: 1593 vs
  1580). Newest root wins (base → update → DLC); comparisons with the
  viewer must allow for the difference.
- **Derived textures — in bake (user, 2026-10-02).** See below; `assets/`
  holds both the game's textures and the derived ones.
- **Texture swizzle.** `component_select` is ignored by every viewer path
  (BC4/BC5/R8 go to the GPU unswizzled). Our KTX2 keeps it (`KTXswizzle`);
  applying it is a fidelity question to settle per texture use.
- **Derived textures** (ours in the viewer, SI-FMT-10, SI-MAT-03/04): BC1
  normal → BC5, BC1 blue → BC4 gloss, albedo + `_ms0` → RGBA8,
  metal-roughness and emission RGBA8, rebuilt BC1 mips when Tex2 is
  missing, odd BC sizes rounded to 4. Done once in bake with the viewer's
  code; their SI marks move into bake.

## Objects (`objects.rs`)

- Map units: `MainFieldUnits::cell` → `PlacedActor {name, hash_id, translate, rotate (radians; scalar = Y), scale, params}`. Cells 1000 m:
  columns A–J from x = −5000, rows 1–8 from z = −4000 (`map::cell_name`).
  `placement()`: `Quat::from_euler(ZYX, z, y, x)`.
- `skipped()`: prefixes `Enemy_ Npc_ Animal_ Weapon_ Player Horse_`, suffix
  `_Far` (SI-WLD-06). Not read: links, `!Parameters`, lazy traverse.
- Distances (ours): spawn 220 m, despawn 240 m, far 700 m, `_Far` horizon
  3500 m (SI-WLD-03); LOD 45/110 m × clamp(size/5, 1..8) (SI-WLD-05); edge
  fade over the last 12 % (SI-WLD-04); trees hand off to billboards
  (`FarTrees::hand_off`, dissolve from 0.8 of it).
- `_Far` stand-ins: coarsest LOD only, `VisibilityRange::abrupt(700, 3500)`.
- FPS: `stream_objects` clones every unspawned actor within 700 m each
  frame and calls `library.actor()` (allocates); `stream_far_models` walks
  all 14 316 `_Far` actors every frame; cells never evicted; LOD levels
  beyond the 3 used still spawn. Needs a spatial index (initial plan).
- Gameplay stubs: `player::Player`, `enemy::Enemy` (`solidify_objects`),
  avian colliders — not used by the renderer.

## Models (`models.rs`)

- Actor → `ModelRef {folder, units}` from the actor pack's first
  `Actor/ModelList/*` (`ActorPacks::models`). Files: `Model/<folder>.sbfres`
  or split `-NN` parts; textures `.Tex1`/`.Tex1.1` (base) + `.Tex2` (update)
  via `assemble_texture(tex1, tex2)`. Whole folders decode per request —
  bake one GLB per unit.
- Per shape: `_p0`, `_n0` (default +Y), `_u0`, `_t0` (generated if
  missing, w = sign), `_u1`/`_i0`/`_w0` only for skinned characters; `_c0`
  unused. Rigid shapes take `bones[shape.bone]`, single-bone skins
  `matrix_to_bone`: bind pose baked, placed objects need no skinning.
  Triangle lists only. LODs = `shape.lods[1..]` index lists (compacted).
  Also `radius` (max vertex length), collision and sky-occluder trimeshes.
- Per material: mode 2 → Mask(alpha_test or 0.5), 3 → Blend, else Mask if
  alpha_test else Opaque; `cull_back` → double-sided (SI-MAT-01);
  `tex_srt0` → uv transform. Albedo `_a0`/`a0` (+ `_ms0` alpha merge),
  else `albedo_constant()` / `MALICE_ALBEDO`. Rocks: sampler `tma` → layer
  `texture_array_index0` of the terrain albedo array. Normals `_n0`; gloss
  map when `uking_grossy_color == "402"` and `_s0` = `_n0`. Translucency
  slot 2 (leaves). Flags from render_info: `gsys_dynamic_depth_shadow`,
  `gsys_cube_map`, `uking_edit_sky_occlusion`. `MaterialLook` from shader
  options (`uking_material_behave` + `const_color/valueN`,
  `uking_normalmap_blend_ratio`, crown `const_vector0`, fresnel cheat,
  `uking_grossy_intensity`, `uking_chara_size`, transmission → `leaf_light`).
  Samplers always repeat/linear/aniso 8 (SI-MAT-01).
- Objects use `ObjectMaterial` twins of a `CharacterMaterial` base; a
  world-only port can build `ObjectMaterial` directly.

## Far trees (`far_trees.rs`)

- `ActorInfo.product.sbyml` (species `mainModel`, aabb, `traverseDist`,
  has `_Far`), `Terrain.sbfres` `TeraTree` `Tree0/1` alpha test, atlases
  `Tree0Alb`/`Tree0NrmTrs` (128×128 ×100, BC3 sRGB / BC1),
  `Tree1Alb`/`Tree1NrmTrs` (64×128 ×49) with Tex2 mips and `file` user data
  (views), `TreeDitherMask` (32×32 BC4, all levels), per-cell
  `<cell>_TeraTree.sblwp` (`prod::parse`: species, translate, rotate deg,
  scale; newest copy is the DLC's). Hand-off = traverseDist clamped
  150..700 m (SI-TRE-01).

## Grass (`grass*.rs`)

- Samples from the baked tiles (`grass_at`, `material_at`). Still from the
  game: `GrassAlb` layer 0 (32×128 BC1 sRGB, mips rebuilt), `TeraGrass`
  `Blade1/2` `uking_grass_lod_color[3]`, `Blade1`/`Cross1` swell params,
  `GrassCrossAlb` (256×128 BC3 ×2, SI-GRS-02 mips), `Game/Stats/archive/ *.sstats` (500 m quarters: `terrain_embedded_edge`, `terrain_is_in_door`,
  `terrain_hidden`; loaded synchronously on the main thread today), the
  material mean colours (already in `materials.ron`).
- Gameplay hooks to drop: player press/mow, combat.
- FPS: blade and card materials `get_mut` every frame (initial plan).

## Water (`water_material.rs`)

- `WaterAlb` table (Tex2, 7×1×8 RGBA16F), `TeraWater` material params
  (`tex_srt0..5`, `indirect_scale2/4`, `const_color2/3/5`,
  `const_vector0/1`, `const_value2/3/6`), `WaterNrm`/`WaterEmm` (Tex1 L0, 8
  layers → RG8/R8 arrays with rebuilt mips). Mesh data already in tiles
  (`mesh.rs` builds the patches). Sea horizon ring (SI-WAT-06).

## Hateno region (2×2 km around x 3592.7, z 2121.9)

- Cells H–J × 6–8 (9 cells): 18 718 placed actors (16 123 pass the filter,
  1 380 `_Far`). Inside the box: 6 325 model instances of 484 names, 170
  BFRES folders (~71 MB sbfres + ~207 MB textures). Top: TreeBroadleaf_A
  599, CypressLow_A 587, Village_Hateno_A 526, Mineral_A 350, HopBush_A 281.
- `_Far` within 3.5 km: 4 581 (405 distinct). TeraTree billboards in the 9
  cells: 2 410.
- Mountain cloud caps (`effects/`): placements of `MountainCloud*` +
  ELink/PTCL data (stage 2, particles).

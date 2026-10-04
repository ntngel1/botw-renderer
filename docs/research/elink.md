# ELink (xlink2) evaluation: the baked subset and how it plays

Companion to [weather.md](weather.md) §2–§5. Sources: the decomp's
`lib/xlink2` (Switch, structures), the Wii U `U-King.rpx` v208 in Ghidra
(behaviour), `ELink2/ELink2DB.sbelnk` v208 (data). Code:
`crates/bake/src/elink.rs` (reader), `crates/asset-format/src/elink.rs`
(format), `crates/render/src/effects/elink.rs` (evaluator).

## 1. Resource layout as read (Wii U, big-endian)

The Switch structures hold on Wii U with these offsets (checked against the
data: every container, condition and trigger of the baked users decodes to
names and values that make sense, e.g. weather.md §3):

| structure | size | fields used |
|---|---|---|
| `ResAssetCallTable` | 0x20 | `+0` key name, `+4` asset id, `+6` flag (bit 0 container), `+8` duration, `+0xc` parent, `+0x18` param pos (asset: into the asset param table; container: into the user's container table after the call table, −1 none), `+0x1c` condition pos (−1 none) |
| `ResContainerParam` | 0xc | type (0 switch, 1 random, 2 random2, 3 blend, 4 sequence), children start, end (inclusive) |
| `ResSwitchContainerParam` | 0x18 | + `+0xc` watched property name, `+0x14` s16 local index, `+0x16` u8 is global |
| `ResSwitchCondition` | 0x14 | `+0` parent container type 0, `+4` property type (0 enum, 1 s32, 2 f32), `+8` compare, `+0xc` value (enum: name offset), `+0x10` s16 local enum index, `+0x13` is global |
| `ResRandomCondition` | 0x8 | type 1/2, weight |
| `ResProperty` | 0x10 | name, is global, trigger start, end (inclusive) |
| `ResPropertyTrigger` | 0x14 | `+4` call pos (÷0x20 = entry), `+8` condition pos |
| `ResAlwaysTrigger` | 0x10 | `+4` call pos |
| `ResCurveCallTable` | 0x14 | first point, count, curve type, is global (u16 each), property name |

Trigger tables start at user `+triggerTablePos`: action slots (8), actions
(0xc), action triggers (0x18), properties (0x10), property triggers (0x14),
always triggers (0x10).

## 2. Behaviour (Wii U, fact)

- **Compare** (`0x03b9a954` int, `0x03b9a8b8` float): `property ⋄ value`
  with 0 `==`, 1 `>`, 2 `≥`, 3 `<`, 4 `≤`, 5 `≠` (decomp enum reversed).
  The property definition's type picks the int or float compare; enum
  entries compare as indices (a condition's enum name is resolved to the
  property's entry, else −1).
- **Switch** (`0x03b9a3b0`, calc `0x03b9a6b0`): the first child, in table
  order, whose condition holds; a child **without a condition always
  matches**. While watched it re-picks every frame: a different pick fades
  the old child (`fadeBySystem`) and starts the new one; a child that
  ended is started again while it still matches; the switch itself never
  ends.
- **Blend**: every child; ends when all have. **Random**: one child by the
  running weight (`r·Σw < Σ…`) when it starts. **Sequence**: children one
  after another.
- **Property trigger** (`0x03ba4250`): condition true → play (again when
  its event has ended); false → fade. A trigger without a condition never
  plays. **Always trigger**: plays, and again when its event ended.
- **Matrix** (`XLINK_AssetExecutorELinkCalcMtx` `0x03b824dc`): see the
  table in `render::effects::elink::set_matrix` (refines weather.md §3.3:
  result = base × SRT, SRT = (user scale ×) `Scale`, rotation X→Y→Z
  (`0x0243f28c`, R = Rz·Ry·Rx), `Position`; modes 5/6 rotate the position
  by the source's 3×3 only, `0x03c6fe08`). Scale and offset are therefore
  in the set's matrix; the evaluator leaves `EffectSet::scale`/`offset` at
  1/0.
- Curve type 0: linear, clamped at the end points (weather.md §2.3). All
  curves of the baked users are type 0 (the bake reports any other).
- Global enums (`EFFECT_CreateELinkGlobalPropertyDefs` `0x03954fb0`):
  `天候` = the weather names; `地方` = the 20 climates in `ClimateDefines`
  order (ハイラル平原, 北ハイラル平原, ヘブラ氷雪, タバンタ乾燥,
  ラネール山氷雪, ゲルド砂漠 Lv1, ゲルド高原乾燥, オルディン気候 Lv0,
  タムール平原, ゾーラ温帯, ハテール平原, フィローネ亜熱帯, 南ハテール温湿,
  オルディン気候 Lv1, Lv2, Lv3, まよいの森, ゲルド高地氷雪, コログの森,
  ゲルド砂漠 Lv2; strings `0x10341fa0…`); `シーンタイプ` = なし,
  オープンワールド, Cダンジョン, GameTset, 四大遺物, ビューワー (11 slots);
  `カメラ位置が屋内` = False, True.

## 3. Baked format (`assets/effects/elink.ron`, `asset_format::elink`)

- Users: `Camera`, `Chemical` (lightning keys, user hash `0x77af8f95`)
  and every user that a map actor names and whose top-level `Always` is a
  container (census of all 80 cells, v208: 25, among them `Rain_Distance`,
  `Thundercloud_Distance`, `Snow_Distance_*`, `SandStorm_Distance*`,
  `Darkness_Distance`, fog/haze locators). Call tables are kept whole and
  in order (indices are the game's); containers keep their child index
  lists and watched property; conditions keep enum values **by name**.
- Assets: the set name, the effect file holding it (the user's own file,
  else `GameResident`, else any baked file — all `Camera`/`Chemical` sets
  are in `GameResident`), `Matrix`, `RotateSource`, and the numeric
  parameters as constant (the table's default when unset), random (min,
  max, spread; drawn when the asset starts) or curve (property, type,
  points).
- `placed`: the effect-only actors playing such a user, with their
  transform; `actor_users`: actors with models in the baked cells that do.
- The files of those users are baked by the `effects` step
  (`bake::elink::EFFECT_FILES`).

Not kept: action triggers, trigger overwrite parameters (none on the baked
triggers), bit flags, arrange groups (`ArrangeAssetName`, demos only),
sound/`val*` parameters, `Clip`, `DirectionalVel`.

## 4. Open

- Who creates the `Camera` user and its source matrix (taken as the main
  camera's transform), who plays `FieldEnvEffect` and the locators'
  `Always`, and who sets `カメラとの距離` (taken as the 3-D distance).
- `RandomContainer2`'s difference from `Random` (not decompiled).
- The `ChemicalMgr` consumer of the lightning requests (which key each
  request bit plays, holding of the warning).
- The duration field `+8` is 1 on almost every entry (−1 on the one asset
  with a `Duration` parameter); its meaning is not used.

# AI framework: `ksys::act::ai` (ActionBase, Action, Ai)

Date: 2026-10-02 Subsystem: ai. Related ADRs: 0005 (upstream reference: `../adr/0005-game-layer.md`), 0006 (upstream reference: `../adr/0006-module-map.md`).

## Question and sources

- What to install: slots vtable basic classes Action and AI on Wii U,
  layout of their fields and non-virtual logic of execution (entry, output, state transfer, change of daughter node, frame) is the basis of the framework of `original_game_runtime::ksys::act::ai`.
- Ghidra: `update/code/U-King.rpx` Wii U v208 (artifact `wiiu-rpx`)
  Ghidra 12.1.4, PowerPC Espresso, `botw-wiiu-analysis` base - read only (decompilation to the addresses below).
- Classes and methods vtable: [ classes AI](wiiu-aidef.md), local complete
  `artifacts/symbols/wiiu-aidef-ba58da5b.json` file.
- Decompile Switch 1.5.0 zeldaret/botw `5b26254b`:
  `src/KingSystem/ActorSystem/actAi{ActionBase,Action,Ai}.{h,cpp}` is a method name and ad order only; behavior is taken from Wii U code.

## Slots vtable (code Wii U)

Numbering as in [ classes AI](wiiu-aidef.md#from-factory-to-vtable-code): slot i - function by `vptr + 8·i + 4`, call slot i in code - `lwz r12,0xc(this)` and offset `8·i + 4` (`0x64` - slot 12, `0xd4` - 26). The order of slots coincides with the order of declaring virtual methods in Switch headers at one destructor record; checked by the bodies of functions `PlayerLand` (vtable `0x101d2f70`) and `PlayerNormal` (`0x101e115c`).

| Slot | Method (Switch name) | Proof of Wii U |
|---:|---|---|
| 1 | `checkDerivedRuntimeTypeInfo` | `0x02ca8fcc`: Comparison with a chain of five type descriptors |
| 2 | — | `0x0420cf58` stub on all 3,439 classes; Switch `getRuntimeTypeInfo` |
| 3 | destroyer | `0x02ca910c`: `ParamPack` destructor over `this+4`, release at bit 1 |
| 4 | `isFailed` | `0x02ca8f2c`: 1 byte bit of `+0xb` |
| 5 | `isFinished` | `0x02ca8f38`: bit 0 byte `+0xb` |
| 6 | `isChangeable` | Ai `0x037b7e30`: Slot 6 current descendant, otherwise 2 bytes of `+0xb` |
| 7, 8 | `hasPreDeleteCb`, `hasUpdateForPreDeleteCb` | `0x02ca8f44`, `0x02ca8f4c`: 0 |
| 9 | — | `0x0420cf58` plug; 17 `{0, 0}` classes; Switch `m9` |
| 10, 11 | `oneShot_`, `init_` | `0x02ca8f54`, `0x02ca8f5c`: 1 |
| 12 | `enter_` | `PlayerLand::enter_` `0x02ca83c8` ( landing  (upstream reference: `wiiu-player-land.md`)) |
| 13 | `reenter_` | General `0x030ea334`: `x = 0` - slot 12 with `nullptr`, bit `0x40` |
| 14, 15 | `leave_`, `loadParams_` | [ Classes AI](wiiu-aidef.md#restrictions) |
| 16, 17 | `handleMessage_`, `handleAck_` | `0x02ca8f68`, `0x02ca8f70`: 0 |
| 18, 19 | `updateForPreDelete`, `onPreDelete` | `0x02ca8f78`:1; `0x02ca8f80`: empty |
| 20 | `calc` | Action - common `0x0379489c` (slot 31); Ai - `0x037b89c4` |
| 21 | `getCurrentName` | common `0x030eaddc` |
| 22 | `changeChildLater` | Action `0x02ca8f84`: 0; Ai – General `0x037b8924` |
| 23 | `getParams` | Action – General `0x030eaf1c`; Ai – `0x037b8a1c` |
| 24 | `getNumChildren` | Action `0x02ca8f8c`: 0 |
| 25 | `initChildren` | Action `0x02ca8f94`:1; Ai – General `0x037b8d18` |
| 26 | `getCurrentChild` | Action `0x02ca8f9c`: 0; Ai – General `0x037b8db4` |
| 27 | `getType` | Action `0x02ca8fc4`: 1; AI – 0 (`enter` `0x030ea83c` compares to 1 and 0) |
| 28 | `reenter` | Action `0x02ca8fa4`: Slot 13 with `x = 0`; Ai – General `0x037b8de8` |
| 29 | `postLeave` | Action `0x02ca8fb8`: empty; Ai `0x02d218d0`: `changeChildIdx(0xffff)` |
| 30 | `getChild` | Action `0x02ca8fbc`: 0; Ai `0x02d218a8`: Descendant buffer element |
| 31 | Action: `calc_`; Ai: `getNames` | `PlayerLand::calc_` `0x02ca8898`; Ai – General `0x037b8544` |
| 32 | Ai: `calc_` | `Ai::calc` calls `+0x104` |
| 33 | Ai: `handlePendingChildChange_` | `PlayerNormal` `0x02d21894`: `changeChild(+0x14, nullptr)` |

1771 Action in slots 20, 21, 23 has one function for all, in slot 13 – for all.
1762. 1172 AI has one feature in slots 22, 23, 26 for all, 25 – except for the first time.
The trivial embedded base methods (slots 4-11, 16-19, 22, 24, 26, 27, 29, 30) each class has a copy: 1,759 different addresses per slot from Action. The Wii U compiler doesn't merge those copies; hence most of the 49,600 functions up to 16 bytes in the AI class region. It's one base behavior, not 1,759 different ones.

Eight AI classes redefine `calc` (slot 20) with one `0x0315a65c` function: `Fork2AI`...`Fork6AI`, `Fork2AIUpperLowerBody`, `ForkBeastGanonRoot`, `AirOctaRoot` – their common base without definition in AIDef (vtable `0x102880ec`, 0x20 bytes, `0x0315a034`): slot 32, then `calc` of each descendant, then a delayed shift; `enter_` `0x0315a3b0` enters all descendants. `calc` has its own `RootAi` ([ root of the tree ](#tree-root-rootai-and-playerroot-code-2026-10-02)).

## Fields (code Wii U)

`ActionBase`, 0x10 bytes to heir fields:

| Displacement | Field. | Evidence |
|---|---|---|
| +0x0 | actor | `*this` in `getName` `0x030ea5e4`, `setRootAiFlag` `0x030e9a40` |
| +0x4 | `ParamPack` | The `0x02ca910c` destructor is called `0x030eb73c(this+4, 2)` |
| +0x8 | definition index, s16 | `takeOver` `0x030eab20`, `getName` |
| +0xa | root index, s8 | `takeOver` |
| +0xb | flags, u8 | 0x01 Finished, 0x02 Failed, 0x04 Changeable (`enter` resets `& 0xf8`), 0x10 DynamicParamChild (`changeChild`), 0x40 (`reenter_`) |
| +0xc | vptr | [ Classes AI](wiiu-aidef.md#from-factory-to-vtable-code) |

`Ai` adds: +0x10 current descendant, +0x12 previous, +0x14 deferred, +0x16 "new" (all u16, 0xffff - no); +0x18 number of descendants, +0x1c pointer per array (`getCurrentChild` `0x037b8db4`, `changeChildIdx` `0x037b7cd4`, `changeChild` `0x037b7e9c`). Access to the array by index abroad returns element 0 (`sead::Buffer::operator[]`, the same pattern in all these functions). Flags 0x08, 0x20, 0x80 are known only from Switch.

Constructors: `ActionBase` `0x030e9980` takes the actor from the `{actor, s32 definition, s32 root-index}` argument, trims the indices to s16 and s8, constructs `ParamPack` (`0x030eb700`), flags - 0; `Ai` `0x037b7cf0` calls him, puts all four indices of descendants in 0xffff and an empty buffer.

Root Flags: `actor+0x390` – RootAi, `setRootAiFlag(n)` `0x030e9a40` makes `u16 [root+0xd0] |= 1 << n`; `changeChild` and `takeOver` put n = 8.

## Logic (code Wii U)

- `enter(params, context)` `0x030ea83c`: if type 1 or (type 0 and descendants)
  0) — `0x037afd00(actor, getName(), context)` (Switch `Actor::onAiEnter`); `& 0xf8` flags; parameters in `ParamPack` (`0x030ea68c`); input behavior (`0x030ea79c`); slot 12.
- `leave()` `0x030eaaac`: first `leave` of the current descendant, then behavior
  at output (`0x030eaa0c`), slot 14, slot 29.
- `takeOver(src, context)` `0x030eab20`: with equal indices of definition and
  root - `onAiEnter` for type 1, flags are copied from `src`, parameters through a time packet (32 entries by 0x34 bytes, `0x0420cbd8`), `leave` current descendant, entry behavior, slot 28 (result `& 1`); in both outcomes `setRootAiFlag(8)`, with unequal indexes result 0.
- `setFinished` `0x030ea2cc`: `& ~0x02 | 0x01` flags.
- `Action::calc` `0x0379489c`: slot 31.
- `Ai::calc` `0x037b89c4`: slot 32; `calc` current descendant (`0x037b88e0`)
  If there is a delayed shift (`0x037b84ec`: slot 33, return 1) – `calc` is a new descendant in the same frame.
- There is a delayed shift (`0x037b7cac`): delayed  ⁇  0xffff and  ⁇  current.
- `Ai::changeChild(idx, params)` `0x037b7e9c`: with a flag 0x10 and
  `params = nullptr` parameters are collected in a time packet (`0x030ea934`). Then: "new" = idx; `leave` of the current descendant; `changeChildIdx(idx)` (previous = current, current = idx, deferred = 0xffff); if the idx element is not empty - its `enter(params, getName())`; `setRootAiFlag(8)`. Call `0x037b7cac` between them without using the result - no effect.
- `Ai::changeChildLater(name)` `0x037b8924`: Index by name; 0xffff
  return 0; equal to the current - return the current; otherwise, deferred = index, return the element.
- `Ai::getChildIdx(name)` `0x037b81a4`: Binary search by array of descendants
  Compared to `0x037b8098`: an empty element is considered to be the same as a key, otherwise, a byte-byte `getName()` comparison of a descendant with a key (a byte difference sign without a sign) works only if the descendants are ordered by name.
- `Ai::reenter(other, context)` `0x037b8de8`: `other` should be Ai
  (slot 1) with the same number of descendants (slot 24), otherwise 0. Deferred = 0xffff; slot 13 with `x = 0`; if bit 0x40 is raised, `leave` of the current descendant and bit reset; current and previous are copied from `other`; if both have a current descendant, `takeOver` (result `&`).
- `Ai::isChangeable` `0x037b7e30`: Slot 6 of the current descendant, otherwise bit 2.
- Factories Action: `setFactories(count, table)` `0x03795254` (ignored)
  count < 1 and an empty table; `getFactory(name)` `0x03794d0c` — `0x030c5d30` (crc32 starting 0xffffffff: cycle) `0x030c5cc0` table to NUL byte, table `0x030c5bb8` with polynomial 0xedb88320, the result is inverted, binary search by records `{hash, fn}` comparatively `0x03794ce8` The tables are sorted by hash.[AI classes](wiiu-aidef.md#factory-tables-binary-data)), hash equal `zlib.crc32` name. `0x03795274` At Switch. `Actions::clone`) creates a class as a factory, calls `init(heap, 1)` `0x030ea38c`If you refuse, remove it (slot 3).

## Descendants and static parameters (code, 2026-10-02)

Ghidra, Wii U v208, read only (decompilation).

- `initChildren` (slot 25 Ai) `0x037b8d18(this, aidef, heap)`: when
  descendants of class in AIDef `aidef+0x400` <1 - return 1, otherwise the definition index `this+8` (s16) ≥ 0: AIProgram `*(actor+0x39c)+0x74`, definitions of AI - record array 0x68 bytes (`+0x24c` number `+0x250` Array; index abroad – entry 0; entry `+0x54` - number of descendants, `+0x58` - the array of u16 of their indices; `0x037b8af0`.
- `0x037b8af0(this, n, aidef, {count, idx}, heap)`: Continues if
  `count == n` or `0x03396334(actor)`/`0x033965b8(actor)` (not read) and `n` > 0; a pile of `heap` or `0x030aa1ac(DAT_1046c8b0)`; array `n` signposts`this+0x18` number `+0x1c` It's a reset. RootAi is zero. `actor+0x390`; `numAis` = number of definitions of AI (`+0x24c`), +1 if slot 1 `this` scribe `DAT_1031b5b0` (conclusion: "This is RootAi"). Every index `i` < `numAis` → `RootAi+0x48[i]` (AI, number) `+0x44`otherwise `RootAi+0x30[i − numAis]` (Action, `+0x2c`); empty - return 0. Order - as in AIProgram, without sorting (`getChildIdx` (a) is based on the order of data.
- `getStaticParam` `0x030e9be0(this, out, name)`: AIProgram as above
  `0x037751c0(prog, getType())` (slot 27) → `{count, strata}`, `defIdx·0x68` entry, then `0x03774fd4(prog, out, record, name)` – search for `SInst` by name (not read) So read `loadParams_`: `PlayerNormal` `0x02d204ac` (21 `+0x48…+0x98`).

## Building a tree (code, 2026-10-02)

Ghidra, Wii U v208, read only; names by decompiling Switch (`actAiRoot.cpp`, `actAiAi.cpp`, `actAiAction.cpp`, `actAiClassDef.h`, `resResourceAIProgram.{h,cpp}`), behavior by Wii U code.

- `RootAi::init` `0x0315b394`: Looking for a definition of `+0x28`
  `"Root"` (`0x1046cd24`) first among AI (index i), then among Action ( `numAis + 1 + j` index; no one found return 0); then in order `Actions::init` `0x03794e14` (`RootAi+0x2c`), `Ais::init` `0x037b95dc` (`+0x44`), Behaviors `0x03100180`, Queries `0x0314c74c`, and at the end of `0x037b8af0(RootAi, 2, {"DemoRootAI", "Root"}, {2, [numAis, index Root]})` - two descendants of RootAi: the extra `Ais[numAis]` object and the "Root".
- `Actions::init` / `Ais::init`: array by number of definitions (AI +1,
  object `def_idx` −1, `root_idx` 0 and the default name `0x0315af04`: `"DemoRootAI"`, `"Root"` down `root_idx` < 2, otherwise empty. Passage 1 - creating a class by the name `+0x24` factory`0x037b94d4`/`0x03794d0c`), without the factory, `DummyAi` `0x0385bd84` (0x20 bytes) / `DummyAction` `0x03114278` (0x10); callback count (slots 7, 8). Pass 2 - `ActionBase::init` `0x030ea38c(obj, heap, 0)` Each in the order of indexes; first failure is to return 0. AI pass 3 is `0x037b862c` (Parameters of children in the `DynamicParamChild`Actions are created and initialized before AI, AI — all before the first. `init`therefore `initChildren` He finds any children.
- `ActionBase::init` `0x030ea38c(this, heap, skip)`: Flags from the definition
  (`+0x64`  ⁇  0 - `TriggerAction`, bit 0x08 byte `+0xb`; `+0x66`  ⁇  0 - `DynamicParamChild`, bit 0x10); `AIClassDef::getDef(set)` `0x037bb544` by class name (`0x030e9efc`) and type (slot 27); slot 25 `initChildren(set)`; without `skip` - RootAi card and tree parameters (`0x0315b35c`, `0x0315b378`); with the number of dynamic parameters > 0 and without `DynamicParamChild` - `0x030eb974`; then slot 15 `loadParams_` and slot 11 `init_` (its result is the result of `init`).
- `AIDefSet` on Wii U: 256 pointers to children's names (`+0x0…+0x3ff`), number
  `+0x400` children, then parameters. Source: `AIDef_Game.product.sbyml` (Bootup.pack): `childs` class key (Switch `AIClassDef::getDef`). `PlayerNormal` has 89 children, `PlayerRoot` has 2 (`ForDemo`, `Normal`), `DemoRootAI` `childs` has an empty string.
- `0x03396334`/`0x033965b8` is the actor profile of `"Player"` (`0x03393850`).
  Switch, `"Camera"`: `ChildIdx` may not match AIDef.
- Slot 25 `DemoRootAI` `0x03157b68`: children - `DemoAIActionIdx` AIProgram
  (`+0x2fc`) via `0x037b8af0(this, n, 0, …)`; without them, 1.
- AIProgram: `SInst` – `0x03772704`, `ChildIdx` – `0x03772b74`
  (line) `0x103170f8`, `0x1031713c`). `0x03772704` (Switch `parseDefParams`): static parameters of the AIDef class by list type (AI-) `getDef` 0, Action 1, Behavior 2, Query 3 `0x037bb7a8`), number - `min(AIDef, SInst file)`; `n` parameter with AIDef names and types and a value of 0 (line, Tree - `""`Int, UInt, Float, Bool – 0; Vec3 – 0x10549dd0), Type 6 – empty; SInst object (SIST)`0x03ad425c`The file values are given by the name of the general parsing AAMP agl (Switch). `applyResParameterArchive`; on the Wii U not read. Flags `+0x64`/`+0x66` — `TriggerAction`/`DynamicParamChild` AIDef (Action only) `TriggerAction`).
- `getStaticParam<f32>` → `0x03774fd4`: search for the parameter by hash name
  (`0x03774d3c`, crc32 `0x03acfdbc`) among the `+0x2c/+0x30` record; found and type 1 (F32) - pointer to the value, otherwise - on `0x10316c24` (0.0), return 0. Index < 0 or abroad - record 0 (comparison without a sign).
- `getName` `0x030ea5e4`: `+0x28` records at `def_idx` ≥ 0 otherwise
  `0x030e9e6c`.
- `init_` (slot 11) `PlayerNormal` `0x02d21b48`, `PlayerJump`
  `0x02ca27ec`, `PlayerFall` `0x02c8d9a4`, `PlayerLand` `0x02ca8f5c` – `return 1`; `loadParams_` `PlayerLand` `0x02ca8f64` is empty; `PlayerJump` `0x02ca242c` – 10 fields `+0x14…+0x38`, `PlayerFall` `0x02c8d6c4` – 3 fields `+0x14…+0x1c`.

- Read definitions: `0x037751c0` is an AI list (Type 0) or an Action list.
  `getStaticParam` `0x030e9be0` and `Ai::initChildren` `0x037b8d18` - from the section above. `0x030e9e6c`: `root_idx` < 0 is empty, AI. `0x0315af04`, Action — `0x0315e190` (empty). `doGetDef` `0x037ba63c`: `TriggerAction` (AI, Action), `DynamicParamChild` (AI), `CalcTiming` and `NoStop` (Behavior); types named `Type` — `Int` 1, `Float`/`Angle` 2, `Bool` 4, `Vec3`/`Angle3` 3, `String`/`AS` 0, `Tree` 5, `Actor` 8, `MesTransceiverId` 9, `BaseProcHandle` 10, `AITreeVariablePointer` 6, `Rail` 11, other and no record. `Name` or `Type` — 12; `Value` They only read types 2 and 3. `childs` Non-array: node by line displacement, not array and not hash - 0 children (`0x0396ce3c`).
- `DemoRootAI` (factory `0x02ac8728`): slot 11 `0x03157610` - buffer from
  `RootAi+0xb0` pointers (16 with `0xfd5643b5` actor tag, no buffer tag) in `+0x20/+0x24`, `return 1`; slot 15 `0x031587b8` is empty.
- `loadParams_` `PlayerNormal` `0x02d204ac`: 21 `+0x48…+0x98` fields fine
  AIDef.

Data (`Player_Link.baiprog` from `TitleBG.pack` v208 updates; in base - 228 Action): 23 AI, 232 Action; "Root" - AI 2 (`PlayerRoot`, children AI 0 `ForDemo` and AI 1 `Normal`). "Normal" `ChildIdx` - 89 entries in the order of `childs` AIDef (16 AI, 73 Action), `SInst` - 21 AIDef parameters of the same order and type (`ToFallHeightForJustRush` = 2.0). `SInst` names of all player definitions are among the AIDs created, the types are the same; the number of `ChildIdx` of each is equal to the number of `childs`.

Not read: analysis of AAMP agl (recording values `SInst`), map parameters, trees and dynamic (`0x0315b35c`, `0x0315b378`, `0x030eb974`, `0x037b862c`), Behaviors/Queries `init`, callbacks (slots 7, 8).

## Tree root: RootAi and PlayerRoot (code, 2026-10-02)

Ghidra, Wii U v208, read only (decompilation, listing; bytes without functions - reading memory, without disassembling into the database).

- Creation: `0x037a8014(actor, aidef, heap)` allocates 0xdc bytes,
  designer `RootAi` `0x0315acd0({actor, −1, −1})` (Definition and root indexes -1: `getName` - an empty line, `actor+0x390` = RootAi, then `RootAi::init` `0x0315b394`Designer fields: `+0x20` vptr `IRootAi` `0x102883f4`, `+0x24` f32 1.0, `+0x2c` Actions, `+0x44` Ais, `+0x5c` Behaviors, `+0x74` Queries, `+0x8c` behavior `[2][3]` (s) `CalcTiming`), `+0xa4` list `+0xa8` pointer to the list (Switch) `SomeStruct*`), `+0xac` u32 (at Switch) `mI`; only writes `handleMessage_` `0x0315d224`, `enter_`, `leave_`), `+0xb4`/`+0xc0` vec3, `+0xcc` f32 1.0, `+0xd0` flags u16 (`setRootAiFlag`), `+0xd2` flags from the last shot, `+0xd4`/`+0xd8` `ParamPack` maps and wood.
- vtable `RootAi` `0x10288404` (slot i - `vptr + 8·i + 4`): 6
  `isChangeable` `0x0315e188` — 1; 11 `init_` `0x0315d570` - 0. RootAi builds. `init`not `ActionBase::init`); 12 `enter_` `0x0315d584`; 14 `leave_` `0x0315d64c`; 16 `handleMessage_` `0x0315d224`; 20 `calc` `0x0315de5c`; 24 `getNumChildren` `0x0315e090` (`+0x18`); 29 `postLeave` `0x0315e0c0` (`changeChildIdx(0xffff)`); 30 `getChild` `0x0315e098` (buffer element, abroad - 0); 32 `calc_` `0x0315d68c`; 33 `0x0315e084` — `changeChild(delayed, nullptr)`21, 22, 23, 25, 26, 28, 31 are common functions of Ai.
- Entrance: Actor training `0x037afe68` - `enter(RootAi, nullptr, ctx)`
  `0x030ea83c` (`ctx` - `DAT_10549fec` in `.bss`, the value at launch is not read); when you recreate an actor from another - `takeOver` `0x030eab20`, if you fail - the same `enter`. RootAi - AI with two children, so `onAiEnter` is not called.
- `RootAi::enter_` `0x0315d584`: `+0xd0` and `+0xd2` = 0; if `+0xac` ==
  1: `0x03396614(actor)` (actor profile `"NPC"`, string `0x102b96c4`) - yes: `changeChild(1, nullptr)` and bit 6 `+0xd0`; no: `changeChild(0, nullptr)` (`0x0315d578`, `DemoRootAI`); otherwise `changeChild(1, nullptr)` ("Root"). Then at `actor+0x448`  ⁇  0 - `0x0315c3b8` (not read); `+0xac` = 5. `leave_` `0x0315d64c`: `0x0315c100`, `+0xac` 5 → 0.
- `RootAi::calc` `0x0315de5c` is a `0x037a5080` actor.
  @`0x037a5154` (Link has one) `0x037a5db0`, [frame-piece](wiiu-as-frame-ctrl.md#the-order-of-the-actors-frame-2026-10-01)passage `0x037a6ec4` He only calls him when he's slotting. `0x294` = 1, Link - 0, except in case `0x03394870(actor, 0x5a5e004)` and `0x037ffec8(DAT_1047c3d8)` (both unread) Order: `Ai::calc` `0x037b89c4` (slot 32 RootAi, `calc` Delayed shift; with bit 8 `+0xd0` — `0x0315c1c4` (`0x030ff974` for every conduct `+0x8c[2][3]`); `0x0315c100` (reviewing the list) `+0xa4`: `0x030ffaf0`then `0x030ffb44` zeroing `+0xc` elements; `0x0315c018(this, 0, actor+0x3b0` bit `0x200`)` — slot 15 (`+0x3c`) time-behavior 0 (`+0x98[0]`, batless `+0x8c[0]`); upon `actor+0x448` ≠ 0 — `0x0315c638` (unread). Same one. `0x0315c018` timelessly 1 caller `0x037a6ec4' later.
- `RootAi::calc_` `0x0315d68c`: Actor's Name Compared to `"ForestStone"`
  At the beginning, the result is not used (@`0x0315d730`…@`0x0315d774` go to @`0x0315d778` exit immediately if `actor+0x54` ==3 or bit 0 `actor+0x64`At `actor+0x404` ⁇  0 - discharge through `0x0379c1b4(actor, 0x1a, 1)` and `0x030f02d0` (conditions for `actor+0x1f4`,`+0x1f0`byte `+0xb6`Then on the flags. `+0xd0` and the current descendant (slot 26): bit 6 - with a descendant  ⁇  0 `changeChild(0)`; otherwise bit 7 is for the descendant 0 `changeChild(1)`; otherwise, if there is a descendant, `isFinished` or `isFailed` (slots 5, 4) and index not 0 or 1 `changeChild(1)` (RootAi has two children, impossible to reach.) Bit 9 is the jolt of the physical body.`+0xb4`/`+0xc0`, ×30, `actor+0xe8`/`0x134`, `0x0344b794` either `0x0348c8fc`/`0x03489868`then `+0xb4…+0xc8` = 0, `+0xcc` = 1.0. With a descendant  ⁇  0 - `0x03158b58(*(+0xa8), actor)`At the end. `+0xd2`=`+0xd0`, `+0xd0`= 0. -`PlayerRoot` (vtable `0x101e1b74`): 6 `isChangeable` `0x02d2bcb4`— 0; 11`init_` `0x02d2bb30` — 1; 12 `enter_` `0x02d2ba58` - there is a delayed shift (`0x037b7cac`): `changeChild(delayed, nullptr)`otherwise `changeChild("Normal", nullptr)` (`0x037b8284`); 14 `leave_``0x02d2bb38` 15 `loadParams_` `0x02d2bb3c` - empty; 32 `calc_``0x02d2babc` - there's a delayed shift: `changeChild(delayed, nullptr)`; 33 `0x02d2bb5c` The same is true without a check; 20 `Ai::calc`.`ForDemo` Only a delayed shift is selected (`changeChildLater`), which he does not call `PlayerRoot`.
- Bottom line for the player: RootAi `enter_` → Root
  RootAi name, empty. `PlayerRoot::enter_` "Normal" (context) `"Root"`); frame - `RootAi::calc` → `calc_` RootAi → `Ai::calc`«Root» (`PlayerRoot::calc_`then `Ai::calc` Normal is the behavior of RootAi.

Not read: `handleMessage_` `0x0315d224` who bets `+0xac`= 1),`0x0315c3b8`, `0x0315c638`, `0x03158b58` and what `actor+0x448`,`+0x404`, `+0x54`, `+0x64`who puts bits 6, 7, 9 `+0xd0`; behavior (`0x030ff974`, `0x030ffaf0`, slot 15 Behavior.

## Reproduction and limitations

Addresses are read in Ghidra by decompilation (`decompile_function`); slots are read by `artifacts/symbols/wiiu-aidef-ba58da5b.json` (`methods[i]` is slot i + 1). Transfer: the original game runtime AI implementation, markup - in wiiu-symbols.toml (upstream reference: `wiiu-symbols.toml`).

Not moved: name of the node (`getName` `0x030ea5e4`(as discussed above); parameters (`ParamPack`, `InlineParamPack`, `0x030ea68c`, `0x030ea934`); behavior at entry and exit (`0x030ea79c`, `0x030eaa0c`→ RootAi`0x0315bf3c`/`0x0315bf84`); `onAiEnter` `0x037afd00`(a) the tree is built (above). `getFactory` AI, Behavior and Query slots; Behaviour (13) and Quary (12) `calc` Fork-II `0x0315a65c`The root of the tree has been moved. `RootAi` — `root.rs` (`RootAi::create`/`enter`/`calc`, `RootAiClass`),`PlayerRoot` — `uking/ai/player_root.rs`Unread - in the section of the root.

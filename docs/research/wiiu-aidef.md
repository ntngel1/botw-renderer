# AI classes from Wii U factory tables

Date: 2026-10-01 Subsystem: Transfer Status Tools (ADR 0002 (upstream reference: `../adr/0002-port-status-symbols.md`),  card  (upstream reference: `../tasks/port-status.md`)) Conclusion: wiiu-aidef.json (upstream reference: `wiiu-aidef.json`) - summary without listings of methods; the complete file is written locally in `artifacts/symbols/`.

## Question and sources

- What to install: All AI, Action, Behavior, Query and AS nodes in
  Binary Wii U - name, factory, constructor, vtable and its methods; compare quantities with Switch decompilation.
- Binary: `update/code/U-King.rpx` Wii U v208 (artifact `wiiu-rpx`)
  read directly (zlib compressed ELF sections). Ghidra (`botw-wiiu-analysis`) - read only, to check the code templates at the addresses below.
- Names: AIDef of the game itself - `update/content/Pack/Bootup.pack`
  `Actor/AIDef/AIDef_Game.product.sbyml` (artifact `wiiu-bootup`).
- Decompilation: zeldaret/botw (Switch 1.5.0) `5b26254b` - only for
  reconciliations: `src/Game/AI/ai{Action,Ai,Query}Factories.cpp`, `data/aidef_vtables.yml`, `src/KingSystem/Resource/Actor/resResourceASResource.cpp`.
- Tool: `tools/research/wiiu_aidef.py`, synthetic RPX test.
  `tools/research/wiiu_aidef_test.py`.

## Factory tables (binary data)

`{u32 crc32(name), u32 create_fn}` hash-sorted arrays in `.rodata`. Each table is logged by code: `lis/addi` addresses of a table next to `li` of its length, then switching to `setFactories`. For example, Action: `0x02aa9da4` `lis r4,0x101a` · `li r3,0x6eb` · `addi r4,r4,0x1bd0` · `b 0x03795254`.

| species | Table | Records. | Registry | vptr in the object | Slots in the database |
|---|---|---:|---|---|---:|
| Action | `0x101a1bd0` | 1771 | `0x02aa9dac` | +0xc | 31 |
| AI | `0x101a5578` | 1172 | `0x02ac7b04` | +0xc | 33 |
| Behavior | `0x101a7e64` | 224 | `0x02adc690` | +0x10 | 13 |
| Query | `0x101aaa38` | 165 | `0x02b083b4` | +0xc | 12 |
| AS | `0x1047d090` | 107 | `0x0392d358` | +0 | 33 |

- AS - not hashes, but records of 16 bytes in `.data`: name pointer, factory
  The `0x0392d32c` static initializer writes the `{0x6b, 0x1047d090}` buffer in `0x10595364`. The `0x1047d0f0` address from the [ notes of AS](wiiu-as-frame-ctrl.md#selectors-the-choice-of-the-branch-2026-09-30) is the `BoneVisibilityAsset` entry, not the beginning of the table.
- Factory AS node 81 general class (for example, `0x0392e144` 22)
  Classes differ only in table writing. 13 classes of Action also have the same `0x02aaa890` factory (`LinkTagCountAction`, `SetTraverseDist`, ...): on the Wii U, it is one C++ class under different names AIDef.

## Names: crc32 from AIDef

AIDef Wii U: `AIs` 1172, `Actions` 1771, `Behaviors` 224, `Querys` 175 names. The hashes of all Action, AI, and Behavior table entries match crc32 names one to one. Query matched 165; 10 names without a factory are marked in AIDef. `SystemQuery: true`: `CheckGetDemoType`, `CheckTreasure`, `IsArrow`, `IsBow`, `IsFaceToFaceWithPlayer`, `IsOnLinkTag`, `IsProtectiveGear`, `IsRunningOnNX`, `IsShield`, `IsWeapon`Their hashes are not in the binary; where they are executed (apparently, the EventFlow system queries) is not traced.

Name confidence is `factory-crc32` (ADR 0002): the name and hash are taken from Wii U data, the class code is taken from the same table entry. AS names are written in the table by the line `proven`. AIDef names match in pairs: there is no one type of collision name crc32.

## From factory to vtable (code)

- Factory (`PlayerLand` `0x02ac02ac`): `li r3,0x14` (size of object)
  `li r5,4`, `bl 0x0308e5a0` (allocation in the heap), then `bl 0x02ca8364` (constructor) with `this` in r3.
- Constructor: `this = 0` emits itself (`bl 0x0308e578`)
  The base builder (`0x02c6430c`), then `lis r0,0x101d` · `addic r0,r0,0x2f70` · `stw r0,0xc(r31)` — vptr sheet class. The compiler takes `addic`, because `addi r0,r0,x` is `li r0,x`.
- Constructors write vptr and nested objects (buffers, delegates) in other
  vptr is searched only by the displacement common to the view: +0xc for Action, AI and Query, +0x10 for Behavior, +0 for AS nodes.
- Vtable: 8-byte `{u32 amendment, u32 function}` recordings, slot i – function
  `vptr + 8·i + 4`, slot 0 - `{0, 0}` ( climbing  (upstream reference: `wiiu-player-climb.md`), `PlayerWallJump`). The table ends on the first record that is not `{0, code-dress}`: the next vtable or the data immediately after it (some classes of the terminator `{0, 0}` is not). `0x0420cf58` (`li r3,0xd` · `b 0x0420cf00`, emergency stop) - a plug purely virtual call; stands, for example, in slots 2 and 9 of the Action base.
- 17 classes slot 9 - `{0, 0}` inside vtable (13 Action like)
  `DungeonRotate*`, `FlyingBalloonObserverTag`, `CallOvserveActorTag` , and 4 AI `*Remains*`. Within a number of base slots, such a record is considered an empty slot (`null` in the full file), then the end of the table. The number of base slots is the most frequent length of strict reading (31, 33, 13, 12, 33); no vtable is shorter than the base of its kind.
- Check to known addresses: `PlayerLand` → `0x101d2f70`, slot 12 –
  `enter_` `0x02ca83c8`, slot 31 - `calc_` `0x02ca8898` ( landing (upstream reference: `wiiu-player-land.md#playerland-code`)); `PlayerWallJump` → `0x101dd078`; `PlayerJump` → `0x101d1ee0` ( endurance  (upstream reference: `wiiu-player-stamina.md`)); `BoolSelector` → `0x103444f0`, `YSpeedSelector` → `0x103461c8`, `NodePosSelector` → `0x10345294`, `PreASSelector` → `0x10345428` ([AS](wiiu-as-frame-ctrl.md)). All 3439 classes are disassembled without failure.

## Verification with Switch 1.5.0

- Many of the hashes of the factories are exactly the same: Action 1771, AI 1172.
  Query 165; Behavior 224 (no table in decompilation on the Switch, `aidef_vtables.yml` name crc32 reconciliation).
- AS: same 107 classes in the same order.
  `GearSelector` on Wii U 54, in decompilation Switch 64. On Wii U, inputs 0...66 are occupied by everyone, and in the Switch table, input 54 is not occupied by anyone and 64 costs twice (still `DungeonClearSelector`) - similar to a transfer error in decompilation, but the Switch binary is not checked.
- Numbers 1927/1261/164 of `data/status_{action,ai,query}.yml` are C++ classes
  Decompilation, together with the basic, with the tables of factories, are incomparable.

## Coverage of Ghidra functions

Classes 82,695: 3354 factories, 3,354 constructors, 75,987 different vtable methods (108,561 slots) have their own and common addresses. In modification 140 (Ghidra 12.1.4) all factories and constructors were the beginnings of functions, and of the methods only 14,180; 61,806 lay outside all functions: autoanalysis does not create a function that only vtable refers to.

2026-10-01, with the permission of the user, the writer of the database created functions using methods: `scripts/port_status.py methods` → `tools/ghidra/CreateFunctions.java` Run result: 61,361 (and 190 in the trial run at the same addresses); 246 were already functions by the time they were processed - these were created by background autoanalysis, apparently as targets of direct calls from new bodies; 9 are tailing goals `b` from newly created methods (created separately, owners' bodies recalculated, outcome) `split`One is not created: `0x0420cf58` (`li r3,0xd; b 0x0420cf00`The stub of a pure virtual call lies in the body of an old function. `0x0420cf00` after `bl`, which Ghidra does not consider irrevocable; existing functions are not shared by the script. `.text`New download (modification 946917): 159,516 functions, U-King code 159,099 (was 96,957); methods without function in classes 0, total `0x0420cf58` It doesn't count.

Page base numbers (portability status card, 2026-10-01) Upload modification 140: 97,374 non-external functions (97,811 in the Ghidra counter along with 437 external ones); 417 of these are RPL import plugs `0xc000…` (`.fimport_*`, GX2/VPAD/coreinit), not included; base - 96,957 functions, 32,892,072 bytes. After run (modification 946917) - 159,099 functions, 34,167,804 bytes; warnings "address in code without function" 49 (was 59); user solution 2026-10-01 (on recommendation): methods vtable creates the database writer, rather than being added to the percentage base separately (therecommendation is not added to the basis of interest).ADR 0002 (upstream reference: `../adr/0002-port-status-symbols.md`)Copy of the project before recording `game-data/botw-wiiu/backups/botw-wiiu-analysis-2026-10-01-pre-vtable`; together with the record, 71 instructions disassembled by the previous session outside functions (5 places, without symbols) are stored.

## Restrictions

- The code is read linearly, without a transition graph: for short factories and
  This is enough for designers (no refusals), for other functions - not.
- The non-zero-corrected vtable record (multiple inheritance) is terminated
  I would like a list; the classes in the tables do not have such.
- Slots are places in the vtable, not methods names.Slots appointment
  We know only where they were taken apart (`enter_` 12, `leave_` 14, `loadParams_` 15, `calc_` 31 at Action). Switch's methods on Wii U prove nothing (DATA-001 (archived reference)).

## Reproduction

```bash
python3 tools/research/wiiu_aidef.py --config <research.local.toml> --botw <checkout zeldaret/botw>
```

`--config` gives the root of `wiiu_update`; both artifacts are sha256 checked against the registry. Writes wiiu-aidef.json (upstream reference: `wiiu-aidef.json`) and `artifacts/symbols/wiiu-aidef-ba58da5b.json` (with method lists, 1.9 MB, not in Git). `scripts/port_status.py build` picks up the full file itself.

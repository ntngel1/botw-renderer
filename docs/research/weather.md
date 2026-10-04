# Weather: precipitation, lightning, wet surfaces (Wii U v208)

Research of 2026-10-03, static: Ghidra on `U-King.rpx` v208, the game dump
(`Pack/Bootup.pack` → `ELink2/ELink2DB.sbelnk`,
`Effect/GameResident.sesetlist`; `Effect/*_Distance.sesetlist`; actor packs;
map units) and the Cemu GLSL dump of the shading programs
(`game-data/reference/visual-formulas/cemu-sessions/20260928T012415Z-cache-replay`).
Nothing was run in an emulator; no frame was compared. **Fact** = read in
code or data (address / file given); **hypothesis** = marked as such.

Addresses are Wii U v208. Offsets of `WeatherMgr` (`WorldMgr.mMgrs[3]`,
`WorldMgr` = `DAT_1047be88`, manager array `+0x464`, count `+0x45c`) are
Wii U offsets (Switch decomp `worldWeatherMgr.h` is 0x10 larger in places).
Ghidra names added this session: `WEATHER_Calc` (`0x03667d20`),
`WEATHER_ChaseValueClampedStep` (`0x0366c048`),
`WEATHER_CalcLightningStrikesAndFlash` (`0x03666714`),
`WEATHER_PickPointAheadOfCamera` (`0x03666338`),
`WEATHER_RaycastGroundHeight` (`0x036665e8`),
`WEATHER_ShiftScheduleAndRollNewDay` (`0x03665ac0`),
`WEATHER_RollNewWeather` (`0x036651c4`), `WEATHER_SaveScheduleToGameData`
(`0x0366531c`), `WEATHER_BuildForecastBands` (`0x0366774c`),
`WORLD_GetClimateWantedWeather` (`0x036723a8`),
`EFFECT_UpdateXLinkGlobalProperties` (`0x0383b398`),
`EFFECT_CreateELinkGlobalPropertyDefs` (`0x03954fb0`),
`EFFECT_SetGlobalPropertyFloat/Int` (`0x0383a31c` / `0x0383a2c8`),
`KSYS_SetRainUniformsAndVariation` (`0x034087bc`),
`XLINK_AssetExecutorELinkCalcMtx` (`0x03b824dc`).

Frame factor `t` below = `*(DAT_1047c258+0xc0)[core]` (1 at 30 fps), as in
the other research. "chase(x → T; r, max, min)" is the game's clamped
exponential step (`0x0366c048`, also inlined many times in `WEATHER_Calc`):

```text
Δ = T − x
if |Δ| ≤ min·t:  x = T
else step = |Δ|·(1 − (1 − r)^t), clamped to [min·t, max·t];  x += sign(Δ)·step
```

## 1. The pipeline in one picture

```text
WeatherMgr (WEATHER_Calc, every frame)
  weather taken +0x18 / previous +0x19 / transition +0x14   (already ported)
  "concentrations" +0x2cc … +0x2ec (chased 0…1, rain 0…2)   ← new here
  wetness rain_ratio +0x2c0, rainfall +0x2c4, +0x2c8         ← new here
  lightning automaton +0x310…, flash L = +0x2f0              ← new here
        │                                  │
        │ EFFECT_UpdateXLinkGlobalProperties (0x0383b398)
        ▼                                  ▼ KSYS_SetRainUniformsAndVariation
xlink2 global properties (ELink + SLink)       uking_dynamic_rain_ratio / _rainfall,
  濃度:雨 = +0x2cc, 濃度:雷 = +0x2d0 …            DeferredMain assign variation 1 (rain)
        │                                          → deferred shading programs 33/113…
        ▼ property triggers of ELink user "Camera"
emitter sets of GameResident: FieldRain01, FieldRain02a/b, FieldRainDepth01,
FieldSnow, FieldSnowHeavy, FieldRain01BlueSky, SkyLightning, EnvSandStorm,
Strong_Wind_Area, FieldHaze_Lv01…07, VolumeMask*  (all relative to the camera)
```

The precipitation is **not** code that spawns emitters: `WeatherMgr` only
produces numbers; the ELink database (data) decides which sets play, where,
and how strongly. A renderer can reproduce it by evaluating the ELink user
"Camera" (§3) on the property values (§2).

## 2. Concentrations and the global properties

### 2.1 Property definitions (fact)

`EFFECT_CreateELinkGlobalPropertyDefs` (`0x03954fb0`, a vtable method of the
definition table at `0x10342170`, 0x2e = 46 properties, count from
`0x0395849c`) builds the xlink2 global property table; property **index =
slot offset / 4**. `EFFECT_UpdateXLinkGlobalProperties` (`0x0383b398`,
called each frame) writes the values through
`EFFECT_SetGlobalPropertyFloat/Int` (`0x0383a31c` / `0x0383a2c8`, which set
the value on both systems `DAT_1047f018` ELink and `DAT_1047f028` SLink via
xlink2 `System::setGlobalPropertyValue` `0x03b9b05c` / `0x03b9afcc`).
Weather-related rows:

| idx | name | type / range (def) | value written (`0x0383b398`) |
|---|---|---|---|
| 2 | 天候 (weather) | enum of weather names (`0x036722bc(i)`) | `WeatherMgr+0x18` (`0x0366ad14`) |
| 3 | 露出 (exposure) | f32 0…1 | `EnvMgr+0x3ce90` (EnvMgr = mMgrs[6]) |
| 4 | 時刻 (hour) | f32 0…24 | `TimeMgr+0x98 / 15` |
| 6 | 風の強さ (wind) | f32 0…30 | 4-sample running mean ×0.25 of the wind speed at the camera (`0x0366d388`, mMgrs[5]) |
| 7 | 気温 (temperature) | f32 | `TempMgr+0x2c` (mMgrs[4]) |
| 8 / 9 | 標高 / 湿度 | f32 | `TempMgr+0x24` / `+0x28` |
| 10 | 地方 (region) | enum | climate id (`0x036723a0` = `WorldMgr+0x5f8`) |
| 14 | 濃度:VFog | f32 0…1 | `EffectMgr(DAT_1047c210)+0x13508` |
| **15** | **濃度:雨 (rain)** | f32, def. 0…1 | **`WeatherMgr+0x2cc`** (0…2) |
| **16** | **濃度:雷 (thunder)** | f32 0…1 | **`WeatherMgr+0x2d0`** |
| 17 | 濃度:胞子 (spores) | f32 | `+0x2d4` |
| 18 | 濃度:火粉 (sparks) | f32 | `+0x2d8` |
| 19 | 濃度:砂嵐 (sandstorm) | f32 | `+0x2e0` |
| 20 | 濃度:霧 (fog) | f32 | `+0x2e4` |
| 21 | 濃度:強風 (strong wind) | f32 | `+0x2e8` |
| 22 | 濃度:灼熱 (scorching) | f32 | `+0x2dc` |
| 32 | カメラ位置が屋内 (camera indoors) | enum False/True | bit 11 of `EffectMgr+0x1353c` |
| 33 | 室内率 (indoor ratio) | f32 | set in `0x03590964` (not traced) |

(The other indices: 0 pause, 1 slow, 5 time band, 11/12 ecosystem area /
sound, 13 scene type `WorldMgr+0x530`, 23–25 fog/sensor, 28 player air
time, 29 nearby material, 30 nearest water type, 31 water distance, 34
effect load, …; names from the strings at `0x10341b18…0x10342144`.)

### 2.2 How the concentrations are chased (fact, `WEATHER_Calc` 0x03667d20)

Rain hold timer `+0x31c`: set to 4 while the taken weather `+0x18` is rain-
like (2 Rain, 8 BlueskyRain, 4 Snow — `0x0366805c…`) and the transition
`+0x14 ≥ 0.65`, or while it still runs; heavy (3, 5, 7) likewise
(`0x03668018…`). All timers count down by 1 a frame, `+0x31c…+0x33c` only
while `0x031cad5c(*(0x1046d3ac))` is true (`0x03669810`, already in the
symbol notes).

```text
濃度:雨  +0x2cc = chase(→ T; 0.1, M, 0.001)          (0x036698e8…0x0366998c)
   T = 1   while +0x31c ≠ 0 and +0x18 ∈ {2 Rain, 8 BlueskyRain, 4 Snow};  M = 0.002
   T = 2   while +0x31c ≠ 0 and +0x18 ∈ {3 HeavyRain, 7 ThunderRain, 5 HeavySnow}; M = 0.004
   T = 0   otherwise;                                                    M = 0.004
濃度:雷  +0x2d0 = chase(→ 1 if timer +0x320 ≠ 0 and (0x031cad5c() or +0x18 = 7) else 0;
                      0.1, M′, 0.001)                 (M′ = the M of this frame’s rain step)
   +0x320 = 4 while +0x18 = 7 (ThunderRain) and +0x14 ≥ 0.3 (0x03667fd8)
濃度:胞子 +0x2d4, 濃度:火粉 +0x2d8, 灼熱 +0x2dc, 砂嵐 +0x2e0, 霧 +0x2e4, 強風 +0x2e8:
   chase(→ 1 while their timer runs, else 0; 0.1, 0.01, 0.01)
   timers: +0x324, (+0x328/+0x32c), +0x330, +0x334, +0x338, +0x33c, armed for 4
   frames by 0x036660cc(mgr, k): k = 0→+0x324, 1→+0x328, 2→+0x334, 3→+0x338,
   4→+0x33c, 5→+0x330, 6→+0x340. Callers: ChangeWeatherTagRoot::calc_
   (0x02538310), 0x02136504, EnvMgr 0x03641140, and WEATHER_Calc itself
   (k = 1 when the tag level 0x03677250 > 0). 火粉 targets 0.33/0.66/1 by
   that level (0x03669a94).
+0x2ec = 1 while +0x340 runs, else 0 (instant).
Set weather (WorldMgr+0x649 ≠ 0xff without +0x64d, or the +0x610 path):
   all rates/steps = 1 → every value jumps to its target (0x03669840).
```

The doubling of `M` is how the code is written: the register holding 0.002
is doubled (`fadds f26,f26,f26` at `0x03669948` / `0x0366995c`) on the
heavy and the "off" branches and stays doubled for the thunder chase that
follows (`0x036699b8`). Consequence at 30 fps: rain fades **in** over ≈
500 frames (≈ 17 s, the same as the weather transition), heavy rain 0 → 2
over ≈ 500 frames, and rain fades **out** 1 → 0 over ≈ 250 frames (≈ 8 s)
— all *after* the hold timer has expired, i.e. once the taken weather is
no longer rainy (the hold is re-armed every frame while it is).

### 2.3 Reading of the values by ELink (fact, xlink2)

Property triggers fire when their condition becomes true and the asset is
released when it becomes false (xlink2 `PropertyTriggerCtrl`). Compare
types in the resource are, by the data (`=` on enums, `>0` on 濃度), in
the order **0 Equal, 1 GreaterThan, 2 GreaterThanOrEqual, 3 LessThan,
4 LessThanOrEqual, 5 NotEqual** — the decomp's `xlink2Types.h` lists them
reversed. Curve values (`Alpha`, `EmissionRate` …) are linear in the
property and clamp at the end points (curve type 0), so a 濃度:雨 of 2
reads the value at 1.

## 3. ELink user "Camera": what plays (data, `ELink2DB.sbelnk` v208)

User "Camera": 379 call-table entries, 12 property triggers, 3 always
triggers, local properties シーン状態, 受ける風の強さ. Triggers (resource
`ResPropertyTrigger`, asset index = ctb offset / 0x20):

| Trigger (global property, condition) | Container/asset |
|---|---|
| 濃度:雨 > 0 | [22] `RainSnow` |
| 天候 = BlueskyRain | [21] `BlueSkyRain` |
| 濃度:雷 > 0 | [23] `ThunderStorm` |
| 風の強さ ≥ 8 | [26] `StrongWind` |
| 濃度:砂嵐 ≥ 0.1 | [36] `WMSandStorm` |
| 濃度:胞子 ≥ 0.1 | [27] `Spore` |
| 濃度:火粉 ≥ 0.1 | [32] `FireArea` |
| 濃度:ＢＭ > 0.1 | [31] `GrudgeEnv_BloodyMoon` |
| シーンタイプ = 四大遺物, 生態系エリア = 79, 最寄水タイプ = 沼/熱湯/氷水, シーン状態 … | grudge / terra-water entries |
| always | [33] `VolumeSwitch_Dust`, [34] `VolumeSwitch_Fog`, [35] `VolumeSwitch_Add` |

`FieldEnvEffect` ([28], the haze) has no trigger in the user: it is played
by key from code (caller not traced).

### 3.1 Rain and snow — the `RainSnow` tree

```text
[22] RainSnow           switch on 気温 (TempMgr+0x2c)
 ├ [133] Snow   (気温 ≤ −2)     switch on 濃度:雨
 │   ├ [135] HeavySnow  (≥ 2)  FieldSnowHeavy  Matrix 4, RotateSource 1, EmissionRate = 濃度:雨 [0→0, 1→1]
 │   └ [136] NormalSnow (> 0)  FieldSnow       Matrix 4, RotateSource 1, EmissionRate = 濃度:雨 [0→0, 1→1]
 └ [134] Rain   (気温 > −2)     switch on 濃度:雨
     ├ [137] HeavyRain  (≥ 2)  blend:
     │     FieldRainDepth01  Matrix 2, RotateSource 1, Alpha = 濃度:雨 [0→0, 1→0.9]
     │     FieldRain02a      Matrix 4, RotateSource 1, EmissionRate = 濃度:雨 [0→0, 1→1]
     │     FieldRain02b      Matrix 4, RotateSource 1, EmissionRate = 濃度:雨 [0→0, 1→1]
     └ [138] NormalRain (> 0)  blend:
           FieldRainDepth01  Matrix 2, RotateSource 1, Alpha = 濃度:雨 [0→0, 1→0.7]
           FieldRain01       Matrix 4, RotateSource 1, EmissionRate = 濃度:雨 [0→0, 1→1]
[21] BlueSkyRain (天候 = BlueskyRain): FieldRain01BlueSky, Matrix 4, RotateSource 1,
     Alpha = 濃度:雨 [0→0, 1→1], EmissionRate = 濃度:雨 [0→0, .32→.08, .62→.28, .84→.6, 1→1]
```

All with `BitFlag` arrange value (type 5, not decoded). A switch container
takes its first matching child, so heavy rain/snow starts only when 濃度:雨
reaches exactly 2 (the chase snaps at the end) and normal rain plays while
it climbs from 1 to 2; on the way down the switch goes back to normal
rain at once. The rain/snow choice uses the **effect** temperature 気温
(TempMgr+0x2c) with −2 °C, independently of `WeatherMgr`'s own rain→snow
switch (`0x03666048`: TempMgr `0x0365cee4` or `+0x50` ≤ −2).

Emitter data (GameResident, `ptcl_info`):

| set | emitters (attributes) | emission | volume | particle |
|---|---|---|---|---|
| FieldRain01 | rain_near (CSDP, EP04, FCSF), rain_far (same) | loop ×10 / ×10 | shape 9, r (15, 7.5, 15) / (200, 200, 200); translate y +4 | life 25 / 21, size 0.04 / 0.12, v = 1.2 / 4 down, camera alpha near 0–6 / 5–15, far 80–100 |
| FieldRain01BlueSky | same two | loop ×5 / ×5 | same | same |
| FieldRain02a | rain_near, rain_far | ×10 / ×14 | same | size 0.05 / … |
| FieldRain02b | rain_middle (CSDP, FCSF) | ×2 | r (15, 3, 15), y +4 | life 20, size 6, v 0.3, soft 5 |
| FieldRainDepth01 | depthRain (CSDP) | once ×1 every 14 frames | point, translate z +0.1 | life 15, billboard 0, size 16 × 10, texture 6d88accf scrolled (0.13, −0.013)/frame |
| FieldSnow | Flake_Far (CSDP, EP04, FCSF, FCLN), Flake_Near (CSDP, EP04, FCSF) | ×3 / ×1.5 | r (200, 200, 200) | life 50, primitive 0cc1dd9e, size 0.8 / 0.25, v 0.2 / 0.08 |

`EP04` (emitter plugin 4) on every falling-precipitation emitter is the
likely mechanism that keeps the drops around the camera (**hypothesis**:
eft "area loop"; its 112-byte bodies are, e.g. rain_near
`[2.3, 1, 6, 2, 13, 5, 13, 0, 0, 2, −11, 0…, 1, …]`, rain_far
`[18, 15, 23, 2, 80, 64, 80, 0, 0, 5, −85, …]`, near/far of 02a
`[10, 30, 40, 4, 60, 48, 60, 0, 0, 5, −85, …]`; layout not decoded —
for the eft researchers).

### 3.2 Thunder, wind, sandstorm, haze, volume masks

- `ThunderStorm` → **SkyLightning**, Matrix 6, EmissionRate = 濃度:雷
  [0→0, .5→.25, 1→1], valDrawPriority 255. Emitters: Flash_Far (shape 8
  r 13000 × 500, y +750, ×1.2 every 0 (+30 random) frames, size 500, life 42),
  Flash_Big / Flash_Big_Blur / Flash_Cloud (shape 4 r 2500 × 200, y +900, one
  every 29 (+30) frames, size 1800 / 900). These are the sky flashes in the
  clouds; they run with 濃度:雷 independently of the strike automaton (§5).
- `StrongWind` (風の強さ ≥ 8) → switch on カメラ位置が屋内 = False →
  **Strong_Wind_Area**, Matrix 6.
- `WMSandStorm` (濃度:砂嵐 ≥ 0.1) → カメラ位置が屋内 = False →
  **EnvSandStorm**, Matrix 2, EmissionRate = 濃度:砂嵐 [0→0, 1→1]
  (emitters Moya / Chip, shape 11 r 300, EP04, FRND, EAER).
- `FieldEnvEffect` (haze, played from code): only when シーンタイプ =
  オープンワールド; by 地方: Gerudo highland/desert Lv1/Lv2 → when 風の強さ
  > 5 `Field_DesertSand_Lv1` (PositionZ −50) + `_Lv2` (−150), Matrix 0;
  Gerudo-highland/Hebra/Lanayru snow regions → `FieldHaze_Lv01…` with
  EmissionRate by 時刻; other regions → switch by **天候** (Bluesky, Cloudy,
  Rain, HeavyRain, Snow, HeavySnow, ThunderRain, BlueskyRain each a blend of
  `FieldHaze_Lv01…Lv07` at PositionZ −30/−250/−500 …, Matrix 0, with
  EmissionScale by 受ける風の強さ and LifeScale by 風の強さ), the nearest of
  them only if not on stone/sand, not the castle (生態系エリア ≠ 79),
  濃度:ＢＭ < 0.1 and **カメラ位置が屋内 = False**. Per-weather
  parameters are in the DB (user "Camera", entries 146–299).
- `VolumeSwitch_Dust/Fog/Add` (always): VolumeMaskDust01 / Fog01 /
  Add01test, Matrix 4, Alpha by 濃度:VFog; the dust one only while 濃度:雨 <
  0.5 (`VolumeSwitch_Dust_ifRain`).

### 3.3 Where they are placed: the ELink `Matrix` parameter (fact)

`XLINK_AssetExecutorELinkCalcMtx` (`0x03b824dc`, uses the accessor strings
`getMtxSetType` `0x103c54e4`, `getRotateSourceType` `0x103c54f4`): the
source matrix is the user's bone/root matrix; with `RotateSource` = 1 the
rotation comes from the user's mtx source `+0x20` and the translation from
the bone. Then by `Matrix`:

| Matrix | result |
|---|---|
| 0 | source matrix × asset SRT, with the user's scale |
| 1 | same, scale per axis |
| 2 | source rotation (normalised, unit scale) + translation; asset offset in local axes |
| 3 / 5 | translation only, world axes (identity rotation), user scale; 5 also rotates the asset offset by the source matrix |
| 4 / 6 | translation only, world axes, unit scale; 6 rotates the offset by the source |

So the falling rain and snow (Matrix 4) sit at the source position with
world-aligned axes; FieldRainDepth01 and EnvSandStorm (Matrix 2) turn with
the source; SkyLightning (6) is world-aligned. **Hypothesis**: the "Camera"
user's matrix is the camera's world matrix (name; haze at negative
PositionZ in front; FieldRainDepth01 a camera-facing sheet). The creation
of the Camera user was not traced.

### 3.4 Under a roof / indoors

- No ELink condition hides rain or snow indoors: the `RainSnow` and
  `BlueSkyRain` trees have no カメラ位置が屋内 / 室内率 switch (they do for
  strong wind, sandstorm, haze, terra water). Fact (data).
- No depth or height-map test was found for the rain particles in the
  emitter data beyond the standard depth test and soft-particle fields;
  the `CSDP` (custom shader) and `EP04` blocks are not decoded, so a
  shader-side occlusion cannot be excluded (open).
- The **wetness** is occluded (§6): by the top-down depth map
  `gsys_depth_shadow_quarter` and the inner mask of `gsys_user2`.

## 4. Distant weather actors (`*_Distance`)

Actors `Rain_Distance`, `Thundercloud_Distance`, `Snow_Distance_Gerudo`,
`Snow_Distance_Lanayru`, `SandStorm_Distance_*` (ActorLink → same-named
ELink user; profile `EffectLocaterFar` / `EffectLocater`, LOD
`NoXlinkSkip`, life `Landmark05km`/`Landmark03km`). Placements in MainField
(static+dynamic units, census of all 80 cells): one each —
Rain_Distance I-4 (3224.7, 164.1, −318.7), Thundercloud_Distance C-4
(−2250, 216, −960), Snow_Distance_Gerudo C-6 (−2620, 550, 1555),
Snow_Distance_Lanayru I-6 (3860, 713, 1310), SandStorm_Distance_daytime B-7,
_Ibutsu A-7 / B-8, _A8 A-8, _Battle C-8; plus Darkness_Distance F-1.

Their users have no triggers; the key `Always` is played by the actor
(hypothesis: by the EffectLocater class). Content (data):

```text
Rain_Distance / Thundercloud_Distance / Snow_Distance_*:
  Always: switch 濃度:雨 → only while 濃度:雨 < 0.5 (the local rain hides it)
    blend: FarWeather_Cloud (or FarWeather_SnowCloud), Matrix 0,
           Scale, PositionY, Alpha as curves of the local property カメラとの距離
           (Rain: Scale 1100→800 over 100→4000 m, Y 450→300 over 400→3000 m,
            Alpha 0 at 240 m → 1 at 400 m)
         + FarWeather_Rain when distance ≤ 4000 (Thundercloud: < 2000, with
           FarWeather_Lightning; cloud colour 0.47/0.44/0.44)
SandStorm_Distance: Alpha = 1 − 濃度:砂嵐, Clip 2, while 濃度:砂嵐 ≤ 0.5;
SandStorm_Distance_A8: within 300 m a ring of 8 SandStorm_Distance_Pizza at 320 m.
```

## 5. Lightning

### 5.1 When (fact, `WEATHER_CalcLightningStrikesAndFlash` 0x03666714)

Enabled (`bVar4`) when the climate's wanted weather (`0x03672890`) is 6 or
7, the transition `+0x14 ≥ 1`, cloudiness `SkyMgr+0x2120 ≥ 0.99` and
`WorldMgr+0x538 > 30`. Parameters from the constructor (`0x03664acc`):
`+0x344 = 10` s, `+0x348 = 8` s, `+0x378 = 100`, `+0x37c = 75`, `+0x36c = 5`.
State `+0x310`, timer `+0x314` (frames, decreases by t):

| state | action |
|---|---|
| 0 idle | on enable: timer 150, bluff countdown `+0x354 = 90`, → 1. Forced (`+0x381`): timer `+0x318`, → 3 |
| 1, 2 wait | each frame with probability 1/100 (`+0x378`), if `+0x354 ≤ 0`: `+0x354 = 90` (a far "bluff" flash). State 1 end: timer = rand(300) → 2. State 2 end: timer = 10·30 = 300; if EnvMgr id (`0x0364be24`) < 1 or > 35 → 3 (strike) else → 1 |
| 3 warning | strike point once: 90 m ahead of the camera along its view direction (xz), ±30 m random in x and z, y = ground raycast from 2000 down to −500 (`0x036665e8`); every frame `ChemicalMgr` request bit 0x10 with progress timer/300 (warning effect); at half time a second request |
| 4 strike | `ChemicalMgr` request bit 0x08 at the point (the bolt), flash trigger, timer 30 → 5 |
| 5 | after 30 frames: timer 90; → 6 if still enabled, else 0 |
| 6 | after 90 frames: timer 8·30 = 240, → 1 |

Bluff flash: while `+0x354 > 0` (90 frames) request bit 0x40 at a point
chosen like the strike point (stored `+0x2b4…`); when it reaches 0, bit 0x20
(far bolt) and the flash trigger. Thunder rumble: while the state ≠ 0, with
probability 1/75 a frame, bit 0x80. The requests go to
`WorldMgr.mMgrs[8]+0x5ac` (`0x0367a1a0`; mMgrs[8] = ChemicalMgr by the
Switch order; flags at `+0xb8`, positions `+0x130…+0x150`, progress
`+0x154`: `0x031a3bd0`, `0x031a3c24`, `0x031a3c74`, `0x031a3c9c`,
`0x031a3cc4`). Who consumes them was not traced.

The effects are chemical ELink keys (in 16 Chemical users, e.g. hash
`0x139ad55a`): `Chemical_LightningSign_OT` → **LightningSign_OT**
(warning), `Chemical_Lightning` = blend of **Chemical_Lightning**, one
random of **Lightning_1…Lightning_6** (bolt; random container) and
**Chemical_Lightning_Ground**, all with valCombo 2, valPower 1.5,
valDampDist 100 (camera shake), and `Far_Lightning` → **Lightning_Far**
(sizes 650 at y +550…560).

### 5.2 The flash `L = +0x2f0` (fact)

Automaton `+0x34c` (`0x03666e20…`), started by a flash trigger only when
the EnvMgr id ≤ 0 or in 36…44: L = 0, timer 30 → 1: chase L → 0.5 (r 0.5,
max 0.5, min 0.1) until the 30-frame timer ends → 2: chase → 1 (r 0.1, max
0.1, min 0.01) → 3: chase → 0 (r 0.1, max 0.12, min 0.005) → 4 → 0. While
`+0x34c ≠ 0`, `EnvMgr 0x0363d0e4(2)` (→ `0x038c1ee8` on `KSys+0x85c`: rate
`+0x190 = 0`; mode 0 = 0.2, 1 = 0.4; object not identified), back to 0
after.

Uses of L:

- **Sky LUT `cFade` (`EnvMgr` sky object `+0x760`)** = lerp(night fade, 1,
  L) in states 1 and ≥ 3, exactly 1 in state 2 (`0x036461e4…0x0364628c`):
  the sky turns isotropic (no sun/Mie shape) during the flash.
- **`uking_dynamic_base_light_change_ratio` = 1 − min(fade + L, 1)**
  (`0x03657c94`, `0x03408770`): the directional light is dimmed by the
  flash.
- `proj_shadow_off`: `x = F·s·q·(1 − L)` (already in the CPU notes,
  `0x03658284`); depth shadows: `0x03408948(f26, f13·(1 − L))`
  (`0x036585e8`) — `depth_shadow_scale` and `world_shadow_off` follow L.
- `TEMPMGR_UpdateMoisture` reads `EnvMgr+0x2f0/+0x2d0` (not WeatherMgr) —
  unrelated.

## 6. Wet surfaces

### 6.1 Rain ratio and rainfall (fact, `WEATHER_Calc`)

```text
rainy (+0x18 ∈ {2, 8, 4}) or heavy ({3, 7, 5}), and (+0x14 ≥ 0.65 or hold):
  targets (ratio, rainfall, +0x2c8) = (0.725, 1.0, 0.8) for wanted 2/8,
                                      (0.8, 1.2, 1.0)  for wanted 3/7,
                                      none (dry branch) otherwise
  only once 濃度:雨 > 0.8:  +0x2c0, +0x2c4: chase(r 0.1, max = min = 0.005)
                            +0x2c8: chase(r 0.1, 0.00125, 0.00125)
dry branch:
  snow (+0x18 4/5):   +0x2c0, +0x2c4 → 0 with (0.1, 0.025, 0.025)
  otherwise:          +0x2c0, +0x2c4 → 0 with (0.1, 0.0005, 0.00005)
  clear (+0x18 = 0):  +0x2c8 → 0 with (0.1, 0.001, 0.001); else (0.1, 0.0005, 0.00005)
WorldMgr+0x53c (stage timer): values jump to their targets.
+0x370 ≠ 0 (countdown 0x03669d7c): rainfall = (float)+0x304.
```

So the ground gets wet in ≈ 145 frames (5 s) after the rain is dense, and
dries **very slowly**: the step falls to 0.00005·t near 0 — about 0.725 /
(0.0005…0.00005) → a few thousand frames (minutes); snow dries at once
(0.025). `+0x2c8`'s reader was not found.

### 6.2 To the shaders (fact, `KSYS_SetRainUniformsAndVariation` 0x034087bc)

```text
uking_dynamic_rain_ratio = (1 − (KSys+0x1f4)->+0x1f8) · rain_ratio     (scene_material[1].w)
uking_dynamic_rainfall   = rainfall                                    (scene_material[2].x)
variation = 1 if the rain_ratio uniform > 0 else 0; 2 if (KSys+0xc4)[0]->+0x3f5c->+0x1c0
0x039e462c(KSys+0xb70, 0, variation): every material of that model gets
  gsys_assign_variation = variation (vtable +0x1c4)
```

(`(KSys+0x1f4)+0x1f8` is not identified — a 0…1 suppression factor.)

The rain uniforms are read **only** by the `uking_sys_shading` programs of
assign variation 1 (scan of all uking_sys/terrain/grass/tree/flower/mat
native programs for kcache reads of scene_material [1].w / [2].x, script
`rainscan.py`): rain_ratio in pre-shading PS **109, 113, 141, 145**
(`preshading_field_leaf`, `preshading_field`, `preshading_field_water`,
`preshading_field_xlu`), rainfall in main PS **25, 29, 33** (`field_water`,
`field_leaf`, `field_hybrid`). No `uking_mat` / terrain G-buffer program
reads them: wetness is a deferred-lighting effect, no per-material input.
Cemu GLSL of the rain variants exists (PS 33 `7f33027db819f935`, PS 113
`7bcb527bda035be4`, PS 25 `07d4d8363600f2d4`, PS 29 `5648500869d5db39`,
PS 109 `b5ea2a9280688ac1`, PS 141 `28e6a2be507943e4`, PS 145
`eb6287332a08ee60`); uniforms mapped with `cemu_uniform_map.py`
(scene_material = constant bank 10).

### 6.3 Wetness in the pre-shading (PS 113, read in GLSL)

PS 112 (dry) writes `Shadow.y = 0`; PS 113 writes the wetness there:

```text
w_raw = e39.w·(1 − DSQ) − Y + 1.3 − flag·(2N_y − 1)     (the same top-down term as vis' u, ×4)
w     = sat(w_raw)                                       (1-m band instead of the 4-m of vis)
n     = T0(X/2, Z/2).x;  k = 4 + 0.03/(z·ctx[17].y)
p     = T0(time·0.125 + 4·(Y + 0.5·T0(k(X + n), k(Z + n)).x), 0; bias 0.5).x
Shadow.y = rain_ratio · (1 − w³) · (1 − user2.y) · (1 − sat(z/120 − 2/3)) · (0.5 + 0.5·p)
```

X, Y, Z world position (env 43–45), z view depth, T0 = `uking_tex0`, time
= `ctx[20].y`, user2 = inner mask (1 inside). So wetness: only under open
sky (top-down depth map, 1 m band), outside, fading out between 80 and
200 m, modulated by a world-space noise that pulses with time (the
`uking_tex0` row lookup; exact texture content not checked).

### 6.4 Wetness in the main pass (PS 33 vs PS 32, read in GLSL)

With `S.y` = wetness from §6.3, `gl` = gloss (normal.w), `flag` = bit 0 of
normal.w·255, `V̂` = normalised view vector:

```text
gl′ = gl + (0.75 − gl)·sat(2·S.y)                                (gloss toward 0.75)
N′  = normalize(N + V̂ · rainfall·sat(2·S.y − 1)·(0.1 + 0.4·flag))  (normal tilted)
```

Everything else as PS 32 (`docs/research/wiiu-field-shading.md`, «PS 32»).
No albedo darkening was seen in PS 33; PS 113's Diffuse/Specular were
not diffed against PS 112 line by line (open). PS 25/29/109/141/145 not
read.

### 6.5 Puddles, ripples, drops on the screen

- No puddle/ripple uniform exists (`uking_dynamic_*` list: only
  rain_ratio and rainfall); `XRainSplashRatio` / `Test_FootEffect_Rain` /
  `Run_Rain` are actor/AI parameters. Water ripples in rain were not found
  in the water shaders' uniforms (no rain input in `uking_terrain_water`
  programs, scan above).
- Screen: the only camera-attached rain layer is FieldRainDepth01 (§3.1).
  No postfx rain-drop pass was found (`agl_technique_pfx` not scanned for
  rain inputs — open).

## 7. Wind

The field wind used by grass is the climate's wind (`0x03672fe8` /
`0x036730bc`: climate `+0x20c × +0x230`, overrides `+0x644`, `+0x5d0`,
`+0x5d4`) with the WindMgr fade/reroll already ported — **no weather term**
(fact). Weather affects wind only through effects: Strong_Wind_Area when
風の強さ ≥ 8, sandstorm/strong-wind concentrations from map tags.

## 8. Schedule `_2a` (Wii U `+0x1a`) and the wanted weather

Fact (`0x03665ac0`, `0x036651c4`, `0x0366531c`, `0x036723a8`, `0x0366774c`):

- `WeatherMgr+0x1a`: 20 climates × 18 bytes = 3 days × 6 bands. When the
  weekday changes (`TimeMgr+0x11c % 7` ≠ `+0x30c`) each climate's days 1–2
  move to 0–1 and day 2 gets 6 new rolls; then the table is packed into the
  GameData flags `climateWeather`, `…2`, `…3` (4 bits per band). Not done
  in scene mode 3.
- Band of the time angle d (`TimeMgr+0x98`, 15° per hour): byte 0 = 4–8 h,
  1 = 8–12, 2 = 12–16, 3 = 16–20, 4 = 20–24, 5 = 0–4 h of the same day.
- Roll (`WEATHER_RollNewWeather`): r = rand(99) + 1; clear if r ≤
  Bluesky (`climate+0x1c`), cloudy if within Cloudy (`+0x2c`), rain (`+0x3c`),
  heavy rain (`+0x4c`), else **7 ThunderRain** (`+0x5c` StormRate unread);
  climate 9 with its flag (`0x0364c1d4`, TimeMgr+0xf4, Vah Ruta) → clear in
  the roll; before the paraglider flag (`0x03665108`, TimeMgr+0xf8) only
  0/1 survive.
- Wanted weather (`WORLD_GetClimateWantedWeather`, field scene): the band's
  byte of day 0; climate bool `+0x6c` → clear between 6:00 and 18:00, `+0x7c`
  → clear outside it; with the paraglider flag: climate 9 before Ruta → 8;
  `BlueSkyRainPat` (`+0x30c`) = 2 turns rain (2) into 8, = 1 gives 8 when the
  hourly roll `WeatherMgr+0x383` is set (chance `+0x36c` = 5 % at the hour
  change between 6:00 and 18:00 when the wanted weather is clear; cleared
  with 1/900 a frame after minute 30); climate index 0 with
  `EnvMgr 0x03641aac > 0` → clear; set weather `+0x649` (< 9) and the
  `+0x610` path override.
- `+0x274[6]`: the forecast of the current and next five bands, with the
  same overrides (modes `+0x380` 1–6).

## 9. Gaps against the renderer (botw renderer)

Ported already: weather roll/transition, palettes, fog, clouds, bloom
moisture, wind. Missing, all now specified above:

1. Concentrations `+0x2cc…+0x2ec` and their chases (§2.2).
2. The ELink "Camera" property-trigger evaluation and the precipitation /
   thunder / haze / sandstorm / volume-mask sets placed by `Matrix` (§3).
3. Lightning automaton, strike/bluff effects and the flash L on `cFade`,
   base light and shadows (§5).
4. rain_ratio / rainfall chases and the rain shading variation (PS 113
   Shadow.y, PS 33 gloss/normal) (§6).
5. Distance actors (§4).
6. Schedule by bands and days (§8) instead of SI-WTH-03.

## 10. Open questions

- `EP04` and `CSDP` blocks of the rain/snow emitters (area loop? occlusion?).
- Which object creates the ELink user "Camera" and its source matrix.
- Writers of カメラ位置が屋内 (EffectMgr+0x1353c bit 11) and 室内率.
- `(KSys+0x1f4)+0x1f8` (rain suppression) and the variation-2 flag.
- The consumer of the ChemicalMgr lightning requests; SLink thunder keys
  (`EnvThunder`, `cLightningNear`) and their timing.
- `KSys+0x85c` object (`0x038c1f74`): rate frozen during the flash.
- EnvMgr id `0x0364be24` ranges gating strikes (≤ 0 or > 35) and flashes.
- PS 113 Diffuse/Specular differences, PS 25/29/109/141/145; `uking_tex0`
  content in PS 113.
- Who plays `FieldEnvEffect` and the `Always` key of the distance actors.

## 11. Wet ground, exact (follow-up, 2026-10-03)

Method. Every rain program was compared with its dry twin by **executing
both Cemu GLSL dumps** in a small bit-exact interpreter (scratch
`glslrun.py`/`difftest.py`: the Cemu GLSL is translated to Python with
`intBitsToFloat`/`floatBitsToInt` semantics; uniforms are fed by
constant bank/vec4 — through `cemu_uniform_map.py` for remapped shaders,
directly for `uf_blockPSn[i]` ones; textures return a deterministic hash of
(unit, coordinates), identical for both programs). 12 random input sets
per pair; tolerance 1e-4 relative. Then the rain terms were read from the
backward slice of the changed output (`slice.py`) and the forward taint of
the rain inputs (`taint.py`). Pairs (uking_sys model 1, assign variation
0 → 1): pre-shading 112→113 (`preshading_field`), 108→109
(`preshading_field_leaf`), 140→141 (`preshading_field_water`), 144→145
(`preshading_field_xlu`); main pass 32→33 (`field_hybrid`), 28→29
(`field_leaf`), 24→25 (`field_water`). Cemu hashes: 112 `bec68ec6f40a864f`,
113 `7bcb527bda035be4`, 108 `fb2e18ae56397ca7`, 109 `b5ea2a9280688ac1`, 140
`59cba7eb9a9c1df6`, 141 `28e6a2be507943e4`, 144 `09085793b5a9f364`, 145
`eb6287332a08ee60`, 32 `8d24f32f18e6de47`, 33 `7f33027db819f935`, 28
`3179b85d41bfb80d`, 29 `5648500869d5db39`, 24 `2e2543216c04766d`, 25
`07d4d8363600f2d4`.

### 11.1 Result of the differential run (fact)

- **Pre-shading (113, 109, 141, 145):** with rain_ratio = 0 every output
  equals the dry program's; with rain_ratio > 0 **only output 0 `.y`
  (Shadow.y) changes** (the dry programs write 0 there). Diffuse, Specular,
  Fog, Upscale and Shadow.x/z/w are bit-identical. Nothing else changes in
  rain in the pre-shading.
- **Main pass (33, 29, 25):** with the Shadow target's `.y` forced to 0 the
  outputs equal the dry program's for any rainfall; the rain enters only
  through Shadow.y (sampler `gsys_static_depth_shadow` slot = the
  pre-shading Shadow target, unit 11) and `rainfall`.

### 11.2 `uking_tex0` in the pre-shading (fact, data)

`SystemModel.sbfres` (Bootup_Graphics), model `DeferredMain`: materials
`preshading_field`, `preshading_field_leaf`, `preshading_field_xlu`,
`preshading_field_water` all have textures `sampler0 = cloud_noise`,
`lut = highlight`, sampler assign `uking_tex0 ← sampler0`,
`uking_tex1 ← lut`; sampler0 word0 = clamp X/Y **0/0 (wrap)**, mag/min
**1 (bilinear)**, mip **1 (point)** (lut: mip 2). So the rain noise is the
same `cloud_noise` texture (64×64 BC4, channel R, prior research) as the
height-fog noise; `uking_tex0` is shader unit 0 in all four programs
(`sampler_locations`). (`ssao` is bound only to `preshading_shadow_*`.) A
runtime override of this binding was not seen (not searched beyond the
material).

### 11.3 Units (fact)

- **`ctx[20]`** (gsys_context member 10) = four wrapped clocks, written by
  `0x03a0abe0` from `0x039a8e70` as `(T0, T1, T2, T3)` = system `+0x4820, +0x4828, +0x4830, +0x4838`; each pair `(period, value)` at
  `+0x481c + 8i`. Each frame `value += +0x4700`, wrapped while ≥ period
  (`0x039a90d0` loop); `+0x4700 = +0x46fc · t`, or 0 when bit 0 of `+0x46dc`
  is set without bit 5 of `+0x46e0` (paused). Init (`0x039a8590`): all
  periods **120.0**, values 0 (`0x1034bd70`, `0x1034bd6c`); `+0x46fc = 1.0`
  (`0x039a5e70`, `0x1034bd64`). `t` = `*(DAT_1047c258+0xc0)[core]`
  (`0x034165f0` → `0x034098b8` → `0x03a122f8` → `+0x354` →
  `0x03993d5c` → `0x039a8e70`), the 30-fps frame factor. So **`ctx[20].y`
  = frames at 30 fps, wrapped to [0, 120) — a 4-second cycle**; no other
  writer of the periods was found.
- **`ctx[17].y` = tan(fovy/2)** (`qF4`, water research
  `wiiu-water-variants.md` "Layout and coefficients": ctx[17] = (tan·aspect, tan, fovy, 0)). `z·ctx[17].y` is the half-height of the view at depth z in meters, so `k = 4 + 0.03/(z·tan(fovy/2))` (cycles per metre).
- z = `half_depth·ctx[16].x + ctx[14].x` = near + d·(far − near), metres.

### 11.4 Shadow.y (wetness) of the pre-shading programs

Common notation: P = (Sem0.z·(−z), Sem0.w·(−z), −z, 1) view-space position;
world X/Y/Z = rows `environment[43]`, `[44]`, `[45]` · P (inverse view);
N = normalize(2·normal.xyz − 1) of the G-buffer, `N_y` = dot(N,
ctx[12].xyz) (world up component); flag = bit 0 of int(normal.w·255);
DSQ = `gsys_depth_shadow_quarter`(X·e39.z − e39.x, Z·e39.z − e39.y, LOD 0).x
(top-down depth map, e = environment); u2 = `gsys_user2`.y (inner mask);
T0 = `uking_tex0` = cloud_noise.r; time = ctx[20].y.

```text
PS 113 (field) and PS 145 (xlu):
  w    = sat(e39.w·(1 − DSQ) − Y + 1.3 − flag·(2·N_y − 1))     (0x3fa66666 = 1.3)
  n    = T0(X/2, Z/2; LOD 0)
  k    = 4 + 0.03 / (z·tan(fovy/2))
  q    = T0(k·(X + n), k·(Z + n); LOD 0)
  p    = T0((time·0.125 + 4·(Y + 0.5·q), 0); implicit LOD + bias b)
  fade = sat(z/120 − 2/3)                                     (0x3c088889, 0xbf2aaaab)
  Shadow.y = rain_ratio · (1 − w³) · (1 − u2) · (1 − fade) · (0.5 + 0.5·p)
  b = 0.5 in PS 113, 0.25 in PS 145
PS 109 (field_leaf): as PS 113, bias b = 0.25.
PS 141 (field_water):
  w    = sat(e39.w·(1 − DSQ) − Y)                             (no +1.3, no normal/flag term)
  p    = T0((time·0.125 + 2·q, 0); bias 0.25)                 (no world-Y term)
  rest as PS 113.
In 141/145, z for k and fade comes from the textureGather of the half depth
(.w component), z for the world position from the point sample; in 113/109
both from the point sample.
```

The pulse texel `v = 0` lies on the boundary between the first and last
rows of the wrapped, bilinear texture (half-texel), so `p` is the 50/50 mix
of rows 0 and 63 at `u = time/8 + …` (texture coordinate units, wrap).

### 11.5 Main pass (fact)

```text
S  = Shadow target (unit 11) at the pixel's upscale-selected texel
wet_g = sat(2·S.y)            wet_n = sat(2·S.y − 1)
V̂  = normalize(P)             (from the camera)
PS 33 (field_hybrid), PS 29 (field_leaf):
  gl′ = gl + (0.75 − gl)·wet_g                       gl = normal.w
  N′  = normalize(N + V̂ · rainfall·wet_n·(0.1 + 0.4·flag))
  then the dry program with N → N′ and gl → gl′ (flag, metalness, albedo unchanged)
PS 25 (field_water):
  Up  = (ctx[0].y, ctx[1].y, ctx[2].y)               (world up in view space)
  N′  = normalize(N − Up · (N·Up) · 1.6·rainfall·wet_n)       (2·0.8)
  gloss unchanged; then the dry program with N → N′
```

Wetness therefore darkens nothing directly: it raises gloss (shinier,
sharper reflections and highlights) wherever Shadow.y > 0 and, above
Shadow.y = 0.5 with rainfall > 0, tilts the normal toward the camera
(ground, leaves) or flattens its vertical component (water) — the
ripple-like noise comes from `p`/`q` in Shadow.y.

The dry/wet equality checks (12 inputs per pair) are the evidence that no
other term differs; the inputs were random, so branches never reached by
real data were exercised, but no frame was rendered.

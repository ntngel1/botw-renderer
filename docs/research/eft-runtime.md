# eft runtime (BotW Wii U, EFTB v20): how emitters are simulated

Source: `U-King.rpx` v208 in Ghidra (`botw-wiiu-analysis`), decompiled and read
2026-10-03. The library is NintendoWare **eft2** (strings `sysEmitterPluginAttr*`,
`SuperStripe`, `ConnectStripe`, `sysCurlNoiseTextureArray`); code lives at
`0x03b54000..0x03b81d00`, xlink2 ELink at `0x03b81d00..0x03b88000`, the BotW
glue (`gsys` particle manager, xlink callbacks) around `0x0378xxxx`,
`0x0387xxxx`, `0x039fxxxx`. Key functions were renamed `eft_*` in the Ghidra
project (list in [Function map](#function-map)).

Conventions:
- **Fact** = read from code (address given) or counted in the game data with
  `tools/research/ptcl_fields.py` (marked *data*). **Hypothesis** = marked so.
- `res+X` = offset into the emitter data (`EMTR` node data, 0xA88 bytes, the
  parser's `d + X`). `em+X` = runtime `Emitter` object (0x528 bytes, pool at
  `System+0x70`). `set+X` = runtime `EmitterSet`. `er+X` = runtime
  `EmitterResource` (0x1a8 bytes, built by `eft_Resource_InitEmitterResource`
  `0x03b61e98`). `rand()` = emitter RNG (below), uniform in [0,1).
- Frames: all times are frames at 30 fps. The step passed to the calc
  (`set+0x38`, `em+0x4c`) is the game frame-rate scale `*(DAT_1047c258+0xc0)[core]`
  (1 at 30 fps; same value the rest of the game uses) — stored per draw group in
  `PtclMgr+0x617c[group]` by `0x03783fcc`, read by `0x039fd590`.

## 0. Runtime objects and the frame

- `gsys` ParticleMgr (`0x039fd5c4` → `eft_System_CalcGroup 0x03b66b2c`) calcs
  each emitter-set *group* (`set+0x14`, a list `System+0x78+group*4`) with that
  group's step. `eft_EmitterSet_Calc 0x03b5a348` → for each emitter (creation
  order, list `set+0x100`, next `em+0x78`) and its child emitters:
  `eft_EmitterSet_CalcEmitter 0x03b5a0f0` → per-emitter game callback
  (`System+0x1750` = BotW `botw_EmitterCalcLodCallback 0x0387b8d0`, §1.6) →
  `eft_Emitter_Calc 0x03b6ba64`.
- An emitter whose calc returns "done" is freed (`0x03b59f64`); a set with no
  emitters left is killed (`0x03b66b2c`, `set+0x110 == 0`).
- Draw: `gsys_PtclMgr_DrawPath 0x039fcde8` uploads the view UBO and draws the
  sets of the requested groups for one **draw-path mask** (§6).

### Emitter RNG (fact)
- State `em+0x138` (u32). `rand()` = `(float)state * 2^-32`, then
  `state = state*0x41C64E6D + 0x3039` (value is taken **before** the step;
  `eft_Random_GetF32 0x03b6d494`, inlined everywhere). Integer randoms in
  `[0,n)` are `(u64)state*n >> 32` (`0x03b57e2c`, life random).
- Seed (`eft_Emitter_Initialize 0x03b579cc`), mode `res+0x757`:
  0 → next value of a global xorshift128 (`DAT_10597bf4`, game-random);
  1 → `set+0x1c` (per-set random value, also xorshift128, `0x03b5ae0c`);
  2 → fixed `res+0x760 * 0xDFDC1C35` (u32 wrap). *Data*: 0: 8551, 1: 787, 2: 388.
  The seed also initialises two u16 table cursors: `em+0x134 = seed & 0xffff`,
  `em+0x136 = seed >> 16`.
- Two global 512-entry vec3 tables, generated once
  (`eft_Random_InitVecTables 0x03b5f06c`) by a standard xorshift128 with state
  `(x,y,z,w) = (0x178EAB2C, 0xE318145E, 0x45F0CDB4, 0x720A056D)`; each draw
  `f = asfloat(u>>9 | 0x3F800000) - 1` ∈ [0,1), value `2f-1`. Per index i,
  six draws in order: `A[i].xyz`, `B[i].xyz`; `B[i]` is then normalised.
  `A` (`DAT_1047ee9c`, box [-1,1)³) is read with cursor `em+0x134`, `B`
  (`DAT_1047eea0`, unit vectors) with `em+0x136`; each read post-increments the
  cursor, index `cursor & 0x1ff`.

## 1. Lifecycle

### 1.1 Creation
- xlink2 ELink (`0x03b834bc`, string `ptclSys->createEmitterSetID`) calls
  `eft_System_CreateEmitterSetID 0x03b663c4`(set handle, emitter-set index,
  resource id, group id). Group = ELink asset group (`0xff` → user default
  `+0x4c`). Then it sets the set matrix (`eft_EmitterSet_SetMtx 0x03b59550`)
  and applies the ELink parameter block (`0x03b82188`, §1.7).
- `eft_EmitterSet_Initialize 0x03b5af5c`: set defaults (ratios 1, colours 1,
  scales 1, priority `set+9 = 0x80`), identity matrices; then
  `eft_EmitterSet_CreateEmitters 0x03b5ae0c` creates every top-level emitter
  (`eft_EmitterSet_CreateEmitter 0x03b5abc0` → `eft_Emitter_Initialize`).
  Child emitters are **not** created here (§5).
- `eft_Emitter_Initialize 0x03b579cc`: RNG seed; `em+0x6c` fade-out = 1;
  `em+0x70` fade-in = 0 if `res+0x75b || res+0x75c` else 1; colour
  multipliers `em+0x42c..0x448` = 1; ratios `em+0x5c` (LOD emission ratio),
  `em+0x60` (interval scale), `em+0x68` (life scale) = 1; `em+0x58` (current
  interval) = 0; per-particle buffers (`eft_Emitter_AllocParticleBuffers 0x03b57174`); local matrix (`eft_Emitter_RandomizeLocalMatrix 0x03b56a60`);
  emitter-animation defaults (§1.5). The emitter frame `em+0x48`, interval
  counter `em+0x50` and emission accumulator `em+0x54` start at 0 (their reset
  site was not located; hypothesis: zeroed pool).

### 1.2 Emitter-local matrix
`0x03b56a60`: `rot = res+0x788 + (2·rand()-1)·res+0x794`,
`trans = res+0x770 + (2·rand()-1)·res+0x77c` (per axis, draws in order
rx,ry,rz,tx,ty,tz), `scale = res+0x7a0`; SRT at `em+0x1cc`, RT (no scale) at
`em+0x1fc`. Rotation is sead `makeSRT` (R = Rz·Ry·Rx, radians). It is rebuilt
after every emission when `res+0x758` is set (`0x03b57e2c`; *data* 2029
emitters).

World matrices (`eft_Emitter_Calc`): `em+0x22c` (SRT) = `set+0x48` (set SRT) ·
local SRT; `em+0x25c` (RT) = `set+0x78` (set RT) · local RT. Recomputed only
when the set matrix changed (`set+10`, set by `SetMtx` and after an emission)
or when the emitter has transform animations (`er+0x181`, §1.5) — then local =
`makeSRT(EAES, EAER, EAET)` (EAER keys are degrees, ×π/180; without EAER the
default 0x788 is radians). Child emitters use their local matrix with the
parent particle's world position added to the translation (§5).

### 1.3 Start, duration, one-time vs loop (fact, `0x03b6ba64`)
Let `f = em+0x48` (frame before this step), `start = res+0x7f4` (int),
`dur = res+0x7fc` (int), `oneTime = res+0x7f0`.
- Root emitter: emission window `[start, start+dur)`; the window end is only
  enforced when `oneTime`. A looping root emitter emits forever until the set
  fades/stops.
- Child emitter (`em[0] = 1`, §5): `start = parentLife · res+0x7f8 / 100`
  (percent of the parent particle's life), end = `start + dur` if `oneTime`,
  else the parent particle's life.
- No emission when: emission disabled (`set+6`, set stop), `em+5 == 0`,
  `f < start`, or (`oneTime || child`) and `f >= end` and the emitter has
  already emitted once (`em+1`, set in `0x03b6ed38`).
- After the step `em+0x48 += step`.

### 1.4 Fade in/out and death
- Fade-in (`res+0x75b` alpha, `res+0x75c` scale): `em+0x70 += step / res+0x76c`
  until 1 (if `res+0x76c < 1` → 1 at once).
- Fade-out starts when the set is faded (`set+1`, `eft_EmitterSet_Fade 0x03b59e2c`) or the emitter itself (`em+0x516`, set by the LOD callback
  returning 2): if `res+0x754` emission stops; if `res+0x755` (alpha) or
  `res+0x756` (scale): `em+0x6c -= step / res+0x768`; when it drops below 0
  (or `res+0x768 <= 0`) the emitter dies immediately. *Data*: 0x754=1 in 8495
  emitters, 0x755 in 3573, 0x768 mostly 10.
- The fade factors reach the shader through the dynamic UBO (§7): alpha factor
  = `(0x75b ? fadeIn : 1)·(0x755 ? fadeOut : 1)`, scale factor =
  `(0x75c ? fadeIn : 1)·(0x756 ? fadeOut : 1)`.
- Death (emitter calc returns 0):
  - loop emitter: when (set fading or `em+0x516` or child) and
    `f > start + lastEmitFrame(em+0x64) + life(res+0x8b8)`; or when the set
    stopped emitting (`set+6`) and no particle can still be alive
    (`0x03b56738`);
  - one-time: when `f > end + life (+ res+0x768 if 0x755||0x756)`; never if
    `res+0x8a8` (infinite life); stripes add their history length
    (plugin 2: `er+0x1a4→+0x14 × step` `0x03b78380`; 3: `0x03b81b48`).
- `eft_EmitterSet_Kill 0x03b59e38` deletes at once; `0x03b5a9bc` pre-runs
  N frames with step 1 (ELink bit 21).

### 1.5 Emitter animations (`EA**` nodes)
`eft_EmitterAnim_Eval 0x03b75734`, evaluated every frame at the **emitter
frame** `em+0x48` (frames). Node data: `u8 enabled, u8 loop, u16 pad, u32 keyCount, u32 ?, keys[] {f32 x,y,z, f32 frame}` (key 0 at +0xc, 16 bytes).
Linear interpolation; before key0 → key0; at/after the last key → last value
and the slot is marked done (not re-evaluated unless `loop`); `loop` wraps
`fmod(frame, lastKey.frame)`. One key → constant. Slots and defaults
(`eft_Emitter_AllocParticleBuffers` tail, `0x03b57174`):

| node | slot `em+` | default | used as |
|---|---|---|---|
| EAES | 0x460 | res 0x7a0 emitter scale | local matrix (§1.2) |
| EAER | 0x46c | res 0x788 rotation | local matrix, degrees |
| EAET | 0x478 | res 0x770 translation | local matrix |
| EAC0 | 0x484 | res 0x7ac emitter colour0 rgb | dyn UBO colour0 |
| EAC1 | 0x490 | res 0x7bc emitter colour1 rgb | dyn UBO colour1 |
| EATR | 0x49c | res 0x800 emission rate | emission count |
| EAPL | 0x4a8 | res 0x8b8 particle life | new particles' life (clamped ≤ res 0x8b8) |
| EAA0 | 0x4b4 | res 0x7b8 emitter alpha0 | dyn UBO alpha0 |
| EAA1 | 0x4c0 | res 0x7c8 emitter alpha1 | dyn UBO alpha1 |
| EAOV | 0x4cc | res 0x96c all-direction velocity | × `set+0x154` |
| EADV | 0x4d8 | res 0x970 directional velocity | × `set+0x174` at emission |
| EASL | 0x4e4 | res 0x9c8 particle scale | new particles' scale |
| EASS | 0x4f0 | res 0x868 shape (form) scale | × `set+0xa8` |
| EAGV | 0x4fc | res 0x814 gravity scale | CPU gravity |

Without any EA node, `em+0x4cc = res+0x96c·set+0x154` and
`em+0x4f0 = res+0x868·set+0xa8` every frame.

### 1.6 LOD by camera distance (BotW callback `0x0387b8d0`)
Fact; this is game code, not eft. `d` = distance from the camera to the set
position. Evaluated each frame if `res+0x759`, otherwise only while the
emitter frame is 0.
- `near = res+0x7cc`, `far = res+0x7d0` (−1 = no far limit; *data*: −1 in 9120
  emitters).
- `d < near`: if the set is not manual and set-resource byte `+0x16` is 0 →
  callback returns 2 (emitter fades out, §1.4); otherwise the emitter is hidden
  (`em+0x40 = em+0x44 = 0`).
- `d > far` (far ≠ −1): hidden unless `res+0x75a && res+0x7d4 != 0`.
- otherwise visible (`em+0x44 = 0xffffffff`).
- If `res+0x75a`: `t = clamp((d-near)/(far-near), 0, 1)`,
  `r = res+0x7d4 · 0.01`, emission ratio `em+0x5c = min(1, (1-t)(1-r) + r)`.
  So `res+0x7d4` is the **percentage of emission kept at `far`**.
- Draw path switching (same callback): path 5 → 8 if `d ≥ 80` else 0; path 6
  → 7 if `d < DAT_1047c210+0x13540` else 8 (`0x03b568bc`).
- Other branches hide or kill emitters for game states (actor flags, cut-scene
  mode); not needed for a renderer.

### 1.7 Set-level parameters
`set` fields written by the API / ELink (`0x03b82188` applies a bitmask of
user parameters; ELink resource parameters per
`lib/xlink2/include/xlink2/xlink2ResourceAccessorELink.h` in the Switch decomp:
delay, duration, scale, position, rotation, colour, alpha, emission rate /
scale / interval, directional velocity, life scale):

| set field | meaning (evidence) |
|---|---|
| +0x48 / +0x78 | SRT / RT matrix; +0xb4..0xbc column scales (`SetMtx 0x03b59550`) |
| +0x3c | emission ratio, clamped ≤ 1 (`0x03b59824`) — multiplies the count |
| +0x40 | interval scale, clamped ≥ 1 (`0x03b59840`) |
| +0x44 | life scale, clamped ≤ 1 (`0x03b5985c`) |
| +0xa8..0xb0 | emitter volume scale (× form scale) |
| +0xc0..0xcc | set colour rgba (dyn UBO) |
| +0xdc..0xe4 | particle scale; +0xf4..0xfc = it × matrix scale (dyn UBO +0x34) |
| +0xe8..0xf0 | particle scale applied at emission (× EASL) |
| +0x154 / +0x174 | all-direction / directional velocity scale |
| +0x158 | velocity-random scale |
| +0x15c..0x164 | extra velocity, world → set-local at emission |
| +0x178..0x180 | (with `set+5 = 1`) ELink bit 16, not traced |
| +6 | stop emission; +1 fade; +9 draw priority (default 0x80) |
| emitters `em+0x42c..0x448` | colour0/colour1 rgba multipliers (`0x03b59d7c/0x03b59db8`) |

## 2. Emission (fact, `eft_Emitter_Calc 0x03b6ba64`)

### 2.1 Time-based (`res+0x7f2 == 0`)
```
if counter(em+0x50) < interval(em+0x58):  counter += step
else:
    over   = counter - interval
    R      = (volume in {5,6}) ? res+0x800 : EATR(em+0x49c)
    n_f    = (R - (res+0x804/100)·res+0x800·rand()) · em+0x5c · set+0x3c + accum(em+0x54)
    n_f    = max(n_f, 0);  n = (uint)n_f;  accum = n_f
    if n == 0: counter = 0
    else:
        emit n particles (eft_Emitter_EmitParticles 0x03b6ed38)
        interval = (res+0x808 + 1 + randInt[0, res+0x80c)) · em+0x60 · set+0x40   (0x03b57e2c)
        if res+0x758: re-randomise the local matrix
        accum -= n;  counter = step + over;  lastEmit(em+0x64) = frame
```
- Interval is "frames skipped": interval field 0 → every frame, 2 → every
  3rd frame. The first emission happens on the first frame (interval starts 0).
- `res+0x804` (`rate_random`) is a **percentage reduction** of the rate, using
  the static rate `res+0x800` even when EATR animates it.
- The fractional count is carried over (`accum`), so rate 0.5 emits every
  other emission.
- Volume types 2 and 13 multiply n by the division count (§3.1); types 5, 6,
  15 had `res+0x800` overwritten at load (§3.1).

### 2.2 Distance-based (`res+0x7f2 != 0`, *data* 232 emitters)
`move = |emitter world position − previous|` (`em+0x298`); `unit = res+0x824`,
`min = res+0x828`, `max = res+0x82c`, `thr = res+0x830`. `dist = min` if
`move < thr` or `move == 0`, else `clamp(move, min, max)`. `accum += dist`;
`n = accum / unit`; for each k the emitter translation is temporarily set to
`lerp(prevPos, curPos, …)` (`0x03b6d500/0x03b6d51c`) and one particle is
emitted; `accum -= n·unit`.

### 2.3 Particle slots
- CPU calc: first free slot (`info[0] == 0`). GPU calc: ring index `em+0x170`
  (wraps at capacity `em+0x16c`), reusing a slot whose particle has expired;
  live count `em+0x20` (≤ capacity).
- Capacity (`0x03b57174`) ≈ `ceil(life / (interval+1)) · rate + ceil(rate)`
  (bounded by duration for one-time emitters; ×divisions for types 2/13).

## 3. Particle initialisation (`eft_Emitter_InitParticle 0x03b6d628`)

Per emission call one extra `rand()` (`e`) is drawn first
(`0x03b6ed38`) and passed to the volume function. Order inside one particle:

1. **Volume** `table 0x1047efc4[res+0x838]`(pos, vel, emitter, index i,
   count, anim slots) → local position `p` and velocity `v`
   (all-direction velocity is already in `v`). Returning 0 drops the particle.
2. Store emitter matrices into attributes (sysEmtMat/EmtRTMat, §4.3).
3. `res+0x984` ≠ 0 (XZ diffusion): `v += normalize(p.x, 0, p.z)·res+0x984`
   (random XZ direction if `p.xz ≈ 0`, two `rand()`).
4. Directional: `dv = EADV · set+0x174`; `k = 1 - rand()·(res+0x994/100)·set+0x158`
   (**velocity random is a percentage**). Direction `dir = res+0x974..0x97c`;
   if `res+0x7f3` the direction is in world space and is rotated into emitter
   space by the inverse emitter RT (follow type 0) or the per-particle stored
   matrix (types 1/2). Diffusion angle `a = res+0x980` (degrees): if `a == 0`
   `v = (dir·dv + v)·k`; else a random vector in a cone around +Y —
   `y = rand()·(1-c) + c` with `c = 1 - a/90`, azimuth `2π·rand()`,
   radius `sqrt(1-y²)` — rotated by the shortest-arc rotation from +Y to `dir`,
   then `v = (cone·dv + v)·k`. (So a = 90° is a hemisphere, 180° a full sphere;
   uniform in y, not in angle.)
5. `res+0x810` ≠ 0: `p += B[cursor136]·res+0x810` (position random, unit-vector
   table, not the LCG).
6. `p += em+0x458[i]` (optional per-index offsets, custom code) and
   `p += em+0x44c` (offset, normally 0).
7. `v += A[cursor134] ⊙ res+0x988..0x990` (per-axis velocity random, box table).
8. `v += em+0x298 · res+0x998` (inherit emitter movement).
9. `v += set+0x15c..0x164` transformed by the transpose of the set RT.
10. GPU stream-out type (`res+0x752 == 2`): `p += v`.
11. **Scale** (`attr+0x00`): `s = EASL ⊙ set+0xe8..0xf0 · (1 - rand()·rnd/100)`;
    one shared random when `res+0x9d4 == res+0x9d8` (then `res+0x9d4` is used for
    all axes), else three randoms with `res+0x9d4/0x9d8/0x9dc` (percentages).
12. `attr.scale.w = 1 + res+0x8c0·(1 - 2·rand())` — a per-particle **speed
    multiplier** for position integration (§4.1).
13. Birth frame `data+0x1c` = `info+8` = emitter frame; `info+0x18 = 0`.
14. **Life** (`res+0x8a8 == 0`): `life = (EAPL - EAPL·randInt[0,res+0x8bc)·0.01) · em+0x68 · set+0x44` (**life random is an integer percentage reduction**);
    infinite life = 2^28. Stored `data+0xc` (float) and `info+0xc`.
15. `attr.random (+0x10)` = 4 × `rand()` (sysRandomAttr).
16. `attr.initRotate (+0x20)` = `res+0x700..0x708` (the random part is applied in
    the shader from sysRandomAttr, mirror in §4.2).
17. `attr.color0/1 (+0x30/+0x40)` = 1 (overwritten by child inheritance, §5).
18. One child emitter per child resource (§5).

### 3.1 Volume functions (table `0x1047efc4`, *data* counts in brackets)
`rx,ry,rz = res+0x85c..0x864 ⊙ EASS`, `omni = EAOV (em+0x4cc)`. Angles in
radians, `sweep = res+0x840`, `start = res+0x848` (`= 2π·e` if `res+0x839`).
`angle = start + rand()·sweep - sweep/2` (arc centred on `start`).

| type | fn | position / velocity |
|---|---|---|
| 0 point [4571] | `0x03b6f32c` | p = 0; v = B[c136]·omni |
| 1 circle [188] | `0x03b6f390` | p = (sinθ·rx, 0, cosθ·rz), v = (sinθ,0,cosθ)·omni |
| 2 circle, equal div [608] | `0x03b6f50c` | `N = res+0x880`; if `res+0x874 == 0` N −= (int)(e·res+0x884·0.01·N); if sweep ≠ 2π, N −= 1. index = i (mode 0), randInt (1), sequential `em+0x34` (2); θ = start + idx·sweep/N − sweep/2 + (2rand−1)·res+0x84c |
| 3 circle fill [439] | `0x03b6f8bc` | radius factor `sqrt(lerp((1-c)², 1, rand))`, `c = res+0x850`; v = normalised(p in unit-circle space)·omni |
| 4 sphere [865] | `0x03b6fbd4` | y = 2rand−1 (or `res+0x83a` latitude mode: y ∈ [cos(res+0x844), 1], then rotated from +Y to axis `res+0x83e` ∈ 0..5); xz on the arc; p = dir⊙r, v = dir·omni |
| 5 sphere, equal div [140] | `0x03b70400` | unit table `PTR 0x1047eea8[res+0x83c]` (2,3,4,6,8,12,20,32 points); index by mode `res+0x874`; latitude cut drops points with y < cos(lat) |
| 6 sphere, equal div 64 [925] | `0x03b70ca4` | table `PTR 0x1047eed0[res+0x83d − 4]` (4..64 points), same rules |
| 7 sphere fill [1039] | `0x03b71544` | as sphere, radius factor `1 − c + c·sqrt(rand)` |
| 8 cylinder [225] | `0x03b71e38` | circle + y = (2rand−1)·ry |
| 9 cylinder fill [215] | `0x03b71f14` | circle fill + y = (2rand−1)·ry |
| 10 box [125] | `0x03b71ff0` | random point on a face chosen by raw RNG state (< 1/3 → ±z face, < 2/3 → ±y, else ±x; sign by next state < 2^31); v = normalised(p)·omni |
| 11 box fill [127] | `0x03b72318` | filled shell between `1-c` and 1 of the box (see code); v = normalised random dir·omni |
| 12 line [58] | `0x03b72890` | z = rand·L − (res+0x854·L + L)/2, `L = res+0x858·EASS.z`; v = (0,0,omni) |
| 13 line, equal div [131] | `0x03b72934` | `N = res+0x888` (random reduction `res+0x88c`%), z = idx/(N−1)·L − (res+0x854·L+L)/2 |
| 14 rectangle [9] | `0x03b72c4c` | random point on the rectangle outline (rx, rz); v = normalised(p)·omni |
| 15 primitive [61] | `0x03b72eb8` | vertex `idx` of the shape primitive (`res+0x878` id → `er+0x68`), p = vertex ⊙ EASS (no radius), v = normal·omni; mode `res+0x874` 0 = i, 1 = random, 2 = sequential; no primitive → point |

Load-time patch (`eft_EmitterResource_Setup 0x03b5f62c`): for types 2, 13 with
`res+0x874 == 0` the rate `res+0x800 := 1`; type 5 → table size, type 6 →
`res+0x83d`, type 15 → vertex count; other types set `res+0x874 := −1`.
So `res+0x874` = "emit every point at once (0) / one random point (1) / one
sequential point (2)".

## 4. Particle update

### 4.1 CPU calc type (`res+0x752 == 0`, *data* 1650 emitters)
`eft_Emitter_CalcParticlesCpu 0x03b6ae7c` → `eft_Particle_CalcCpu 0x03b5d9b0`
per live particle, `age = emFrame − birth`, `dt = em+0x4c`. Positions and
velocities live in two ping-pong buffers (`em+0x144` current, `+0x148`
previous; swapped each frame by `0x03b56900`).
```
k  = attr.scale.w · dt
p  = (age <= 0) ? p + v·k : prevP + prevV·k
air = res+0xC0
v  = (age <= 0 ? v : prevV) · (air < 1 ? pow(air, dt) : 1)
g  = EAGV(em+0x4fc)            // default res+0x814
if g > 0:
    G = res+0x818..0x820 · g    // raw vector, not normalised
    if res+0x7f1 (world gravity): G = Rᵀ·G   (R = emitter RT, or per-particle RT for follow type 1)
    v += G · dt
fields (§8) in order FRND, FRN1, FMAG, FSPN, FCOL, FCOV, FCLN, FPAD, FCSF
if age >= life: free the slot
diff = p − prevP if any |component| > 0.01, else keep the previous diff   (sysLocalDiffAttr)
```
Explicit Euler with the position step taken **before** the velocity update.
Air resistance is `pow(air, dt)` per step (frame-rate independent).
*Data*: gravity scale is typically 0.0109 ≈ 9.8 m/s² / 30²; direction mostly
(0,−1,0). `res+0xB0..0xBC` holds a copy of `res+0x818..0x820` and `res+0x814`
(all 9726 emitters) — that is the static-UBO copy used by the GPU types.

Colour, alpha, scale animation, rotation and texture animation are **not**
computed on the CPU for drawing even in CPU calc type; they are done in the
vertex shader from the attributes and UBOs. The CPU has exact mirrors of those
formulas (used to give a child emitter its parent particle's current values):

### 4.2 Shader-side animation, from the CPU mirrors (fact)
Static UBO offsets are given as `res` offsets (UBO byte = res − 0x50, §7).
- **8-key tracks** (`eft_Anim8Key_Eval 0x03b5bc84`): keys `{x,y,z,t}` with
  `t` ∈ [0,1] of the particle's life. Count 0 → 1.0; 1 → key 0. If the loop
  period `P > 0`: `t = fmod(age + rnd·randomStart·P, P) / P` with
  `rnd = sysRandomAttr.x`; else `t = age / life`. Before key0 → key0, at/after
  the last → last, else linear. Tracks and their loop settings:

  | track | keys / count | loop on, period (frames), random start |
  |---|---|---|
  | colour0 | 0x3c0 / 0x60 | 0x8d8, 0x8e4, 0x8dd |
  | alpha0 | 0x440 / 0x64 | 0x8d9, 0x8e8, 0x8de |
  | colour1 | 0x4c0 / 0x68 | 0x8da, 0x8ec, 0x8df |
  | alpha1 | 0x540 / 0x6c | 0x8db, 0x8f0, 0x8e0 |
  | scale | 0x600 / 0x70 | 0x8dc, 0x8f4, 0x8e1 |
  | (param) | 0x680 / 0x74 | — |

  Load-time patch writes the periods as floats to res 0x80..0x90 (0 when the
  loop flag is off) and the random-start flags as 1.0/0.0 to res 0x94..0xa4.
  Keys beyond the count are filled with the last key (setup copies them up to
  8), so the shader can always read 8.
- **Colour/alpha source** `res+0x9a4` (colour0), `0x9a5` (colour1), `0x9a6`
  (alpha0), `0x9a7` (alpha1): **0 = constant** (setup copies `0x9a8/0x9b8` rgb,
  `0x9b4/0x9c4` alpha into key 0), **2 = 8-key animation**, **3 = random key**:
  `key[(int)(min(rnd.x, 0.999999)·count)]` (`0x03b5e234`). Value 1 never occurs
  (*data*: colour0 0/2/3 = 6652/2356/718).
- Final colour0 = track · attr.color0 · emitter colour0 (dyn UBO) ·
  colour scale `res+0x3b0`; alpha0 = track · attr.alpha · emitter alpha.
- **Scale**: `scale = attr.scale.xyz · track(scale)`, then optional fluctuation
  (`res+0x9ed` on X, `0x9ee` on Y, `0x9ec` on alpha0) with wave type
  `res+0x9ef >> 4`: 0 sine `1 − amp·(cos(2π·u)+1)/2`, 1 saw `|1 − frac(u)·amp|`,
  2 square `|1 − (frac(u) ≥ 0.5)·amp|`, `u = rnd.x·phaseRnd + (age+phase)/period`
  (`0x03b5d82c/0x03b5d8a4/0x03b5d91c`). Parameters: X amp `res+0xe0`, period
  `0xe8`, phase random `0xf0`, phase `0xf8`; Y uses `0xe4, 0xec, 0xf4, 0xfc`
  (hypothesis for which of 0xf0/0xf8 is phase vs phase-random: by argument
  order).
- **Rotation** (`eft_CalcParticleRotateCpu 0x03b5ec74`), per axis, enabled by
  `res+0x8b0/0x8b1/0x8b2` (setup zeroes 0x700/0x710/0x720/0x730 of disabled
  axes): `r0 = ±init(0x700) + rnd·random(0x710)`, `w = ±(add(0x720) + pairRnd·0.5·addRandom(0x730))` where X uses `rnd.x` and `(rnd.x+rnd.y)/2`,
  Y `rnd.y`/`(rnd.y+rnd.z)/2`, Z `rnd.z`/`(rnd.z+rnd.x)/2`; the sign flips
  (init and add) when `res+0x8ad/0x8ae/0x8af` and `rnd.y/rnd.z/rnd.x ≥ 0.5`.
  Resistance `q = res+0x72c`: `rot = r0 + w·(q == 1 ? age : (1 − q^age)/(1 − q))`.
  `res+0x8ab` (4/5/6) is the Euler order (not used on the CPU).
- **Texture pattern / UV** (setup, `0x03b5f62c`): per sampler s (flags at
  `0xa58+0x10s`, pattern block `0x110+0x90s`, UV block `0x2c0+0x50s`):
  `+0` pattern type 1/2/3 → flag bits, 4 → setup fills the table with
  `0..count−1` and copies `0x118` → `0x110` (hypothesis: "random"/"table" naming
  of 1..4 is for the shader agent); `+1` scroll, `+2` rotate, `+3` scale (when
  off, setup zeroes the add/initial/random values and scale initial = 1);
  `+4` mirror/repeat: writes UV tiling `(1|2, 1|2)` to `0x300+0x50s`
  (0 → (1,1), 1 → (2,1), 2 → (1,2), 3 → (2,2)). Remaining flags are packed into
  the static UBO flag word (§7).

### 4.3 GPU calc types (`res+0x752` = 1 GPU [4117], 2 GPU stream-out [3959])
No per-particle CPU work after emission; the shader evaluates position from
`(p0, v0, age)` (type 1) or integrates in a stream-out pass (type 2, buffers
`em+0x2c0/+0x31c`, attributes `sysInPos`/`sysInVec`, swap `em+0x378`). Fields
for these types come from the field UBO (§8). Per-particle vertex attributes
(instanced, `eft_Shader_BindAttributes 0x03b639f4`; format 0x813 = 4×f32):

| buffer (stride) | attr @offset | contents (written at emission) |
|---|---|---|
| 8 (0x30) | sysLocalPosAttr @0 | p.xyz, life |
| | sysLocalVecAttr @0x10 | v.xyz, birth frame |
| | sysLocalDiffAttr @0x20 | position delta (CPU type) |
| 9 (0xb0) | sysScaleAttr @0 | scale.xyz, speed multiplier |
| | sysRandomAttr @0x10 | 4 randoms |
| | sysInitRotateAttr @0x20 | res 0x700..0x708, ? |
| | sysColor0Attr @0x30 / sysColor1Attr @0x40 | 1 or inherited |
| | sysEmtMat0..2 @0x50/0x60/0x70 | emitter SRT rows at birth |
| | sysEmtRTMat0..2 @0x80/0x90/0xa0 | emitter RT rows at birth |
| 0 | sysPosAttr | quad `(−.5,.5,0,0) (−.5,−.5,0,1) (.5,−.5,0,2) (.5,.5,0,3)`, index 0,1,2,3, quad primitive (`0x03b6aca8`); or the particle primitive's vertices (normal 2, tangent 15, colour 3/4, uv 5) |

Age in the shader = dyn UBO emitter frame (+0x20) − birth frame.

### 4.4 Follow type `res+0x753` (fact from sort/field/gravity code)
0 = particles live in emitter space and use the **current** emitter matrix;
1 = no follow: the emitter matrix at birth (sysEmtMat) is used for the whole
life; 2 = position only: birth rotation/scale with the **current** emitter
translation. Setup ORs `0x200/0x800/0x400` (types 0/1/2) into the flag word
`res+0x54` (`table 0x103a5754`).

## 5. Child emitters
- Created at **particle birth**, one per child resource per parent particle
  (`0x03b6d628` tail → `eft_EmitterSet_CreateEmitter(set, childRes, 0, parent, index)`), only from CPU parents (*data*: all 154 child emitters have CPU
  parents). Child fields: `em+0x3b0` parent emitter, `+0x3b4/0x3b8` parent ids,
  `+0x3bc` particle index, `+0x3c0` parent particle life, `+0x3c4` parent
  frame, `+0x3c8` p, `+0x3d4` v, `+0x3e0` scale attr, `+0x3f0` init rotation,
  `+0x400` random attr.
- Every parent CPU calc updates the child: `+0x410` = particle age, `+0x3c0`
  = life, `+0x414` = particle world position, `+0x420` = world velocity
  (`0x03b6ae7c`). The child's emitter matrix is its local matrix translated by
  `+0x414` (parent rotation is not applied).
- Emission window: §1.3 (`res+0x7f8` = start as % of the parent's life;
  *data*: 60 or 0 typical).
- Inheritance at the child's particle birth (`eft_Emitter_InheritFromParentParticle 0x03b6eae4`, flags in the **child** resource): `0x7d8` velocity
  (`p += parentVel·res+0x7e8`), `0x7d9` scale (parent's current animated scale
  × `res+0x7ec`), `0x7da` rotation, `0x7dc/0x7de` colour0/alpha0,
  `0x7dd/0x7df` colour1/alpha1 (parent's current animated values). In the dyn
  UBO, `0x7de&&0x7e2` / `0x7df&&0x7e3` multiply the child alpha by the parent
  emitter alpha and fade.
- Draw order: children with `res+0x7e1 == 1` are drawn before their parent,
  others after (`0x03b5a7d4`).

## 6. Draw: order, sorting, render state

### 6.1 Draw paths and passes
- Each emitter has a draw path `res+0x764` (0..26; *data* 7: 2906, 6: 2658,
  8: 1866, 21: 741, 0: 522, 20: 364 …), stored `em+0x2b8`; the set ORs
  `1<<path` into `set+0x184`. The game draws a pass with a path mask
  (`0x039fcde8` arg 5): constants at the callers — `0x039bd008` is called with
  paths 0x1a, 0x16, 0x14, 0x15, 0x11, 0x12, 4, 3, 7, 9, 0x13, 8, 0x17, 0xc,
  0xd, 0x18, 0x19 from different render steps (`0x03405ec8`, `0x039b19e4`,
  `0x039b18a8`, `0x039b1e08`, `0x039aa628`, `0x039d19d4`, `0x039aab74`,
  `0x039b2a88`, `0x03a15764`, `0x03a159e8`, `0x034030c4`), `0x039bd3e0` with a
  path from a table, `0x03a25294` with path 3, `0x03a255b0` with 14 or 16.
  **Which render pass each path is drawn in is open** (needs the render-step
  map). Paths 8 and 23 get an extra per-emitter draw callback
  (`0x03b67478` registrations at `0x03783220`).
- Within a pass: sets are collected per group (`eft_System_AddSortEmitterSets 0x03b66fbc`) with key `priority(set+9)<<24 | z24` where z24 encodes the view
  z of the set position (sign → 0x800000, exponent−64 in 7 bits, 16 mantissa
  bits), sorted **descending** (`0x03b6718c`; with view z negative in front,
  farther sets first, higher priority first), then drawn
  (`eft_System_DrawSortedEmitterSets 0x03b67224` → `eft_EmitterSet_Draw 0x03b5a8ac`). Inside a set: emitter creation order.
- Emitter drawn if `res+0x750` (visible), `em+2`, fade-out > 0, it has
  particles (or is a stripe), `em+0x44 & mask` and the path bit match
  (`0x03b5a5a4`).

### 6.2 Particle sort `res+0x751` (`eft_Renderer_DrawSortedParticles 0x03b5883c`)
0 = no sort, one instanced draw. Otherwise one draw per particle in sorted
order: 1 = birth frame **descending** (newest first; comparator `0x03b58820`),
2 = view-space z **ascending** (farthest first = back to front; world position
via the follow-type rules of §4.4; comparator `0x03b58804`), 3 = birth frame
ascending (oldest first). *Data*: 0: 8830, 2: 756, 3: 118, 1: 22.

### 6.3 Render state (`eft_SetRenderState 0x03b5f5ac`, block `res+0x898`)
`0x898` blend enable, `0x899` depth test, `0x89a` depth func, `0x89b` depth
write, `0x89c` alpha test, `0x89d` alpha func, `0x8a0` alpha ref,
`0x89e` blend type, `0x89f` cull (0 none, 1 cull back = front only, 2 cull
front). Blend types (`eft_SetBlendType 0x03b5f420`, GX2 blend enums by argument
order; the GX2 import names are not resolved in this RPX):
0 `src·a + dst·(1−a)`; 1 additive `src·a + dst`; 2 subtract `dst − src·a`;
**3 multiply `dst·src`; 4 screen `src·(1−dst) + dst`**.

### 6.4 Textures
Samplers `res+0x9f8 + 0x20s`: wrap `+8/+9` → GX2 via table `0x103a68e0`
`[1,0,2,3]`: 0 mirror, 1 repeat, 2 clamp, 3 mirror-once; `+10` filter
0 = linear, else point; `+0xc` max LOD, `+0x10` LOD bias (`0x03b68268`).
Also bound when the shader uses them: frame buffer copy, depth buffer, curl
noise array.

## 7. Uniform blocks (fact)

- **sysViewUniformBlock**: 0x140 bytes built by the game (`0x039fcde8`, from
  the gsys camera/projection) and copied/byte-swapped by
  `eft_System_BeginRender_SetViewUbo 0x03b66e90`. Layout not decoded here.
- **sysEmitterStaticUniformBlock**: the emitter data bytes `res+0x50..0x750`
  (0x700 bytes) after the load-time patch, byte-swapped to GPU order
  (`0x03b68578`), bound per emitter (`er+8`). CPU emitters get a private copy
  (`er+0x14`), GPU ones use the data in place. Patched by `0x03b5f62c`:
  constant colours into key 0 and keys padded to 8; loop periods/flags into
  0x80..0xa4; texture-animation values; rotation axes; `res+0x50` flag word:
  bit 3 world gravity (0x7f1), bits 0..2 scale wave type (1/2/4), bits 4..7 /
  8..11 / 12..15 pattern type of samplers 0/1/2, 0x10000/0x20000/0x40000
  rotation random-direction (0x8ad..0x8af), 0x80000/0x100000/0x2000000 sampler 0
  flags `0xa5d/0xa5e/0xa5f` (×0x10 offsets for samplers 1, 2), 0x10000000
  `0x8b3`, 0x20000000/0x40000000 `0x8ac == 0/1`; `res+0x54`: bit 0 `0x8b4`,
  field presence (FRND 2, FPAD 4, FMAG 8, FCOV 0x10, FSPN 0x20, FCOL 0x40,
  FCLN 0x80, FRN1 0x100), follow type bits; `res+0x5c` = first word of FCSF;
  `er+0x194..0x19c` = `res+0x700..0x708`. The uniform names inside the block
  are for the shader research.
- **sysEmitterDynamicUniformBlock** (0xc0 bytes, triple-buffered `em+0x110`,
  byte-swapped; `eft_Emitter_UpdateDynamicUbo 0x03b6b724`, every frame):

  | off | value |
  |---|---|
  | 0x00 | colour0 rgb = `em+0x42c..434 · EAC0 · set+0xc0..0xc8` |
  | 0x0c | alpha0 = `em+0x438 · EAA0` (× parent EAA0 for inheriting children) |
  | 0x10 | colour1 rgb = `em+0x43c..444 · EAC1 · set colour rgb` |
  | 0x1c | alpha1 = `em+0x448 · EAA1` |
  | 0x20 | emitter frame |
  | 0x24, 0x28 | 1.0 |
  | 0x2c | frame step |
  | 0x30 | `set+0xcc` (set alpha) · alpha fade (§1.4) |
  | 0x34..0x3c | `set+0xf4..0xfc` (particle scale × set matrix scale) · scale fade |
  | 0x40..0x7c | emitter SRT (3 rows + `0,0,0,1`) |
  | 0x80..0xbc | emitter RT (3 rows + `0,0,0,1`) |
- **sysEmitterFieldUniformBlock** (0x120 bytes, `er+0x24`), filled at load from
  the field nodes (`0x03b5f62c` tail): FRND → +0x00..0x44, FRN1 → +0x50..0x5c,
  FPAD → +0x60..0x6c, FMAG → +0x70..0x80, FCOV → +0x90..0xa0, FSPN →
  +0xb0..0xb8, FCOL → +0xc0..0xcc, FCLN → +0xd0..0xf4, FCSF → +0x100..0x11c
  (word-for-word copies; see the code for the per-field order).
- sysEmitterPluginUniformBlock (stripes) and sysCustomShaderUniformBlock0..3
  (custom-shader callbacks, `res+0x92c` id, BotW callbacks `0x03788fb4` etc.)
  were not decoded.

## 8. Attribute nodes (`eft_Resource_InitEmitterResource 0x03b61e98`)
Each attribute node's data pointer goes to an `er` slot; counts are *data*
over all effect files (9726 emitters).

| magic | count | `er+` | meaning (evidence) |
|---|---|---|---|
| EAES EAER EAET EAC0 EAC1 EATR EAPL EAA0 EAA1 EAOV EADV EASL EASS EAGV | 129…1036 | 0x148..0x17c | emitter animations, §1.5; EAES/EAER/EAET also set `er+0x181` |
| CSDP | 4615 | 0x188 (+size−0x20 at 0x190) | custom shader parameters for the game's custom-shader callback |
| CUDP | 0 | 0x18c | custom user data |
| CADP | 1194 | 0x184 | custom action parameters (callback id `res+0x968`), CPU emitters only |
| EP01..EP04 | 35/123/128/61 | 0x1a0 = 1..4, data 0x1a4 | emitter plugins (stripe family; 2/3 extend lifetime by history) |
| FRND | 127 | 0x124 | noise-type random field, `0x03b73144` |
| FRN1 | 13 | 0x128 | eft1 random: every `u32 +0xc` frames of age `v += A[c134] ⊙ (+0..+8)` |
| FMAG | 29 | 0x12c | magnet: bytes follow-emitter, x/y/z enable; target +8..; strength +4; v += (target − p − v)·k (partially recovered, `0x03b5c144`) |
| FSPN | 141 | 0x130 | spin about axis `+4` (0 X, 1 Y, 2 Z) by `+0`·π/180 per frame, outward push (`0x03b5c670`) |
| FCOL | 0 | 0x134 | collision with plane y = `+4` (local / world `+1`): type `+0` 0 bounce (v.y = −v.y·`+8`, v ·= `+0x10`, count ≤ `+0xc`), 1 kill |
| FCOV | 210 | 0x138 | convergence: p += (target − p)·ratio·speed·dt |
| FPAD | 10 | 0x13c | position add `+4..+0xc` per frame (world flag `+0`) |
| FCSF | 3607 | 0x140 | custom field: the CPU callback `System+0x1758` is never set by BotW, so only the field UBO copy matters; 3599 of 3607 are GPU-SO emitters |
| FCLN | 775 | 0x144 | curl noise, `0x03b69810` (5 KB), mostly GPU-SO |

Field parameters may themselves be animated over the particle's life
(`eft_FieldAnim_Eval 0x03b5b8f8`: `+4` loop, `+8` random start, `+0xc` count,
`+0x10` period, keys at `+0x14`). All fields run on the CPU only for CPU-calc
emitters; for GPU types they reach the shader via the field UBO.

## 9. Corrections to `crates/botw-formats/src/ptcl/emitter.rs` (do not edit there; for the port)

| field | parser says | code says |
|---|---|---|
| colour/alpha source `0x9a4..0x9a7` | 1 = Random, ≥2 = Animated | **0 constant, 2 animated, 3 random key**; 1 never occurs |
| `Emission::rate_random` 0x804 | "up to N more" | percentage **reduction** of `rate` |
| `Emission::interval` 0x808 | frames between emissions | frames **skipped**: period = interval+1 (+randInt[0,0x80c)) |
| `Emission::duration`/`one_time` | — | window end only used for one-time and child emitters; looping roots emit forever |
| `Particle::life_random` 0x8bc | percentage (guess) | confirmed: integer percentage reduction |
| `Velocity::random` 0x994 | additive | percentage reduction of (directional + all-direction) |
| `Velocity::diffusion_angle` 0x980 | radians | **degrees**, cone in y ∈ [1−a/90, 1] around +Y rotated to `direction` |
| `Velocity::diffusion` 0x988 | per-axis diffusion | per-axis velocity random (box table) |
| `scale_random` 0x9d4 | percentage (guess) | confirmed percentage; one random if x == y |
| gravity | 0xB0·0xBC | CPU uses `0x818..0x820 · 0x814` (EAGV-animatable); 0xB0..0xBC is the UBO copy (identical in all files) |
| `Info::emission_range` / `ratio_far` | — | near cull / far cull (−1 none) / % kept at far; BotW callback, §1.6 |
| `Info::alpha_fade_time` 0x768 / `fade_in_time` 0x76c | — | confirmed; gated by 0x755/0x756 and 0x75b/0x75c |
| `Info::calc_type` 0x752 | — | 0 CPU, 1 GPU, 2 GPU stream-out (copy at 0x83f) |
| `Info::sort_type` 0x751 | 0 none, 1 dist, 2 dist rev, 3 index | 0 none, 1 newest first, 2 back-to-front, 3 oldest first |
| `RenderState::blend_type` 3/4 | 3 screen, 4 multiply | **3 multiply, 4 screen** |
| `Particle::rotation_type` 0x8ab | axes | Euler order (4/5/6); axes enabled by 0x8b0..0x8b2 |
| `Shape::caliber_ratio` 0x850 | inner radius fraction | **fill thickness from the surface**: inner radius = 1 − value |
| `Shape::sweep_*` | — | sweep 0x840 centred on start 0x848 (random start 0x839); latitude 0x844 with axis 0x83e when 0x83a |
| `Sampler::filter` | — | 0 linear, else point |
| `Info::translate_random` etc. | — | applied once per emitter (and per emission if 0x758) |
| `Emission::position_random` 0x810 | — | radius along a random unit vector (table B) |
| `Track::Random` "key 0" | stand-in | per particle `key[(int)(rnd.x·count)]` |

New fields worth reading: 0x754–0x75c (fade/LOD flags), 0x757/0x760 (seed),
0x764 (draw path), 0x7d8–0x7ec (child inheritance), 0x7f2/0x824–0x830
(distance emission), 0x7f3 (world direction), 0x7f8 (child start %),
0x814/0x818 (gravity), 0x874–0x88c (equal-division modes), 0x8ad–0x8b2
(rotation), 0x8c0 (speed random), 0x8d8–0x8f4 (anim loops), 0x984, 0x998,
0x9ec–0x9ef (fluctuation).

## Function map
Renamed in Ghidra (prefix `eft_`; BotW glue `botw_`, `gsys_`):
`0x03b5f62c` EmitterResource_Setup, `0x03b61e98` Resource_InitEmitterResource,
`0x03b5af5c` EmitterSet_Initialize, `0x03b5abc0` EmitterSet_CreateEmitter,
`0x03b579cc` Emitter_Initialize, `0x03b57174` Emitter_AllocParticleBuffers,
`0x03b56a60` Emitter_RandomizeLocalMatrix, `0x03b57e2c` Emitter_ResetEmitInterval,
`0x03b6ba64` Emitter_Calc, `0x03b6ed38` Emitter_EmitParticles,
`0x03b6d628` Emitter_InitParticle, `0x03b6eae4` Emitter_InheritFromParentParticle,
`0x03b6ae7c` Emitter_CalcParticlesCpu, `0x03b5d9b0` Particle_CalcCpu,
`0x03b6b724` Emitter_UpdateDynamicUbo, `0x03b75734` EmitterAnim_Eval,
`0x03b5bc84` Anim8Key_Eval, `0x03b5b8f8` FieldAnim_Eval, `0x03b5de40/0x03b5e234/ 0x03b5e754/0x03b5ec74` CalcParticle{Scale,Color0,Color1,Rotate}Cpu,
`0x03b6f32c..0x03b72eb8` Emit_* (16 volumes), `0x03b5beb4..0x03b69810` Field_*,
`0x03b5a348` EmitterSet_Calc, `0x03b5a8ac` EmitterSet_Draw,
`0x03b58d34` Renderer_DrawEmitter, `0x03b5883c` Renderer_DrawSortedParticles,
`0x03b5f5ac` SetRenderState, `0x03b66fbc/0x03b67224` System sort/draw,
`0x03b59550` EmitterSet_SetMtx, `0x03b5f06c` Random_InitVecTables,
`0x03b639f4` Shader_BindAttributes, `0x0387b8d0` botw_EmitterCalcLodCallback,
`0x039fcde8` gsys_PtclMgr_DrawPath.

## Open questions
1. Draw path → render pass: which BotW render step draws paths 0, 6, 7, 8,
   20, 21 … (and therefore before/after translucents, distortion, post-fx).
   Needs the render-step map around `0x039bd008`/`0x039bd3e0` callers.
2. Static UBO field names (shader side) and the semantics of flag bits
   `0xa5d/0xa5e/0xa5f`, `0x8ac`, `0x8b3/0x8b4`, low nibble of `0x9ef`,
   pattern types 1–4.
3. GPU calc position formula, FRND (`0x03b73144`), FCLN (`0x03b69810`), FSPN
   and FMAG strengths — the CPU versions exist but were only partly read; the
   GPU versions are in the shaders.
4. Box fill / sphere fill velocity details, sphere latitude axis table
   (`res+0x83e` 0..5 → which axis), equal-division point tables
   (`0x103beab4..`) — data is in the RPX, not yet dumped.
5. Custom shader callbacks (`res+0x92c` ids 3/4, CSDP) and what BotW writes
   into sysCustomShaderUniformBlock.
6. View UBO layout (0x140 bytes) — the game builds it in `0x039fcde8`.
7. Where the emitter frame/counter are zeroed (assumed 0 at creation).

Tool: `tools/research/ptcl_fields.py` (`hist <off> <fmt>`, `find`, `attrs`,
`attr <MAGIC>`, `dump <emitter>`) surveys raw fields over GameResident and all
actor effect files; it reads the dump in place and caches a pickle outside the
repo (`PTCL_CACHE`).

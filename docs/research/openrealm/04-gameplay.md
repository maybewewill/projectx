# OpenRealm Warcraft III Gameplay Simulation: Behaviour Reference

Repo (read-only): `/home/user/corepunch/open-realm`. Docs: `docs/games/warcraft-3/`, `docs/wc3-data-model.md`. Code: `games/warcraft-3/game/`, `server/sv_routing.c`, `common/shared.h`.

Caveat: OpenRealm states retail numbers come from ROC/TFT data, uses Warsmash as an independent reference, and leaves some retail behaviour unrecovered. "Documented" below means OpenRealm documents it, not that it is verified retail behaviour.

---

## 1. Attack and damage math

Sources: `docs/games/warcraft-3/attack-damage.md`, `docs/wc3-data-model.md` (Combat Damage Model), `games/warcraft-3/game/skills/s_attack.c`, `games/warcraft-3/game/g_main.c`.

**Three-layer attack state** (`unitAttack_t`):
- `damageBase` + `numberOfDice`/`sidesPerDie`: permanent range. Displayed min = `base+N`, max = `base+N*S` (floored at 0).
- `temporaryDamageBonus`: item bonus (`AIat`), added to rolls, not to base.
- `permanentDamageBonus`: upgrade/hero ledger (`ratx`, `ratd`) so Hero recomputation does not erase it.

**Roll** (`s_attack.c:168`, `ai_rolldamage1`): `raw = base + sum(rand()%S+1 per die) + temporaryBonus`. Zero-sided die counts as +1. Uses process `rand()`, not a seeded RNG (documented gap).

**Type table**: loaded from Misc `DamageBonus{Normal,Pierce,Siege,Chaos,Magic,Hero,Spells}` (`g_main.c:373-413`), with a hardcoded fallback `g_default_damage_table` (`s_attack.c:302-313`). Defense columns: small, medium, large, fort, normal, hero, divine, none. Rows (attack type: values):
- normal: 1.00 1.50 1.00 0.70 1.00 1.00 0.05 1.00
- pierce: 2.00 0.75 1.00 0.35 1.00 0.50 0.05 1.50
- siege: 1.00 0.50 1.00 1.50 1.00 0.50 0.05 1.50
- spells: 1.00 1.00 1.00 1.00 1.00 0.70 0.05 1.00
- chaos: all 1.00
- magic: 1.25 0.75 2.00 0.35 1.00 0.50 0.05 1.00
- hero: 1.00 1.00 1.00 0.50 1.00 1.00 0.05 1.00
- none: all 1.00

**Armor**: `K = Misc.DefenseArmor` (stock 0.06, `g_main.c:417`), `A = G_UnitArmorValue`.
- `A >= 0`: `mult = 1/(1+K*A)`
- `A < 0`: `mult = 2 - (1-K)^(-A)`

**Final** (`s_attack.c:320-360`): `dmg = base * typeMult`; if target Ethereal, physical returns 0 and magic/spells get x1.66; then `dmg *= armorMult`; `(int)dmg`, minimum 1 when base > 0. The Ethereal branch is in code but not described in the armor section of the doc.

**Armor ledger**: `permanent_armor_bonus` (research `rarm`) and `temporary_armor_bonus` (items `AIde`) are separate from base, so Agility recomputation preserves them. Timed `Bdef` layers on top.

**Hero math**: primary attribute adds `Misc.StrAttackBonus` (stock 1.0) to damage. Agility attack speed: `clamp(agi*AgiAttackSpeedBonus, -0.90, +4.00)`, divisor `1+bonus`, applied to damage point and recovery. Agility armor uses `AgiDefenseBonus` (stock 0.3). Limitation: Hero base vs bonus attributes are not split, so primary-stat item bonuses fold into the permanent range.

**Timing and range** (`attack-damage.md`, `group-attack-chase-gaps.md`): `ua1c` cooldown (starts at swing), `udp1` damage point, `ubs1` backswing, `ua1r` range, `RngBuff` range buffer (only while cooldown runs, and only for stationary/chase), `uamn` min range. At damage point the true unbuffered range is rechecked; a target escaping in windup cancels the launch. Recovery waits for `max(cooldown - damagePoint, backswing)`. Building targets are measured to the pathing footprint, unit targets by collision radius.

**Projectiles**: basic ranged attacks are `MOVETYPE_FLYMISSILE` entities. Type is snapshotted at launch; armor applies at impact. Flight is a straight 3D line (no authored arc). Defend (`Adef`, `Misc.DefendDeflection`) can retarget a basic missile once back to its owner (`projectile_reflected`); a building-source success consumes it. Artillery (`WPN_ARTILLERY`): `minRange` is a real dead zone; target point snapshotted at the damage point; splash bands (`Farea/Harea/Qarea`, `Hfact/Qfact`) snapshotted at launch.

**Hit order** (`S_ResolveAttackHit`): Evasion, then Curse miss, damage bonuses, then Bash. `S_AttackHitAllowed` applies the Ethereal gate to secondary victims. `T_Damage` ignores zero-life targets; `unit_die` is one-shot.

**Target legality**: `UnitWeapons` `targs1` decodes to a targetflag mask enforced by `S_AttackCanTarget()`. Auto-acquire: mobile units acquire anywhere in acquisition range; `AI_IMMOBILE` units only acquire what they can already hit.

**Documented gaps (attack)**: pre-damage and post-damage listener architecture (crit, bash, orbs, Wind Walk); generic low-ground miss/evasion; attack-type vs damage-type split; `MSPLASH`, `MBOUNCE`, `MLINE`/`ALINE` damage; full Attack 1/2 weapon priority; seeded combat RNG; non-Agility attack-speed buffs; attack-facing gate for windup; projectile arc and post-impact death presentation.

---

## 2. Movement and pathfinding

Sources: `docs/games/warcraft-3/pathfinding.md`, `group-attack-chase-gaps.md`, `naval-movement.md`, `docs/wc3-data-model.md` (Pathfinding and Collision). Code: `games/warcraft-3/game/skills/s_move.c`, `server/sv_routing.c`.

**Layering**: `order/behaviour -> target + interaction range -> routing -> collision-aware step`. `s_move.c` does per-tick steering and block-and-slide. `sv_routing.c` does static line tests, connectivity and flow fields. Routing never changes a target.

**Algorithm: hybrid**
- Open ground: collision-sized line test (no search).
- Nearby obstacle: bounded A*, max 48 pathing cells away, 2,048 expansions (`sv_routing.c:118-119`, `CM_FindPathWaypoint` at `:1077`). Returns the farthest waypoint with a clear collision-sized line.
- Long/shared route: destination-rooted reverse SPFA integration field, cached by (adjusted target cell, collision radius, pathing mask), 4 LRU entries, generation-stamped handles. Built incrementally by `CM_ProcessPathJobs` each frame. Budget cvar `wc3_path_work_budget`, default 65536, clamped 256..65536 (`WC3_PATH_WORK_BUDGET` define and `G_RunFrame`, `g_main.c`).
- Dynamic units: swept-circle move-time validation, block-and-slide, speed-priority give-way. Not baked into routes.

**Grid and sizes**: 32-unit pathing cells. Unit collision is `ucol` (e.g. Peasant 16), used as `radius/32` cells. Buildings block by their baked pathing-texture footprint, not by `ucol`. Trees are footprint-only. Diagonal steps are rejected when either side cell of the corner is blocked, in lines, fields and steps alike.

**Movement class (`movetp`)**:

| movetp | static blocked when | dynamic domain |
|---|---|---|
| foot, horse, hover | nowalk | ground |
| fly | nofly | air |
| float | nowater | sea |
| amph | nowalk AND nowater (REQUIRE_ALL) | ground + sea |

Red pathing-texture pixels set nowalk and nowater; green sets nofly. A walkable bridge clears nowalk over its lane but not nowater. Unknown movetp is movement-disabled.

**Dynamic collision**: units block each other when their domain masks overlap. Structures always block ground/sea precisely, in addition to the static footprint. Flyers ignore ground units. Command-time obstacles are requester-aware.

**Turn before propulsion**: `propWin` (`uprw`, degrees, converted to radians at spawn) gates translation. Turning happens every tick; `unit_moveindirection()` only translates when facing error is strictly inside the window. Zero means turn in place (`s_move.c:596-608`). Shared by all movers.

**Moving-target refresh**: long-route field is reused until the target moves more than 10% of mover-to-target distance and the field is 400 ms old (64-unit bound without a mover).

**Interaction endpoints**: Attack chases use the attacker's collision radius. `flow_goal_reached` lets the behaviour steer at the real target. Gold mine approach is static-only (live units ignored). Return uses the innermost collision-safe ring around the footprint. Lumber fallback (`s_harvest_lumber.c`, not the router) reselects a tree when the clicked one is unreachable. Move, Patrol and Attack-move never retarget on routing failure.

**Plain Move**: a distant temporary block does not cancel the order. An unreachable click resolves to `CM_ClosestReachablePointForRadius()`.

**Scripted repositioning**: `SetUnitPosition` runs a 64-unit square spiral (up to 300 candidates) for a legal point. `SetUnitX/Y` stay raw.

**Documented gaps (movement)**: exact retail route algorithm unknown (A* and SPFA are "behaviourally supported approximations"; the Game.dll audit does not establish the accelerator). No central surround-slot allocator; per-unit range only. Missing tests: group surrounds at 4/12/24/50, moving-target chase through obstacles, ground-to-air profile transform, save during a real chase, 100 vs 20 battle cost. Cargo unload with no legal point places the passenger at the transport (retail unresolved).

---

## 3. Game loop timing and order queue

Sources: `docs/architecture/server.md`, `docs/games/warcraft-3/order-queue.md`, `pause-and-modal-ui.md`, `time-of-day.md`.

**Tick**: `FRAMETIME = 100` ms (`common/shared.h:60`), so 10 Hz. `SV_Frame` runs at most one simulation step per call. If it is more than one tick behind, the deadline is clamped to realtime. Each step: `SV_RunGameFrame()` (`framenum++`, `time += FRAMETIME`, `ge->RunFrame()`), then snapshots. Pause freezes `sv.time`/`framenum` and sends only keep-alives. Resume rebases the deadline, so paused time is not replayed. Continuous effects scale by `FRAMETIME/1000` (e.g. regen, movement distance). Timers are in `FRAMETIME` units so they survive save/load.

**`G_RunFrame` order** (`games/warcraft-3/game/g_main.c:1120`): time sync; scripts, time of day, timers, events, JASS events, timer dialogs; `G_RunClients` (camera); `G_RunEntities`; `CM_ProcessPathJobs` (resumable fields, after entities); UI and client snapshot writers; `G_SolveCollisions`; `G_RunDeferredFrees` (JASS RemoveUnit retires here); fog update and deltas.

**Order queue** (`order-queue.md`, `g_local.h:659`):
- Per-unit pending FIFO, separate from the current behaviour and from producer training queues.
- Client sends `smart/point/select/button ... queue`. The server gates it on `menu_t.supports_order_queue`.
- Rules: `queue=false` clears the FIFO and starts now. `queue=true` with an active behaviour (`umove_t.ability` non-null) appends. `queue=true` while idle starts now.
- Entries store the order name, target kind, point, and entity as number + `spawn_time` (never a pointer, so slot reuse fails closed).
- Bounded ring of 16 (an OpenRealm cap, not a Warsmash limit). Overflow silently rejects the newest.
- Death and normal Move/Attack/Smart clear it. Stop guard return sits below the FIFO.
- Issued-order events fire at submission, not execution.

**Ability scheduling** (`ability-system.md`, `ability-procedures.md`, `ability-implementation.md`):
- The shared spell path does resource, cooldown, range, target and cast-phase work, then sends `A_VALIDATE`/`A_EXECUTE`. A handled `false` is final.
- Persistent behaviour uses `AB_UPDATE`. The update roster is deduplicated by procedure, so each shared handler runs once per unit tick.
- Channels/waves use `AB_CHANNEL` plus a thinker; the caster incarnation (spawn time) is checked on active/end/cleanup.
- Animation callbacks stay ordinary C callbacks; `umove_t.proc` records the owning ability.
- Documented gap: no generic cast-presentation scheduler, and no global combat-animation scheduler (animation is still driven by the model-sequence state machine).

---

## 4. Ability architecture: SLK to code

Sources: `docs/games/warcraft-3/ability-system.md`, `ability-procedures.md`, `ability-implementation.md`, `ability-and-item-effects.md`, `ability-verification-review.md`, `docs/wc3-data-model.md`. Code: `games/warcraft-3/game/skills/s_skills.c` (registry `abilitylist`), `skills/s_*.c` (owners), `skills/s_spell.c`.

**Pipeline**:
```
AbilityStrings.txt -> name, tooltip, hotkey
AbilityData.slk    -> area, range, duration, cost, cooldown, targs, DataA-I, rawcodes
DDX/SLK loaders    -> AbilityData_t, abilityLevel_t
registry (s_skills.c) -> ability_t {classname, proc, flags, target_type, orders}
runtime            -> damage, status, movement, combat, summons, morphs, presentation
```
- The registry is generated from `*AbilityStrings.txt` (`tools/generate_ability_registry.py`). Verify with `tools/check_wc3_ability_registry.py`.
- Procedures are named after TFT `CAbility*` classes (look them up in `tft-ability-classes.txt`). The class tree is an offline reference only; there is no runtime inheritance. Delegation is an explicit call to the parent on unhandled messages.
- Flags (`AB_SPELL`, `AB_CHANNEL`, `AB_TOGGLE`, `AB_AUTOCAST`, `AB_COMMAND`, `AB_ITEM`, `AB_UPDATE`, `AB_INNATE`) are independent policy, not ancestry.
- Identity: `abilityitem_t { code, ability }` lets aliases share a procedure. Costs, ranks, buffs and cooldown grouping use the authored rawcode. Cooldowns and saves never key on procedure pointers.

**Data access** (never copy tables into C): `S_SpellData(code, level, idx)` for DataA-I; `S_SpellDataId` for rawcode fields; `S_SpellNumber` for Area/Rng; `S_SpellDuration` for Dur/HeroDur; `G_AbilityLevel(FS_SLKKey(name), level)` for a whole row. ROC `Data<level><slot>` and TFT `Data<slot-letter><level>` both map to `abilityDataRow_t.data[level][slot]`.

**Map overrides**: `war3map.w3a` edits build `AbilityData_t` rows (`G_SetMapAbilityOverrides`). Covered: `alev/arlv/alsk/apri`, leveled `amcs/acas/adur/ahdu/acdn/aare/aran/atar`, DataA-I. Full AbilityMetaData coverage is still follow-up.

**Data set**: `fs_expansion` (RoC/TFT) and W3I `gameDataSet` choose the `Custom_V0/1` or `Melee_V0/1` overlay. Lookup: map archive, then prefix, then root. Map load reloads all sheet and ability caches as one operation.

**Pitfalls the docs record**: `targs` without friend/enemy/neutral allows enemies. `BuffID=Bams,Bam2` is two tokens and `unit_addtimedstatus` reads only the first four characters. `code=` aliases can differ (Anti-Magic Shell `Aams` vs `Aam2`). ROC may omit `BuffID` while the strings name the buff.

**Coverage**: 193 registered TFT class IDs at the verification review. Registration is not gameplay completeness.

---

## 5. Known gaps vs retail (as documented)

- **Combat**: see section 1 list. Also: MSPLASH/MBOUNCE/MLINE/ALINE; seeded RNG; Hero base vs bonus attributes.
- **Movement**: exact Blizzard routing unknown; surround solver absent; several listed test gaps (section 2).
- **Order queue** (`order-queue.md`): no queued-waypoint presentation channel; Patrol cannot take Shift points; no explicit spell queuing; queued construction does not reserve resources; no `onCancelFromQueue()`; no general `interruptable()` contract; `BlzQueue*OrderById` natives deferred.
- **Abilities** (`ability-and-item-effects.md`, `ability-verification-review.md`): generic `TargetArt`/`Targetattach` and buff lifetime (e.g. `BOeq`); EffectSound/EffectSoundLooped; Earthquake `Oeq4`/Final Area semantics; Ghost/Ghost Visible not in the invisibility path; typed SPELLS/COLD damage for some Jaina spells; MDX attachment tokens; `healing-spray.md` DataC missiles and DataE; `inferno.md` tree destruction; `dispel-magic.md` point-area filter; `guard-position.md` `SetUnitCreepGuard` not native.
- **Economy and heroes**: `neutral-shops.md` (Aall sharing, patron selection, out-of-range pawn, Hero/Tavern sales); `hero-abilities.md` (showEyeCandy ignored, single-Hero XP factors missing); `hero-revival.md` (name formatting, level overlay); `building-construction.md` (partial w3u merge, build-on-target grid exclusion); `inventory-and-items.md` (held-item cursor, general powerup acquisition, cooldownID/ignoreCD); `allies-menu.md` (resource trading not authoritative).
- **Presentation** (not core sim): `environmental-fog.md`, `minimap-markers.md`, `camera-zoom.md`, `land-mines.md`, `dark-portal.md`.
- **Doc inconsistency**: `pathfinding.md:104` says the production budget is 32,768 pops per frame, but the code default and `pathfinding.md:76-79` say 65,536. The code is authoritative.

---

## 6. Points to copy for another engine

1. Fixed 100 ms step; scale continuous effects by `dt`.
2. Damage: roll (base + dice + temp bonus), then type table, then armor `1/(1+K*A)` or `2-(1-K)^(-A)`, then min 1. Snapshot type at launch; apply armor at impact.
3. Keep base, permanent and temporary damage/armor as separate ledgers so recomputation cannot erase them.
4. Movement class maps to a static route predicate and a dynamic collision domain. Amphibious means walkable OR swimmable.
5. Route ground movers with their real radius; key caches by (target, radius, policy).
6. Budget route work; use bounded local A* for nearby obstacles and shared resumable fields for long routes.
7. Turn-before-propulsion with a per-unit window.
8. Queue entries store id + spawn generation, not pointers.
9. Ability registry: one procedure per behaviour, many rawcodes per procedure, state on units, data read by rawcode.
10. Treat section 5 as open work; do not claim parity where OpenRealm says it is approximate.

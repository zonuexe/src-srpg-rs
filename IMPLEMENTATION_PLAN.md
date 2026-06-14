# src-core Feature Parity Implementation Plan

## Executive Summary

This plan brings the Rust `src-core` port from its current minimal state (~37 files, basic script interpreter) to feature parity with the C# SRC.Sharp reference (387 files, full SRPG engine). The plan is organized into **7 phases**, prioritized by:

1. **Blocking dependency** — what must exist before other features can work
2. **Playable battle experience** — what makes the game actually playable
3. **Test coverage** — what can be verified at each stage

---

## Current State Assessment

### What's Working (✅)
- **Script interpreter**: ~100 commands implemented, ~70 stubs (`event_runtime.rs`)
- **Data parsers**: `pilot.txt`, `unit.txt` (weapons parsed), `item.txt`, `map.txt`, `sp.txt`, `terrain.txt`
- **Basic combat prediction**: `combat::predict` with status effects
- **Movement**: Dijkstra range calculation
- **Turn/Stage management**: Phase cycling, auto-fire labels
- **UI scaffolding**: Command menus, dialog system, script overlay
- **Save/Load**: JSON serialization round-trip
- **36 integration tests** covering script commands

### Critical Gaps (⚠️)
- `UnitInstance` is a **minimal struct** (165 lines) — no weapon runtime state, no ability system, no condition lifetime, no feature activation, no multi-pilot support, no item slots
- `PilotData` is **static only** — no `PilotInstance` with runtime stats (level, exp, sp, morale, plana, skills)
- `ItemData` is **static only** — no `ItemSlot` system, no equipment validation
- `Combat` is **prediction only** — no actual attack execution (hit roll, damage application, counter-attack, support attack/guard)
- `Statuses` are **just strings** — no `Condition` system with lifetimes, effects, or proper management
- **No AI** — `run_ai_phase` is a placeholder
- **No standalone expression evaluator** — functions are embedded in `event_runtime.rs`
- **No effect system** — visual effects are stubs

---

## Phase 1: Unit Runtime Model Foundation

**Goal**: Make `UnitInstance` functional enough for real battles. This is the **critical path** — everything else depends on it.

**Duration estimate**: 4-6 weeks
**Blocking**: Phases 2-7

### 1.1 Create `UnitWeapon` Runtime Struct
**Complexity**: Medium
**Files**: `src/unit_weapon.rs`, modify `src/unit_instance.rs`

Create a runtime weapon struct that tracks per-instance state:
```rust
pub struct UnitWeapon {
    pub weapon_data_name: String,  // references WeaponData in GameDatabase
    pub bullet_remaining: i32,      // consumed bullets (not shared across instances!)
    pub en_consumed_this_battle: i32, // EN consumed this attack
    pub is_disabled: bool,          // disabled by conditions/features
}
```

**Success criteria**:
- `UnitInstance` has `weapons: Vec<UnitWeapon>`
- `SetBullet` command sets `bullet_remaining` on the instance, not the static data
- `Weapon` command in `.eve` creates both `WeaponData` (static) and populates `UnitInstance.weapons`
- Unit tests: bullet consumption, EN consumption, disabled flag

**Verification**:
- `cargo test -p src-core` passes
- New test: `weapon_bullet_consumption_is_per_instance`
- New test: `setbullet_modifies_instance_not_data`

---

### 1.2 Create `UnitAbility` Runtime Struct
**Complexity**: Medium
**Files**: `src/unit_ability.rs`, modify `src/unit_instance.rs`

Create runtime ability tracking:
```rust
pub struct UnitAbility {
    pub ability_data_name: String,
    pub is_available: bool,
    pub en_consumed: i32,
    pub range: i32,
}
```

**Success criteria**:
- `UnitInstance` has `abilities: Vec<UnitAbility>`
- `UseAbility` command (currently stub) can check availability
- Ability EN consumption is tracked per-instance

**Verification**:
- New test: `ability_availability_checks_en_and_conditions`

---

### 1.3 Create `Condition` System
**Complexity**: High
**Files**: `src/condition.rs`, modify `src/unit_instance.rs`

Replace `Vec<String>` statuses with a proper condition system:
```rust
pub struct Condition {
    pub name: String,
    pub lifetime: i32,  // turns remaining (-1 = permanent)
    pub level: i32,     // strength level
    pub data: String,   // extra data
}

pub enum ConditionEffect {
    AttackDisabled,
    MoveDisabled,
    DefenseDown { amount: i32 },
    DamageOverTime { amount: i32 },
    // ... etc
}
```

**Success criteria**:
- `UnitInstance.statuses` becomes `conditions: Vec<Condition>`
- `SetStatus` creates a `Condition` with proper lifetime
- `UnsetStatus` removes by name
- `begin_phase` decrements `lifetime` and removes expired conditions
- `ConditionEffect` enum defines gameplay effects
- Combat system reads conditions for hit/damage modifiers

**Verification**:
- Existing tests `status_morale.rs` still pass
- New test: `condition_lifetime_decrements_on_phase_change`
- New test: `condition_attack_disabled_prevents_attack`
- New test: `condition_move_disabled_prevents_move`

---

### 1.4 Create `Feature` Activation System
**Complexity**: Medium
**Files**: `src/feature.rs`, modify `src/unit_instance.rs`

The `UnitData.features: Vec<(String, String)>` is already parsed. Add runtime activation:
```rust
pub struct ActiveFeature {
    pub name: String,
    pub value: String,
    pub is_active: bool,  // checked against pilot skills, conditions, etc.
}
```

**Success criteria**:
- `UnitInstance` has `active_features: Vec<ActiveFeature>`
- `IsAvailable(unit, feature)` function works correctly
- Features like "格闘強化", "射撃強化" modify combat stats
- `Unit.status_update()` recalculates feature activation

**Verification**:
- New test: `feature_activation_checks_pilot_skills`
- New test: `feature_combat_modifiers_apply`

---

### 1.5 Create `PilotInstance` Struct
**Complexity**: High
**Files**: `src/pilot_instance.rs`, modify `src/db.rs`, `src/unit_instance.rs`

Create a runtime pilot with mutable stats:
```rust
pub struct PilotInstance {
    pub pilot_data_name: String,  // references PilotData
    pub id: String,               // unique ID (like UnitInstance.uid)
    pub level: i32,
    pub total_exp: i32,
    pub sp_remaining: i32,
    pub morale: i32,
    pub plana: i32,
    pub infight: i32,      // modified by level, skills, items
    pub shooting: i32,
    pub hit: i32,
    pub dodge: i32,
    pub technique: i32,
    pub intuition: i32,
    pub skills: Vec<ActiveSkill>,
    pub is_main_pilot: bool,
    pub is_support: bool,
    pub support_index: i32,
}
```

**Success criteria**:
- `GameDatabase` has `pilot_instances: Vec<PilotInstance>`
- `Place` command creates both `UnitInstance` and `PilotInstance`
- `Ride` command links `PilotInstance` to `UnitInstance`
- `ReplacePilot` swaps `PilotInstance` references
- `LevelUp` modifies `PilotInstance.level` and recalculates stats
- `ExpUp` accumulates into `PilotInstance.total_exp`
- `Skill` function reads from `PilotInstance.skills`
- `SP` function returns `PilotInstance.sp_remaining`
- `Plana` function returns `PilotInstance.plana`

**Verification**:
- Existing tests `pilot_function.rs` still pass
- New test: `pilot_instance_creation_on_place`
- New test: `level_up_increases_pilot_stats`
- New test: `ride_links_pilot_to_unit`
- New test: `replace_pilot_preserves_unit_stats`

---

### 1.6 Integrate Multi-Pilot Support into `UnitInstance`
**Complexity**: Medium
**Files**: `src/unit_instance.rs`

```rust
pub struct UnitInstance {
    // ... existing fields ...
    pub pilot_ids: Vec<String>,  // main + sub + support pilots
    // pilot_name becomes derived from main pilot
}
```

**Success criteria**:
- `UnitInstance` supports 1 main pilot + N sub-pilots + N support pilots
- `Pilot(unit, N)` function returns Nth pilot
- `CountPilot(unit)` returns total pilots
- Combat uses main pilot for attack calculations, sub-pilots for support

**Verification**:
- New test: `multi_pilot_unit_counts_correctly`
- New test: `sub_pilot_support_attack_bonus`

---

### 1.7 Create `ItemSlot` System
**Complexity**: Medium
**Files**: `src/item_slot.rs`, modify `src/unit_instance.rs`

```rust
pub enum SlotType {
    RightHand,
    LeftHand,
    RightShoulder,
    LeftShoulder,
    Body,
    Head,
    Item,  // general item slot
}

pub struct ItemSlot {
    pub slot_type: SlotType,
    pub equipped_item: Option<String>, // ItemData name
    pub is_fixed: bool,  // cursed/forced equipment
}
```

**Success criteria**:
- `UnitInstance` has `item_slots: Vec<ItemSlot>`
- `Item` command validates slot compatibility
- `Equip` checks `ItemData.part` against `SlotType`
- Two-handed weapons occupy both hand slots
- Fixed items cannot be removed
- `RemoveItem` frees the slot
- `ExchangeItem` swaps between slots/units

**Verification**:
- Existing tests `item_equip.rs` still pass
- New test: `two_handed_weapon_occupies_both_hands`
- New test: `fixed_item_cannot_be_removed`
- New test: `item_slot_validation_rejects_wrong_part`

---

### 1.8 Implement `UnitInstance::update()` for Stat Recalculation
**Complexity**: High
**Files**: `src/unit_instance.rs`, `src/db.rs`

This is the **heart** of the unit runtime model. When anything changes (pilot levels up, item equipped, condition applied, feature activated), stats must be recalculated.

**Recalculation order** (matching C# `Unit.Update()`):
1. Base stats from `UnitData`
2. Apply rank-up modifications
3. Build feature list (unit data + items + pilots)
4. Resolve pilot skill bonuses (格闘UP, 射撃UP, etc.)
5. Re-validate required skills for features/weapons/abilities
6. Calculate effective HP, EN, Armor, Mobility, Speed
7. Update weapon data (power, range, EN cost)
8. Update ability data
9. Update terrain adaptation
10. Update resistances/weaknesses

**Success criteria**:
- `UnitInstance` has `update(&mut self, db: &GameDatabase)` method
- Called automatically after: `Place`, `Ride`, `Item`, `RemoveItem`, `LevelUp`, `SetSkill`, `Transform`, `Combine`, `Split`
- `effective_max_hp()` uses updated value, not just `UnitData.hp`
- `effective_armor()` includes item + feature + pilot bonuses

**Verification**:
- New test: `update_recalculates_after_item_equip`
- New test: `update_recalculates_after_level_up`
- New test: `update_recalculates_after_condition_change`
- New test: `effective_stats_include_all_bonuses`

---

### 1.9 Implement Weapon Availability Checks
**Complexity**: Medium
**Files**: `src/unit_weapon.rs`, `src/unit_instance.rs`

```rust
impl UnitInstance {
    pub fn is_weapon_available(&self, weapon_idx: usize, db: &GameDatabase) -> bool {
        // Check: unit has acted, has enough EN, has bullets, 
        // morale requirement met, not disabled by conditions
    }
    
    pub fn weapon_range(&self, weapon_idx: usize, db: &GameDatabase) -> (i32, i32) {
        // Return (min, max) range considering features
    }
}
```

**Success criteria**:
- `has_acted` prevents weapon use
- EN check prevents use if `en_consumed > max_en - current_en`
- Bullet check prevents use if `bullet_remaining <= 0`
- Morale check prevents use if `morale < necessary_morale`
- Conditions like "攻撃不能" prevent use

**Verification**:
- New test: `weapon_unavailable_when_no_en`
- New test: `weapon_unavailable_when_no_bullets`
- New test: `weapon_unavailable_when_has_acted`
- New test: `weapon_unavailable_when_morale_too_low`

---

### 1.10 Implement Basic Attack Execution
**Complexity**: High
**Files**: `src/combat.rs` (major refactor), `src/unit_instance.rs`

Replace `combat::predict` with full attack execution:
```rust
pub struct AttackResult {
    pub hit: bool,
    pub damage: i64,
    pub is_critical: bool,
    pub target_destroyed: bool,
    pub counter_attack: Option<Box<AttackResult>>,
}

pub fn execute_attack(
    attacker: &mut UnitInstance,
    defender: &mut UnitInstance,
    weapon_idx: usize,
    db: &mut GameDatabase,
    rng: &mut impl Rng,
) -> AttackResult {
    // 1. Check weapon availability
    // 2. Roll for hit (random vs hit_chance)
    // 3. If hit: calculate damage
    // 4. Roll for critical
    // 5. Apply damage to defender
    // 6. Check if defender destroyed
    // 7. Consume EN/bullets on attacker
    // 8. Set attacker.has_acted = true
    // 9. If defender alive and in range: prompt counter-attack
}
```

**Success criteria**:
- Attack consumes weapon bullets/EN
- Attack sets `has_acted = true`
- Damage reduces defender HP (via `damage` field)
- Defender destroyed when `damage >= max_hp`
- Counter-attack executes if defender survives and has available weapon
- `total_exp` added to attacker when defender destroyed
- `Destruction <name>` label auto-fires

**Verification**:
- Existing tests `map_attack.rs` still pass
- New test: `attack_consumes_bullets`
- New test: `attack_sets_has_acted`
- New test: `attack_kills_target_when_damage_exceeds_hp`
- New test: `counter_attack_executes_when_defender_survives`
- New test: `attacker_gains_exp_on_kill`
- Integration test: full attack → counter-attack → kill flow

---

## Phase 2: Combat System Completion

**Goal**: Make battles fully playable with all combat mechanics.

**Duration estimate**: 3-4 weeks
**Depends on**: Phase 1

### 2.1 Implement Counter-Attack Logic
**Complexity**: Medium
**Files**: `src/combat.rs`

**Success criteria**:
- Counter-attack only if defender survived, has acted = false, has weapon in range
- Counter-attack uses defender's best available weapon
- Counter-attack also sets `has_acted = true` on defender
- Support guard can intercept counter-attack

**Verification**:
- New test: `counter_attack_only_with_surviving_defender`
- New test: `counter_attack_uses_best_weapon`
- New test: `counter_attack_sets_defender_has_acted`

---

### 2.2 Implement Support Attack / Guard
**Complexity**: High
**Files**: `src/combat.rs`, `src/unit_instance.rs`

**Success criteria**:
- Adjacent ally with "援護" skill can perform support attack (1/turn)
- Adjacent ally with "援護" skill can perform support guard (1/turn)
- Support attack uses ally's best weapon in range
- Support guard absorbs damage for target
- `support_attack_remaining` decremented on use

**Verification**:
- New test: `support_attack_triggers_with_adjacent_ally`
- New test: `support_guard_absorbs_damage`
- New test: `support_attack_limited_to_once_per_turn`

---

### 2.3 Implement Terrain Adaptation for Weapons
**Complexity**: Medium
**Files**: `src/combat.rs`, `src/data/unit.rs`

**Success criteria**:
- Weapon power reduced if terrain adaptation is poor (C/D/-)
- Weapon power bonus if terrain adaptation is good (S/A)
- Terrain adaptation string (4 chars) maps to terrain class

**Verification**:
- New test: `terrain_adaptation_reduces_weapon_power`
- New test: `terrain_adaptation_bonus_on_good_adaptation`

---

### 2.4 Implement Special Defense (回避, 防御, バリア, シールド)
**Complexity**: High
**Files**: `src/combat.rs`

**Success criteria**:
- Defender can choose: 回避 (dodge), 防御 (defend), バリア (barrier), シールド (shield)
- 回避: hit chance reduced by dodge stat
- 防御: damage halved
- バリア: absorbs damage up to barrier strength
- シールド: chance to nullify damage

**Verification**:
- New test: `dodge_reduces_hit_chance`
- New test: `defend_halves_damage`
- New test: `barrier_absorbs_damage_up_to_limit`
- New test: `shield_chance_to_nullify`

---

### 2.5 Implement Map Attack Shapes
**Complexity**: Medium
**Files**: `src/combat.rs`

**Success criteria**:
- `Ｍ全` (all): hits all units in range
- `Ｍ投L<n>` (throw): hits n-radius circle
- `Ｍ直` (straight): hits line from attacker
- `Ｍ拡` (spread): hits expanding area
- `Ｍ扇` (fan): hits扇形 area
- `Ｍ移` (move): attacker moves to target area
- `Ｍ線` (line): hits line between two points
- `識` (identify): distinguishes friend/foe

**Verification**:
- New test: `map_attack_all_hits_all_in_range`
- New test: `map_attack_fan_hits_correct_shape`
- New test: `map_attack_identify_skips_allies`

---

## Phase 3: Pilot Runtime Model

**Goal**: Make pilots fully functional with skills, SP, and level progression.

**Duration estimate**: 2-3 weeks
**Depends on**: Phase 1 (1.5)

### 3.1 Implement Pilot Skill System
**Complexity**: High
**Files**: `src/pilot_instance.rs`, `src/skill.rs`

**Success criteria**:
- `ActiveSkill` struct with name, level, is_available
- `Skill(pilot, name)` function returns skill level
- `IsSkillAvailable(pilot, name)` checks conditions
- Skills affect combat: 格闘UP, 射撃UP, 命中UP, 回避UP, 技量UP, 反応UP
- Skills affect movement: 水上移動, 空中移動
- Skills affect SP: SP消費減少, SPアップ

**Verification**:
- New test: `skill_level_returns_correct_value`
- New test: `skill_combat_modifiers_apply`
- New test: `skill_movement_modifiers_apply`

---

### 3.2 Implement Special Power (SP) System
**Complexity**: Medium
**Files**: `src/pilot_instance.rs`, `src/data/special_power.rs`

**Success criteria**:
- `SpecialPower` command checks SP cost against `PilotInstance.sp_remaining`
- SP cost reduced by "SP消費減少" skill
- SP effects applied as `Condition` on target unit
- One-turn SP effects cleared at `begin_phase`
- `RecoverSP` restores SP

**Verification**:
- New test: `special_power_consumes_sp`
- New test: `special_power_blocked_when_insufficient_sp`
- New test: `sp_cost_reduced_by_skill`
- New test: `one_turn_sp_cleared_at_phase_change`

---

### 3.3 Implement Pilot Level-Up and Stat Growth
**Complexity**: Medium
**Files**: `src/pilot_instance.rs`

**Success criteria**:
- `LevelUp` command increases `PilotInstance.level`
- Stat growth based on `PilotData` base stats + growth type
- `ExpUp` accumulates; level up at 100 exp per level
- `Level(pilot)` returns current level

**Verification**:
- New test: `level_up_increases_level`
- New test: `exp_up_accumulates`
- New test: `stat_growth_on_level_up`

---

### 3.4 Implement Pilot-Morale System Integration
**Complexity**: Low
**Files**: `src/pilot_instance.rs`, `src/unit_instance.rs`

**Success criteria**:
- `Morale(pilot)` returns `PilotInstance.morale`
- `IncreaseMorale` modifies `PilotInstance.morale`
- Morale affects weapon availability (necessary_morale)
- Morale affects SP availability (some SP require minimum morale)

**Verification**:
- New test: `morale_affects_weapon_availability`
- New test: `morale_affects_sp_availability`

---

## Phase 4: AI System (COM)

**Goal**: Make enemy AI functional for playable battles.

**Duration estimate**: 2-3 weeks
**Depends on**: Phase 1, Phase 2

### 4.1 Implement Basic Enemy AI
**Complexity**: High
**Files**: `src/ai.rs` (new), modify `src/app.rs`

**Success criteria**:
- `run_ai_phase` in `app.rs` calls AI for each enemy unit
- AI selects action: Move + Attack, or Wait
- AI targets nearest enemy unit
- AI uses best available weapon
- AI moves within movement range toward target

**Verification**:
- New test: `ai_moves_toward_nearest_enemy`
- New test: `ai_attacks_when_in_range`
- New test: `ai_waits_when_no_targets`

---

### 4.2 Implement AI Target Selection with Damage Expectation
**Complexity**: Medium
**Files**: `src/ai.rs`

**Success criteria**:
- AI evaluates all possible targets
- AI prefers targets with highest expected damage
- AI considers counter-attack risk
- AI considers support attack/guard availability
- AI considers terrain bonuses

**Verification**:
- New test: `ai_prefers_high_damage_target`
- New test: `ai_avoids_high_counter_risk`

---

### 4.3 Implement AI Movement (Pathfinding to Targets)
**Complexity**: Medium
**Files**: `src/ai.rs`, `src/movement.rs`

**Success criteria**:
- AI uses `movement::compute_range` to find reachable cells
- AI prefers cells that put it in attack range of target
- AI avoids cells with enemy support attack range
- AI considers terrain defense bonuses

**Verification**:
- New test: `ai_prefers_attack_range_position`
- New test: `ai_avoids_enemy_support_range`

---

## Phase 5: Expression System

**Goal**: Create a standalone expression evaluator matching C# SRC.Sharp's capabilities.

**Duration estimate**: 3-4 weeks
**Depends on**: Phase 1 (for unit/pilot functions)

### 5.1 Create Standalone Expression Parser/Evaluator
**Complexity**: High
**Files**: `src/expression/` (new directory)

**Architecture** (matching C#):
```
src/expression/
├── mod.rs          # public API
├── eval.rs         # core evaluation logic
├── value_type.rs   # ValueType enum (Undefined, String, Numeric)
├── operator.rs     # OperatorType enum + precedence
├── variable.rs     # variable resolution (local/global/sub-local)
├── var_data.rs     # VarData struct
└── functions/      # function implementations
    ├── mod.rs
    ├── math.rs
    ├── string.rs
    ├── unit.rs
    ├── pilot.rs
    ├── info.rs
    ├── list.rs
    └── other.rs
```

**Success criteria**:
- `Eval(expr)` function evaluates arithmetic expressions
- Operator precedence: `^` > `* /` > `+ -` > `&` > comparison > `Not` > `And` > `Or`
- Type coercion: numeric vs string comparison
- Short-circuit evaluation for `And`/`Or`
- `Like` operator for pattern matching

**Verification**:
- New test: `eval_arithmetic_expression`
- New test: `eval_operator_precedence`
- New test: `eval_string_concatenation`
- New test: `eval_comparison_with_coercion`
- New test: `eval_short_circuit_and_or`

---

### 5.2 Implement Function Registry Pattern
**Complexity**: Medium
**Files**: `src/expression/functions/mod.rs`

**Success criteria**:
- `IFunction` trait with `invoke()` method
- Function registry: `HashMap<String, Box<dyn IFunction>>`
- Functions registered at startup
- User-defined functions (event labels) callable via `Call()`

**Verification**:
- New test: `function_registry_lookup`
- New test: `user_defined_function_call`

---

### 5.3 Migrate Existing Functions from `event_runtime.rs`
**Complexity**: Medium
**Files**: `src/expression/functions/*.rs`, `src/event_runtime.rs`

**Success criteria**:
- All existing functions (`HP`, `MaxHP`, `EN`, `Morale`, `Exp`, `X`, `Y`, `Distance`, `Count`, `Exists`, `Random`, `Money`, `Turn`, `Phase`, `TerrainId`, `List`, `Llength`, `Lindex`, `Lsearch`, `Lsplit`, `Lremove`, `Replace`, `Min`, `Max`, `Abs`, `Len`, `Dir`, `RGB`, `IIF`, `StrCmp`, `Left`, `Right`, `Mid`, `InStr`, `InStrRev`, `String`, `Wide`, `LCase`, `UCase`, `Trim`, `Asc`, `Chr`, `Format`, `Nickname`, `Pilot`, `Item`, `Party`, `UnitID`, `PilotID`, `TextWidth`, `TextHeight`, `KeyState`, `Term`, `Area`, `IsDefined`, `IsAvailable`, `LSet`, `RSet`, `Skill`, `Level`, `SP`, `Plana`, `Relation`, `Info`, `Args`, `Not`, `Eval`, `Int`, `Round`, `RoundUp`, `RoundDown`, `Sqr`, `Sin`, `Cos`, `Tan`, `Atn`, `IsVarDefined`, `IsNumeric`) moved to expression system
- `event_runtime.rs` calls into expression system
- No regression in existing tests

**Verification**:
- All existing expression tests pass
- New test: `expression_system_integration`

---

## Phase 6: Command Completion

**Goal**: Implement remaining stub commands to reduce no-ops.

**Duration estimate**: 2-3 weeks
**Depends on**: Phase 1, Phase 5 (for expression-dependent commands)

### 6.1 Implement File I/O Commands
**Complexity**: Medium
**Files**: `src/event_runtime.rs`

**Commands**: `Open`, `Close`, `Read`, `Write`, `LineRead`, `CopyFile`, `RemoveFile`, `RenameFile`, `CreateFolder`, `RemoveFolder`

**Success criteria**:
- VFS (Virtual File System) already exists in `App.virtual_files`
- File I/O commands work with VFS
- `Dir(path, kind)` returns file list from VFS
- `EOF(handle)` checks end-of-file

**Verification**:
- New test: `file_io_read_write_roundtrip`
- New test: `dir_lists_vfs_files`

---

### 6.2 Implement Intermission Commands
**Complexity**: Medium
**Files**: `src/scene/intermission.rs`, `src/event_runtime.rs`

**Commands**: `CallIntermissionCommand`, `IntermissionCommand` (enhance existing)

**Success criteria**:
- Intermission menu shows all registered commands
- Selecting a command runs its `.eve` file
- "次のステージへ" advances to next stage
- Sub-command execution returns to intermission

**Verification**:
- Existing tests `intermission.rs` still pass
- New test: `intermission_subcommand_returns_to_menu`

---

### 6.3 Implement Effect Commands
**Complexity**: High
**Files**: `src/effect.rs` (new), `src/event_runtime.rs`, `src/script_overlay.rs`

**Commands**: `Effect`, `Explode`, `Sepia`, `Monotone`, `WhiteIn`, `WhiteOut`, `Water`, `ColorFilter`

**Success criteria**:
- `Effect` command queues visual effects
- Effects rendered via `ScriptOverlay`
- `FadeIn`/`FadeOut` enhanced with proper alpha transitions
- `Sepia`/`Monotone` apply color filters

**Verification**:
- New test: `effect_command_queues_overlay`
- New test: `fade_transition_changes_alpha`

---

## Phase 7: UI Abstraction & Polish

**Goal**: Create proper UI abstraction and complete status displays.

**Duration estimate**: 2-3 weeks
**Depends on**: Phase 1

### 7.1 Create UI Interface Layer
**Complexity**: Medium
**Files**: `src/ui/` (new directory)

**Architecture**:
```
src/ui/
├── mod.rs          # UI trait definitions
├── gui.rs          # IGUI trait
├── gui_map.rs      # IGUIMap trait
├── gui_screen.rs   # IGUIScreen trait
├── gui_status.rs   # IGUIStatus trait
└── play_sound.rs   # IPlaySound trait
```

**Success criteria**:
- Traits define all UI operations
- `src-web` implements these traits
- `App` holds `Box<dyn IGUI>` etc.
- No direct canvas manipulation in `src-core`

**Verification**:
- `src-core` compiles without `src-web`
- New test: `ui_trait_object_creation`

---

### 7.2 Implement Status Display
**Complexity**: Medium
**Files**: `src/ui/gui_status.rs`, `src-web/src/render.rs`

**Success criteria**:
- `ShowUnitStatus` command (currently stub) displays unit details
- Status panel shows: HP, EN, Armor, Mobility, Speed, Morale, SP
- Status panel shows: equipped items, active conditions, pilots
- Status panel shows: weapon list with availability

**Verification**:
- New test: `show_unit_status_displays_correct_data`

---

### 7.3 Implement Unit Detail View
**Complexity**: Low
**Files**: `src-web/src/render.rs`

**Success criteria**:
- Unit list scene (`scene/unit_list.rs`) shows full unit details
- Pilot list scene (`scene/pilot_list.rs`) shows full pilot details
- Clicking a unit in list shows detail panel

**Verification**:
- Manual QA: unit list displays correctly in browser

---

## Cross-Cutting Concerns

### WASM Compatibility
All code must compile to `wasm32-unknown-unknown`:
- No `std::fs` (use VFS)
- No threads (use single-threaded logic)
- No `std::time` (use `instant` crate or app-managed time)

### Save/Load Compatibility
- All new structs must be `Serialize` + `Deserialize`
- Save format version must be bumped when adding fields
- Migration logic for old save files

### Test Strategy Summary
| Phase | New Unit Tests | New Integration Tests | Manual QA |
|-------|---------------|----------------------|-----------|
| 1 | 25+ | 5+ | Browser combat smoke test |
| 2 | 15+ | 5+ | Full battle scenario |
| 3 | 10+ | 3+ | Pilot progression |
| 4 | 10+ | 3+ | AI vs AI battle |
| 5 | 15+ | 3+ | Expression edge cases |
| 6 | 10+ | 3+ | File I/O, intermission |
| 7 | 5+ | 2+ | UI responsiveness |

### Verification Steps (Per Phase)
1. **Build**: `cargo check --target wasm32-unknown-unknown`
2. **Test**: `cargo test -p src-core`
3. **Lint**: `cargo clippy --workspace --all-targets -- -D warnings`
4. **Integration**: Run `sparobo_load.rs` test — must reach battle with no script errors
5. **Manual**: Load `スパロボ戦記.zip` in browser, verify combat works

---

## Task Delegation Guide

Each task above is designed to be **independently delegable** to a sub-agent:

- **Single-file tasks** (1.1, 1.2, 1.3, 1.4, 1.7, 3.2, 3.3, 3.4, 4.1, 4.2, 4.3, 5.2, 6.1, 6.2, 6.3, 7.1, 7.2, 7.3): Can be worked on in parallel once Phase 1 core structs exist
- **Multi-file tasks** (1.5, 1.6, 1.8, 1.9, 1.10, 2.1-2.5, 3.1, 5.1, 5.3): Require coordination, should be sequential within the task
- **Foundation tasks** (1.1-1.4, 1.7): Must complete before dependent tasks start

### Recommended Delegation Order
```
Wave 1 (parallel): 1.1, 1.2, 1.3, 1.4, 1.7
Wave 2 (parallel): 1.5, 1.6, 1.8, 1.9
Wave 3: 1.10
Wave 4 (parallel): 2.1, 2.2, 2.3, 2.4, 2.5
Wave 5 (parallel): 3.1, 3.2, 3.3, 3.4
Wave 6 (parallel): 4.1, 4.2, 4.3
Wave 7 (parallel): 5.1, 5.2, 5.3
Wave 8 (parallel): 6.1, 6.2, 6.3
Wave 9 (parallel): 7.1, 7.2, 7.3
```

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Phase 1 is too large | Can split: 1.1-1.4 as "Data Structures", 1.5-1.6 as "Pilot Integration", 1.7-1.10 as "Combat Integration" |
| C# partial classes don't map cleanly to Rust | Use Rust modules + impl blocks; one file per C# partial |
| Save format breakage | Version field + migration functions |
| WASM performance | Profile after Phase 2; optimize hot paths |
| Test flakiness | Use deterministic RNG in tests; snapshot tests for complex flows |

---

## Success Metrics

- **Phase 1 complete**: `UnitInstance` has 500+ lines, supports multi-pilot, items, conditions, features, weapons, abilities
- **Phase 2 complete**: Full attack → counter-attack → support → kill flow works in tests
- **Phase 3 complete**: Pilot level-up, SP system, skills all tested
- **Phase 4 complete**: AI can win a battle against itself
- **Phase 5 complete**: All existing expression tests pass with new system
- **Phase 6 complete**: < 30 stub commands remaining (from ~70)
- **Phase 7 complete**: Browser UI shows unit/pilot details

**Final goal**: `sparobo_load.rs` test drives a full battle without script errors, with AI making meaningful decisions.

//! Knob slider system (SPEC §20) — 15 SPEC knobs + max_floors for v0.6.0.
//!
//! The three const tables (`KNOB_LABEL`, `KNOB_STEP`, `KNOB_DEFAULT`) are
//! the single source of truth. The `Knobs` struct, `Default`, serde
//! per-field defaults, and runtime helpers (`get`/`set`/`clamp`/`reset`)
//! all read from them, so SPEC §20 stays the contract and renumbering is a
//! one-line change.
//!
//! `#[serde(default = "fn")]` is on every field — an old save without a
//! `knobs` key deserializes to `Knobs::default()` without a migration step.
//!
//! `allow: SIZE_OK` — 16 named fields × 16 serde-default fns × 16-arm
//! matches in `get`/`set` are mechanical, required by serde derive, and
//! cannot be split without sacrificing the per-field `#[serde(default)]`
//! migration guarantee.

#![deny(rust_2018_idioms)]

use serde::{Deserialize, Serialize};

/// Strongly typed identifier for every knob in the slider UI.
///
/// Order is part of the public surface — the modal renders `0..KnobId::COUNT`
/// in declaration order, so renumbering is a UI change, not a data change.
/// The 16th entry (`MaxFloors`) is the only knob beyond SPEC §20; it controls
/// run length and is needed for v0.6.0.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[repr(u8)]
pub enum KnobId {
    HpStart = 0,
    MpStart = 1,
    FoodStart = 2,
    EnemySpeedMul = 3,
    EnemyHpMul = 4,
    EnemyAtkMul = 5,
    VisionRange = 6,
    ScalingFloor = 7,
    AutopilotMode = 8,
    AutopilotSpeed = 9,
    PacingRecovery = 10,
    RelicDensity = 11,
    GoldDensity = 12,
    MonsterDensity = 13,
    TrapDensity = 14,
    MaxFloors = 15,
}

impl KnobId {
    /// Number of knobs. Const so it can be used to size static arrays.
    pub const COUNT: usize = 16;

    /// Convert 0-based cursor index to a `KnobId`. Returns `None` for out-of-range.
    ///
    /// Used by the slider UI to map a `usize` cursor (the modal renders
    /// `0..KnobId::COUNT` in declaration order) back into a typed enum.
    /// The exhaustive match guarantees that adding a new knob forces an
    /// update here — impossible to forget, unlike `as` casts.
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::HpStart),
            1 => Some(Self::MpStart),
            2 => Some(Self::FoodStart),
            3 => Some(Self::EnemySpeedMul),
            4 => Some(Self::EnemyHpMul),
            5 => Some(Self::EnemyAtkMul),
            6 => Some(Self::VisionRange),
            7 => Some(Self::ScalingFloor),
            8 => Some(Self::AutopilotMode),
            9 => Some(Self::AutopilotSpeed),
            10 => Some(Self::PacingRecovery),
            11 => Some(Self::RelicDensity),
            12 => Some(Self::GoldDensity),
            13 => Some(Self::MonsterDensity),
            14 => Some(Self::TrapDensity),
            15 => Some(Self::MaxFloors),
            _ => None,
        }
    }
}

/// Display label (Korean-first) for each knob (SPEC §20 + UX-facing `max_floors`).
pub const KNOB_LABEL: [(KnobId, &str); KnobId::COUNT] = [
    (KnobId::HpStart, "시작 HP"),
    (KnobId::MpStart, "시작 MP"),
    (KnobId::FoodStart, "시작 식량"),
    (KnobId::EnemySpeedMul, "적 속도"),
    (KnobId::EnemyHpMul, "적 HP"),
    (KnobId::EnemyAtkMul, "적 공격"),
    (KnobId::VisionRange, "시야"),
    (KnobId::ScalingFloor, "층 스케일"),
    (KnobId::AutopilotMode, "자동 모드"),
    (KnobId::AutopilotSpeed, "자동 속도"),
    (KnobId::PacingRecovery, "회복 빈도"),
    (KnobId::RelicDensity, "유물"),
    (KnobId::GoldDensity, "골드"),
    (KnobId::MonsterDensity, "적 밀도"),
    (KnobId::TrapDensity, "함정"),
    (KnobId::MaxFloors, "최대 층"),
];

/// Per-knob (min, max, step). Step is the unit that the modal `'h'`/`'l'`
/// keys shift by. Float knobs use 0.05 (~5%); integer knobs use 1 (so vision
/// 3→4→5…) unless listed otherwise (autopilot speed step is 0.1;
/// max_floors step is 1).
pub const KNOB_STEP: [(KnobId, f32, f32, f32); KnobId::COUNT] = [
    (KnobId::HpStart, 20.0, 200.0, 5.0),
    (KnobId::MpStart, 0.0, 100.0, 5.0),
    (KnobId::FoodStart, 0.0, 20.0, 1.0),
    (KnobId::EnemySpeedMul, 0.5, 3.0, 0.05),
    (KnobId::EnemyHpMul, 0.0, 5.0, 0.05),
    (KnobId::EnemyAtkMul, 0.0, 3.0, 0.05),
    (KnobId::VisionRange, 3.0, 15.0, 1.0),
    (KnobId::ScalingFloor, 0.5, 2.0, 0.05),
    (KnobId::AutopilotMode, 0.0, 1.0, 1.0),
    (KnobId::AutopilotSpeed, 1.0, 5.0, 0.1),
    (KnobId::PacingRecovery, 0.5, 2.0, 0.05),
    (KnobId::RelicDensity, 0.0, 3.0, 0.05),
    (KnobId::GoldDensity, 0.0, 3.0, 0.05),
    (KnobId::MonsterDensity, 0.0, 2.0, 0.05),
    (KnobId::TrapDensity, 0.0, 2.0, 0.05),
    (KnobId::MaxFloors, 1.0, 20.0, 1.0),
];

/// Per-knob default. Values match SPEC §20 exactly. Float rounding kept to
/// cents (0.05) so storage is stable across compile targets.
///
/// `autopilot_mode = 0.0` corresponds to `FullAuto` per SPEC §20.
pub const KNOB_DEFAULT: [(KnobId, f32); KnobId::COUNT] = [
    (KnobId::HpStart, 100.0),
    (KnobId::MpStart, 50.0),
    (KnobId::FoodStart, 8.0),
    (KnobId::EnemySpeedMul, 1.0),
    (KnobId::EnemyHpMul, 1.0),
    (KnobId::EnemyAtkMul, 1.0),
    (KnobId::VisionRange, 8.0),
    (KnobId::ScalingFloor, 1.0),
    (KnobId::AutopilotMode, 0.0),
    (KnobId::AutopilotSpeed, 2.0),
    (KnobId::PacingRecovery, 1.0),
    (KnobId::RelicDensity, 1.0),
    (KnobId::GoldDensity, 1.0),
    (KnobId::MonsterDensity, 1.0),
    (KnobId::TrapDensity, 1.0),
    (KnobId::MaxFloors, 8.0),
];

fn default_for(id: KnobId) -> f32 {
    KNOB_DEFAULT
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, v)| *v)
        .expect("KNOB_DEFAULT must cover every KnobId")
}

fn range_for(id: KnobId) -> (f32, f32) {
    KNOB_STEP
        .iter()
        .find(|(k, _, _, _)| *k == id)
        .map(|(_, lo, hi, _)| (*lo, *hi))
        .expect("KNOB_STEP must cover every KnobId")
}

fn step_for(id: KnobId) -> f32 {
    KNOB_STEP
        .iter()
        .find(|(k, _, _, _)| *k == id)
        .map(|(_, _, _, s)| *s)
        .expect("KNOB_STEP must cover every KnobId")
}

fn knob_default_hp_start() -> f32 {
    default_for(KnobId::HpStart)
}
fn knob_default_mp_start() -> f32 {
    default_for(KnobId::MpStart)
}
fn knob_default_food_start() -> f32 {
    default_for(KnobId::FoodStart)
}
fn knob_default_enemy_speed_mul() -> f32 {
    default_for(KnobId::EnemySpeedMul)
}
fn knob_default_enemy_hp_mul() -> f32 {
    default_for(KnobId::EnemyHpMul)
}
fn knob_default_enemy_atk_mul() -> f32 {
    default_for(KnobId::EnemyAtkMul)
}
fn knob_default_vision_range() -> f32 {
    default_for(KnobId::VisionRange)
}
fn knob_default_scaling_floor() -> f32 {
    default_for(KnobId::ScalingFloor)
}
fn knob_default_autopilot_mode() -> f32 {
    default_for(KnobId::AutopilotMode)
}
fn knob_default_autopilot_speed() -> f32 {
    default_for(KnobId::AutopilotSpeed)
}
fn knob_default_pacing_recovery() -> f32 {
    default_for(KnobId::PacingRecovery)
}
fn knob_default_relic_density() -> f32 {
    default_for(KnobId::RelicDensity)
}
fn knob_default_gold_density() -> f32 {
    default_for(KnobId::GoldDensity)
}
fn knob_default_monster_density() -> f32 {
    default_for(KnobId::MonsterDensity)
}
fn knob_default_trap_density() -> f32 {
    default_for(KnobId::TrapDensity)
}
fn knob_default_max_floors() -> f32 {
    default_for(KnobId::MaxFloors)
}

/// All 16 knobs as a flat struct. `f32` storage, serde-transparent via RON,
/// `#[serde(default = "fn")]` on every field so an old save without a
/// `knobs` key loads as `Knobs::default()` without a migration step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knobs {
    #[serde(default = "knob_default_hp_start")]
    pub hp_start: f32,
    #[serde(default = "knob_default_mp_start")]
    pub mp_start: f32,
    #[serde(default = "knob_default_food_start")]
    pub food_start: f32,
    #[serde(default = "knob_default_enemy_speed_mul")]
    pub enemy_speed_mul: f32,
    #[serde(default = "knob_default_enemy_hp_mul")]
    pub enemy_hp_mul: f32,
    #[serde(default = "knob_default_enemy_atk_mul")]
    pub enemy_atk_mul: f32,
    #[serde(default = "knob_default_vision_range")]
    pub vision_range: f32,
    #[serde(default = "knob_default_scaling_floor")]
    pub scaling_floor: f32,
    #[serde(default = "knob_default_autopilot_mode")]
    pub autopilot_mode: f32,
    #[serde(default = "knob_default_autopilot_speed")]
    pub autopilot_speed: f32,
    #[serde(default = "knob_default_pacing_recovery")]
    pub pacing_recovery: f32,
    #[serde(default = "knob_default_relic_density")]
    pub relic_density: f32,
    #[serde(default = "knob_default_gold_density")]
    pub gold_density: f32,
    #[serde(default = "knob_default_monster_density")]
    pub monster_density: f32,
    #[serde(default = "knob_default_trap_density")]
    pub trap_density: f32,
    #[serde(default = "knob_default_max_floors")]
    pub max_floors: f32,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            hp_start: default_for(KnobId::HpStart),
            mp_start: default_for(KnobId::MpStart),
            food_start: default_for(KnobId::FoodStart),
            enemy_speed_mul: default_for(KnobId::EnemySpeedMul),
            enemy_hp_mul: default_for(KnobId::EnemyHpMul),
            enemy_atk_mul: default_for(KnobId::EnemyAtkMul),
            vision_range: default_for(KnobId::VisionRange),
            scaling_floor: default_for(KnobId::ScalingFloor),
            autopilot_mode: default_for(KnobId::AutopilotMode),
            autopilot_speed: default_for(KnobId::AutopilotSpeed),
            pacing_recovery: default_for(KnobId::PacingRecovery),
            relic_density: default_for(KnobId::RelicDensity),
            gold_density: default_for(KnobId::GoldDensity),
            monster_density: default_for(KnobId::MonsterDensity),
            trap_density: default_for(KnobId::TrapDensity),
            max_floors: default_for(KnobId::MaxFloors),
        }
    }
}

impl Knobs {
    /// Read a knob by id (returns `f32` to allow float-field access).
    pub fn get(&self, id: KnobId) -> f32 {
        match id {
            KnobId::HpStart => self.hp_start,
            KnobId::MpStart => self.mp_start,
            KnobId::FoodStart => self.food_start,
            KnobId::EnemySpeedMul => self.enemy_speed_mul,
            KnobId::EnemyHpMul => self.enemy_hp_mul,
            KnobId::EnemyAtkMul => self.enemy_atk_mul,
            KnobId::VisionRange => self.vision_range,
            KnobId::ScalingFloor => self.scaling_floor,
            KnobId::AutopilotMode => self.autopilot_mode,
            KnobId::AutopilotSpeed => self.autopilot_speed,
            KnobId::PacingRecovery => self.pacing_recovery,
            KnobId::RelicDensity => self.relic_density,
            KnobId::GoldDensity => self.gold_density,
            KnobId::MonsterDensity => self.monster_density,
            KnobId::TrapDensity => self.trap_density,
            KnobId::MaxFloors => self.max_floors,
        }
    }

    /// Write a knob by id. Caller is responsible for clamping; use
    /// `set_clamped` if you want bounds enforcement.
    pub fn set(&mut self, id: KnobId, value: f32) {
        match id {
            KnobId::HpStart => self.hp_start = value,
            KnobId::MpStart => self.mp_start = value,
            KnobId::FoodStart => self.food_start = value,
            KnobId::EnemySpeedMul => self.enemy_speed_mul = value,
            KnobId::EnemyHpMul => self.enemy_hp_mul = value,
            KnobId::EnemyAtkMul => self.enemy_atk_mul = value,
            KnobId::VisionRange => self.vision_range = value,
            KnobId::ScalingFloor => self.scaling_floor = value,
            KnobId::AutopilotMode => self.autopilot_mode = value,
            KnobId::AutopilotSpeed => self.autopilot_speed = value,
            KnobId::PacingRecovery => self.pacing_recovery = value,
            KnobId::RelicDensity => self.relic_density = value,
            KnobId::GoldDensity => self.gold_density = value,
            KnobId::MonsterDensity => self.monster_density = value,
            KnobId::TrapDensity => self.trap_density = value,
            KnobId::MaxFloors => self.max_floors = value,
        }
    }

    /// `set` with bounds enforcement (clamps to `KNOB_STEP` min/max for that
    /// id).
    pub fn set_clamped(&mut self, id: KnobId, value: f32) {
        let (lo, hi) = range_for(id);
        let clamped = value.max(lo).min(hi);
        self.set(id, clamped);
    }

    /// Clamp every field to its bound. Idempotent.
    pub fn clamp(&mut self) {
        for (id, _, _, _) in KNOB_STEP.iter().copied() {
            let v = self.get(id);
            let (lo, hi) = range_for(id);
            let clamped = v.clamp(lo, hi);
            self.set(id, clamped);
        }
    }

    /// Restore all knobs to SPEC §20 defaults.
    pub fn reset(&mut self) {
        *self = Knobs::default();
    }

    /// Restore only the knob at `id`.
    pub fn reset_one(&mut self, id: KnobId) {
        self.set(id, default_for(id));
    }

    /// Step of one knob (used by modal `'h'`/`'l'` keys).
    pub fn step_of(id: KnobId) -> f32 {
        step_for(id)
    }

    /// Min/max range of one knob.
    pub fn range_of(id: KnobId) -> (f32, f32) {
        range_for(id)
    }

    /// Default value of one knob.
    pub fn default_of(id: KnobId) -> f32 {
        default_for(id)
    }

    /// Display label of one knob (Korean-first).
    pub fn label_of(id: KnobId) -> &'static str {
        KNOB_LABEL
            .iter()
            .find(|(k, _)| *k == id)
            .map(|(_, v)| *v)
            .expect("KNOB_LABEL must cover every KnobId")
    }
}

#[cfg(test)]
mod tests {
    use crate::knobs::{KnobId, Knobs};

    // ─── 1. Defaults match SPEC §20 exactly (15 SPEC knobs + max_floors=8) ─────

    #[test]
    fn defaults_match_spec_section_20() {
        let k = Knobs::default();
        assert_eq!(k.hp_start, 100.0, "hp.start");
        assert_eq!(k.mp_start, 50.0, "mp.start");
        assert_eq!(k.food_start, 8.0, "food.start");
        assert_eq!(k.enemy_speed_mul, 1.0, "enemy.speed_mul");
        assert_eq!(k.enemy_hp_mul, 1.0, "enemy.hp_mul");
        assert_eq!(k.enemy_atk_mul, 1.0, "enemy.atk_mul");
        assert_eq!(k.vision_range, 8.0, "vision.range");
        assert_eq!(k.scaling_floor, 1.0, "scaling.floor");
        assert_eq!(
            k.autopilot_mode, 0.0,
            "autopilot.mode (0=FullAuto per SPEC §20)"
        );
        assert_eq!(k.autopilot_speed, 2.0, "autopilot.speed");
        assert_eq!(k.pacing_recovery, 1.0, "pacing.recovery");
        assert_eq!(k.relic_density, 1.0, "relic.density");
        assert_eq!(k.gold_density, 1.0, "gold.density");
        assert_eq!(k.monster_density, 1.0, "monster.density");
        assert_eq!(k.trap_density, 1.0, "trap.density");
        assert_eq!(k.max_floors, 8.0, "max_floors (16th knob, v0.6.0)");
    }

    // ─── 2. clamp() constrains every field to its [min, max] range ─────────────

    #[test]
    fn clamp_constrains_values_to_min_max_range() {
        let mut k = Knobs::default();

        // Underflow every field below its min, then clamp.
        k.set(KnobId::HpStart, -50.0);
        k.set(KnobId::MpStart, -10.0);
        k.set(KnobId::FoodStart, -1.0);
        k.set(KnobId::EnemySpeedMul, 0.0);
        k.set(KnobId::EnemyHpMul, -1.0);
        k.set(KnobId::EnemyAtkMul, -1.0);
        k.set(KnobId::VisionRange, 0.0);
        k.set(KnobId::ScalingFloor, 0.0);
        k.set(KnobId::AutopilotMode, -5.0);
        k.set(KnobId::AutopilotSpeed, 0.0);
        k.set(KnobId::PacingRecovery, 0.0);
        k.set(KnobId::RelicDensity, -1.0);
        k.set(KnobId::GoldDensity, -1.0);
        k.set(KnobId::MonsterDensity, -1.0);
        k.set(KnobId::TrapDensity, -1.0);
        k.set(KnobId::MaxFloors, 0.0);

        k.clamp();

        assert_eq!(k.get(KnobId::HpStart), 20.0, "hp_start min");
        assert_eq!(k.get(KnobId::MpStart), 0.0, "mp_start min");
        assert_eq!(k.get(KnobId::FoodStart), 0.0, "food_start min");
        assert_eq!(k.get(KnobId::EnemySpeedMul), 0.5, "enemy_speed_mul min");
        assert_eq!(k.get(KnobId::EnemyHpMul), 0.0, "enemy_hp_mul min");
        assert_eq!(k.get(KnobId::EnemyAtkMul), 0.0, "enemy_atk_mul min");
        assert_eq!(k.get(KnobId::VisionRange), 3.0, "vision_range min");
        assert_eq!(k.get(KnobId::ScalingFloor), 0.5, "scaling_floor min");
        assert_eq!(k.get(KnobId::AutopilotMode), 0.0, "autopilot_mode min");
        assert_eq!(k.get(KnobId::AutopilotSpeed), 1.0, "autopilot_speed min");
        assert_eq!(k.get(KnobId::PacingRecovery), 0.5, "pacing_recovery min");
        assert_eq!(k.get(KnobId::RelicDensity), 0.0, "relic_density min");
        assert_eq!(k.get(KnobId::GoldDensity), 0.0, "gold_density min");
        assert_eq!(k.get(KnobId::MonsterDensity), 0.0, "monster_density min");
        assert_eq!(k.get(KnobId::TrapDensity), 0.0, "trap_density min");
        assert_eq!(k.get(KnobId::MaxFloors), 1.0, "max_floors min");

        // Overflow every field past its max, then clamp.
        k.set(KnobId::HpStart, 9999.0);
        k.set(KnobId::MpStart, 9999.0);
        k.set(KnobId::FoodStart, 9999.0);
        k.set(KnobId::EnemySpeedMul, 9999.0);
        k.set(KnobId::EnemyHpMul, 9999.0);
        k.set(KnobId::EnemyAtkMul, 9999.0);
        k.set(KnobId::VisionRange, 9999.0);
        k.set(KnobId::ScalingFloor, 9999.0);
        k.set(KnobId::AutopilotMode, 9999.0);
        k.set(KnobId::AutopilotSpeed, 9999.0);
        k.set(KnobId::PacingRecovery, 9999.0);
        k.set(KnobId::RelicDensity, 9999.0);
        k.set(KnobId::GoldDensity, 9999.0);
        k.set(KnobId::MonsterDensity, 9999.0);
        k.set(KnobId::TrapDensity, 9999.0);
        k.set(KnobId::MaxFloors, 9999.0);

        k.clamp();

        assert_eq!(k.get(KnobId::HpStart), 200.0, "hp_start max");
        assert_eq!(k.get(KnobId::MpStart), 100.0, "mp_start max");
        assert_eq!(k.get(KnobId::FoodStart), 20.0, "food_start max");
        assert_eq!(k.get(KnobId::EnemySpeedMul), 3.0, "enemy_speed_mul max");
        assert_eq!(k.get(KnobId::EnemyHpMul), 5.0, "enemy_hp_mul max");
        assert_eq!(k.get(KnobId::EnemyAtkMul), 3.0, "enemy_atk_mul max");
        assert_eq!(k.get(KnobId::VisionRange), 15.0, "vision_range max");
        assert_eq!(k.get(KnobId::ScalingFloor), 2.0, "scaling_floor max");
        assert_eq!(k.get(KnobId::AutopilotMode), 1.0, "autopilot_mode max");
        assert_eq!(k.get(KnobId::AutopilotSpeed), 5.0, "autopilot_speed max");
        assert_eq!(k.get(KnobId::PacingRecovery), 2.0, "pacing_recovery max");
        assert_eq!(k.get(KnobId::RelicDensity), 3.0, "relic_density max");
        assert_eq!(k.get(KnobId::GoldDensity), 3.0, "gold_density max");
        assert_eq!(k.get(KnobId::MonsterDensity), 2.0, "monster_density max");
        assert_eq!(k.get(KnobId::TrapDensity), 2.0, "trap_density max");
        assert_eq!(k.get(KnobId::MaxFloors), 20.0, "max_floors max");
    }

    // ─── 3. RON serde round-trip preserves all fields ──────────────────────────

    #[test]
    fn ron_serde_round_trip_preserves_all_fields() {
        let mut k = Knobs::default();
        k.set(KnobId::HpStart, 150.0);
        k.set(KnobId::MpStart, 75.0);
        k.set(KnobId::FoodStart, 12.0);
        k.set(KnobId::EnemySpeedMul, 1.5);
        k.set(KnobId::EnemyHpMul, 2.25);
        k.set(KnobId::EnemyAtkMul, 0.75);
        k.set(KnobId::VisionRange, 10.0);
        k.set(KnobId::ScalingFloor, 1.25);
        k.set(KnobId::AutopilotMode, 1.0);
        k.set(KnobId::AutopilotSpeed, 3.5);
        k.set(KnobId::PacingRecovery, 1.5);
        k.set(KnobId::RelicDensity, 1.75);
        k.set(KnobId::GoldDensity, 0.5);
        k.set(KnobId::MonsterDensity, 1.25);
        k.set(KnobId::TrapDensity, 0.25);
        k.set(KnobId::MaxFloors, 15.0);

        let ron_str = ron::to_string(&k).expect("serialize Knobs");
        let back: Knobs = ron::from_str(&ron_str).expect("deserialize Knobs");

        assert_eq!(k, back, "RON round-trip must preserve every field exactly");
    }

    // ─── 4. Migration: a save string without `knobs` loads as default ──────────

    #[test]
    fn ron_migration_without_knobs_field_loads_as_default() {
        // We cannot modify `meta::SoulRemembrance` in this wave. To prove the
        // migration pattern works at the unit level, craft a minimal save
        // struct that has `#[serde(default)]` on its `knobs` field, mirroring
        // the per-field pattern that Knobs itself uses. An "old save" RON
        // string with no `knobs` key must deserialize into a struct whose
        // `knobs == default`.

        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Save {
            champion_count: u32,
            wisps: u8,
            marks_of_departed: u32,
            last_wager_gold: u32,
            clears: u32,
            #[serde(default)]
            knobs: Knobs,
        }

        // Exact RON string from the task description — five legacy fields,
        // no `knobs` key. This is the v0.5.x on-disk format.
        let old_save =
            "(champion_count: 0, wisps: 0, marks_of_departed: 0, last_wager_gold: 0, clears: 0)";

        let parsed: Save = ron::from_str(old_save).expect("old save must deserialize");
        assert_eq!(
            parsed.knobs,
            Knobs::default(),
            "missing `knobs` key must default to Knobs::default() — no migration step"
        );
        assert_eq!(parsed.champion_count, 0);
        assert_eq!(parsed.wisps, 0);
        assert_eq!(parsed.marks_of_departed, 0);
        assert_eq!(parsed.last_wager_gold, 0);
        assert_eq!(parsed.clears, 0);
    }

    // ─── 5. reset_one() restores only the target knob ──────────────────────────

    #[test]
    fn reset_one_restores_only_target_knob() {
        let mut k = Knobs::default();

        k.set(KnobId::HpStart, 77.0);
        k.set(KnobId::MpStart, 33.0);
        k.set(KnobId::FoodStart, 15.0);
        k.set(KnobId::EnemySpeedMul, 2.5);
        k.set(KnobId::EnemyHpMul, 4.0);
        k.set(KnobId::EnemyAtkMul, 2.0);
        k.set(KnobId::VisionRange, 12.0);
        k.set(KnobId::ScalingFloor, 1.5);
        k.set(KnobId::AutopilotMode, 1.0);
        k.set(KnobId::AutopilotSpeed, 4.0);
        k.set(KnobId::PacingRecovery, 1.5);
        k.set(KnobId::RelicDensity, 2.0);
        k.set(KnobId::GoldDensity, 2.0);
        k.set(KnobId::MonsterDensity, 1.5);
        k.set(KnobId::TrapDensity, 1.5);
        k.set(KnobId::MaxFloors, 14.0);

        // Reset only vision_range; everything else must stay dirty.
        k.reset_one(KnobId::VisionRange);
        assert_eq!(k.vision_range, 8.0, "reset_one restored vision_range");
        assert_eq!(k.hp_start, 77.0);
        assert_eq!(k.mp_start, 33.0);
        assert_eq!(k.food_start, 15.0);
        assert_eq!(k.enemy_speed_mul, 2.5);
        assert_eq!(k.enemy_hp_mul, 4.0);
        assert_eq!(k.enemy_atk_mul, 2.0);
        assert_eq!(k.scaling_floor, 1.5);
        assert_eq!(k.autopilot_mode, 1.0);
        assert_eq!(k.autopilot_speed, 4.0);
        assert_eq!(k.pacing_recovery, 1.5);
        assert_eq!(k.relic_density, 2.0);
        assert_eq!(k.gold_density, 2.0);
        assert_eq!(k.monster_density, 1.5);
        assert_eq!(k.trap_density, 1.5);
        assert_eq!(k.max_floors, 14.0);
    }

    // ─── 6. reset() restores every knob to SPEC §20 default ─────────────────────

    #[test]
    fn reset_restores_all_knobs_to_default() {
        let mut k = Knobs::default();

        k.set(KnobId::HpStart, 11.0);
        k.set(KnobId::MpStart, 22.0);
        k.set(KnobId::FoodStart, 3.0);
        k.set(KnobId::EnemySpeedMul, 0.7);
        k.set(KnobId::EnemyHpMul, 0.2);
        k.set(KnobId::EnemyAtkMul, 0.3);
        k.set(KnobId::VisionRange, 5.0);
        k.set(KnobId::ScalingFloor, 0.6);
        k.set(KnobId::AutopilotMode, 1.0);
        k.set(KnobId::AutopilotSpeed, 3.0);
        k.set(KnobId::PacingRecovery, 0.8);
        k.set(KnobId::RelicDensity, 0.4);
        k.set(KnobId::GoldDensity, 0.4);
        k.set(KnobId::MonsterDensity, 0.4);
        k.set(KnobId::TrapDensity, 0.4);
        k.set(KnobId::MaxFloors, 17.0);

        k.reset();

        assert_eq!(k, Knobs::default(), "reset() must restore every field");
    }
}

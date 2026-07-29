//! Core ECS components and common types for asciirogue.
//! No rendering, no I/O — pure game data.

#![deny(rust_2018_idioms)]

pub mod i18n;
pub mod meta;
pub mod vision;

use serde::{Deserialize, Serialize};

/// World coordinate (tile, not pixel).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TilePos(pub i32, pub i32);

impl TilePos {
    pub fn new(x: i32, y: i32) -> Self {
        Self(x, y)
    }
    pub fn x(&self) -> i32 {
        self.0
    }
    pub fn y(&self) -> i32 {
        self.1
    }
}

/// 8-direction movement delta (SPEC §10 vim + yubn).
///
/// hjkl are cardinals, yubn are diagonals. We use (dx, dy) signed deltas:
///   h = (-1, 0)  j = (0, 1)  k = (0, -1)  l = (1, 0)
///   y = (-1, -1) u = (1, -1) b = (-1, 1)  n = (1, 1)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Direction {
    Up,      // k
    Down,    // j
    Left,    // h
    Right,   // l
    UpLeft,  // y
    UpRight, // u
    DownLeft,// b
    DownRight,// n
    None,
}

impl Direction {
    pub const EIGHT: [Direction; 8] = [
        Direction::Up,
        Direction::UpRight,
        Direction::Right,
        Direction::DownRight,
        Direction::Down,
        Direction::DownLeft,
        Direction::Left,
        Direction::UpLeft,
    ];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up       => ( 0, -1),
            Direction::Down     => ( 0,  1),
            Direction::Left     => (-1,  0),
            Direction::Right    => ( 1,  0),
            Direction::UpLeft   => (-1, -1),
            Direction::UpRight  => ( 1, -1),
            Direction::DownLeft => (-1,  1),
            Direction::DownRight=> ( 1,  1),
            Direction::None     => ( 0,  0),
        }
    }
}

/// Game tick (turn counter).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tick(pub u64);

/// Energy unit — entities gain energy each tick and act when full.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Energy(pub i32);

/// Constant: energy required to spend a turn.
pub const ENERGY_PER_TURN: i32 = 100;

// ─── ECS components ──────────────────────────────────────────────────────────

/// Position in dungeon tile coordinates.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Position(pub TilePos);

/// Glyph + color of an entity. Rendering reads this directly.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Renderable {
    pub glyph: char,
    /// 24-bit RGB packed into u32 (high byte unused).
    pub fg_rgb: u32,
}

impl Renderable {
    pub const fn new(glyph: char, fg_rgb: u32) -> Self {
        Self { glyph, fg_rgb }
    }
}

/// Marker: blocks movement (walls, creatures). Players can't walk through these.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BlocksTile;

/// Marker: this entity is the player (only one).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Player;

/// Marker: this entity is an enemy / NPC that the player can fight.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Enemy;

/// Field-of-view component. `range` = sight radius in tiles.
/// `visible` and `revealed` are filled by the vision system each tick.
#[derive(Clone, Debug)]
pub struct Viewshed {
    pub range: i32,
    /// Tiles visible to this entity this tick (re-populated).
    pub visible: Vec<TilePos>,
    /// Tiles ever seen by this entity (for "memory" of past rooms).
    pub revealed: Vec<bool>,
}

impl Viewshed {
    pub fn new(range: i32, width: i32, height: i32) -> Self {
        let cap = (width.max(0) as usize) * (height.max(0) as usize);
        Self {
            range,
            visible: Vec::with_capacity(cap),
            revealed: vec![false; cap],
        }
    }
}

// ─── Combat / stats ───────────────────────────────────────────────────────────

/// Hit points. Current goes to 0 → entity despawned.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}
impl Health {
    pub fn new(max: i32) -> Self {
        Self { current: max, max }
    }
    pub fn is_dead(&self) -> bool {
        self.current <= 0
    }
    pub fn take(&mut self, dmg: i32) -> i32 {
        let dealt = dmg.max(0).min(self.current);
        self.current -= dealt;
        dealt
    }
}

/// Mana (magic resource).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Mana {
    pub current: i32,
    pub max: i32,
}
impl Mana {
    pub fn new(max: i32) -> Self {
        Self { current: max, max }
    }
    pub fn spend(&mut self, cost: i32) -> bool {
        if self.current >= cost {
            self.current -= cost;
            true
        } else {
            false
        }
    }
}

/// Base stats — derive from class/race and grow with level-ups.
///
/// v0.5.7: equipment-derived bonuses live alongside the base stats. They
/// are recomputed whenever an item is equipped or unequipped (see
/// `Equipment::recompute_into_stats`). `attack_bonus` and `defense_bonus`
/// are consumed by `combat::attack::resolve_attack`; the other bonuses
/// are reserved for future systems (casting, dodge, max-HP, etc.).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub strength: i32,
    pub dexterity: i32,
    pub intellect: i32,
    pub wisdom: i32,
    pub constitution: i32,
    /// Equipment-derived bonus added to outgoing damage.
    pub attack_bonus: i32,
    /// Equipment-derived bonus subtracted from incoming damage (min 0).
    pub defense_bonus: i32,
}
impl Stats {
    pub const fn new(s: i32, d: i32, i: i32, w: i32, c: i32) -> Self {
        Self {
            strength: s,
            dexterity: d,
            intellect: i,
            wisdom: w,
            constitution: c,
            attack_bonus: 0,
            defense_bonus: 0,
        }
    }
}

impl Energy {
    pub fn gain(&mut self, by: i32) {
        self.0 = (self.0 + by).max(0);
    }
    pub fn spend_turn(&mut self) {
        self.0 = (self.0 - ENERGY_PER_TURN).max(0);
    }
}

/// Marker: this entity can take its own turn (moves / attacks).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AiKind {
    /// BFS pathfind toward player.
    Chase,
    /// Like Chase but flees when HP low.
    Coward,
    /// Patrol until player in FOV, then chase.
    PatrolThenChase,
}

/// AI behaviour attached to an enemy. The actual decision (which step to take)
/// lives in `combat::ai::take_turn`.
#[derive(Clone, Debug)]
pub struct Ai {
    pub kind: AiKind,
    pub speed: i32, // energy gained per tick (player = 100/turn)
    pub vision: i32,
    /// Last known player position. Updated when the enemy "sees" the player.
    /// `None` while patrol / unaware.
    pub last_known_player: Option<TilePos>,
    /// Action cooldown (in player-turns). > 0 means this enemy waits this
    /// turn before it can act again. Reset to 0 each player action so only
    /// one hit per enemy per turn.
    pub cooldown: u8,
}

impl Ai {
    pub const fn new(kind: AiKind, speed: i32, vision: i32) -> Self {
        Self {
            kind,
            speed,
            vision,
            last_known_player: None,
            cooldown: 0,
        }
    }
}

/// Display name (Korean-first).
#[derive(Clone, Debug)]
pub struct Name(pub String);

/// Composite status effects on an entity. Each counter is "turns remaining".
#[derive(Copy, Clone, Debug, Default)]
pub struct StatusEffects {
    pub poison_turns: u8,
    pub burn_turns: u8,
    pub slow_turns: u8,
    pub bleed_turns: u8,
}

// ─── Items / inventory ────────────────────────────────────────────────────────

/// Kind of item. v0.1 covers gold, potions, weapons, armor.
/// v0.2+ will add enchantments and named relics.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Coin,
    HealthPotion,
    ManaPotion,
    Weapon,
    Armor,
}

/// A single item lying on the ground or held by an entity.
#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    /// Korean display name.
    pub name: String,
    /// Glyph used when on the ground.
    pub glyph: char,
    /// RGB pack for ground rendering.
    pub fg_rgb: u32,
    /// For weapons/armor this is the bonus to STR / CON.
    pub bonus: i32,
}

impl Item {
    pub fn gold(n: i32) -> Self {
        Self { kind: ItemKind::Coin, name: format!("금화 {}", n), glyph: '$', fg_rgb: 0xFF_D2_46, bonus: n }
    }
    pub fn potion_hp() -> Self {
        Self { kind: ItemKind::HealthPotion, name: "치유 물약".into(), glyph: '!', fg_rgb: 0x5A_DC_A0, bonus: 15 }
    }
    pub fn potion_mp() -> Self {
        Self { kind: ItemKind::ManaPotion, name: "마나 물약".into(), glyph: '?', fg_rgb: 0x5A_9C_F0, bonus: 10 }
    }
    pub fn weapon(b: i32, name: &str) -> Self {
        Self { kind: ItemKind::Weapon, name: name.into(), glyph: ')', fg_rgb: 0xC8_C8_C8, bonus: b }
    }
    pub fn armor(b: i32, name: &str) -> Self {
        Self { kind: ItemKind::Armor, name: name.into(), glyph: '[', fg_rgb: 0xB8_B8_BC, bonus: b }
    }
}

/// Inventory — fixed-size list of items. Empty slots hold None.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub slots: Vec<Option<Item>>,
    pub max: usize,
}
impl Inventory {
    pub fn new(max: usize) -> Self {
        Self { slots: vec![None; max], max }
    }
    /// Returns Ok(idx) on insert, Err(()) if full.
    pub fn push(&mut self, item: Item) -> Result<usize, ()> {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(item);
                return Ok(i);
            }
        }
        Err(())
    }
    pub fn take(&mut self, idx: usize) -> Option<Item> {
        self.slots.get_mut(idx).and_then(|s| s.take())
    }
    pub fn put_at(&mut self, idx: usize, item: Item) -> Result<(), ()> {
        match self.slots.get_mut(idx) {
            Some(slot) if slot.is_none() => {
                *slot = Some(item);
                Ok(())
            }
            _ => Err(()),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }
    /// First slot holding `kind`, if any.
    pub fn find_kind(&self, kind: ItemKind) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().map_or(false, |i| i.kind == kind))
    }
    /// First slot holding an item whose glyph matches, if any.
    pub fn find_glyph(&self, glyph: char) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().map_or(false, |i| i.glyph == glyph))
    }
}

/// v0.5.7: two equipment slots — weapon + armor. Items reference the same
/// `Item` data; the slot just records which one is currently in use. The
/// bonus is folded into the entity's `Stats` whenever equipment changes,
/// so combat code can keep reading from `Stats.attack_bonus / defense_bonus`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Equipment {
    pub weapon: Option<usize>,
    pub armor: Option<usize>,
}
impl Equipment {
    pub const fn new() -> Self {
        Self { weapon: None, armor: None }
    }

    /// Slot of the equipped item, if the given item is currently worn.
    pub fn slot_of(&self, inv_slot: usize) -> Option<EquipSlot> {
        if self.weapon == Some(inv_slot) {
            Some(EquipSlot::Weapon)
        } else if self.armor == Some(inv_slot) {
            Some(EquipSlot::Armor)
        } else {
            None
        }
    }
}

/// v0.5.7: distinguishes equipment slots. Inventory items are referenced
/// by index; Equipment records which index fills which slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EquipSlot {
    Weapon,
    Armor,
}
impl EquipSlot {
    pub fn accepts(self, kind: ItemKind) -> bool {
        matches!(
            (self, kind),
            (EquipSlot::Weapon, ItemKind::Weapon) | (EquipSlot::Armor, ItemKind::Armor)
        )
    }
}

/// Per-floor theme (SPEC §7 — 1~28층 환경).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FloorTheme {
    Cave,        // 1-4
    Catacomb,    // 5-8
    Garden,      // 9-12
    Library,     // 13-16
    Forge,       // 17-20
    Temple,      // 21-24
    FinalMaw,    // 25-27
    Boss,        // 28
}

impl FloorTheme {
    pub fn from_depth(depth: u32) -> Self {
        match depth {
            1..=4 => Self::Cave,
            5..=8 => Self::Catacomb,
            9..=12 => Self::Garden,
            13..=16 => Self::Library,
            17..=20 => Self::Forge,
            21..=24 => Self::Temple,
            25..=27 => Self::FinalMaw,
            _ => Self::Boss,
        }
    }
    /// Short display name (Korean first).
    pub fn label(self) -> &'static str {
        match self {
            Self::Cave => "동굴",
            Self::Catacomb => "카타콤",
            Self::Garden => "정원",
            Self::Library => "도서관",
            Self::Forge => "대장간",
            Self::Temple => "신전",
            Self::FinalMaw => "심연",
            Self::Boss => "쏠쏠리의 방",
        }
    }
}

/// Floor identifier: integer 1..=28.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Floor(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_threshold_baseline() {
        assert!(ENERGY_PER_TURN == 100);
    }

    #[test]
    fn direction_deltas_match_vim_yubn() {
        assert_eq!(Direction::Left.delta(), (-1, 0));
        assert_eq!(Direction::Right.delta(), (1, 0));
        assert_eq!(Direction::Up.delta(), (0, -1));
        assert_eq!(Direction::Down.delta(), (0, 1));
        assert_eq!(Direction::UpLeft.delta(), (-1, -1));
        assert_eq!(Direction::DownRight.delta(), (1, 1));
    }

    #[test]
    fn tile_pos_basics() {
        let p = TilePos::new(3, 4);
        assert_eq!(p.x(), 3);
        assert_eq!(p.y(), 4);
    }

    #[test]
    fn renderable_constructs() {
        let r = Renderable::new('@', 0xFFD65A);
        assert_eq!(r.glyph, '@');
        assert_eq!(r.fg_rgb, 0xFFD65A);
    }

    #[test]
    fn health_dies_at_zero() {
        let mut h = Health::new(10);
        assert!(!h.is_dead());
        assert_eq!(h.take(3), 3);
        assert_eq!(h.current, 7);
        assert_eq!(h.take(100), 7, "cannot over-damage; capped at current");
        assert!(h.is_dead());
    }

    #[test]
    fn mana_spend_respects_pool() {
        let mut m = Mana::new(5);
        assert!(m.spend(3));
        assert_eq!(m.current, 2);
        assert!(!m.spend(5), "cannot spend more than current");
        assert_eq!(m.current, 2);
    }

    #[test]
    fn inventory_round_trip() {
        let mut inv = Inventory::new(4);
        assert!(inv.is_empty());
        assert_eq!(inv.push(Item::potion_hp()), Ok(0));
        assert_eq!(inv.push(Item::potion_mp()), Ok(1));
        assert_eq!(inv.push(Item::gold(15)), Ok(2));
        // 4th slot filled:
        assert_eq!(inv.push(Item::weapon(2, "단검")), Ok(3));
        // 5th push should fail.
        assert_eq!(inv.push(Item::potion_hp()), Err(()));
        let it = inv.take(2).expect("gold slot");
        assert_eq!(it.kind, ItemKind::Coin);
        assert_eq!(it.bonus, 15);
        assert!(inv.slots[2].is_none());
        assert!(!inv.is_empty());
        // Take all
        for i in 0..4 {
            inv.take(i);
        }
        assert!(inv.is_empty());
    }

    #[test]
    fn floor_theme_mapping() {
        assert_eq!(FloorTheme::from_depth(1), FloorTheme::Cave);
        assert_eq!(FloorTheme::from_depth(4), FloorTheme::Cave);
        assert_eq!(FloorTheme::from_depth(5), FloorTheme::Catacomb);
        assert_eq!(FloorTheme::from_depth(28), FloorTheme::Boss);
    }
}

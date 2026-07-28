//! Core ECS components and common types for asciirogue.
//! No rendering, no I/O — pure game data.

#![deny(rust_2018_idioms)]

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
}

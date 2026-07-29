//! Procedural dungeon generation for asciirogue.
//!
//! v0.1: simple BSP — split room rectangles recursively, connect via corridors.

#![deny(rust_2018_idioms)]

use asciirogue_core::TilePos;
use serde::{Deserialize, Serialize};

pub mod bsp;

/// A tile in the dungeon.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Tile {
    Wall,
    Floor,
    /// Impassable (boundary or pillar).
    Rock,
    /// v0.5.10: stairs leading to the next floor. Passable (you can stand on
    /// it) and visually distinct (▼ glyph). Exactly one is placed per floor
    /// by `bsp::generate`, in the last (farthest) room.
    StairsDown,
}

impl Tile {
    pub fn is_passable(self) -> bool {
        matches!(self, Tile::Floor | Tile::StairsDown)
    }
}

/// Axis-aligned rectangular room.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Rect {
    pub fn new(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        debug_assert!(x1 <= x2 && y1 <= y2);
        Self { x1, y1, x2, y2 }
    }
    pub fn width(&self) -> i32 {
        self.x2 - self.x1 + 1
    }
    pub fn height(&self) -> i32 {
        self.y2 - self.y1 + 1
    }
    pub fn center(&self) -> (i32, i32) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x1 && x <= self.x2 && y >= self.y1 && y <= self.y2
    }
}

/// A generated dungeon floor.
#[derive(Clone, Debug)]
pub struct Dungeon {
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<Tile>,
    pub rooms: Vec<Rect>,
    /// v0.5.12: location of the StairsDown tile, if any. `None` for floors
    /// that don't generate a descent (e.g. the bottom of a finite run).
    /// Populated by `bsp::generate`. Useful for callers that need to avoid
    /// spawning enemies or loot on top of the stairs.
    pub stairs: Option<TilePos>,
}

impl Dungeon {
    pub fn at(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return Tile::Rock;
        }
        self.tiles[(y as usize) * (self.width as usize) + (x as usize)]
    }
    pub fn set(&mut self, x: i32, y: i32, t: Tile) {
        if x >= 0 && y >= 0 && x < self.width && y < self.height {
            self.tiles[(y as usize) * (self.width as usize) + (x as usize)] = t;
        }
    }
    pub fn carve_room(&mut self, r: Rect) {
        for y in r.y1..=r.y2 {
            for x in r.x1..=r.x2 {
                let p = self.at(x, y);
                let on_edge = x == r.x1 || y == r.y1 || x == r.x2 || y == r.y2;
                match p {
                    Tile::Rock if on_edge => self.set(x, y, Tile::Wall),
                    _ => self.set(x, y, Tile::Floor),
                }
            }
        }
    }
    pub fn carve_corridor(&mut self, ax: i32, ay: i32, bx: i32, by: i32) {
        let (cx, _cy) = (ax, by);
        for x in ax.min(cx)..=ax.max(cx) {
            self.set(x, ay, Tile::Floor);
        }
        for x in cx.min(bx)..=cx.max(bx) {
            self.set(x, by, Tile::Floor);
        }
        for y in ay.min(by)..=ay.max(by) {
            self.set(bx, y, Tile::Floor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basic() {
        let r = Rect::new(2, 3, 7, 5);
        assert_eq!(r.width(), 6);
        assert_eq!(r.height(), 3);
        assert_eq!(r.center(), (4, 4));
        assert!(r.contains(4, 4));
        assert!(!r.contains(1, 4));
    }

    #[test]
    fn dungeon_bounds() {
        let d = Dungeon {
            width: 10,
            height: 10,
            tiles: vec![Tile::Rock; 100],
            rooms: vec![],
            stairs: None,
        };
        assert_eq!(d.at(-1, 0), Tile::Rock);
        assert_eq!(d.at(0, 10), Tile::Rock);
        assert_eq!(d.at(0, 0), Tile::Rock);
    }
}

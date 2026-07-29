//! Binary Space Partition dungeon generator.
//!
//! Simple recursive BSP that splits the playable area into two halves, then
//! splits each half recursively, ending with leaves that each place a single
//! room. Sibling rooms are connected with L-shaped corridors.

use crate::{Dungeon, Rect, Tile};
use asciirogue_core::TilePos;

/// Configuration knobs for BSP generation.
#[derive(Copy, Clone, Debug)]
pub struct Config {
    /// Minimum dimension of a leaf node (no further splitting if smaller).
    pub min_leaf_size: i32,
    /// Minimum room dimension (width/height).
    pub room_min: i32,
    /// Maximum room dimension (width/height).
    pub room_max: i32,
    /// Maximum recursion depth.
    pub max_depth: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_leaf_size: 8,
            room_min: 4,
            room_max: 9,
            max_depth: 5,
        }
    }
}

/// Pseudo-random number generator (simple xorshift for determinism).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let s = if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed };
        Self(s)
    }
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
    fn gen_range(&mut self, lo: i32, hi_inclusive: i32) -> i32 {
        if hi_inclusive <= lo {
            return lo;
        }
        let span = (hi_inclusive - lo + 1) as u32;
        lo + (self.next_u32() % span) as i32
    }
    fn gen_room(&mut self, area: &Rect, cfg: &Config) -> Rect {
        let max_w = cfg.room_max.min(area.width() - 2).max(cfg.room_min);
        let max_h = cfg.room_max.min(area.height() - 2).max(cfg.room_min);
        let w = self.gen_range(cfg.room_min, max_w);
        let h = self.gen_range(cfg.room_min, max_h);
        let max_dx = (area.width() - w - 1).max(0);
        let max_dy = (area.height() - h - 1).max(0);
        let dx = self.gen_range(0, max_dx);
        let dy = self.gen_range(0, max_dy);
        Rect::new(area.x1 + 1 + dx, area.y1 + 1 + dy, area.x1 + dx + w, area.y1 + dy + h)
    }
}

struct Node {
    rect: Rect,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
    room: Option<Rect>,
}

fn build(node: &mut Node, rng: &mut Rng, cfg: &Config, depth: u32) {
    if depth >= cfg.max_depth
        || node.rect.width() < cfg.min_leaf_size * 2
            && node.rect.height() < cfg.min_leaf_size * 2
    {
        node.room = Some(rng.gen_room(&node.rect, cfg));
        return;
    }
    let horizontal = node.rect.height() > node.rect.width();
    let max_dim = if horizontal { node.rect.height() } else { node.rect.width() };
    let min_dim = cfg.min_leaf_size;
    if max_dim < min_dim * 2 {
        node.room = Some(rng.gen_room(&node.rect, cfg));
        return;
    }
    let cut = rng.gen_range(min_dim, max_dim - min_dim);
    let (lr, rr) = if horizontal {
        let mid_y = node.rect.y1 + cut;
        (
            Rect::new(node.rect.x1, node.rect.y1, node.rect.x2, mid_y - 1),
            Rect::new(node.rect.x1, mid_y, node.rect.x2, node.rect.y2),
        )
    } else {
        let mid_x = node.rect.x1 + cut;
        (
            Rect::new(node.rect.x1, node.rect.y1, mid_x - 1, node.rect.y2),
            Rect::new(mid_x, node.rect.y1, node.rect.x2, node.rect.y2),
        )
    };
    let mut left = Node { rect: lr, left: None, right: None, room: None };
    let mut right = Node { rect: rr, left: None, right: None, room: None };
    build(&mut left, rng, cfg, depth + 1);
    build(&mut right, rng, cfg, depth + 1);
    node.left = Some(Box::new(left));
    node.right = Some(Box::new(right));
    node.room = None;
}

fn collect_rooms(node: &Node, out: &mut Vec<Rect>) {
    if let Some(r) = node.room {
        out.push(r);
    }
    if let Some(l) = &node.left {
        collect_rooms(l, out);
    }
    if let Some(r) = &node.right {
        collect_rooms(r, out);
    }
}

fn first_room(node: &Node) -> Option<Rect> {
    if let Some(r) = node.room {
        return Some(r);
    }
    if let Some(l) = &node.left {
        if let Some(r) = first_room(l) {
            return Some(r);
        }
    }
    if let Some(r) = &node.right {
        if let Some(r) = first_room(r) {
            return Some(r);
        }
    }
    None
}

fn connect(node: &Node, dungeon: &mut Dungeon) {
    if let (Some(l), Some(r)) = (&node.left, &node.right) {
        // Connect the closest pair of rooms between the two subtrees.
        let la = first_room(l).unwrap();
        let ra = first_room(r).unwrap();
        let ac = la.center();
        let bc = ra.center();
        dungeon.carve_corridor(ac.0, ac.1, bc.0, bc.1);
        connect(l, dungeon);
        connect(r, dungeon);
    }
}

/// Generate a dungeon.
pub fn generate(width: i32, height: i32, seed: u64, cfg: Config) -> Dungeon {
    let mut rng = Rng::new(seed);
    let tiles = vec![Tile::Rock; (width * height) as usize];
    let mut dungeon = Dungeon { width, height, tiles, rooms: Vec::new(), stairs: None };

    let mut root = Node {
        rect: Rect::new(0, 0, width - 1, height - 1),
        left: None,
        right: None,
        room: None,
    };
    build(&mut root, &mut rng, &cfg, 0);
    collect_rooms(&root, &mut dungeon.rooms);
    let rooms_snapshot = dungeon.rooms.clone();
    for r in &rooms_snapshot {
        dungeon.carve_room(*r);
    }
    connect(&root, &mut dungeon);
    // After connecting corridors, place a StairsDown tile in the last room (farthest from start).
    // Use the last room in `rooms_snapshot` (which is farthest due to BSP ordering).
    if let Some(last_room) = rooms_snapshot.last() {
        let (cx, cy) = last_room.center();
        // Place stairs on that tile; if a boss later occupies it (BOSS_FLOOR),
        // the boss entity will sit on the stairs tile.
        dungeon.set(cx, cy, Tile::StairsDown);
        // v0.5.12: also publish the location so callers can avoid spawning
        // enemies, loot, or boss on the same tile.
        dungeon.stairs = Some(TilePos(cx, cy));
    }
    dungeon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_a_floor() {
        let d = generate(60, 30, 12345, Config::default());
        // We should have carved at least one room.
        assert!(!d.rooms.is_empty(), "BSP should produce rooms");
        // First room's center should be a Floor.
        let r = d.rooms[0];
        let (cx, cy) = r.center();
        assert!(d.at(cx, cy).is_passable(), "room center {:?} should be floor at ({},{})", d.at(cx, cy), cx, cy);
    }

    #[test]
    fn determinism() {
        let a = generate(40, 24, 42, Config::default());
        let b = generate(40, 24, 42, Config::default());
        assert_eq!(a.tiles, b.tiles);
        assert_eq!(a.rooms.len(), b.rooms.len());
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate(40, 24, 1, Config::default());
        let b = generate(40, 24, 2, Config::default());
        assert_ne!(a.tiles, b.tiles);
    }
}

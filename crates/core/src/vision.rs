//! Field-of-view (shadowcasting) for asciirogue.
//!
//! Wraps the `doryen-fov` crate. Visibility includes diagonals (8-way).

#![deny(rust_2018_idioms)]

use doryen_fov::{FovAlgorithm, FovRecursiveShadowCasting, MapData};
use crate::TilePos;

/// `blocks[x + y * width]` is `true` if the tile blocks sight (walls, rocks).
/// `radius = 0` means no limit.
pub fn compute(
    origin: TilePos,
    width: i32,
    height: i32,
    radius: i32,
    blocks: &[bool],
    out: &mut Vec<TilePos>,
) {
    out.clear();
    let mut map = MapData::new(width as usize, height as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            // doryen-fov: transparent=true means you can see THROUGH it.
            // wall = blocks sight → transparent = false.
            map.set_transparent(x, y, !blocks[x + y * width as usize]);
        }
    }
    let mut fov = FovRecursiveShadowCasting::new();
    fov.compute_fov(
        &mut map,
        origin.0 as usize,
        origin.1 as usize,
        radius as usize,
        true, // light_walls — walls visible if at the edge of FOV
    );
    for y in 0..height {
        for x in 0..width {
            if map.is_in_fov(x as usize, y as usize) {
                out.push(TilePos(x, y));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_map(w: i32, h: i32) -> Vec<bool> {
        vec![false; (w * h) as usize]
    }

    #[test]
    fn origin_always_visible() {
        let w = 10;
        let h = 10;
        let blocks = empty_map(w, h);
        let mut out = Vec::new();
        compute(TilePos(5, 5), w, h, 4, &blocks, &mut out);
        assert!(out.contains(&TilePos(5, 5)), "origin should be visible");
    }

    #[test]
    fn wall_blocks_two_steps_away() {
        let w = 10;
        let h = 10;
        let mut blocks = empty_map(w, h);
        // Wall directly south of origin at (5,6). Origin at (5,5).
        blocks[(5 + 6 * w) as usize] = true;
        let mut out = Vec::new();
        compute(TilePos(5, 5), w, h, 8, &blocks, &mut out);
        assert!(out.contains(&TilePos(5, 6)), "wall cell is itself visible (light_walls=true)");
        assert!(!out.contains(&TilePos(5, 7)), "one tile south of wall should be hidden");
        assert!(!out.contains(&TilePos(5, 8)), "two tiles south of wall should be hidden");
    }

    #[test]
    fn determinism_is_perfect() {
        let w = 18;
        let h = 12;
        let blocks = empty_map(w, h);
        let mut a = Vec::new();
        let mut b = Vec::new();
        compute(TilePos(3, 3), w, h, 6, &blocks, &mut a);
        compute(TilePos(3, 3), w, h, 6, &blocks, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn radius_limits_visibility() {
        let w = 30;
        let h = 1;
        let blocks = empty_map(w, h);
        let mut out = Vec::new();
        compute(TilePos(15, 0), w, h, 5, &blocks, &mut out);
        // Should NOT see (21, 0) — it's 6 tiles away, outside radius 5.
        assert!(!out.contains(&TilePos(21, 0)));
        // SHOULD see (19, 0) — 4 tiles away, inside radius 5.
        assert!(out.contains(&TilePos(19, 0)));
    }
}

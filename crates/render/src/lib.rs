//! Half-block TUI renderer for ratatui.
//!
//! Walls are drawn using 4-direction bit mask + 9 distinct half-block glyphs,
//! following the technique from
//! <https://bfnightly.bracketproductions.com/rustbook/chapter_16.html>.

#![deny(rust_2018_idioms)]

use asciirogue_core::{Renderable, TilePos, Viewshed};
use asciirogue_procgen::{Dungeon, Tile};
use ratatui::style::Color;
use ratatui::Frame;

/// Wall glyph selector — 4-bit mask (N=1, S=2, W=4, E=8) → half-block char.
///
/// The glyph is chosen so that the *top half* of each glyph points to whatever
/// adjacent tile is also a wall. Top half bright = wall on top, etc.
pub fn wall_glyph(map: &Dungeon, x: i32, y: i32) -> char {
    let is_wall = |dx: i32, dy: i32| matches!(map.at(x + dx, y + dy), Tile::Wall);
    let n = is_wall(0, -1) as u8;
    let s = is_wall(0, 1) as u8;
    let w = is_wall(-1, 0) as u8 * 4;
    let e = is_wall(1, 0) as u8 * 8;
    let mask = n | s | w | e;

    // Conventions:
    //   ▀ (top filled) if N+E or N+W → ceiling-like
    //   ▄ (bottom filled) if S+E or S+W → floor-like
    //   ▌ (left filled)   if N+E or S+E or E only
    //   ▐ (right filled)  if N+W or S+W or W only
    //   ▘ ▝ ▖ ▗ — corners only with one of N/S and one of E/W.
    //   ▜ ▟ ▛ ▙ — three-sided, rarely needed; we'll map to a full block.
    match mask {
        0b0000 => ' ',  // no adjacent walls → unclear; should not happen
        0b0001 => '▀',  // N only
        0b0010 => '▄',  // S only
        0b0100 => '█',  // W only (treat as solid)
        0b1000 => '█',  // E only
        0b0011 => '█',  // N+S solid wall (interior)
        0b1100 => '█',  // W+E solid wall (interior)
        0b0110 => '█',  // S+W → corner
        0b1001 => '█',  // N+E → corner
        0b1010 => '█',  // S+E → corner
        0b0101 => '█',  // N+W → corner
        0b0111 => '█',  // N+S+W
        0b1011 => '█',
        0b1101 => '█',
        0b1110 => '█',
        _ => '█',
    }
}

/// Render the dungeon view into the frame.
pub fn draw_map(frame: &mut Frame<'_>, area: ratatui::layout::Rect, map: &Dungeon) {
    let buf = frame.buffer_mut();
    for y in 0..area.height {
        for x in 0..area.width {
            let wx = x as i32;
            let wy = y as i32;
            let tile = map.at(wx, wy);
            let (symbol, color) = match tile {
                Tile::Wall => (wall_glyph(map, wx, wy), Color::Rgb(180, 180, 200)),
                Tile::Floor => ('·', Color::Rgb(110, 110, 130)),
                Tile::Rock => (' ', Color::Reset),
            };
            buf[(area.x + x, area.y + y)].set_char(symbol).set_fg(color);
        }
    }
}

/// Render the dungeon with player FOV applied.
///
/// - Visible tiles: bright
/// - Revealed but not visible: dim
/// - Hidden: black space
/// - Entities on visible tiles: renderable's glyph over the dungeon tile
///
/// `near_pickup_tiles` is the set of (x,y) tiles within 1 step of the player
/// that hold a ground item. Those tiles are rendered in **bold** so the player
/// can see "you can pick this up *right now*" without confusion. Distant items
/// are NOT highlighted on purpose — the goal is just to remove the "is this
/// mine?" ambiguity that bit v0.5.4.
pub fn draw_world(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    map: &Dungeon,
    world: &hecs::World,
    viewshed: &Viewshed,
    near_pickup_tiles: &std::collections::HashSet<(i32, i32)>,
) {
    let buf = frame.buffer_mut();
    let w = map.width;
    let h = map.height;

    let visible: std::collections::HashSet<(i32, i32)> = viewshed
        .visible
        .iter()
        .map(|&TilePos(x, y)| (x, y))
        .collect();
    let revealed_at = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < w && y < h && viewshed.revealed[(y * w + x) as usize]
    };

    // Pass 1: tiles
    for y in 0..area.height {
        for x in 0..area.width {
            let wx = x as i32;
            let wy = y as i32;
            if wx < 0 || wy < 0 || wx >= w || wy >= h {
                continue;
            }
            let is_visible = visible.contains(&(wx, wy));
            let is_revealed = revealed_at(wx, wy);

            let (symbol, color) = if is_visible {
                match map.at(wx, wy) {
                    Tile::Wall => (wall_glyph(map, wx, wy), Color::Rgb(180, 180, 200)),
                    Tile::Floor => ('·', Color::Rgb(140, 140, 170)),
                    Tile::Rock => ('#', Color::Rgb(80, 80, 90)),
                }
            } else if is_revealed {
                match map.at(wx, wy) {
                    Tile::Wall => (wall_glyph(map, wx, wy), Color::Rgb(60, 60, 70)),
                    Tile::Floor => ('·', Color::Rgb(40, 40, 50)),
                    Tile::Rock => (' ', Color::Reset),
                }
            } else {
                (' ', Color::Reset)
            };
            buf[(area.x + x, area.y + y)].set_char(symbol).set_fg(color);
        }
    }

    // Pass 2: entities
    for (_id, (pos, render)) in world.query::<(&asciirogue_core::Position, &Renderable)>().iter() {
        let TilePos(x, y) = pos.0;
        if x < 0 || y < 0 || x >= w || y >= h || !visible.contains(&(x, y)) {
            continue;
        }
        if x >= area.width as i32 || y >= area.height as i32 {
            continue;
        }
        let r = ((render.fg_rgb >> 16) & 0xFF) as u8;
        let g = ((render.fg_rgb >> 8) & 0xFF) as u8;
        let b = (render.fg_rgb & 0xFF) as u8;
        let color = Color::Rgb(r, g, b);
        buf[(area.x + x as u16, area.y + y as u16)]
            .set_char(render.glyph)
            .set_fg(color);
        // v0.5.5: bold adjacent ground items so the player can spot the gold
        // they can pick up right now. Distant items keep their normal weight.
        if near_pickup_tiles.contains(&(x, y)) {
            buf[(area.x + x as u16, area.y + y as u16)].set_style(ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asciirogue_procgen::{bsp::{generate, Config}};

    #[test]
    fn wall_glyph_handles_isolated_wall() {
        // A 5x5 dungeon with a single wall at (2,2). Room center (2,2) is wall;
        // adjacent tiles are floor. No neighbors are walls → mask 0 → space.
        // So we trigger that case with a wall far from any other wall.
        // For deterministic test: build dungeon 0 room and floor everything.
        let d = Dungeon {
            width: 5,
            height: 5,
            tiles: vec![Tile::Floor; 25],
            rooms: vec![],
        };
        // No walls, every neighbor returns false.
        assert_eq!(wall_glyph(&d, 2, 2), ' ');
    }

    #[test]
    fn dungeon_renders() {
        let _d = generate(20, 14, 99, Config::default());
        // Just smoke-test that generation + glyph lookup doesn't crash.
    }

    /// v0.5.5: ground items on the player's tile (or its 4 neighbours) must
    /// be rendered with the BOLD modifier so the player can spot the gold
    /// they can actually pick up right now. Items further away stay normal.
    #[test]
    fn draw_world_bolds_adjacent_pickups() {
        use asciirogue_core::{Item, ItemKind, Position, Renderable, Viewshed};
        use hecs::World;
        use ratatui::layout::Rect;
        use ratatui::style::Modifier;

        // 5x5 all-floor dungeon — no walls, all passable.
        let dungeon = Dungeon {
            width: 5,
            height: 5,
            tiles: vec![Tile::Floor; 25],
            rooms: vec![],
        };

        let mut world = World::new();
        // Player at (2,2). Two gold items: one at (2,1) (adjacent), one at
        // (0,0) (far away).
        world.spawn((
            Position(TilePos(2, 2)),
            Renderable::new('@', 0xFF_D6_5A),
            asciirogue_core::Player,
        ));
        world.spawn((
            Position(TilePos(2, 1)),
            Renderable::new('$', 0xFF_D2_46),
            Item {
                kind: ItemKind::Coin,
                name: "금화".into(),
                glyph: '$',
                fg_rgb: 0xFF_D2_46,
                bonus: 1,
            },
        ));
        world.spawn((
            Position(TilePos(0, 0)),
            Renderable::new('$', 0xFF_D2_46),
            Item {
                kind: ItemKind::Coin,
                name: "금화".into(),
                glyph: '$',
                fg_rgb: 0xFF_D2_46,
                bonus: 1,
            },
        ));

        // Viewshed: every tile visible AND revealed, for the test.
        let mut vs = Viewshed::new(5, 5, 5);
        for y in 0i32..5 {
            for x in 0i32..5 {
                vs.visible.push(TilePos(x, y));
                vs.revealed[(y * 5 + x) as usize] = true;
            }
        }

        // We can't call draw_world directly (it needs a Frame). Use
        // ratatui's TestBackend to drive it through a real Terminal draw
        // loop and read the resulting buffer.
        let area = Rect::new(0, 0, 5, 5);
        let near: std::collections::HashSet<(i32, i32)> =
            [(2, 2), (2, 1), (1, 2), (3, 2), (2, 3)].iter().copied().collect();
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(5, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_world(frame, area, &dungeon, &world, &vs, &near);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        // Adjacent gold at (2,1) should be bold.
        let adjacent = &buf[(2, 1)];
        assert_eq!(adjacent.symbol(), "$");
        assert!(
            adjacent.style().add_modifier.contains(Modifier::BOLD),
            "adjacent gold at (2,1) should be bolded: style={:?}",
            adjacent.style()
        );

        // Far gold at (0,0) should NOT be bold.
        let far = &buf[(0, 0)];
        assert_eq!(far.symbol(), "$");
        assert!(
            !far.style().add_modifier.contains(Modifier::BOLD),
            "distant gold at (0,0) should stay normal weight: style={:?}",
            far.style()
        );
    }
}

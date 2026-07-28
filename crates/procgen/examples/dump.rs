//! Headless dump of a generated floor — used to verify the wall_glyph
//! output matches the design PNG.
use asciirogue_procgen::bsp::{generate, Config};
use asciirogue_render::wall_glyph;

fn main() {
    let dungeon = generate(60, 24, 0xCAFE_BABE_DEAD_BEEFu64, Config::default());
    let h = dungeon.height;
    let w = dungeon.width;
    println!("Dungeon: {}x{} ({} rooms)", w, h, dungeon.rooms.len());
    for y in 0..h {
        let mut line = String::new();
        for x in 0..w {
            line.push(match dungeon.at(x, y) {
                asciirogue_procgen::Tile::Wall  => wall_glyph(&dungeon, x, y),
                asciirogue_procgen::Tile::Floor => '.',
                asciirogue_procgen::Tile::Rock  => ' ',
            });
        }
        println!("{}", line);
    }
}

//! Render the header line exactly the way `App::draw` does, so we can
//! verify the v0.5.5 "pickup: …" hint end-to-end without driving a TTY.
//!
//!   cargo run --example header_probe -p asciirogue

use asciirogue::{nearest_pickup_info, pick_up_outcome, App, PickupOutcome};
use asciirogue_core::{Direction, Health, Mana, Player, Position};

fn main() {
    let seed: u64 = 7581304714302077700;
    let mut app = App::new(seed);

    // Scenario A: teleport the player to the spot the user was stuck on
    // (13, 20) with a gold at (12, 20). This mirrors the screenshot.
    let player = find_player(&app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = asciirogue_core::TilePos(13, 20);
        world.get::<&mut asciirogue_core::Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            asciirogue_core::Position(asciirogue_core::TilePos(12, 20)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            asciirogue_core::Item::gold(7),
        ));
    }
    print_header(&app, "before pickup (player at (13,20), gold at (12,20))");
    let player = find_player(&app);
    let outcome = {
        let mut gold_gained: u32 = 0;
        pick_up_outcome(app.world_mut(), player, &mut gold_gained)
    };
    eprintln!("  >> pick_up_outcome = {:?}\n", outcome);
    assert_eq!(outcome, PickupOutcome::Picked, "adjacent gold must be picked up after v0.5.6");
    print_header(&app, "after pickup (gold consumed)");

    // Same scenario again, but this time gold is 2 tiles away → NoItem.
    let mut app = App::new(seed);
    let player = find_player(&mut app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = asciirogue_core::TilePos(13, 20);
        world.get::<&mut asciirogue_core::Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            asciirogue_core::Position(asciirogue_core::TilePos(11, 20)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            asciirogue_core::Item::gold(7),
        ));
    }
    print_header(&mut app, "scenario B: gold 2 tiles left");
    let player = find_player(&mut app);
    let outcome = {
        let mut gold_gained: u32 = 0;
        pick_up_outcome(app.world_mut(), player, &mut gold_gained)
    };
    eprintln!("  >> pick_up_outcome = {:?}", outcome);
    assert_eq!(outcome, PickupOutcome::NoItem, "out-of-range gold must NOT be picked up");
}

fn print_header(app: &App, label: &str) {
    let player = find_player(app);
    let player_pos = app.world_ref().get::<&Position>(player).unwrap().0;
    let visible_count = app
        .world_ref()
        .get::<&asciirogue_core::Viewshed>(player)
        .map(|v| v.visible.len())
        .unwrap_or(0);
    let hp_mp = match (
        app.world_ref().get::<&Health>(player),
        app.world_ref().get::<&Mana>(player),
    ) {
        (Ok(h), Ok(m)) => format!("HP {}/{}  MP {}/{}", h.current, h.max, m.current, m.max),
        _ => String::from("?"),
    };
    let pickup_hint = nearest_pickup_info(app.dungeon_ref(), app.world_ref(), player)
        .map(|(dist, dir, glyph)| {
            let arrow = match dir {
                Direction::Up => "↑",
                Direction::Down => "↓",
                Direction::Left => "←",
                Direction::Right => "→",
                Direction::UpLeft => "↖",
                Direction::UpRight => "↗",
                Direction::DownLeft => "↙",
                Direction::DownRight => "↘",
                Direction::None => "·",
            };
            let label = match glyph {
                '$' => "$",
                '!' => "!",
                '?' => "?",
                ')' => ")",
                '[' => "[",
                _ => "·",
            };
            format!("pickup: {} {} {}  ", arrow, label, dist)
        })
        .unwrap_or_default();
    let header = format!(
        "asciirogue — seed {}  {}F {}  player ({},{})  visible {}  {}{}",
        7581304714302077700u64,
        app.floor(),
        asciirogue_core::FloorTheme::from_depth(app.floor()).label(),
        player_pos.0,
        player_pos.1,
        visible_count,
        pickup_hint,
        hp_mp
    );
    eprintln!("\n[{}]\n  HEADER: {}", label, header);
}

fn find_player(app: &App) -> hecs::Entity {
    let mut found = hecs::Entity::DANGLING;
    for (e, _) in app.world_ref().query::<&Player>().iter() {
        found = e;
    }
    found
}
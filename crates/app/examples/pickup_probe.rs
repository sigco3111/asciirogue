//! Pickup diagnostic probe — verifies v0.5.5 fix:
//!   - g/$, Shift+4 pickup still works correctly
//!   - new nearest_pickup_info() BFS returns distance + direction + glyph
//!
//!   cargo run --example pickup_probe -p asciirogue

use asciirogue::{nearest_pickup_info, App};
use asciirogue_core::{Inventory, Item, Player, Position, Renderable, TilePos};

fn main() {
    // Exact seed from the user's bug-report screenshot.
    let seed: u64 = 7581304714302077700;
    let mut app = App::new(seed);

    let player = find_player(&app);
    let ppos = app.world_ref().get::<&Position>(player).unwrap().0;
    eprintln!("[init] player = ({},{})", ppos.0, ppos.1);

    // ── Scenario A: teleport the player to (13, 20), drop gold at (12, 20).
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(12, 20)),
            Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    eprintln!("[setup-A] moved player to (13,20), dropped gold at (12,20)");

    let info_a = nearest_pickup_info(app.dungeon_ref(), app.world_ref(), player);
    eprintln!("[scenario-A nearest_pickup_info] = {:?}", info_a);
    // Expected: Some((1, Direction::Left, '$')) → header would say "pickup: ← $ 1"

    // Try g.
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    let ev = Event::Key(KeyEvent {
        code: KeyCode::Char('g'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
    app.handle(&ev);
    eprintln!("[scenario-A after g] msgs = {:?}", app.messages());

    // ── Scenario B: despawn all ground items → header should be empty.
    {
        let world = app.world_mut();
        let items: Vec<_> = world.query::<&Item>().iter().map(|(e, _)| e).collect();
        for e in items {
            let _ = world.despawn(e);
        }
    }
    let info_b = nearest_pickup_info(app.dungeon_ref(), app.world_ref(), player);
    eprintln!("[scenario-B nearest_pickup_info, no items] = {:?}", info_b);

    // ── Scenario C: fresh seed, ground items in BSP-generated rooms.
    let fresh = App::new(seed);
    let info_c = nearest_pickup_info(fresh.dungeon_ref(), fresh.world_ref(), find_player(&fresh));
    eprintln!("[scenario-C fresh seed] = {:?}", info_c);
}

fn find_player(app: &App) -> hecs::Entity {
    let mut found = hecs::Entity::DANGLING;
    for (e, _) in app.world_ref().query::<&Player>().iter() {
        found = e;
    }
    found
}
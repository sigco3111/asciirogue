//! v0.5.6: Verify pickup now covers the player's tile AND its 4 neighbours.
//!
//!   cargo run --example pickup_probe -p asciirogue

use asciirogue::{nearest_pickup_info, pick_up_outcome, App, PickupOutcome};
use asciirogue_core::{Inventory, Item, Player, Position, Renderable, TilePos};

fn main() {
    let seed: u64 = 7581304714302077700;
    let mut app = App::new(seed);

    let player = find_player(&app);

    // ── Scenario A: gold on the player's tile → g should pick it up. ──
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(13, 20)),
            Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    let player = find_player(&app); // re-resolve after any realloc
    let outcome = pick_up_outcome(app.world_mut(), player);
    let msgs = app.messages().to_vec();
    eprintln!(
        "[A] gold on player tile → outcome={:?} msgs={:?}",
        outcome, msgs
    );
    let count_a = inv_count(&app, player);
    eprintln!("    inventory items after pick = {}", count_a);
    assert_eq!(
        outcome,
        PickupOutcome::Picked,
        "gold ON player tile must be picked up"
    );
    assert_eq!(count_a, 1, "gold must be in inventory after pick");

    // ── Scenario B: gold one tile to the LEFT → g should also pick it up
    //    (the bug we just fixed). ──
    let mut app = App::new(seed);
    let player = find_player(&mut app);
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
    let player = find_player(&mut app);
    let info = nearest_pickup_info(app.dungeon_ref(), app.world_ref(), player);
    eprintln!("[B] gold at (12,20), player at (13,20) → nearest_pickup_info = {:?}", info);
    let outcome = pick_up_outcome(app.world_mut(), player);
    eprintln!("    pick_up_outcome = {:?}", outcome);
    let count_b = inv_count(&app, player);
    eprintln!("    inventory items after pick = {}", count_b);
    assert_eq!(
        outcome,
        PickupOutcome::Picked,
        "gold one tile to the LEFT must be picked up after v0.5.6"
    );
    assert_eq!(count_b, 1, "gold must be in inventory after pick");

    // ── Scenario C: gold 2 tiles away → g should say NoItem. ──
    let mut app = App::new(seed);
    let player = find_player(&mut app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(11, 20)),
            Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    let player = find_player(&mut app);
    let outcome = pick_up_outcome(app.world_mut(), player);
    eprintln!("[C] gold 2 tiles left → outcome={:?}", outcome);
    assert_eq!(
        outcome,
        PickupOutcome::NoItem,
        "gold 2 tiles away must NOT be picked up"
    );

    eprintln!("\n=== all assertions passed ===");
}

fn find_player(app: &App) -> hecs::Entity {
    let mut found = hecs::Entity::DANGLING;
    for (e, _) in app.world_ref().query::<&Player>().iter() {
        found = e;
    }
    found
}

fn inv_count(app: &App, player: hecs::Entity) -> usize {
    app.world_ref()
        .get::<&Inventory>(player)
        .map(|i| i.slots.iter().filter(|s| s.is_some()).count())
        .unwrap_or(0)
}
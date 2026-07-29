//! v0.5.9 pickup probe — verify gold converts to App::gold instead of
//! occupying an inventory slot, while non-gold items still work normally.
//!
//!   cargo run --example pickup_probe -p asciirogue

use asciirogue::{nearest_pickup_info, pick_up_outcome, App, PickupOutcome};
use asciirogue_core::{Inventory, Item, Player, Position, TilePos};

fn main() {
    let seed: u64 = 7581304714302077700;

    // ── Scenario A: gold on the player's tile ──
    let mut app = App::new(seed);
    let player = find_player(&app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(13, 20)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    let player = find_player(&app);
    let starting_gold = app.gold();
    let mut gold_gained: u32 = 0;
    let outcome = pick_up_outcome(app.world_mut(), player, &mut gold_gained);
    // Probe stands in for App::handle — fold the gold back into the wallet.
    let new_gold = starting_gold + gold_gained;
    eprintln!(
        "[A] gold on player tile → outcome={:?} gold_gained={} msgs={:?}",
        outcome, gold_gained, app.messages()
    );
    let inv_count_a = inv_count(&app, player);
    eprintln!(
        "    App::gold (simulated): {} → {}, inventory items: {}",
        starting_gold, new_gold, inv_count_a
    );
    assert_eq!(outcome, PickupOutcome::Picked, "gold must be picked");
    assert_eq!(gold_gained, 7, "gold_gained must reflect pickup");
    assert_eq!(inv_count_a, 0, "inventory must stay empty (v0.5.9)");

    // ── Scenario B: gold one tile to the LEFT ──
    let mut app = App::new(seed);
    let player = find_player(&mut app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(12, 20)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    let player = find_player(&mut app);
    let info = nearest_pickup_info(app.dungeon_ref(), app.world_ref(), player);
    eprintln!("[B] gold at (12,20), player at (13,20) → nearest_pickup_info = {:?}", info);
    let mut gold_gained: u32 = 0;
    let outcome = pick_up_outcome(app.world_mut(), player, &mut gold_gained);
    eprintln!("    pick_up_outcome = {:?}, gold_gained = {}", outcome, gold_gained);
    let inv_count_b = inv_count(&app, player);
    eprintln!("    App::gold = {}, inventory items = {}", app.gold(), inv_count_b);
    assert_eq!(outcome, PickupOutcome::Picked, "adjacent gold must be picked");
    assert_eq!(gold_gained, 7);
    assert_eq!(inv_count_b, 0, "inventory stays empty for gold");

    // ── Scenario C: gold 2 tiles away → g should say NoItem. ──
    let mut app = App::new(seed);
    let player = find_player(&mut app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(13, 20);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(11, 20)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            Item::gold(7),
        ));
    }
    let player = find_player(&mut app);
    let mut gold_gained: u32 = 0;
    let outcome = pick_up_outcome(app.world_mut(), player, &mut gold_gained);
    eprintln!("[C] gold 2 tiles left → outcome={:?} gold_gained={}", outcome, gold_gained);
    assert_eq!(outcome, PickupOutcome::NoItem);
    assert_eq!(gold_gained, 0);

    // ── Scenario D: gold + potion in same pickup radius — gold → wallet,
    // potion → slot 0. ──
    let mut app = App::new(seed);
    let player = find_player(&mut app);
    {
        let world = app.world_mut();
        world.get::<&mut Position>(player).unwrap().0 = TilePos(50, 50);
        world.get::<&mut Inventory>(player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        world.spawn((
            Position(TilePos(50, 50)),
            asciirogue_core::Renderable::new('$', 0xFF_D2_46),
            Item::gold(3),
        ));
        world.spawn((
            Position(TilePos(51, 50)),
            asciirogue_core::Renderable::new('!', 0x5A_DC_A0),
            Item::potion_hp(),
        ));
    }
    let player = find_player(&mut app);
    let mut gold_gained: u32 = 0;
    let outcome = pick_up_outcome(app.world_mut(), player, &mut gold_gained);
    eprintln!("[D] gold+potion mix → outcome={:?} gold_gained={}", outcome, gold_gained);
    assert_eq!(outcome, PickupOutcome::Picked);
    assert_eq!(gold_gained, 3, "gold should still be picked up alongside the potion");
    let inv_count_d = inv_count(&app, player);
    eprintln!(
        "    App::gold = {}, inventory items = {} (potion should occupy slot 0)",
        app.gold(),
        inv_count_d
    );
    assert_eq!(inv_count_d, 1, "potion still occupies one slot");

    eprintln!("\n=== all v0.5.9 pickup assertions passed ===");
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
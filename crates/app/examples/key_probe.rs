//! keystroke probe — test that specific keys reach their handlers.

use asciirogue::App;
use asciirogue_core::{Direction, Position};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyEventState};

fn press(app: &mut App, code: KeyCode) {
    let _ = app.handle(&Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }));
}

/// Print last N messages.
fn tail(app: &App, n: usize) -> Vec<String> {
    let m = app.messages();
    let len = m.len();
    m[len.saturating_sub(n)..].to_vec()
}

fn where_player(app: &App) -> (i32, i32) {
    for (_, p) in app.world_ref().query::<&Position>().iter() {
        return (p.0.0, p.0.1);
    }
    (-1, -1)
}

fn where_item(app: &App) -> Option<(i32, i32)> {
    for (_, (p, _i)) in app.world_ref().query::<(&Position, &asciirogue_core::Item)>().iter() {
        return Some((p.0.0, p.0.1));
    }
    None
}

fn teleport(app: &mut App, x: i32, y: i32) {
    // Use world_mut to set player position directly.
    let mut found = None;
    {
        let world = app.world_mut();
        for (e, p) in world.query_mut::<&mut Position>() {
            let _ = e;
            p.0 = asciirogue_core::TilePos(x, y);
            found = Some(e);
            break;
        }
    }
    let _ = found;
    app.recompute_fov(); // Refresh so the player can see what's at (x,y).
}

fn main() {
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);

    println!("=== starting state ===");
    println!("player at: {:?}", where_player(&app));
    println!("first item at: {:?}", where_item(&app));
    println!("last 5 msgs: {:?}", tail(&app, 5));

    if let Some((ix, iy)) = where_item(&app) {
        teleport(&mut app, ix, iy);
        println!("teleported to ({},{}) (item there)", ix, iy);

        // Press 'g'
        press(&mut app, KeyCode::Char('g'));
        println!("--- after press 'g' ---");
        println!("msgs (last 5): {:?}", tail(&app, 5));
        println!("first item now: {:?}", where_item(&app));
        let inv_filled = app
            .world_ref()
            .query::<&asciirogue_core::Inventory>()
            .iter()
            .next()
            .map(|(_, inv)| inv.slots.iter().filter(|s| s.is_some()).count())
            .unwrap_or(0);
        println!("inventory slots filled after 'g': {}", inv_filled);
    } else {
        println!("no item on this floor, can't test pickup");
    }

    // Reset and test $
    println!("\n=== reset, teleport, press '$' ===");
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
    if let Some((ix, iy)) = where_item(&app) {
        teleport(&mut app, ix, iy);
        println!("teleported to ({},{}) (item there)", ix, iy);
        press(&mut app, KeyCode::Char('$'));
        println!("after press '$' msgs: {:?}", tail(&app, 5));
        println!("first item after: {:?}", where_item(&app));
        // Check inventory
        let inv_filled = app
            .world_ref()
            .query::<&asciirogue_core::Inventory>()
            .iter()
            .next()
            .map(|(_, inv)| inv.slots.iter().filter(|s| s.is_some()).count())
            .unwrap_or(0);
        println!("inventory slots filled after '$': {}", inv_filled);
    }

    // Reset and test '?'
    println!("\n=== reset, press '?' ===");
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
    press(&mut app, KeyCode::Char('?'));
    println!("after '?' msgs (last 5): {:?}", tail(&app, 5));

    // Reset and test arrow keys
    println!("\n=== reset, press Down ===");
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
    let (px, py) = where_player(&app);
    press(&mut app, KeyCode::Down);
    let (px2, py2) = where_player(&app);
    println!("player moved ({},{}) -> ({},{})", px, py, px2, py2);

    // Check what happens with just '$' on empty floor
    println!("\n=== '$' on no-item floor ===");
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
    press(&mut app, KeyCode::Char('$'));
    println!("msgs: {:?}", tail(&app, 5));

    // Test inventory-full path: fill the inventory, then try to pick up.
    println!("\n=== fill inventory then '$' ===");
    let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
    if let Some((ix, iy)) = where_item(&app) {
        teleport(&mut app, ix, iy);
        // Pre-fill the inventory with 8 items so it is full.
        let _ = app.world_mut();
        // Easier: directly fill the Inventory via a mutable query.
        let mut count = 0;
        {
            let world = app.world_mut();
            for (_, inv) in world.query_mut::<&mut asciirogue_core::Inventory>() {
                for slot in inv.slots.iter_mut() {
                    if count >= 8 { break; }
                    *slot = Some(asciirogue_core::Item::gold(1));
                    count += 1;
                }
            }
        }
        println!("filled {} slots", count);
        let inv_before = app
            .world_ref()
            .query::<&asciirogue_core::Inventory>()
            .iter()
            .next()
            .map(|(_, i)| i.slots.iter().filter(|s| s.is_some()).count())
            .unwrap_or(0);
        println!("inventory before '$': {}/8 filled", inv_before);
        press(&mut app, KeyCode::Char('$'));
        println!("msgs after '$' on full inv: {:?}", tail(&app, 5));
        let inv_after = app
            .world_ref()
            .query::<&asciirogue_core::Inventory>()
            .iter()
            .next()
            .map(|(_, i)| i.slots.iter().filter(|s| s.is_some()).count())
            .unwrap_or(0);
        println!("inventory after '$' on full inv: {}/8 filled", inv_after);
    }
}

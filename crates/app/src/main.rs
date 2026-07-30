//! asciirogue — Korean-first TUI roguelike with half-block visuals.

use anyhow::{Context, Result};
use asciirogue_combat::{attack, ai};
use asciirogue_core::{
    i18n::{self, Key as I18nKey, Locale},
    meta::SoulRemembrance,
    vision, Ai, AiKind, Direction, EquipSlot, Equipment, FloorTheme, Health, Inventory, Item,
    ItemKind, Mana, Name, Player, Position, Renderable, Stats, TilePos, Viewshed,
};
use asciirogue_procgen::{bsp, Dungeon, Tile};
use asciirogue_render::{draw_world, wall_glyph};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::disable_raw_mode;
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use asciirogue::{
    drop_inventory_at, nearest_pickup_info, nearest_stairs_info, pick_up_outcome, toggle_equip_at,
    use_inventory_at, ModalMode, PickupOutcome,
};
use hecs::World;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::env;
use std::io;

const MAP_W: i32 = 60;
const MAP_H: i32 = 24;
/// Sight radius in tiles. Lower values mean the player has to actively seek
/// encounters instead of being sniped from across a corridor. Tightened
/// from 8 → 5 in v0.5.1 after play-testing revealed enemies chased the
/// player before they were ever visible.
const VIEW_RADIUS: i32 = 5;

/// Game state — BSP dungeon + ECS world + cached viewshed.
struct App {
    dungeon: Dungeon,
    seed: u64,
    running: bool,
    /// Current floor (1..=8).
    floor: u32,
    /// Blocked sight cells computed once per level (true = wall/rock).
    blocks: Vec<bool>,
    world: World,
    player: hecs::Entity,
    /// Combat / event log (newest at back).
    messages: Vec<String>,
    /// Increments each tick to seed deterministic RNG.
    tick: u64,
    /// True when the boss for this floor is defeated; used to enable descent.
    boss_defeated: bool,
    /// v0.5.25: bosses killed this run. Reset on new game, accumulated
    /// when the player defeats a floor boss. Used by the death handler
    /// to grant marks of the departed proportional to progress.
    run_bosses_killed: u32,
    /// Active locale (toggle with `L`).
    locale: Locale,
    /// Persistent meta state (Soul Remembrance).
    remembrance: SoulRemembrance,
    /// Gold the player is carrying right now (collected from coin drops).
    gold: u32,
    /// True when the player has hit 0 HP. The game loop refuses all input
    /// except `R` (restart) and `q` (quit) until the user starts a new run.
    /// Saved-state dumps via `--dump` can still inspect the floor.
    game_over: bool,
    /// v0.5.7: top-level UI mode.
    modal: ModalMode,
    /// v0.5.14: when > 0, the next draw() must render the modal even if
    /// the user has already answered. This guarantees that the popup is
    /// visible for at least one full frame so the player can't miss it
    /// (previously the modal could appear and disappear between two
    /// render calls without ever being visible).
    modal_redraws: u8,
    /// v0.5.15: true when the player is standing on a StairsDown tile.
    /// Set by try_player_act and the debug warp (~) key. Cleared on
    /// descend (y/n) or when the player walks off the stairs. Drives
    /// the header "▼ 도착!" badge so the player can never miss the
    /// prompt again.
    at_stairs: bool,
}

/// Total floors in the run (SPEC §7 — keeping to 8 for v0.4 demo).
const MAX_FLOORS: u32 = 8;
/// Boss appears on this floor (and every `BOSS_EVERY` after; in v0.4 we just
/// put one yellow wyrm at the bottom).
const BOSS_FLOOR: u32 = MAX_FLOORS;
const INVENTORY_MAX: usize = 8;

impl App {
    fn new(base_seed: u64) -> Self {
        let loc = Locale::from_code(&std::env::var("ASCIITROGUE_LANG").unwrap_or_default());
        Self::new_at_with(
            base_seed,
            1,
            load_remembrance(),
            loc,
            0,
        )
    }

    /// Convenience for tests: a fresh App with no saved meta.
    #[allow(dead_code)]
    fn new_at(base_seed: u64, floor: u32) -> Self {
        Self::new_at_with(base_seed, floor, SoulRemembrance::new(), Locale::Korean, 0)
    }

    fn new_at_with(
        base_seed: u64,
        floor: u32,
        remembrance: SoulRemembrance,
        locale: Locale,
        carried_gold: u32,
    ) -> Self {
        let theme = FloorTheme::from_depth(floor);
        // Per-floor seed: combine base + floor so each descent is deterministic.
        let seed = base_seed.wrapping_add((floor as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64));
        let dungeon = bsp::generate(MAP_W, MAP_H, seed, bsp::Config::default());
        let mut world = World::new();

        // Spawn player in the first room, with inventory + resources.
        let (px, py) = dungeon
            .rooms
            .first()
            .map(|r| r.center())
            .unwrap_or((MAP_W / 2, MAP_H / 2));
        let player_pos = TilePos(px, py);
        let player = world.spawn((
            Position(player_pos),
            Player,
            Renderable::new('@', 0xFF_D6_5A),
            Viewshed::new(VIEW_RADIUS, MAP_W, MAP_H),
            Health::new(40 + 5 * (floor as i32 - 1)),
            Mana::new(15 + 2 * (floor as i32 - 1)),
            Stats::new(10, 10, 8, 8, 10),
            Inventory::new(INVENTORY_MAX),
            Equipment::new(),
            Name("모험가".into()),
        ));

        // Populate monsters and loot scaled by floor.
        populate_floor(&mut world, &dungeon, floor, theme);

        let blocks = build_blocks(&dungeon);
        let boss_defeated = floor > BOSS_FLOOR; // past-boss floors count as cleared

        // Compose an opening log line using the active locale.
        let startup_args: [&dyn std::fmt::Display; 2] =
            [&floor as &dyn std::fmt::Display, &theme.label() as &dyn std::fmt::Display];
        let startup_msg = format!(
            "{} — {}",
            t_format_with_locale(I18nKey::MsgFloorEntered, locale, &startup_args),
            match locale {
                Locale::Korean => "h/j/k/l, 화살표, yubn, i 인벤토리, p 포션, 계단에서 y/n",
                Locale::English => "h/j/k/l, arrows, yubn, i inv, p potion, y/n on stairs",
            }
        );

        let mut app = Self {
            dungeon,
            seed,
            running: true,
            floor,
            blocks,
            world,
            player,
            messages: vec![startup_msg],
            tick: 0,
            boss_defeated,
            run_bosses_killed: 0,
            locale,
            remembrance,
            gold: carried_gold,
            game_over: false,
            modal: ModalMode::Closed,
            modal_redraws: 0,
            at_stairs: false,
        };
        app.recompute_fov();
        app
    }

    fn floor_label(&self) -> String {
        format!("{}F {}", self.floor, FloorTheme::from_depth(self.floor).label())
    }

    /// v0.5.9: probe-side accessor for tests.
    pub fn gold(&self) -> u32 {
        self.gold
    }

    fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.messages.len() >= 5 {
            self.messages.remove(0);
        }
        self.messages.push(msg);
    }

    fn handle(&mut self, ev: &Event) {
        if let Event::Key(k) = ev {
            if k.kind == KeyEventKind::Press {
                let dir = key_to_direction(&k.code);
                // v0.5.7: while the inventory modal is open, all keys are
                // routed to the modal handler. Only Esc / I / q reopen the
                // gameplay loop. This means enemies don't move, FOV doesn't
                // recompute, and the player can't accidentally walk into a
                // monster while inspecting loot.
                if let ModalMode::Inventory { cursor } = self.modal {
                    self.handle_inventory_key(k.code, cursor);
                    return;
                }
                // When dead, only R (restart) or q (quit) do anything.
                if self.game_over {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            self.running = false;
                        }
                        KeyCode::Char('R') => {
                            let rem = self.remembrance.clone();
                            let locale = self.locale;
                            *self = App::new_at_with(self.seed, 1, rem, locale, 0);
                        }
                        _ => {}
                    }
                    return;
                }
                // Shift+4 ("$") should also trigger pickup
                if k.code == KeyCode::Char('4') && k.modifiers.contains(KeyModifiers::SHIFT) {
                    let mut gold_gained: u32 = 0;
                    match pick_up_outcome(&mut self.world, self.player, &mut gold_gained) {
                        PickupOutcome::Picked => {
                            self.gold = self.gold.saturating_add(gold_gained);
                            if gold_gained > 0 {
                                self.log(format!("골드 +{}!", gold_gained));
                            } else {
                                self.log("아이템을 주웠습니다.");
                            }
                        }
                        PickupOutcome::NoItem => self.log("주울 아이템이 없습니다."),
                        PickupOutcome::InventoryFull => {
                            if gold_gained > 0 {
                                self.gold = self.gold.saturating_add(gold_gained);
                                self.log(format!(
                                    "골드는 +{} 챙겼지만 인벤토리가 가득 찼습니다.",
                                    gold_gained
                                ));
                            } else {
                                self.log("인벤토리가 가득 찼습니다.");
                            }
                        }
                    }
                    return;
                }
                // v0.5.11/v0.5.14: stairs confirm modal owns input while open.
                // y / Enter descend, n / Esc cancel; everything else ignored.
                // v0.5.14: clear modal_redraws so the popup is not redrawn
                // after the player has answered (prevents "ghost popup").
                // v0.5.17: simplified modal handler. Always log to confirm
                // the branch was reached (debug aid; helps user verify
                // whether the modal handler is being entered at all).
                if let ModalMode::ConfirmStairs { choice } = self.modal {
                    self.modal_redraws = 0;
                    self.log(format!("[modal] key={:?}", k.code));
                    match k.code {
                        // Direct hotkeys — confirm the named choice.
                        // v0.5.23: only close the modal when descent actually
                        // succeeded. If the boss gate (or end-of-run) fires,
                        // descend_internal returns false and the popup stays
                        // open so the player sees the constraint.
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            if self.descend_internal() {
                                self.modal = ModalMode::Closed;
                                self.at_stairs = false;
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            // v0.5.18: n/Esc just closes the modal. We do
                            // NOT re-trigger even if the player is still
                            // on the stairs tile — v0.5.16's recovery UX
                            // created an infinite-modal trap (every key
                            // other than y/n was ignored while on stairs).
                            self.modal = ModalMode::Closed;
                            self.at_stairs = false;
                            self.log(i18n::t_for(I18nKey::MsgStairCancel, self.locale));
                        }
                        // Navigation — only moves the cursor; the modal
                        // stays open. v0.5.21, v0.5.22: switched to vertical
                        // (Up/Down, k/j) to match the modal's two-row layout —
                        // yes on row 1, no on row 2.
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.modal = ModalMode::ConfirmStairs { choice: false };
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.modal = ModalMode::ConfirmStairs { choice: true };
                        }
                        KeyCode::Tab => {
                            self.modal = ModalMode::ConfirmStairs { choice: !choice };
                        }
                        // Confirm — acts on the current selection.
                        // v0.5.23: same gate-aware close as the y/Y hotkeys.
                        KeyCode::Enter => {
                            if choice {
                                if self.descend_internal() {
                                    self.modal = ModalMode::Closed;
                                    self.at_stairs = false;
                                }
                            } else {
                                self.modal = ModalMode::Closed;
                                self.at_stairs = false;
                                self.log(i18n::t_for(I18nKey::MsgStairCancel, self.locale));
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                match k.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        self.running = false;
                    }
                    KeyCode::Char('R') => {
                        // Restart the run from floor 1.
                        let rem = self.remembrance.clone();
                        let locale = self.locale;
                        *self = App::new_at_with(self.seed, 1, rem, locale, 0);
                    }
                    KeyCode::Char('S') => {
                        // Save: persist current meta state to disk.
                        let _ = save_remembrance(&self.remembrance);
                        self.log(format!(
                            "영혼 기억 저장: {} 회 클리어, {} 챔피언 영혼",
                            self.remembrance.clears, self.remembrance.champion_count
                        ));
                    }
                    KeyCode::Char('L') => {
                        // Locale toggle.
                        self.locale = match self.locale {
                            Locale::Korean => Locale::English,
                            Locale::English => Locale::Korean,
                        };
                        self.log(match self.locale {
                            Locale::Korean => "언어: 한국어",
                            Locale::English => "Locale: English",
                        });
                    }
                    KeyCode::Char('?') => {
                        // In-screen help (one message burst).
                        self.log("h/j/k/l 이동 | yubn 대각 | 화살표 이동".to_string());
                        self.log("g 줍기 | i 인벤토리 | p 포션 | R 새 게임 | q 종료".to_string());
                        self.log("S 영혼 저장 | L 언어 토글 | ? 도움말".to_string());
                    }
                    KeyCode::Char('r') => {
                        // Re-roll current floor seed (dev cheat).
                        let new_base = self.seed.wrapping_add(0x12345);
                        let rem = self.remembrance.clone();
                        let locale = self.locale;
                        let gold = self.gold;
                        *self = App::new_at_with(new_base, self.floor, rem, locale, gold);
                    }
                    KeyCode::Char('~') | KeyCode::Char('`') => {
                        // v0.5.14: debug stairs warp. Teleport player onto
                        // the stairs tile so the descent confirm modal can
                        // be tested without walking the whole floor. The
                        // modal then triggers via the same code path as a
                        // normal arrival.
                        if let Some(stairs) = self.dungeon.stairs {
                            {
                                let mut pos = self.world.get::<&mut Position>(self.player).unwrap();
                                pos.0 = stairs;
                            }
                            self.recompute_fov();
                            self.modal = ModalMode::ConfirmStairs { choice: true };
                            self.modal_redraws = 4;
                            self.at_stairs = true;
                            self.log("▼ — 내려갈까? (y/n) [debug warp]");
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        // Debug: clear viewshed memory.
                        if let Ok(mut v) = self.world.get::<&mut Viewshed>(self.player) {
                            v.revealed.iter_mut().for_each(|r| *r = false);
                        }
                    }
                    KeyCode::Char('!') => {
                        // v0.5.26: dev cheat — drop a potion into the
                        // player's inventory for testing. Logs success
                        // or full-inventory refusal.
                        let item = Item::potion_hp();
                        let label = format!("{} (+{})", item.name, item.bonus);
                        let outcome = if let Ok(mut inv) =
                            self.world.get::<&mut Inventory>(self.player)
                        {
                            match inv.push(item) {
                                Ok(_) => "added".to_string(),
                                Err(()) => "full".to_string(),
                            }
                        } else {
                            "no_inventory".to_string()
                        };
                        match outcome.as_str() {
                            "added" => self.log(format!("[치트] 물약 추가: {}", label)),
                            "full" => self.log("[치트] 인벤토리가 가득 차서 추가 실패"),
                            _ => self.log("[치트] 인벤토리가 없어 추가 실패"),
                        }
                    }
                    KeyCode::Char('g') | KeyCode::Char('G') | KeyCode::Char('$') | KeyCode::Char(',') => {
                        let mut gold_gained: u32 = 0;
                        match pick_up_outcome(&mut self.world, self.player, &mut gold_gained) {
                            PickupOutcome::Picked => {
                                self.gold = self.gold.saturating_add(gold_gained);
                                if gold_gained > 0 {
                                    self.log(format!("골드 +{}!", gold_gained));
                                } else {
                                    self.log("아이템을 주웠습니다.");
                                }
                            }
                            PickupOutcome::NoItem => {
                                self.log("주울 아이템이 없습니다.");
                            }
                            PickupOutcome::InventoryFull => {
                                if gold_gained > 0 {
                                    self.gold = self.gold.saturating_add(gold_gained);
                                    self.log(format!(
                                        "골드는 +{} 챙겼지만 인벤토리가 가득 찼습니다.",
                                        gold_gained
                                    ));
                                } else {
                                    self.log("인벤토리가 가득 찼습니다.");
                                }
                            }
                        }
                    }
                    KeyCode::Char('i') => {
                        // v0.5.8: lowercase 'i' opens the inventory modal
                        // (was uppercase 'I' in v0.5.7). Quick-potion is
                        // now 'p' so the modal is one keystroke away.
                        let cursor = self
                            .world
                            .get::<&Inventory>(self.player)
                            .ok()
                            .and_then(|inv| inv.slots.iter().position(|s| s.is_some()))
                            .unwrap_or(0);
                        self.modal = ModalMode::Inventory { cursor };
                    }
                    KeyCode::Char('p') => {
                        // Quick-potion: use the first available potion in
                        // inventory without opening the modal.
                        if let Some(msg) = use_first_potion(&mut self.world, self.player) {
                            self.log(msg);
                        } else {
                            self.log("사용할 물약이 없습니다.");
                        }
                    }
                    d if dir.is_some() => {
                        if let Some(dir) = dir {
                            self.try_player_act(dir);
                        }
                        let _ = d;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Player attempts to move or attack adjacent enemy.
    fn try_player_act(&mut self, dir: Direction) {
        let cur = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        let (dx, dy) = dir.delta();
        if dx == 0 && dy == 0 {
            // wait
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            return;
        }
        let next = TilePos(cur.0 + dx, cur.1 + dy);

        // v0.5.19: Filter to enemies only (those with Health). Ground
        // items (loot) have no Health component; if we let them through
        // player_attack returns early on `already_dead` and the stairs
        // modal trigger at the bottom of this function never runs.
        let target_entity = self.entity_at(next).filter(|&e| {
            self.world.get::<&Health>(e).is_ok()
        });
        if let Some(entity) = target_entity {
            self.player_attack(entity);
            self.tick = self.tick.wrapping_add(1);
            self.enemy_take_turns();
            return;
        }

        // Else try walking.
        if !is_passable(&self.dungeon, next.0, next.1) {
            return; // bump into wall — no turn consumed
        }
        if let Ok(mut p) = self.world.get::<&mut Position>(self.player) {
            p.0 = next;
        }
        self.recompute_fov();
        // Auto-pick up any item on the new tile (e.g., gold `$`). v0.5.9:
        // gold never enters the inventory — it's folded straight into
        // `self.gold`. Silent on NoItem (the auto-pickup runs every step).
        let mut gold_gained: u32 = 0;
        match pick_up_outcome(&mut self.world, self.player, &mut gold_gained) {
            PickupOutcome::Picked => {
                if gold_gained > 0 {
                    self.gold = self.gold.saturating_add(gold_gained);
                    self.log(format!("골드 +{}!", gold_gained));
                } else {
                    self.log("아이템을 주웠습니다.");
                }
            }
            PickupOutcome::InventoryFull => {
                if gold_gained > 0 {
                    self.gold = self.gold.saturating_add(gold_gained);
                    self.log(format!(
                        "골드는 +{} 챙겼지만 인벤토리가 가득 찼습니다.",
                        gold_gained
                    ));
                } else {
                    self.log("인벤토리가 가득 찼습니다.");
                }
            }
            PickupOutcome::NoItem => {}
        }
        self.tick = self.tick.wrapping_add(1);
        self.enemy_take_turns();
        // v0.5.11: walking onto a StairsDown tile pops the descent confirm.
        // v0.5.14: also nudge modal_redraws so the popup is guaranteed one
        // full frame of visibility (vs. flashing for a sub-frame).
        if matches!(self.modal, ModalMode::Closed)
            && matches!(self.dungeon.at(next.0, next.1), Tile::StairsDown)
        {
            self.modal = ModalMode::ConfirmStairs { choice: true };
            self.modal_redraws = 4;
            self.at_stairs = true;
            self.log("▼ — 내려갈까? (y/n)");
        }
    }

    /// v0.5.11: descend to the next floor from a StairsDown tile.
    /// Returns `true` if the player actually descended, `false` if the
    /// descent was blocked (boss gate, end-of-run). v0.5.23: the modal
    /// handler uses this return value to keep the popup open when the
    /// gate fires — otherwise the player sees the popup vanish and
    /// assumes descent succeeded.
    fn descend_internal(&mut self) -> bool {
        let next_floor = self.floor + 1;
        if self.floor == BOSS_FLOOR && !self.boss_defeated {
            self.log(i18n::t_for(I18nKey::MsgBossFirst, self.locale));
            return false;
        }
        if next_floor > MAX_FLOORS {
            self.log(i18n::t_for(I18nKey::MsgBossFirst, self.locale));
            self.log(i18n::t_for(I18nKey::MsgAlreadyAtBottom, self.locale));
            self.remembrance.on_run_end(MAX_FLOORS, self.gold, 1);
            let _ = save_remembrance(&self.remembrance);
            self.log(format!(
                "쏠쏠리의 방 클리어! clears={}, soul_wisps={}",
                self.remembrance.clears,
                self.remembrance.wisps,
            ));
            return false;
        }
        let args: [&dyn std::fmt::Display; 1] = [&next_floor as &dyn std::fmt::Display];
        let msg = t_format_with_locale(I18nKey::MsgStairDescendOk, self.locale, &args);
        let rem = self.remembrance.clone();
        let locale = self.locale;
        let gold = self.gold;
        let seed_save = self.seed;
        *self = App::new_at_with(seed_save, next_floor, rem, locale, gold);
        self.log(msg);
        true
    }

    /// v0.5.7: dispatch one inventory-modal key. Mutates `self.modal`.
    ///
    /// Modal keymap (8-slot inventory):
    ///   j / Down   → cursor down (wraps)
    ///   k / Up     → cursor up   (wraps)
    ///   Enter      → default action for the slot (potion → use, weapon/armor → toggle equip)
    ///   e          → toggle equip / unequip
    ///   u          → use (potion only)
    ///   d / x      → drop
    ///   Esc / I / q → close modal
    fn handle_inventory_key(&mut self, code: KeyCode, cursor: usize) {
        // Number of inventory slots — kept in sync with INVENTORY_MAX.
        let slot_count = self
            .world
            .get::<&Inventory>(self.player)
            .map(|i| i.max)
            .unwrap_or(8);
        let next_cursor = |cur: usize, delta: i32| -> usize {
            let n = slot_count as i32;
            let c = cur as i32 + delta;
            ((c % n + n) % n) as usize
        };
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.modal = ModalMode::Inventory { cursor: next_cursor(cursor, 1) };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.modal = ModalMode::Inventory { cursor: next_cursor(cursor, -1) };
            }
            KeyCode::Char('e') => {
                if let Some(msg) = toggle_equip_at(&mut self.world, self.player, cursor) {
                    self.log(msg);
                }
            }
            KeyCode::Char('u') => {
                if let Some(msg) = use_inventory_at(&mut self.world, self.player, cursor) {
                    self.log(msg);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                if let Some(msg) = drop_inventory_at(&mut self.world, self.player, cursor) {
                    self.log(msg);
                }
            }
            KeyCode::Enter => {
                // Default action depends on the slot's item kind.
                let kind = self
                    .world
                    .get::<&Inventory>(self.player)
                    .ok()
                    .and_then(|i| i.slots.get(cursor).and_then(|s| s.as_ref().map(|it| it.kind)));
                match kind {
                    Some(ItemKind::Weapon) | Some(ItemKind::Armor) => {
                        if let Some(msg) = toggle_equip_at(&mut self.world, self.player, cursor) {
                            self.log(msg);
                        }
                    }
                    Some(_) => {
                        if let Some(msg) = use_inventory_at(&mut self.world, self.player, cursor) {
                            self.log(msg);
                        }
                    }
                    None => {
                        self.log("빈 슬롯입니다.".to_string());
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Char('q') => {
                // v0.5.8: lowercase 'i' now both opens AND closes the modal.
                // We still accept uppercase 'I' for Shift-key muscle memory.
                self.modal = ModalMode::Closed;
            }
            _ => {}
        }
    }

    fn entity_at(&self, pos: TilePos) -> Option<hecs::Entity> {
        let mut found = None;
        for (e, p) in self.world.query::<&Position>().iter() {
            if p.0 == pos {
                found = Some(e);
                break;
            }
        }
        found
    }

    fn player_attack(&mut self, target: hecs::Entity) {
        // Need attacker stats, defender stats, and a pseudo-random d100.
        let attacker_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let target_stats = match self.world.get::<&Stats>(target) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        // If the target is already dead, skip the message (despawn_dead will
        // emit the "x 처치!" announcement separately).
        let already_dead = self
            .world
            .get::<&Health>(target)
            .map(|h| h.is_dead())
            .unwrap_or(true);
        if already_dead {
            return;
        }
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) >> 0;
        // v0.5.7: feed the entity's equipped attack/defense bonuses into
        // the attack roll. attack_bonus raises to-hit, defense_bonus is
        // subtracted from incoming damage.
        let outcome = attack::resolve_attack(
            &attacker_stats,
            &target_stats,
            rng,
            attacker_stats.attack_bonus,
            target_stats.defense_bonus,
        );
        if !outcome.hit {
            self.log("빗나감!");
            return;
        }
        let dealt = if let Ok(mut h) = self.world.get::<&mut Health>(target) {
            h.take(outcome.dmg)
        } else {
            0
        };
        if dealt == 0 {
            // Target had < 1 HP from a previous strike this turn; let
            // despawn_dead report the kill instead of printing "0 피해!".
            self.despawn_dead();
            return;
        }
        let crit_marker = if outcome.crit { "치명타! " } else { "" };
        self.log(format!("{}{} 피해!", crit_marker, dealt));
        // remove dead enemies
        self.despawn_dead();
    }

    fn despawn_dead(&mut self) {
        // Determine which dead entities exist and whether any is the floor boss
        // (we tag bosses as having max HP > 100 — a heuristic but reliable given
        // our difficulty scaling).
        let mut dead: Vec<hecs::Entity> = self
            .world
            .query::<&Health>()
            .iter()
            .filter_map(|(e, h)| {
                if h.is_dead() {
                    Some(e)
                } else {
                    None
                }
            })
            .collect();
        let mut boss: Option<hecs::Entity> = None;
        for &e in &dead {
            if let Ok(h) = self.world.get::<&Health>(e) {
                if h.max > 100 {
                    boss = Some(e);
                    break;
                }
            }
        }
        let count = dead.len();
        for e in dead.drain(..) {
            let _ = self.world.despawn(e);
        }
        if count > 0 {
            self.log(format!("{} 처치!", count));
        }
        if boss.is_some() && !self.boss_defeated {
            self.boss_defeated = true;
            self.run_bosses_killed = self.run_bosses_killed.saturating_add(1);
            self.log(format!("{}층 보스 격파! '>' 내려가기.", self.floor));
        }
        // Death check on player.
        let player_died = self
            .world
            .get::<&Health>(self.player)
            .map(|h| h.is_dead())
            .unwrap_or(false);
        if player_died {
            self.on_player_death();
        }
    }

    /// v0.5.25: treat player death as a real end-of-run event. Idempotent:
    /// repeated calls (e.g. an extra enemy tick before the input loop
    /// notices `game_over`) do NOT re-deposit gold or double-count marks.
    /// Side-effects:
    ///   1. Call `remembrance.on_run_end(floor, gold, run_bosses_killed)`
    ///      so partial progress (last-wager gold, marks of the departed)
    ///      is preserved across deaths.
    ///   2. Persist the remembrance to disk.
    ///   3. Replace the log with a death summary so the player can
    ///      read what they accomplished in this run.
    fn on_player_death(&mut self) {
        if self.game_over {
            return;
        }
        self.game_over = true;
        let bosses = self.run_bosses_killed;
        let floor = self.floor;
        let gold = self.gold;
        self.remembrance.on_run_end(floor, gold, bosses);
        let _ = save_remembrance(&self.remembrance);
        self.messages.clear();
        self.messages.push("═══ 쓰러졌습니다… ═══".to_string());
        self.messages.push(format!(
            "도달: {}F ({}층) — 보스 {}명 처치",
            floor, floor, bosses
        ));
        self.messages.push(format!(
            "남긴 Gold: {} — 영혼 +{} 표시 / +{} 골드 (마지막 노림전)",
            gold,
            bosses,
            (gold / 10).min(200),
        ));
        self.messages.push("R = 새 게임, q = 종료".to_string());
    }

    /// v0.5.25: test-only entry point for the death handler. Tests use
    /// this to force-trigger death without setting up a full enemy
    /// tick. Production code goes through `enemy_take_turns` →
    /// `on_player_death` automatically.
    #[doc(hidden)]
    pub fn on_player_died_for_tests(&mut self) {
        self.on_player_death();
    }

    fn enemy_take_turns(&mut self) {
        // Snapshot player position once.
        let player_pos = match self.world.get::<&Position>(self.player) {
            Ok(p) => p.0,
            Err(_) => return,
        };
        // Iterate over all entities with Ai; for each, decide and act.
        // Snapshot every enemy with Ai. We can't borrow world immutably while
        // mutating, so collect tuples first.
        let plan: Vec<(hecs::Entity, TilePos, Ai)> = self
            .world
            .query::<(&Position, &Ai)>()
            .iter()
            .map(|(e, (p, a))| (e, p.0, a.clone()))
            .collect();
        // For each enemy, compute fresh sight info and decide an action.
        // The sight_blocks closure answers "does this tile block LOS for the
        // enemy?" using the same dungeon passability that the player's FOV
        // uses. We also refresh Ai.last_known_player based on what the enemy
        // sees *now* (Chebyshev ≤ vision AND no wall between).
        for (entity, enemy_pos, ai_behav) in plan {
            let blocks = &self.blocks; // borrow
            let sight_blocks = |x: i32, y: i32| {
                let idx = ((y * self.dungeon.width) + x) as usize;
                idx < blocks.len() && blocks[idx]
            };
            let can_see_player_now = {
                let dx = (player_pos.0 - enemy_pos.0).abs();
                let dy = (player_pos.1 - enemy_pos.1).abs();
                dx.max(dy) <= ai_behav.vision
                    && los_clear(enemy_pos, player_pos, sight_blocks)
            };
            // Update last_known_player whenever the enemy sees the player.
            if can_see_player_now {
                if let Ok(mut ai) = self.world.get::<&mut Ai>(entity) {
                    ai.last_known_player = Some(player_pos);
                }
            }
            let action = ai::take_turn(
                enemy_pos,
                player_pos,
                ai_behav.vision,
                ai_behav.kind,
                if can_see_player_now {
                    Some(player_pos)
                } else {
                    ai_behav.last_known_player
                },
                |x, y| is_passable(&self.dungeon, x, y)
                    && self.entity_at(TilePos(x, y)).is_none(),
                sight_blocks,
            );
            match action {
                ai::AiAction::AttackPlayer => self.enemy_attack(entity),
                ai::AiAction::Step(d) => {
                    let (dx, dy) = d.delta();
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let np = TilePos(enemy_pos.0 + dx, enemy_pos.1 + dy);
                    if is_passable(&self.dungeon, np.0, np.1)
                        && self.entity_at(np).is_none()
                    {
                        if let Ok(mut p) = self.world.get::<&mut Position>(entity) {
                            p.0 = np;
                        }
                    }
                }
                ai::AiAction::Wait | ai::AiAction::Idle => {}
            }
        }
    }

    fn enemy_attack(&mut self, attacker: hecs::Entity) {
        let atk_stats = match self.world.get::<&Stats>(attacker) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let def_stats = match self.world.get::<&Stats>(self.player) {
            Ok(s) => *s,
            Err(_) => Stats::default(),
        };
        let rng = ((self.tick as u32).wrapping_mul(2654435761)) ^ 0xDEAD_BEEF;
        // v0.5.7: equipment bonuses apply on enemy attacks too.
        let outcome = attack::resolve_attack(
            &atk_stats,
            &def_stats,
            rng,
            atk_stats.attack_bonus,
            def_stats.defense_bonus,
        );
        if !outcome.hit {
            self.log("적이 빗나감.");
            return;
        }
        if let Ok(mut h) = self.world.get::<&mut Health>(self.player) {
            h.take(outcome.dmg);
        }
        let crit = if outcome.crit { " (치명타!)" } else { "" };
        self.log(format!("적 명중! {} 피해{}", outcome.dmg, crit));
    }

    fn recompute_fov(&mut self) {
        let pos = if let Ok(p) = self.world.get::<&Position>(self.player) {
            p.0
        } else {
            TilePos(MAP_W / 2, MAP_H / 2)
        };
        let mut visible_buf: Vec<TilePos> = Vec::new();
        vision::compute(
            pos,
            self.dungeon.width,
            self.dungeon.height,
            VIEW_RADIUS,
            &self.blocks,
            &mut visible_buf,
        );
        // Detach borrow scope: read what we need, then take a fresh mutable borrow.
        let visible_for_reveal: Vec<TilePos> = visible_buf.clone();
        if let Ok(mut v) = self.world.get::<&mut Viewshed>(self.player) {
            v.visible = visible_buf;
            let w = self.dungeon.width;
            for &TilePos(x, y) in &visible_for_reveal {
                if x >= 0 && y >= 0 && x < w && y < self.dungeon.height {
                    v.revealed[(y * w + x) as usize] = true;
                }
            }
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        // Header.
        let player_pos = if let Ok(p) = self.world.get::<&Position>(self.player) {
            p.0
        } else {
            TilePos::default()
        };
        let visible_count = if let Ok(v) = self.world.get::<&Viewshed>(self.player) {
            v.visible.len()
        } else {
            0
        };
        let hp_mp = if let (Ok(h), Ok(m)) = (
            self.world.get::<&Health>(self.player),
            self.world.get::<&Mana>(self.player),
        ) {
            format!(
                "HP {}/{}  MP {}/{}  G {}",
                h.current,
                h.max,
                m.current,
                m.max,
                self.gold
            )
        } else {
            format!("?  G {}", self.gold)
        };
        // v0.5.5: tell the player where the nearest pickupable item is.
        // "↓ $ 6" = "gold, 6 steps down". Empty if nothing is in range.
        let pickup_hint = nearest_pickup_info(&self.dungeon, &self.world, self.player)
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
        // Build stairs hint similar to pickup hint.
        let stairs_hint = nearest_stairs_info(&self.dungeon, &self.world, self.player)
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
                format!("stairs: {} {} {}  ", arrow, glyph, dist)
            })
            .unwrap_or_default();
        // v0.5.15: a '▼ 도착!' badge when the player is standing on the
        // stairs tile. Tells the player they answered (or haven't) yet so
        // they don't walk off the stairs without seeing the modal.
        let at_stairs_badge = if self.at_stairs {
            " ▼ 도착! (y/n) "
        } else {
            ""
        };
        let title = Paragraph::new(Span::from(format!(
            "asciirogue — seed {}  {}  player ({},{})  visible {}  {}{}{}{}",
            self.seed,
            self.floor_label(),
            player_pos.0,
            player_pos.1,
            visible_count,
            stairs_hint,
            pickup_hint,
            at_stairs_badge,
            hp_mp
        )))
        .block({
            let mut b = Block::default().borders(Borders::ALL);
            if self.game_over {
                b = b.title(" ☠ YOU DIED ☠ ");
            } else {
                b = b.title(" asciirogue ");
            }
            b
        });
        frame.render_widget(title, chunks[0]);

        // Map area, centred.
        let map_chunk = chunks[1];
        let inner_w = map_chunk.width.min(MAP_W as u16);
        let inner_h = map_chunk.height.min(MAP_H as u16);
        // Snapshot viewshed before passing to the renderer to avoid holding a borrow
        // while we also borrow the world for entity iteration.
        let viewshed_snapshot = if let Ok(v) = self.world.get::<&Viewshed>(self.player) {
            Viewshed {
                range: v.range,
                visible: v.visible.clone(),
                revealed: v.revealed.clone(),
            }
        } else {
            Viewshed::new(VIEW_RADIUS, MAP_W, MAP_H)
        };
        let centered = ratatui::layout::Rect {
            x: map_chunk.x + (map_chunk.width.saturating_sub(inner_w)) / 2,
            y: map_chunk.y + (map_chunk.height.saturating_sub(inner_h)) / 2,
            width: inner_w,
            height: inner_h,
        };
        // v0.5.5: build the set of (x,y) tiles within 1 step of the player
        // that hold a ground item, so the renderer can bold them.
        let mut near_pickup_tiles: std::collections::HashSet<(i32, i32)> =
            std::collections::HashSet::new();
        {
            let px = player_pos.0;
            let py = player_pos.1;
            // The player's own tile is always relevant if it has an item.
            let here = (px, py);
            // Four neighbours.
            let neighbours = [(px - 1, py), (px + 1, py), (px, py - 1), (px, py + 1)];
            for (_, (p, _it)) in self.world.query::<(&asciirogue_core::Position, &Item)>().iter() {
                let k = (p.0.0, p.0.1);
                if k == here || neighbours.contains(&k) {
                    near_pickup_tiles.insert(k);
                }
            }
        }
        draw_world(frame, centered, &self.dungeon, &self.world, &viewshed_snapshot, &near_pickup_tiles);

        // Footer — last few messages + control hint.
        let recent = self
            .messages
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join("  |  ");
        let controls_hint = if self.boss_defeated && self.floor == BOSS_FLOOR {
            "g pick  i inv  p potion  stairs: y/n  R restart"
        } else {
            "g pick  i inv  p potion  stairs: y/n  q quit  R restart"
        };
        let status = Paragraph::new(Span::from(format!(
            "[{}]  rooms {}  {}\n{}",
            recent,
            self.dungeon.rooms.len(),
            self.floor_label(),
            controls_hint,
        )))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(status, chunks[2]);

        // v0.5.7: inventory modal overlay.
        if let ModalMode::Inventory { cursor } = self.modal {
            self.draw_inventory_modal(frame, area, cursor);
        }
        // v0.5.11/v0.5.14: stairs confirm modal — small centered popup.
        // v0.5.14: render whenever modal_redraws > 0 even if self.modal
        // was already cleared, so the popup is visible for at least one
        // full frame after the player steps onto the stairs tile.
        if self.modal_redraws > 0 || matches!(self.modal, ModalMode::ConfirmStairs { .. }) {
            self.draw_confirm_stairs_modal(frame, area);
        }
    }

    /// v0.5.11: popup asking for descent confirmation. Renders on top of
    /// the world map. Pressed key handled by `handle()`. v0.5.13: widened
    /// and restyled with a yellow highlight so the popup is unmistakable
    /// in the middle of gameplay (previously a 40x5 plain box that was
    /// easy to miss when the dungeon behind it was busy).
    /// v0.5.21: visually highlight the currently-selected option with a
    /// `▶` cursor + REVERSED style so the player can see what they're
    /// about to confirm.
    fn draw_confirm_stairs_modal(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        use ratatui::layout::Rect;
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

        // v0.5.24: the boss floor gets an extra hint line that names the
        // boss glyph — without it the player sees the gate rejection
        // in the log line but the popup itself doesn't explain WHY.
        let is_boss_floor = self.floor == BOSS_FLOOR;
        // Bigger, wider popup: 60% of width. 7 rows on regular floors,
        // 9 on the boss floor (2 borders + 7 inner rows incl. hint).
        let popup_w = (area.width * 60 / 100).max(36).min(area.width.saturating_sub(2));
        let popup_h = if is_boss_floor {
            9u16.min(area.height.saturating_sub(2))
        } else {
            7u16.min(area.height.saturating_sub(2))
        };
        let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        let popup: Rect = Rect::new(x, y, popup_w, popup_h);

        // First clear the entire screen so the popup stands out cleanly.
        frame.render_widget(Clear, area);
        // Then clear the popup region (in case the Clear above left artifacts).
        frame.render_widget(Clear, popup);

        let title_text = match self.locale {
            Locale::Korean => "▼ 내려가기 확인",
            Locale::English => "▼ Descend confirm",
        };
        let block = Block::default()
            .title(Span::styled(
                title_text,
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let prompt = i18n::t_for(I18nKey::MsgStairConfirm, self.locale);
        let (y_label, n_label, boss_hint_prefix, boss_hint_suffix) = match self.locale {
            Locale::Korean => (
                "[y] 예 — 다음 층으로",
                "[n] 취소 — 머무름",
                "※ 보스(",
                ") 처치 후 이동 가능",
            ),
            Locale::English => (
                "[y] yes — descend",
                "[n] cancel — stay",
                "※ Defeat boss (",
                ") to descend",
            ),
        };
        let choice = match self.modal {
            ModalMode::ConfirmStairs { choice } => choice,
            // modal_redraws>0 frame after the modal was already cleared
            // (v0.5.14 ghost-popup guard). Render as if yes is selected.
            _ => true,
        };
        // The selected option carries REVERSED + BOLD + accent colour so
        // the player has unambiguous visual feedback. The unselected
        // option stays grey so the focus is clear.
        let yellow = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        let grey = Style::default().fg(Color::Gray);
        let y_style = if choice { yellow.add_modifier(Modifier::REVERSED) } else { grey };
        let n_style = if !choice { grey.add_modifier(Modifier::REVERSED) } else { grey };
        let y_cursor = if choice { "▶ " } else { "  " };
        let n_cursor = if !choice { "▶ " } else { "  " };
        let prompt_styled = match self.locale {
            Locale::Korean => format!("{}  [Y]", prompt),
            Locale::English => format!("{}  [Y]", prompt),
        };
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(prompt_styled, yellow)),
            Line::from(""),
            Line::from(Span::styled(format!("{}{}", y_cursor, y_label), y_style)),
            Line::from(Span::styled(format!("{}{}", n_cursor, n_label), n_style)),
            Line::from(""),
        ];
        if is_boss_floor {
            // v0.5.24: in-popup boss hint — name the glyph (D) so the
            // player knows which entity gates the descent.
            let boss_red = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
            lines.push(Line::from(vec![
                Span::styled(boss_hint_prefix, grey),
                Span::styled("D", boss_red),
                Span::styled(boss_hint_suffix, grey),
            ]));
        }
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    /// v0.5.7: centered modal with the player's inventory. 8-slot grid on
    /// the left, equipped items on the right, controls hint on the bottom.
    fn draw_inventory_modal(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, cursor: usize) {
        use ratatui::layout::{Constraint, Direction, Layout};
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

        // Modal size: 60 cols × 18 rows, centered.
        let w = 60u16.min(area.width);
        let h = 18u16.min(area.height);
        let modal_area = ratatui::layout::Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        };
        // Clear behind the modal so the dungeon doesn't bleed through.
        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .title(" 인벤토리 (i / Esc: 닫기) ")
            .borders(Borders::ALL);
        frame.render_widget(block, modal_area);

        let inner = modal_area.inner(ratatui::layout::Margin::new(1, 1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // slot list
                Constraint::Length(1),  // spacer
                Constraint::Length(3),  // equipped + stats + gold
                Constraint::Min(1),    // controls
            ])
            .split(inner);

        // ── Slot list (8 slots, 2 columns × 4 rows) ──
        let slot_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(std::iter::repeat(Constraint::Length(1)).take(8))
            .split(chunks[0]);
        let inv = self.world.get::<&Inventory>(self.player).ok();
        let equip = self.world.get::<&Equipment>(self.player).ok().map(|e| *e);
        for i in 0..8usize {
            let slot_area = slot_chunks[i];
            let item = inv.as_ref().and_then(|inv| inv.slots.get(i).and_then(|s| s.clone()));
            let tag = match (&item, equip) {
                (Some(it), Some(eq)) if eq.weapon == Some(i) => "[W]",
                (Some(it), Some(eq)) if eq.armor == Some(i) => "[A]",
                _ => "",
            };
            let line = match &item {
                Some(it) => format!(
                    "{:>2}. {} {:<14}  보너스 {:+}",
                    i + 1,
                    tag,
                    format!("{}{}", it.glyph, it.name),
                    it.bonus
                ),
                None => format!("{:>2}. (비어 있음)", i + 1),
            };
            let style = if i == cursor {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            let para = Paragraph::new(Span::styled(line, style));
            frame.render_widget(para, slot_area);
        }

        // ── Equipped panel + stats ──
        let eq_lines = {
            let mut lines = Vec::new();
            if let (Some(inv), Some(eq)) = (inv.as_ref(), equip) {
                let weapon = eq
                    .weapon
                    .and_then(|idx| inv.slots.get(idx).and_then(|s| s.clone()));
                let armor = eq
                    .armor
                    .and_then(|idx| inv.slots.get(idx).and_then(|s| s.clone()));
                let weapon_str = weapon
                    .as_ref()
                    .map(|i| format!("{}{}  보너스 {:+}", i.glyph, i.name, i.bonus))
                    .unwrap_or_else(|| "(없음)".to_string());
                let armor_str = armor
                    .as_ref()
                    .map(|i| format!("{}{}  보너스 {:+}", i.glyph, i.name, i.bonus))
                    .unwrap_or_else(|| "(없음)".to_string());
                lines.push(Line::from(vec![
                    Span::styled("무기: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(weapon_str),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("방어구: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(armor_str),
                ]));
            }
            let stats = self.world.get::<&Stats>(self.player).ok();
            if let Some(s) = stats {
                lines.push(Line::from(format!(
                    "공격 보너스: {:+}    방어 보너스: {:+}    보유 골드: {}",
                    s.attack_bonus,
                    s.defense_bonus,
                    self.gold
                )));
            } else {
                lines.push(Line::from(format!("보유 골드: {}", self.gold)));
            }
            lines
        };
        let eq_para = Paragraph::new(eq_lines);
        frame.render_widget(eq_para, chunks[2]);

        // ── Controls ──
        let ctrl = Paragraph::new(Line::from(vec![
            Span::styled("j/k", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 이동  "),
            Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 기본동작  "),
            Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 장착/해제  "),
            Span::styled("u", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 사용  "),
            Span::styled("d", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 버리기  "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 닫기"),
        ]))
        .wrap(Wrap { trim: true });
        frame.render_widget(ctrl, chunks[3]);
    }
}

// ─── i18n helpers ────────────────────────────────────────────────────────────

/// Look up a translation key in the app's current locale and apply simple
/// `{}`-style positional substitution. For the small set of formats we use,
/// `{}` is replaced in order with the variadic args converted via Display.
#[allow(dead_code)]
fn t_format(app: &App, key: I18nKey, args: &[&dyn std::fmt::Display]) -> String {
    t_format_with_locale(key, app.locale, args)
}

/// Same as `t_format`, but takes the locale directly. Useful when we need to
/// format a string before `App` exists (e.g. inside `new_at_with`).
fn t_format_with_locale(
    key: I18nKey,
    locale: Locale,
    args: &[&dyn std::fmt::Display],
) -> String {
    let raw = i18n::t_for(key, locale);
    let mut out = String::with_capacity(raw.len() + 16);
    let mut arg_idx = 0;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'}') {
            chars.next(); // consume '}'
            if let Some(a) = args.get(arg_idx) {
                use std::fmt::Write;
                let _ = write!(&mut out, "{}", a);
                arg_idx += 1;
            } else {
                out.push_str("{}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[allow(dead_code)]
fn log_t(app: &mut App, key: I18nKey, args: &[&dyn std::fmt::Display]) {
    let s = t_format(app, key, args);
    app.log(s);
}

// ─── save / load ──────────────────────────────────────────────────────────────

fn save_root() -> std::path::PathBuf {
    let mut p = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    p.push(".local");
    p.push("share");
    p.push("asciirogue");
    let _ = std::fs::create_dir_all(&p);
    p.push("remembrance.ron");
    p
}

fn save_remembrance(r: &SoulRemembrance) -> std::io::Result<()> {
    let path = save_root();
    let s = ron::to_string(r).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, s)
}

fn load_remembrance() -> SoulRemembrance {
    let path = save_root();
    match std::fs::read_to_string(&path) {
        Ok(s) => ron::from_str(&s).unwrap_or_default(),
        Err(_) => SoulRemembrance::new(),
    }
}

fn spawn_enemy(
    world: &mut World,
    dungeon: &Dungeon,
    room_idx: usize,
    glyph: char,
    name: &str,
    hp: i32,
    stats: Stats,
    vision: i32,
    kind: AiKind,
) {
    if let Some(room) = dungeon.rooms.get(room_idx) {
        // v0.5.12: if the room center is the stairs tile, nudge to a
        // neighbor so the spawn never sits on the descent.
        let (cx, cy) = room.center();
        let (ex, ey) = match dungeon.stairs {
            Some(s) if (s.0, s.1) == (cx, cy) => {
                let offsets = [(0,1), (1,0), (0,-1), (-1,0), (1,1), (1,-1), (-1,1), (-1,-1)];
                offsets
                    .iter()
                    .map(|&(dx, dy)| (cx + dx, cy + dy))
                    .find(|&(x, y)| room.contains(x, y))
                    .unwrap_or((cx, cy))
            }
            _ => (cx, cy),
        };
        let fg = match glyph {
            'r' => 0xE6_82_82u32, // pink rat
            'g' => 0xB4_5A_D2u32, // magenta goblin
            _ => 0xFF_FF_FFu32,
        };
        world.spawn((
            Position(TilePos(ex, ey)),
            Renderable::new(glyph, fg),
            Health::new(hp),
            stats,
            Ai::new(kind, 100, vision),
            Name(name.to_string()),
        ));
    }
}

/// Spawn monsters + loot scaled by floor number (1..=MAX_FLOORS).
fn populate_floor(world: &mut World, dungeon: &Dungeon, floor: u32, theme: FloorTheme) {
    // Difficulty scales linearly: HP + attack bonus per floor.
    let scale = floor as i32; // 1..=8

    // Spawn 2 standard enemies per floor (in non-boss floors) + 1 boss on BOSS_FLOOR.
    // v0.5.12: avoid placing rats/goblins/bears on the last room so the
    // stairs tile is never blocked by an enemy.
    let room_count = dungeon.rooms.len();
    let last_room_idx = room_count.saturating_sub(1);
    let safe_room = |idx: usize| -> usize {
        let picked = idx.min(room_count.saturating_sub(1));
        if picked == last_room_idx && last_room_idx > 0 {
            last_room_idx.saturating_sub(1)
        } else {
            picked
        }
    };
    if room_count >= 3 {
        let mut idx = 1_usize;
        // Tier 1: a rat in room[1] (always available, weakest).
        spawn_enemy(
            world,
            dungeon,
            safe_room(idx),
            'r',
            "쥐",
            6 + scale,
            Stats::new(3, 8, 1, 1, 5),
            8,
            AiKind::Coward,
        );
        idx += 1;
        // Tier 2: a goblin (or skeleton for catacombs+).
        let glyph2 = match theme {
            FloorTheme::Cave | FloorTheme::Garden => 'g',
            FloorTheme::Catacomb | FloorTheme::Library => 'S',
            FloorTheme::Forge | FloorTheme::Temple => 'T',
            FloorTheme::FinalMaw => 'D',
            FloorTheme::Boss => 'D',
        };
        let name2 = match glyph2 {
            'g' => "고블린",
            'S' => "스켈레톤",
            'T' => "트롤",
            'D' => "드래곤",
            _ => "적",
        };
        spawn_enemy(
            world,
            dungeon,
            safe_room(idx),
            glyph2,
            name2,
            14 + scale * 2,
            Stats::new(5, 6, 2, 2, 7),
            9,
            AiKind::Chase,
        );
    }
    // Spawn a tougher prowler if floor is mid-tier or beyond (room 3+).
    if room_count >= 4 && floor >= 3 {
        spawn_enemy(
            world,
            dungeon,
            safe_room(3),
            'B',
            "곰",
            28 + scale * 2,
            Stats::new(9, 4, 1, 1, 12),
            8,
            AiKind::Chase,
        );
    }

    // Boss on BOSS_FLOOR. v0.5.12: spawn on a tile adjacent to the stairs
    // so the boss does not occupy the descent itself.
    if floor == BOSS_FLOOR {
        if let Some(room) = dungeon.rooms.last() {
            let (cx, cy) = room.center();
            let (sx, sy) = dungeon.stairs.map(|p| (p.0, p.1)).unwrap_or((cx, cy));
            let (bx, by) = if (sx, sy) == (cx, cy) {
                let offsets = [(0,1), (1,0), (0,-1), (-1,0), (1,1), (1,-1), (-1,1), (-1,-1)];
                offsets
                    .iter()
                    .map(|&(dx, dy)| (cx + dx, cy + dy))
                    .find(|&(x, y)| room.contains(x, y))
                    .unwrap_or((cx, cy))
            } else {
                (cx, cy)
            };
            world.spawn((
                Position(TilePos(bx, by)),
                Renderable::new('D', 0xFF_FF_40),
                Health::new(120 + scale * 8),
                Stats::new(12, 8, 6, 6, 16),
                Ai::new(AiKind::Chase, 100, 10),
                Name("황녀 🐲".into()),
            ));
        }
    }

    // Loot: drop a few items per floor (every 2nd room has a chest).
    for (i, room) in dungeon.rooms.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if i % 2 == 0 {
            continue;
        }
        // Pick a free tile within the room. v0.5.12: nudge off the stairs
        // tile if the room center happens to be the descent.
        let cx_center = (room.x1 + room.x2) / 2;
        let cy_center = (room.y1 + room.y2) / 2;
        let (cx, cy) = match dungeon.stairs {
            Some(s) if (s.0, s.1) == (cx_center, cy_center) => (cx_center + 1, cy_center),
            _ => (cx_center, cy_center),
        };
        let cx = cx.max(room.x1 + 1).min(room.x2 - 1);
        let cy = cy.max(room.y1 + 1).min(room.y2 - 1);
        let item = match (floor + i as u32) % 4 {
            0 => Item::gold(10 + scale * 4),
            1 => Item::potion_hp(),
            2 => Item::potion_mp(),
            _ => Item::weapon(1 + scale, if scale >= 3 { "장검" } else { "단검" }),
        };
        // Render on ground: store as an entity with Item component (no health/AI),
        // and a derived glyph/fg.
        let kind_idx = match item.kind {
            asciirogue_core::ItemKind::Coin => 0,
            asciirogue_core::ItemKind::HealthPotion => 1,
            asciirogue_core::ItemKind::ManaPotion => 2,
            asciirogue_core::ItemKind::Weapon => 3,
            asciirogue_core::ItemKind::Armor => 4,
        };
        let glyphs = ['$', '!', '?', ')', '['];
        let fgs = [
            0xFF_D2_46u32,
            0x5A_DC_A0u32,
            0x5A_9C_F0u32,
            0xC8_C8_C8u32,
            0xB8_B8_BCu32,
        ];
        let _ = kind_idx; // enum discriminant kept for downstream extension
        world.spawn((
            Position(TilePos(cx, cy)),
            Renderable::new(glyphs[item.kind as usize], fgs[item.kind as usize]),
            item,
        ));
    }
}

/// Pick up an item lying on the ground at the player's tile (key `g`).
fn try_pickup(world: &mut World, player: hecs::Entity) -> bool {
    let player_pos = match world.get::<&Position>(player) {
        Ok(p) => p.0,
        Err(_) => return false,
    };
    // Collect entity ids at the player's tile that carry an Item.
    let mut item_entities: Vec<hecs::Entity> = Vec::new();
    let to_check: Vec<hecs::Entity> = world
        .query::<(&Position, &Item)>()
        .iter()
        .filter_map(|(e, (p, _))| if p.0 == player_pos { Some(e) } else { None })
        .collect();
    for e in to_check {
        item_entities.push(e);
    }
    if item_entities.is_empty() {
        return false;
    }

    // Pull the items out one at a time so each `Ref` is released before the
    // next borrow. Inventory is mutated AFTER all items are cloned.
    let mut items_to_put: Vec<Item> = Vec::with_capacity(item_entities.len());
    let mut picked = Vec::new();
    for e in item_entities {
        // Borrow `&Item`, clone, drop the Ref, then push to despawn queue.
        let cloned = match world.get::<&Item>(e) {
            Ok(r) => Some((*r).clone()),
            Err(_) => None,
        };
        if let Some(item) = cloned {
            items_to_put.push(item);
            picked.push(e);
        }
    }
    // Free refs before mutating Inventory.
    drop(world.get::<&Health>(player)); // no-op placeholder that runs; ok
    // Now push into Inventory.
    let mut inv = world.get::<&mut Inventory>(player).ok().unwrap();
    for item in items_to_put {
        if inv.push(item).is_err() {
            // Inventory full — keep the corresponding entity on the ground.
            if let Some(e) = picked.pop() {
                // We over-counted by one; ignore — leaving the item where it is.
                let _ = e;
            }
            break;
        }
    }
    drop(inv);
    let count = picked.len();
    for e in picked {
        let _ = world.despawn(e);
    }
    count > 0
}

/// Use the first available potion in the inventory (key `i`).
fn use_first_potion(world: &mut World, player: hecs::Entity) -> Option<String> {
    let mut inv = world.get::<&mut Inventory>(player).ok()?;
    let mut idx: Option<usize> = None;
    for (i, slot) in inv.slots.iter().enumerate() {
        if let Some(it) = slot {
            if matches!(it.kind, asciirogue_core::ItemKind::HealthPotion
                              | asciirogue_core::ItemKind::ManaPotion)
            {
                idx = Some(i);
                break;
            }
        }
    }
    let i = idx?;
    let item = inv.take(i)?;
    let msg = match item.kind {
        asciirogue_core::ItemKind::HealthPotion => {
            drop(inv);
            if let Ok(mut h) = world.get::<&mut Health>(player) {
                h.current = (h.current + item.bonus).min(h.max);
                format!("치유 물약 사용! HP +{}", item.bonus)
            } else {
                String::from("치유 물약...")
            }
        }
        asciirogue_core::ItemKind::ManaPotion => {
            drop(inv);
            if let Ok(mut m) = world.get::<&mut Mana>(player) {
                m.current = (m.current + item.bonus).min(m.max);
                format!("마나 물약 사용! MP +{}", item.bonus)
            } else {
                String::from("마나 물약...")
            }
        }
        _ => String::from("(potion이 아님)"),
    };
    Some(msg)
}

/// v0.1 — descents and saves are scaffolded. We support `>` to descend if boss
/// defeated, and `R` to restart the run from floor 1.

fn key_to_direction(k: &KeyCode) -> Option<Direction> {
    match k {
        // vim cardinals
        KeyCode::Char('h') => Some(Direction::Left),
        KeyCode::Char('j') => Some(Direction::Down),
        KeyCode::Char('k') => Some(Direction::Up),
        KeyCode::Char('l') => Some(Direction::Right),
        // yubn diagonals
        KeyCode::Char('y') => Some(Direction::UpLeft),
        KeyCode::Char('u') => Some(Direction::UpRight),
        KeyCode::Char('b') => Some(Direction::DownLeft),
        KeyCode::Char('n') => Some(Direction::DownRight),
        // arrow keys — 4-way only (no diagonal arrow on standard keyboards)
        KeyCode::Left => Some(Direction::Left),
        KeyCode::Right => Some(Direction::Right),
        KeyCode::Up => Some(Direction::Up),
        KeyCode::Down => Some(Direction::Down),
        // wait
        KeyCode::Char('.') => Some(Direction::None),
        _ => None,
    }
}

fn build_blocks(d: &Dungeon) -> Vec<bool> {
    let w = d.width as usize;
    let h = d.height as usize;
    (0..w * h)
        .map(|i| !matches!(d.tiles[i], Tile::Floor))
        .collect()
}

/// Cheap line-of-sight: walks from `a` toward `b` step by step, returning
/// false if any intermediate tile is sight-blocking or out of bounds.
fn los_clear(a: TilePos, b: TilePos, sight_blocks: impl Fn(i32, i32) -> bool) -> bool {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let dist = dx.abs().max(dy.abs());
    if dist == 0 {
        return true;
    }
    for i in 1..dist {
        let t = i as f32 / dist as f32;
        let px = a.0 + (dx as f32 * t).round() as i32;
        let py = a.1 + (dy as f32 * t).round() as i32;
        if sight_blocks(px, py) {
            return false;
        }
    }
    true
}

fn is_passable(d: &Dungeon, x: i32, y: i32) -> bool {
    // v0.5.20: include Tile::StairsDown. The procgen crate's canonical
    // `Tile::is_passable()` (crates/procgen/src/lib.rs) already lists both
    // Floor and StairsDown — this binary file's older copy missed
    // StairsDown, so `try_player_act` returned early at the wall-check
    // whenever the player walked onto a StairsDown tile, and the
    // descent-confirm modal at the end of `try_player_act` never ran.
    // The `~` debug warp bypasses `is_passable` entirely, which is why
    // the bug looked like "the modal handler works but the natural-arrival
    // path never fires". Keep in sync with the lib.rs twin (line ~1668).
    matches!(d.at(x, y), Tile::Floor | Tile::StairsDown)
}

fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        let mut app = App::new(0xCAFE_BABE_DEAD_BEEF);
        let tick = std::time::Duration::from_millis(100);
        while app.running {
            terminal.draw(|f| app.draw(f))?;
            if crossterm::event::poll(std::time::Duration::from_millis(50))? {
                let ev = crossterm::event::read()?;
                app.handle(&ev);
            } else {
                std::thread::sleep(tick);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

fn dump_to_stdout(seed: u64, count: u32) -> Result<()> {
    for i in 1..=count {
        // Use App::new_at so each floor gets its own seed, theme, monsters and loot.
        let app = App::new_at(seed, i);
        let dungeon = &app.dungeon;
        let world = &app.world;
        let player = app.player;

        // FOV once so the dump mirrors what a player would see on arrival.
        let player_pos = match world.get::<&Position>(player) {
            Ok(p) => p.0,
            Err(_) => continue,
        };
        let blocks = build_blocks(dungeon);
        let mut visible_buf: Vec<TilePos> = Vec::new();
        vision::compute(player_pos, MAP_W, MAP_H, VIEW_RADIUS, &blocks, &mut visible_buf);
        let visible_set: std::collections::HashSet<(i32, i32)> = visible_buf
            .iter()
            .map(|&TilePos(x, y)| (x, y))
            .collect();

        // Build an entity lookup including items (rendered as their item glyph).
        let entity_at: std::collections::HashMap<(i32, i32), char> = world
            .query::<(&Position, &Renderable)>()
            .iter()
            .map(|(_, (p, r))| ((p.0.0, p.0.1), r.glyph))
            .collect();

        let theme = FloorTheme::from_depth(i);
        let boss_marker = if i == BOSS_FLOOR { " [BOSS]" } else { "" };
        println!(
            "── seed 0x{:016X}  {}F {}{}  rooms {}  player@({},{})  visible {} ──",
            app.seed,
            i,
            theme.label(),
            boss_marker,
            dungeon.rooms.len(),
            player_pos.0,
            player_pos.1,
            visible_set.len()
        );
        for y in 0..MAP_H {
            let mut line = String::with_capacity(MAP_W as usize + 16);
            for x in 0..MAP_W {
                if let Some(&glyph) = entity_at.get(&(x, y)) {
                    line.push(glyph);
                } else if visible_set.contains(&(x, y)) {
                    line.push(match dungeon.at(x, y) {
                        Tile::Wall => wall_glyph(&dungeon, x, y),
                        Tile::Floor => '.',
                        Tile::StairsDown => '▼',
                        Tile::Rock => ' ',
                    });
                } else if matches!(dungeon.at(x, y), Tile::Wall | Tile::Rock) {
                    line.push(' ');
                } else {
                    line.push('·');
                }
            }
            println!("{}", line);
        }
        println!();
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let dump_mode = args.iter().any(|a| a == "--dump");
    if dump_mode {
        let count: u32 = args
            .windows(2)
            .find(|w| w[0] == "--count")
            .and_then(|w| w[1].parse().ok())
            .unwrap_or(3);
        let seed: u64 = args
            .windows(2)
            .find(|w| w[0] == "--seed")
            .and_then(|w| parse_seed(&w[1]).ok())
            .unwrap_or(0xCAFE_BABE_DEAD_BEEF);
        return dump_to_stdout(seed, count);
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("asciirogue — Korean-first TUI roguelike");
        eprintln!();
        eprintln!("Keys (vim / yubn / arrows):");
        eprintln!("  h j k l  — cardinal movement (vim)");
        eprintln!("  y u b n  — diagonal movement (yubn)");
        eprintln!("  ← ↓ ↑ →  — cardinal movement (arrow keys)");
        eprintln!("  .        — wait one turn");
        eprintln!("  c        — clear vision memory");
        eprintln!("  r        — new random seed (regenerate current floor)");
        eprintln!("  R        — restart the run from floor 1");
        eprintln!("  g / $    — pick up item under player (alias for $ on a $ glyph)");
        eprintln!("  i        — use first available potion");
        eprintln!("  >        — auto-popup on stairs: y to descend, n to cancel (after defeating the floor's boss)");
        eprintln!("  ?        — show help lines (key list, also in Korean)");
        eprintln!("  S        — save meta (Soul Remembrance) to disk");
        eprintln!("  L        — toggle locale (ko / en)");
        eprintln!("  q / Esc  — quit");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  asciirogue              Launch the TUI");
        eprintln!("  asciirogue --dump       Dump floors (with FOV) to stdout (no TTY required)");
        eprintln!("  asciirogue --dump --count 5 --seed 1  Dump 5 floors with custom seed");
        eprintln!("  asciirogue --help       Show this message");
        eprintln!();
        eprintln!("Environment:");
        eprintln!("  ASCIITROGUE_LANG=ko|en  Override startup locale (default: ko)");
        return Ok(());
    }

    // panic hook — restore the terminal first, then print the panic.
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        original(info);
    }));
    run().context("asciirogue runtime error")
}

fn parse_seed(s: &str) -> Result<u64> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(rest, 16).with_context(|| format!("invalid hex seed: {s}"));
    }
    s.parse::<u64>().with_context(|| format!("invalid decimal seed: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn test_dollar_key_pickup() {
        // v0.5.9: gold does NOT enter the inventory any more — it's
        // converted straight to App::gold at pickup time.
        let mut app = App::new(0);
        let player_pos = app.world.get::<&Position>(app.player).unwrap().0;
        let gold_item = Item::gold(1);
        app.world.spawn((Position(player_pos), gold_item));
        // Empty inventory so we can assert it stays empty.
        {
            let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
            inv.slots.iter_mut().for_each(|s| *s = None);
        }
        let starting_gold = app.gold();
        // Simulate pressing '$' key.
        let ev = Event::Key(KeyEvent {
            code: KeyCode::Char('$'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&ev);
        // Gold must have moved to App::gold, not into a slot.
        assert_eq!(
            app.gold(),
            starting_gold + 1,
            "gold must increment App::gold (was {}, now {})",
            starting_gold,
            app.gold()
        );
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        assert!(
            inv.slots.iter().all(|s| s.is_none()),
            "inventory must stay empty — gold skips it (v0.5.9)"
        );
        // And the player-facing message must say 골드 +1.
        assert!(
            app.messages.iter().any(|m| m.contains("골드")),
            "messages should mention gold: {:?}",
            app.messages
        );
    }

    /// v0.5.9: picking up a potion (non-gold item) still works the old way —
    /// it goes into the inventory slot. Only coins are routed around it.
    #[test]
    fn test_potion_pickup_still_uses_inventory() {
        let mut app = App::new(0);
        let player_pos = app.world.get::<&Position>(app.player).unwrap().0;
        app.world.spawn((Position(player_pos), Item::potion_hp()));
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        drop(inv);
        let g = Event::Key(KeyEvent {
            code: KeyCode::Char('g'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&g);
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        let filled = inv.slots.iter().filter(|s| s.is_some()).count();
        assert_eq!(filled, 1, "non-gold items still occupy a slot");
        assert_eq!(inv.slots[0].as_ref().unwrap().glyph, '!');
    }

    /// v0.5.6 regression: pressing `g` must pick up gold on an adjacent tile,
    /// not just under the player. Reproduces the bug-report scenario where the
    /// player pressed `g` next to a $ and got "주울 아이템이 없습니다" forever.
    ///
    /// v0.5.9 update: gold no longer enters the inventory — verify the gold
    /// went to `App::gold` and the inventory stayed empty.
    #[test]
    fn test_adjacent_pickup() {
        let mut app = App::new(0);
        // Teleport player to (10, 10) so we don't collide with auto-populated
        // loot in the first room.
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(10, 10);
        // Empty the inventory.
        app.world.get::<&mut Inventory>(app.player).unwrap().slots.iter_mut().for_each(|s| *s = None);
        let starting_gold = app.gold();
        // Drop gold to the LEFT of the player.
        app.world.spawn((Position(TilePos(9, 10)), Item::gold(3)));
        // Press 'g'.
        let ev = Event::Key(KeyEvent {
            code: KeyCode::Char('g'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&ev);
        // v0.5.9: gold must have moved to App::gold, not into a slot.
        assert_eq!(
            app.gold(),
            starting_gold + 3,
            "adjacent gold must increment App::gold by 3 (was {}, now {})",
            starting_gold,
            app.gold()
        );
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        assert!(
            inv.slots.iter().all(|s| s.is_none()),
            "inventory must stay empty — gold skips it (v0.5.9)"
        );
        // And the player-facing message must mention 골드.
        assert!(
            app.messages.iter().any(|m| m.contains("골드")),
            "messages should mention gold: {:?}",
            app.messages
        );
    }

    /// v0.5.7: equipping a weapon must flip `Stats.attack_bonus` to its
    /// `bonus` field, and unequipping must clear it back to 0.
    #[test]
    fn test_equip_weapon_updates_attack_bonus() {
        use asciirogue::{equip_inventory_at, recompute_stats_from_equipment, unequip_slot, EquipSlot};
        let mut app = App::new(0);
        // Teleport player and clear inventory.
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(20, 20);
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        drop(inv);
        // Place a +5 sword at slot 0.
        let sword = Item::weapon(5, "롱소드");
        app.world
            .get::<&mut Inventory>(app.player)
            .unwrap()
            .put_at(0, sword)
            .unwrap();
        // No equipment yet → attack_bonus = 0.
        recompute_stats_from_equipment(&mut app.world, app.player);
        let stats = app.world.get::<&Stats>(app.player).unwrap();
        assert_eq!(stats.attack_bonus, 0);
        assert_eq!(stats.defense_bonus, 0);
        drop(stats);
        // Equip it.
        let msg = equip_inventory_at(&mut app.world, app.player, 0).unwrap();
        assert!(msg.contains("장착"), "msg should say equipped: {}", msg);
        let stats = app.world.get::<&Stats>(app.player).unwrap();
        assert_eq!(stats.attack_bonus, 5, "attack_bonus should reflect sword");
        drop(stats);
        // Unequip.
        let _ = unequip_slot(&mut app.world, app.player, EquipSlot::Weapon).unwrap();
        let stats = app.world.get::<&Stats>(app.player).unwrap();
        assert_eq!(stats.attack_bonus, 0, "attack_bonus must clear on unequip");
    }

    /// v0.5.7: armor contributes to defense_bonus, weapon swaps without
    /// losing the other slot.
    #[test]
    fn test_armor_defense_bonus_and_weapon_swap() {
        use asciirogue::{equip_inventory_at, recompute_stats_from_equipment, EquipSlot};
        let mut app = App::new(0);
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(25, 25);
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        drop(inv);
        // Place armor at slot 0, two weapons at slots 1 and 2.
        app.world.get::<&mut Inventory>(app.player).unwrap().put_at(0, Item::armor(3, "가죽갑옷")).unwrap();
        app.world.get::<&mut Inventory>(app.player).unwrap().put_at(1, Item::weapon(4, "단검")).unwrap();
        app.world.get::<&mut Inventory>(app.player).unwrap().put_at(2, Item::weapon(7, "도끼")).unwrap();
        // Equip armor + first weapon.
        equip_inventory_at(&mut app.world, app.player, 0).unwrap();
        equip_inventory_at(&mut app.world, app.player, 1).unwrap();
        recompute_stats_from_equipment(&mut app.world, app.player);
        let s = app.world.get::<&Stats>(app.player).unwrap();
        assert_eq!(s.attack_bonus, 4);
        assert_eq!(s.defense_bonus, 3);
        drop(s);
        // Equip the axe — it should swap with the dagger; armor unaffected.
        equip_inventory_at(&mut app.world, app.player, 2).unwrap();
        let s = app.world.get::<&Stats>(app.player).unwrap();
        assert_eq!(s.attack_bonus, 7, "new weapon must take effect");
        assert_eq!(s.defense_bonus, 3, "armor must NOT be touched by weapon swap");
        drop(s);
        // Inventory should still contain the swapped-out dagger somewhere.
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        let dagger_count = inv.slots.iter().filter(|s| s.as_ref().map_or(false, |i| i.name == "단검")).count();
        assert_eq!(dagger_count, 1, "swapped-out dagger must remain in inventory");
        // Weapon slot should reference slot 2 (the axe).
        let eq = app.world.get::<&Equipment>(app.player).unwrap();
        assert_eq!(eq.weapon, Some(2));
        assert_eq!(eq.armor, Some(0));
    }

    /// v0.5.7: using a healing potion must apply the bonus to HP.
    #[test]
    fn test_use_health_potion() {
        use asciirogue::use_inventory_at;
        let mut app = App::new(0);
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(30, 30);
        // Drain HP to 10/40.
        app.world.get::<&mut Health>(app.player).unwrap().current = 10;
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        inv.put_at(0, Item::potion_hp()).unwrap(); // potion_hp restores +15
        drop(inv);
        let msg = use_inventory_at(&mut app.world, app.player, 0).unwrap();
        assert!(msg.contains("HP +"));
        let h = app.world.get::<&Health>(app.player).unwrap();
        assert_eq!(h.current, 25, "HP must rise by potion.bonus");
    }

    /// v0.5.7: dropping an item onto the player's own tile spawns a ground
    /// entity at that position.
    #[test]
    fn test_drop_inventory_at() {
        use asciirogue::drop_inventory_at;
        let mut app = App::new(0);
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(35, 35);
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        inv.put_at(0, Item::gold(99)).unwrap();
        drop(inv);
        let msg = drop_inventory_at(&mut app.world, app.player, 0).unwrap();
        assert!(msg.contains("내려놓"), "drop msg should be confirmative: {}", msg);
        // Inventory slot 0 is now empty.
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        assert!(inv.slots[0].is_none());
        drop(inv);
        // And there's a ground item at (35, 35).
        let ground = app
            .world
            .query::<(&Position, &Item)>()
            .iter()
            .any(|(_, (p, _))| p.0 == TilePos(35, 35));
        assert!(ground, "dropped item must spawn a ground entity");
    }

    /// v0.5.8: pressing 'p' uses the first available potion without opening
    /// the inventory modal — quick-potion.
    #[test]
    fn test_p_key_quick_potion() {
        let mut app = App::new(0);
        // Drain HP and place a potion in slot 0.
        app.world.get::<&mut Position>(app.player).unwrap().0 = TilePos(40, 40);
        app.world.get::<&mut Health>(app.player).unwrap().current = 5;
        let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
        inv.slots.iter_mut().for_each(|s| *s = None);
        inv.put_at(0, Item::potion_hp()).unwrap();
        drop(inv);
        let before_hp = app.world.get::<&Health>(app.player).unwrap().current;
        let p = Event::Key(KeyEvent {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&p);
        // Modal must NOT open.
        assert!(
            matches!(app.modal, ModalMode::Closed),
            "p must not open the modal: {:?}",
            app.modal
        );
        // HP must rise.
        let after_hp = app.world.get::<&Health>(app.player).unwrap().current;
        assert!(
            after_hp > before_hp,
            "potion must heal ({} → {})",
            before_hp,
            after_hp
        );
        // Slot must be empty.
        let inv = app.world.get::<&Inventory>(app.player).unwrap();
        assert!(inv.slots[0].is_none(), "potion must be consumed");
    }

    /// v0.5.8: pressing 'i' toggles the inventory modal open/close.
    /// 'Esc' closes it. Uppercase 'I' is still accepted inside the modal
    /// for Shift-key muscle memory, but does nothing when the modal is
    /// closed (the dispatcher only listens for lowercase 'i' in that mode).
    #[test]
    fn test_inventory_modal_open_close() {
        let mut app = App::new(0);
        assert!(matches!(app.modal, ModalMode::Closed));
        let i_key = Event::Key(KeyEvent {
            code: KeyCode::Char('i'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        // 1. i opens.
        app.handle(&i_key);
        assert!(
            matches!(app.modal, ModalMode::Inventory { .. }),
            "i must open the modal: {:?}",
            app.modal
        );
        // 2. i again closes (toggle).
        app.handle(&i_key);
        assert!(
            matches!(app.modal, ModalMode::Closed),
            "second i must close the modal (toggle): {:?}",
            app.modal
        );
        // 3. i opens again; Esc closes.
        app.handle(&i_key);
        assert!(matches!(app.modal, ModalMode::Inventory { .. }));
        let esc = Event::Key(KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&esc);
        assert!(
            matches!(app.modal, ModalMode::Closed),
            "Esc must close the modal: {:?}",
            app.modal
        );
        // 4. Uppercase 'I' inside the modal closes it (muscle memory).
        app.handle(&i_key);
        let upper_i = Event::Key(KeyEvent {
            code: KeyCode::Char('I'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        app.handle(&upper_i);
        assert!(
            matches!(app.modal, ModalMode::Closed),
            "uppercase I inside modal must close it (muscle memory): {:?}",
            app.modal
        );
    }

    fn vim_keys_map_to_directions() {
        assert_eq!(key_to_direction(&KeyCode::Char('h')), Some(Direction::Left));
        assert_eq!(key_to_direction(&KeyCode::Char('j')), Some(Direction::Down));
        assert_eq!(key_to_direction(&KeyCode::Char('k')), Some(Direction::Up));
        assert_eq!(key_to_direction(&KeyCode::Char('l')), Some(Direction::Right));
    }

    #[test]
    fn yubn_diagonals() {
        assert_eq!(key_to_direction(&KeyCode::Char('y')), Some(Direction::UpLeft));
        assert_eq!(key_to_direction(&KeyCode::Char('u')), Some(Direction::UpRight));
        assert_eq!(key_to_direction(&KeyCode::Char('b')), Some(Direction::DownLeft));
        assert_eq!(key_to_direction(&KeyCode::Char('n')), Some(Direction::DownRight));
    }

    #[test]
    fn arrow_keys_map_to_cardinals() {
        assert_eq!(key_to_direction(&KeyCode::Left), Some(Direction::Left));
        assert_eq!(key_to_direction(&KeyCode::Right), Some(Direction::Right));
        assert_eq!(key_to_direction(&KeyCode::Up), Some(Direction::Up));
        assert_eq!(key_to_direction(&KeyCode::Down), Some(Direction::Down));
    }

    #[test]
    fn wait_and_others() {
        assert_eq!(key_to_direction(&KeyCode::Char('.')), Some(Direction::None));
        // q / r / c / Esc are handled in App::handle directly — they don't map
        // to a direction (key_to_direction returns None).
        assert_eq!(key_to_direction(&KeyCode::Char('q')), None);
        assert_eq!(key_to_direction(&KeyCode::Char('r')), None);
        assert_eq!(key_to_direction(&KeyCode::Esc), None);
        assert_eq!(key_to_direction(&KeyCode::Char(' ')), None);
    }

    // ─── v0.5.20: stairs arrival modal regression suite ────────────────────────
    //
    // Background: cargo run uses crates/app/src/main.rs (see Cargo.toml
    // [[bin]] path = "src/main.rs"). All v0.5.11–v0.5.19 patches to
    // try_player_act landed in lib.rs only; main.rs's copy of `is_passable`
    // never learned about Tile::StairsDown, so the binary's natural
    // movement was blocked at the stairs tile (modal trigger unreachable).
    // The `~` debug warp bypasses is_passable, which is why the bug
    // looked like "modal handler works fine".

    /// Direct unit test: is_passable(stairs_tile) must be true. If this
    /// returns false, try_player_act returns early at the wall-check and
    /// the modal trigger at the end of the function never runs.
    #[test]
    fn is_passable_accepts_stairs_down() {
        let app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        assert!(
            matches!(app.dungeon.at(sx, sy), Tile::StairsDown),
            "precondition: tile ({sx},{sy}) must be StairsDown (got {:?})",
            app.dungeon.at(sx, sy)
        );
        assert!(
            is_passable(&app.dungeon, sx, sy),
            "is_passable must return true for Tile::StairsDown — \
             otherwise natural movement onto stairs is blocked and the \
             descent confirm modal never fires (v0.5.20)"
        );
    }

    /// End-to-end: position the player on the floor tile directly left
    /// of the stairs, press Right (handle -> try_player_act), and assert
    /// the modal pops. This is the exact user-reported flow:
    /// "자연스럽게 stairs 타일 위로 이동해도 descent confirm modal이 자동으로 뜨지 않음".
    #[test]
    fn walking_onto_stairs_pops_confirm_modal() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        // Put player one step to the LEFT of the stairs.
        let start = TilePos(sx - 1, sy);
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = start;
        }
        assert!(
            matches!(app.dungeon.at(start.0, start.1), Tile::Floor),
            "precondition: starting tile must be Floor, got {:?}",
            app.dungeon.at(start.0, start.1)
        );
        assert!(matches!(app.modal, ModalMode::Closed), "modal must start Closed");

        let pos_before = app.world.get::<&Position>(app.player).unwrap().0;
        eprintln!(
            "[diag] before: pos={:?} stairs=({},{}) start_tile={:?} stairs_tile={:?} passable(stairs)={}",
            pos_before, sx, sy,
            app.dungeon.at(start.0, start.1),
            app.dungeon.at(sx, sy),
            is_passable(&app.dungeon, sx, sy),
        );

        // Walk Right — this is what the user does at the keyboard.
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        let pos_after = app.world.get::<&Position>(app.player).unwrap().0;
        eprintln!(
            "[diag] after: pos={:?} modal={:?} at_stairs={}",
            pos_after, app.modal, app.at_stairs,
        );

        assert!(
            matches!(app.modal, ModalMode::ConfirmStairs { .. }),
            "after walking onto StairsDown, modal must be ConfirmStairs \
             (got {:?}). Player moved {:?} -> {:?}.",
            app.modal, pos_before, pos_after,
        );
        assert!(
            app.at_stairs,
            "after walking onto StairsDown, at_stairs must be true \
             so the header badge renders"
        );
    }

    /// Regression: the `~` debug warp must STILL work (this was the only
    /// path that triggered the modal before v0.5.20). Don't break it.
    #[test]
    fn debug_tilde_warp_still_opens_modal() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('~'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(
            matches!(app.modal, ModalMode::ConfirmStairs { .. }),
            "~ debug warp must still open ConfirmStairs modal (got {:?})",
            app.modal
        );
        assert!(app.at_stairs, "~ debug warp must still set at_stairs");
    }

    /// v0.5.21: the user-reported regression — modal opens but y/n does
    /// nothing. Reproduce the full flow: walk to stairs → press y →
    /// expect modal closed and floor incremented.
    #[test]
    fn pressing_y_in_modal_descends_to_next_floor() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let start_floor = app.floor;
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        // Position player one step to the LEFT of the stairs.
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }

        // Walk Right → modal should pop (v0.5.20).
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }),
            "precondition: modal must be open after walking onto stairs");

        // Now press y. THIS is what the user reports as broken.
        let pos_before_y = app.world.get::<&Position>(app.player).unwrap().0;
        eprintln!("[diag] before y: floor={} pos={:?} modal={:?}",
            app.floor, pos_before_y, app.modal);
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        eprintln!("[diag] after y: floor={} pos={:?} modal={:?}",
            app.floor,
            app.world.get::<&Position>(app.player).unwrap().0,
            app.modal);

        assert!(matches!(app.modal, ModalMode::Closed),
            "v0.5.21: pressing y in ConfirmStairs modal must close it (got {:?})",
            app.modal);
        assert_eq!(app.floor, start_floor + 1,
            "v0.5.21: pressing y must descend to next floor (was {}, now {})",
            start_floor, app.floor);
    }

    /// v0.5.21: n in modal must close without descending.
    #[test]
    fn pressing_n_in_modal_cancels_without_descending() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let start_floor = app.floor;
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }));

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert!(matches!(app.modal, ModalMode::Closed),
            "v0.5.21: pressing n must close the modal (got {:?})", app.modal);
        assert_eq!(app.floor, start_floor,
            "v0.5.21: pressing n must NOT descend (was {}, now {})",
            start_floor, app.floor);
    }

    // ─── v0.5.21: selection-state UX ─────────────────────────────────────
    //
    // The modal now carries a `choice: bool` (true = yes / descend,
    // false = no / cancel). Players navigate with arrow keys / Tab,
    // confirm with Enter, and y/n still work as direct hotkeys.

    /// The modal opens with the "yes / descend" choice selected — the
    /// player walked to the stairs on purpose, so descending is the
    /// natural default. They have to navigate left to back out.
    #[test]
    fn modal_opens_with_yes_selected() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        match app.modal {
            ModalMode::ConfirmStairs { choice } => {
                assert!(choice, "modal must open with yes selected by default (got choice={})", choice);
            }
            other => panic!("expected ConfirmStairs, got {:?}", other),
        }
    }

    /// Arrow Down moves the cursor down the option list to "no / cancel".
    /// The modal stays open — only Enter / y / n close it. v0.5.22: the
    /// navigation is vertical (Up/Down) so it matches the modal's two-row
    /// layout — yes on row 1, no on row 2.
    #[test]
    fn arrow_down_moves_selection_to_no() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: true }));

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: false }),
            "Down arrow must move selection to no (got {:?})", app.modal);
    }

    /// Arrow Up moves the cursor back up to "yes / descend".
    #[test]
    fn arrow_up_moves_selection_to_yes() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: false }));

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: true }),
            "Up arrow must move selection back to yes (got {:?})", app.modal);
    }

    /// Enter with yes selected must descend.
    #[test]
    fn enter_with_yes_selected_descends() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let start_floor = app.floor;
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert_eq!(app.floor, start_floor + 1,
            "Enter with yes selected must descend (was {}, now {})",
            start_floor, app.floor);
        assert!(matches!(app.modal, ModalMode::Closed),
            "Enter must close the modal");
    }

    /// Enter with no selected must cancel without descending.
    #[test]
    fn enter_with_no_selected_cancels() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let start_floor = app.floor;
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: false }));

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert_eq!(app.floor, start_floor,
            "Enter with no selected must NOT descend (was {}, now {})",
            start_floor, app.floor);
        assert!(matches!(app.modal, ModalMode::Closed));
    }

    // ─── v0.5.23: boss gate ─────────────────────────────────────────────────────
    //
    // The v0.5.10-era boss check in `descend_internal` is supposed to
    // block `y` / Enter from descending on the boss floor without
    // defeating the boss. v0.5.23 adds the UX contract: the modal must
    // STAY OPEN when the gate triggers so the player sees the constraint
    // instead of having the modal vanish and assuming descent succeeded.

    /// On the boss floor, walking onto stairs opens the modal but
    /// pressing `y` must NOT descend — the boss is still alive.
    #[test]
    fn cannot_descend_from_boss_floor_without_defeating_boss() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, BOSS_FLOOR);
        let start_floor = app.floor;
        assert_eq!(start_floor, BOSS_FLOOR, "precondition: we are on the boss floor");
        assert!(!app.boss_defeated, "precondition: boss is not yet defeated");

        // Walk to stairs — modal pops.
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("boss floor must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: true }));

        // Press y — must NOT descend because boss is still alive.
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert_eq!(
            app.floor, start_floor,
            "v0.5.23: pressing y on boss floor without defeating boss must NOT descend \
             (was {}, now {})",
            start_floor, app.floor
        );
        assert!(
            app.messages.iter().any(|m| m.contains("보스를 먼저 처치")),
            "modal should log the boss-gate message: {:?}",
            app.messages
        );
    }

    /// v0.5.23: the modal must STAY OPEN when the boss gate triggers —
    /// otherwise the player sees the popup vanish and assumes descent
    /// succeeded. The boss-gate message goes in the log, but the
    /// popup must remain so the player is forced to acknowledge the
    /// constraint (cancel with n/Esc).
    #[test]
    fn boss_gate_keeps_modal_open() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, BOSS_FLOOR);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("boss floor must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: true }));

        // Press y — boss gate triggers.
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert!(
            matches!(app.modal, ModalMode::ConfirmStairs { .. }),
            "v0.5.23: modal must STAY OPEN after boss-gate rejection — \
             otherwise the popup vanishes and the player thinks descent \
             succeeded. Got modal={:?}",
            app.modal
        );
    }

    /// v0.5.23: pressing n on the boss floor cancels the modal — the
    /// player can back out without descending.
    #[test]
    fn boss_gate_n_cancels_modal() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, BOSS_FLOOR);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("boss floor must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }));

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert!(matches!(app.modal, ModalMode::Closed),
            "v0.5.23: n must cancel the modal even when boss gate is active");
    }

    /// Enter with the cursor on "yes" must also be blocked on the boss
    /// floor without defeating — every confirm path goes through
    /// `descend_internal`.
    #[test]
    fn cannot_descend_via_enter_on_boss_floor_without_defeating_boss() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, BOSS_FLOOR);
        let start_floor = app.floor;
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("boss floor must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        // Default choice is yes → Enter descends.
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert_eq!(
            app.floor, start_floor,
            "v0.5.23: pressing Enter on boss floor without defeating boss must NOT descend"
        );
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }),
            "v0.5.23: Enter must also keep the modal open on boss-gate rejection");
    }

    // ─── v0.5.24: boss floor modal hint ──────────────────────────────────────────
    //
    // The user asked for an in-popup hint on the boss floor so they
    // understand the boss gate without having to read the buried log
    // line ("보스를 먼저 처치하세요"). The modal must explicitly tell
    // them: "defeat the boss (D) before you can descend".

    /// On the boss floor, the modal must render an in-popup hint
    /// mentioning the boss glyph (`D`) so the player understands the
    /// gate before they press `y`.
    #[test]
    fn boss_floor_modal_renders_boss_hint() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, BOSS_FLOOR);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("boss floor must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }));

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buf = terminal.backend().buffer().clone();

        // Build a render string — CJK chars are 2 cells wide so we skip
        // the empty continuation cells (matches the v0.5.21 helper).
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let sym = buf[(x, y)].symbol();
                if !sym.is_empty() && sym != " " {
                    rendered.push_str(sym);
                }
            }
            rendered.push('\n');
        }

        assert!(
            rendered.contains("보스"),
            "v0.5.24: boss floor modal must render '보스' hint so the player \
             knows the gate is boss-related — otherwise the popup gives no \
             in-place reason for the rejection. Render:\n{}",
            rendered
                .lines()
                .filter(|l| l.contains("내려") || l.contains("예") || l.contains("취") || l.contains("보"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            rendered.contains("(D)") || rendered.contains('D'),
            "v0.5.24: hint must mention the boss glyph D so the player knows \
             which entity blocks them. Render:\n{}",
            rendered
                .lines()
                .filter(|l| l.contains("보"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// On non-boss floors the boss hint must NOT appear — the gate
    /// doesn't apply, so showing it would mislead the player.
    #[test]
    fn non_boss_floor_modal_does_not_render_boss_hint() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        assert_ne!(app.floor, BOSS_FLOOR);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let sym = buf[(x, y)].symbol();
                if !sym.is_empty() && sym != " " {
                    rendered.push_str(sym);
                }
            }
            rendered.push('\n');
        }

        assert!(
            !rendered.contains("보스"),
            "v0.5.24: non-boss floor modal must NOT show the boss hint — \
             the gate doesn't apply. Modal showed:\n{}",
            rendered
                .lines()
                .filter(|l| l.contains("내려") || l.contains("예") || l.contains("취"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // ─── v0.5.25: player death event ──────────────────────────────────────────────
    //
    // The previous death handler just set game_over=true and pushed two log
    // lines. v0.5.25 treats death as a real end-of-run event:
    //   * Call remembrance.on_run_end(floor, gold, bosses_killed) so partial
    //     progress (marks of the departed, last-wager gold) is saved.
    //   * Persist the remembrance to disk.
    //   * Push a death summary so the player sees what they accomplished.
    //   * The handler fires only once (no double-counting if death-check
    //     somehow runs again while game_over=true).

    /// v0.5.25: when the player dies, the soul remembrance must be
    /// updated with the floor reached and gold at time of death —
    /// even though the run wasn't a clear, partial progress is
    /// preserved so deaths feel meaningful.
    #[test]
    fn death_saves_partial_remembrance() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 4);
        let starting_marks = app.remembrance.marks_of_departed;
        let starting_wager = app.remembrance.last_wager_gold;

        // Inject some gold so the last-wager deposit is non-zero.
        app.gold += 100;

        // Force the player to die by zeroing HP via the world directly.
        if let Ok(mut h) = app.world.get::<&mut Health>(app.player) {
            h.current = 0;
        }

        // Trigger the death handler directly — same path enemy_take_turns
        // uses internally after the player_died check.
        app.on_player_died_for_tests();

        // Floor 4 death must NOT increment clears or champion_count
        // (those only unlock on full clears per on_run_end's contract).
        // But marks_of_departed is allowed to stay the same since no
        // boss was killed on this run.
        assert!(
            app.remembrance.marks_of_departed >= starting_marks,
            "v0.5.25: death must not lose marks (was {}, now {})",
            starting_marks,
            app.remembrance.marks_of_departed
        );
        assert!(
            app.remembrance.last_wager_gold > starting_wager,
            "v0.5.25: death must deposit 10% of gold to last-wager pool \
             (was {}, now {})",
            starting_wager,
            app.remembrance.last_wager_gold
        );
        assert!(app.game_over, "v0.5.25: death must set game_over=true");
    }

    /// v0.5.25: a death summary must appear in the log so the player
    /// sees what they accomplished (floor, gold, marks earned).
    #[test]
    fn death_summary_messages_are_pushed() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 3);
        app.gold += 50;

        if let Ok(mut h) = app.world.get::<&mut Health>(app.player) {
            h.current = 0;
        }
        app.on_player_died_for_tests();

        let joined: String = app.messages.join(" | ");
        assert!(
            joined.contains("쓰러졌") || joined.contains("사망"),
            "v0.5.25: death summary must announce the death. Messages: {}",
            joined
        );
        assert!(
            joined.contains("3F") || joined.contains("3f") || joined.contains("도달"),
            "v0.5.25: death summary must show floor reached (3F). Messages: {}",
            joined
        );
        assert!(
            joined.contains("R") && (joined.contains("새 게임") || joined.contains("재시작")),
            "v0.5.25: death summary must show restart hotkey. Messages: {}",
            joined
        );
    }

    /// v0.5.25: bosses killed during the run must be reflected in the
    /// death summary AND in the marks_of_departed granted to the
    /// remembrance. This is the only way partial progress survives.
    #[test]
    fn death_credits_bosses_killed_to_remembrance() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 6);
        app.gold += 200;
        // Simulate the player having killed 3 bosses during this run
        // before dying on floor 6.
        app.run_bosses_killed = 3;
        let starting_marks = app.remembrance.marks_of_departed;

        if let Ok(mut h) = app.world.get::<&mut Health>(app.player) {
            h.current = 0;
        }
        app.on_player_died_for_tests();

        let joined: String = app.messages.join(" | ");
        assert!(
            joined.contains("보스 3명 처치"),
            "v0.5.25: summary must show bosses killed (3). Messages: {}",
            joined
        );
        assert!(
            joined.contains("+3 표시"),
            "v0.5.25: summary must show marks earned (+3). Messages: {}",
            joined
        );
        assert_eq!(
            app.remembrance.marks_of_departed,
            starting_marks + 3,
            "v0.5.25: death must award marks of the departed for bosses killed"
        );
    }

    /// v0.5.25: the death handler must fire exactly once. If death-check
    /// runs again while game_over=true (e.g. an extra enemy tick before
    /// the modal closes), we must NOT re-deposit gold or re-increment
    /// counters.
    #[test]
    fn death_handler_is_idempotent() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 2);
        app.gold += 200;

        if let Ok(mut h) = app.world.get::<&mut Health>(app.player) {
            h.current = 0;
        }

        let before_marks = app.remembrance.marks_of_departed;
        let before_wager = app.remembrance.last_wager_gold;

        app.on_player_died_for_tests();
        let marks_after_first = app.remembrance.marks_of_departed;
        let wager_after_first = app.remembrance.last_wager_gold;

        app.on_player_died_for_tests();
        app.on_player_died_for_tests();

        assert_eq!(
            app.remembrance.marks_of_departed, marks_after_first,
            "v0.5.25: death handler must be idempotent — marks doubled"
        );
        assert_eq!(
            app.remembrance.last_wager_gold, wager_after_first,
            "v0.5.25: death handler must be idempotent — wager doubled"
        );
        assert!(app.game_over);
        let _ = before_marks;
        let _ = before_wager;
    }

    // ─── v0.5.26: `!` debug cheat — add a potion for testing ────────────────
    //
    // Manual QA for the v0.5.25 death summary was fragile (had to walk
    // the player into an enemy at low HP). Adding a `!` cheat lets us
    // pre-populate inventory without depending on loot drops.

    /// Pressing `!` adds a potion to the player's inventory.
    #[test]
    fn debug_bang_adds_potion() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        // Empty inventory first.
        app.world
            .get::<&mut Inventory>(app.player)
            .unwrap()
            .slots
            .iter_mut()
            .for_each(|s| *s = None);
        let before = app
            .world
            .get::<&Inventory>(app.player)
            .unwrap()
            .slots
            .iter()
            .filter(|s| s.is_some())
            .count();

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('!'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        let after = app
            .world
            .get::<&Inventory>(app.player)
            .unwrap()
            .slots
            .iter()
            .filter(|s| s.is_some())
            .count();
        assert_eq!(
            after,
            before + 1,
            "v0.5.26: `!` must add exactly one slot item (was {}, now {})",
            before,
            after
        );
        let joined: String = app.messages.join(" | ");
        assert!(
            joined.contains("물약") || joined.contains("cheat") || joined.contains("치트"),
            "v0.5.26: `!` must log the cheat. Messages: {}",
            joined
        );
    }

    /// `!` with a full inventory must log a refusal, not silently drop.
    #[test]
    fn debug_bang_refuses_when_inventory_full() {
        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        // Fill every inventory slot with something.
        {
            let mut inv = app.world.get::<&mut Inventory>(app.player).unwrap();
            for i in 0..inv.slots.len() {
                let _ = inv.put_at(i, Item::gold(1));
            }
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Char('!'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));

        let joined: String = app.messages.join(" | ");
        assert!(
            joined.contains("가득") || joined.contains("full") || joined.contains("실패"),
            "v0.5.26: `!` with full inventory must log a refusal. Messages: {}",
            joined
        );
    }
    #[test]
    fn selected_option_renders_with_reverse_style() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: true }));

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let mut found_yes_cell_with_reverse = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                if cell.symbol() == "예"
                    && cell.style().add_modifier.contains(ratatui::style::Modifier::REVERSED)
                {
                    found_yes_cell_with_reverse = true;
                    break;
                }
            }
            if found_yes_cell_with_reverse { break; }
        }
        assert!(found_yes_cell_with_reverse,
            "selected yes option '예' must render with REVERSED style — \
             user has no visual feedback for which option is active");

        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { choice: false }));

        let backend2 = TestBackend::new(80, 24);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2.draw(|f| app.draw(f)).unwrap();
        let buf2 = terminal2.backend().buffer().clone();
        let mut found_cancel_cell_with_reverse = false;
        for y in 0..buf2.area.height {
            for x in 0..buf2.area.width {
                let cell = &buf2[(x, y)];
                if cell.symbol() == "취"
                    && cell.style().add_modifier.contains(ratatui::style::Modifier::REVERSED)
                {
                    found_cancel_cell_with_reverse = true;
                    break;
                }
            }
            if found_cancel_cell_with_reverse { break; }
        }
        assert!(found_cancel_cell_with_reverse,
            "selected '취소' option must render with REVERSED style after navigation");
    }

    /// v0.5.20 surface proof: after walking onto StairsDown, the modal
    /// actually renders to screen (not just the state machine). Drives
    /// `App::draw` through ratatui's TestBackend and reads the buffer.
    /// This is the same code path the real terminal uses.
    #[test]
    fn walking_onto_stairs_renders_modal_to_screen() {
        use ratatui::backend::TestBackend;
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::Span;
        use ratatui::widgets::{Block, Borders, Paragraph};
        use ratatui::Terminal;

        let mut app = App::new_at(0xCAFE_BABE_DEAD_BEEF, 1);
        let (sx, sy) = app
            .dungeon
            .stairs
            .map(|p| (p.0, p.1))
            .expect("floor 1 must place a StairsDown tile");
        {
            let mut pos = app.world.get::<&mut Position>(app.player).unwrap();
            pos.0 = TilePos(sx - 1, sy);
        }
        app.handle(&Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
        assert!(matches!(app.modal, ModalMode::ConfirmStairs { .. }));

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        // App::draw buffer
        let buf = terminal.backend().buffer().clone();

        // Build a string of all cells for searching. CJK chars are 2 cells
        // wide in ratatui: the first cell holds the glyph, the second is
        // an empty continuation. We skip the empty continuations so the
        // resulting string is the actual visible text — that's what a
        // user reads on the terminal.
        let mut rendered = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let sym = buf[(x, y)].symbol();
                if !sym.is_empty() && sym != " " {
                    rendered.push_str(sym);
                }
            }
            rendered.push('\n');
        }

        assert!(
            rendered.contains("내려가기"),
            "modal must render '내려가기' (▼ 내려가기 확인) on screen — \
             walking onto StairsDown produced an invisible modal. \
             Buffer (first 10 lines):\n{}",
            rendered.lines().take(10).collect::<Vec<_>>().join("\n"),
        );
        assert!(
            rendered.contains("예"),
            "modal prompt must include '[y] 예' on screen — \
             user-facing Korean label for the descend hotkey. \
             Buffer (first 10 lines):\n{}",
            rendered.lines().take(10).collect::<Vec<_>>().join("\n"),
        );
    }
}

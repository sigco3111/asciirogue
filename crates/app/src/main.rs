//! asciirogue — Korean-first TUI roguelike with half-block visuals.

use anyhow::{Context, Result};
use asciirogue_procgen::{bsp, Dungeon};
use asciirogue_render::{draw_map, wall_glyph};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::terminal::disable_raw_mode;
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::env;
use std::io;

const MAP_W: i32 = 60;
const MAP_H: i32 = 24;

struct App {
    dungeon: Dungeon,
    seed: u64,
    running: bool,
}

impl App {
    fn new(seed: u64) -> Self {
        let dungeon = bsp::generate(MAP_W, MAP_H, seed, bsp::Config::default());
        Self { dungeon, seed, running: true }
    }

    fn handle(&mut self, ev: &Event) {
        if let Event::Key(k) = ev {
            if k.kind == KeyEventKind::Press {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        self.running = false;
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        self.seed = self.seed.wrapping_add(1);
                        self.dungeon = bsp::generate(MAP_W, MAP_H, self.seed, bsp::Config::default());
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)]).split(area);
        let title = Paragraph::new(Span::from(format!(
            "asciirogue — seed {} | press [Q] quit, [R] new seed",
            self.seed
        )))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, chunks[0]);

        let map_chunk = chunks[1];
        let inner_w = map_chunk.width.min(MAP_W as u16);
        let inner_h = map_chunk.height.min(MAP_H as u16);
        let centered = ratatui::layout::Rect {
            x: map_chunk.x + (map_chunk.width.saturating_sub(inner_w)) / 2,
            y: map_chunk.y + (map_chunk.height.saturating_sub(inner_h)) / 2,
            width: inner_w,
            height: inner_h,
        };
        draw_map(frame, centered, &self.dungeon);

        let status = Paragraph::new(Span::from(format!(
            "{} rooms | size {}x{} | seed {}",
            self.dungeon.rooms.len(),
            self.dungeon.width,
            self.dungeon.height,
            self.seed,
        )));
        frame.render_widget(status, chunks[2]);
    }
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
    for i in 0..count {
        let s = seed.wrapping_add(i as u64);
        let dungeon = bsp::generate(MAP_W, MAP_H, s, bsp::Config::default());
        println!(
            "── seed 0x{:016X}  {} rooms ─────────────────────────────────────────",
            s,
            dungeon.rooms.len()
        );
        for y in 0..MAP_H {
            let mut line = String::with_capacity(MAP_W as usize + 16);
            for x in 0..MAP_W {
                line.push(match dungeon.at(x, y) {
                    asciirogue_procgen::Tile::Wall => wall_glyph(&dungeon, x, y),
                    asciirogue_procgen::Tile::Floor => '.',
                    asciirogue_procgen::Tile::Rock => ' ',
                });
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
        eprintln!("Usage:");
        eprintln!("  asciirogue              Launch the TUI (q / Esc to quit, r for new seed)");
        eprintln!("  asciirogue --dump       Dump a few sample floors to stdout (no TTY required)");
        eprintln!("  asciirogue --dump --count 5 --seed 1  Dump 5 floors with custom seed");
        eprintln!("  asciirogue --help       Show this message");
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
        return u64::from_str_radix(rest, 16)
            .with_context(|| format!("invalid hex seed: {s}"));
    }
    s.parse::<u64>()
        .with_context(|| format!("invalid decimal seed: {s}"))
}

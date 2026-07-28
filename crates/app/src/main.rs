//! asciirogue — Korean-first TUI roguelike with half-block visuals.

use anyhow::{Context, Result};
use asciirogue_procgen::{bsp, Dungeon};
use asciirogue_render::draw_map;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Terminal};
use std::io;
use std::panic;
use std::time::Duration;

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
        // Use ratatui's convenience loop with crossterm backend.
        let _tick = Duration::from_millis(100);
        while app.running {
            terminal.draw(|f| app.draw(f))?;
            if crossterm::event::poll(Duration::from_millis(50))? {
                let ev = crossterm::event::read()?;
                app.handle(&ev);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

fn main() -> Result<()> {
    // Install a panic hook that restores the terminal before printing the panic.
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        original(info);
    }));
    run().context("asciirogue runtime error")
}

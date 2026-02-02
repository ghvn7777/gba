#![allow(dead_code)]

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::DefaultTerminal;

/// Main TUI application state.
pub struct App {
    pub running: bool,
}

impl App {
    pub fn new() -> Self {
        Self { running: true }
    }

    /// Run the TUI event loop.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let _ = frame;
        todo!()
    }

    fn handle_events(&mut self) -> Result<()> {
        if let Event::Key(key) = event::read()?
            && key.code == KeyCode::Char('q')
        {
            self.running = false;
        }
        Ok(())
    }
}

//! TUI module for future interactive terminal UI.
//!
//! This module provides a placeholder for the ratatui-based TUI that will
//! be implemented in a later phase. Currently contains the `App` struct
//! with a minimal event loop skeleton.

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::DefaultTerminal;

/// Main TUI application state.
///
/// Manages the running state and drives the terminal event loop.
/// Full TUI rendering will be implemented in a later phase.
#[allow(dead_code)] // Entire TUI module is scaffolding for a future phase
#[derive(Debug)]
pub struct App {
    /// Whether the application is currently running.
    pub running: bool,
}

#[allow(dead_code)] // Entire TUI module is scaffolding for a future phase
impl App {
    /// Create a new TUI application instance.
    pub fn new() -> Self {
        Self { running: true }
    }

    /// Run the TUI event loop.
    ///
    /// Draws frames and handles keyboard events until the user presses 'q'.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal drawing or event reading fails.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        // Placeholder: full TUI rendering will be implemented in a later phase.
        let _ = frame;
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

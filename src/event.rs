use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, MouseEvent};

/// All events that flow through the app's single channel.
#[derive(Debug)]
pub enum AppEvent {
    /// A key press from the user.
    Key(KeyEvent),
    /// A periodic tick (drives UI refresh).
    Tick,
    /// AI response arrived with a commit message suggestion.
    AiResponse(String),
    /// AI call failed.
    AiError(String),
    /// A mouse event from the user.
    Mouse(MouseEvent),
}

/// Spawns a background thread that reads crossterm events and sends ticks.
pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    _tx: mpsc::Sender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let event_tx = tx.clone();

        thread::spawn(move || {
            loop {
                // Poll for crossterm events with tick_rate timeout.
                // If poll returns true, a real event is ready; read and handle it.
                // Non-key events (resize, mouse) are consumed but do NOT also
                // send a Tick — that would flood the channel during resize bursts.
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        match evt {
                            Event::Key(key) => {
                                if event_tx.send(AppEvent::Key(key)).is_err() {
                                    return;
                                }
                            }
                            Event::Mouse(mouse) => {
                                if event_tx.send(AppEvent::Mouse(mouse)).is_err() {
                                    return;
                                }
                            }
                            // Consumed but ignored — don't also fire a Tick.
                            _ => continue,
                        }
                    }
                }
                // Tick fires either after a real key event or after the timeout.
                if event_tx.send(AppEvent::Tick).is_err() {
                    return;
                }
            }
        });

        Self { rx, _tx: tx }
    }

    /// Get a clone of the sender for pushing AI events back into the channel.
    pub fn sender(&self) -> mpsc::Sender<AppEvent> {
        self._tx.clone()
    }

    /// Receive the next event (blocking).
    pub fn next(&self) -> Result<AppEvent> {
        Ok(self.rx.recv()?)
    }
}

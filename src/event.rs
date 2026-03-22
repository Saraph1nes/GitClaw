use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, MouseEvent, MouseEventKind};

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
                                // Drain any queued scroll events of the same direction
                                // that are already waiting in the OS buffer.  These are
                                // the "momentum" events emitted by trackpads after the
                                // finger lifts — flushing them prevents the view from
                                // continuing to scroll after the user stops.
                                if matches!(
                                    mouse.kind,
                                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                                ) {
                                    while event::poll(Duration::ZERO).unwrap_or(false) {
                                        match event::read() {
                                            Ok(Event::Mouse(m))
                                                if m.kind == mouse.kind => {
                                                // Same-direction queued event — discard it.
                                            }
                                            Ok(other) => {
                                                // Different event: we consumed it from the
                                                // queue but can't push it back.  Re-send it
                                                // through our channel so it isn't lost.
                                                let resend = match other {
                                                    Event::Key(k) => Some(AppEvent::Key(k)),
                                                    Event::Mouse(m) => Some(AppEvent::Mouse(m)),
                                                    _ => None,
                                                };
                                                if let Some(ev) = resend {
                                                    if event_tx.send(ev).is_err() {
                                                        return;
                                                    }
                                                }
                                                break;
                                            }
                                            Err(_) => break,
                                        }
                                    }
                                }
                                if event_tx.send(AppEvent::Mouse(mouse)).is_err() {
                                    return;
                                }
                                // Skip the Tick that follows — the main loop will
                                // redraw immediately upon receiving the Mouse event,
                                // so a second draw from the Tick is wasted work.
                                continue;
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

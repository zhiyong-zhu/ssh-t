mod app;
mod config;
mod cred;
mod sftp;
mod ssh;
mod terminal;
mod tui;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

fn main() -> Result<()> {
    // Create tokio runtime
    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();

    // Load config
    let config = config::AppConfig::load().unwrap_or_default();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let mut app = app::App::new(config);
    run_app(&mut terminal, &mut app)?;

    // Cleanup: disconnect all sessions
    for session in &mut app.sessions {
        if let Some(mut mgr) = session.manager.take() {
            rt.block_on(mgr.disconnect()).ok();
        }
    }
    drop(_guard);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut app::App,
) -> Result<()> {
    // Initial terminal size
    let area = terminal.size()?;
    app.update_terminal_size(area.width, area.height);

    loop {
        // Draw (tab layout is stored in app by the draw function)
        terminal.draw(|f| tui::draw(f, app))?;

        // Poll events with timeout for async task polling
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    // Windows sends key events for both press and release; filter to press only
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    if app.handle_global_key(key)? {
                        continue;
                    }

                    app.handle_key(key)?;
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse)?;
                }
                Event::Resize(cols, rows) => {
                    // Update terminal size for PTY
                    app.update_terminal_size(cols, rows);
                }
                _ => {}
            }
        }

        // Poll SSH events
        app.poll_ssh_events();

        if !app.running {
            break;
        }
    }

    Ok(())
}

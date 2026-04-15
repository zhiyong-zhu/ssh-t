use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent, KeyModifiers};

use crate::config::{AppConfig, AuthMethod, HostConfig};
use crate::ssh::{SshEvent, SshManager};
use crate::sftp::{FileEntry, SftpEngine, TransferEvent, TransferState};
use crate::terminal::TermScreen;
use ratatui::widgets::*;
use tokio::sync::{mpsc, oneshot};

/// Messages from async SFTP operations.
#[derive(Debug)]
enum SftpOp {
    Listed { path: String, entries: Vec<FileEntry> },
    Error(String),
}

/// Current active panel in the TUI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    HostList,
    Terminal,
    Sftp,
    Help,
}

/// Dialog state for interactive inputs.
#[derive(Debug, Clone)]
pub enum Dialog {
    None,
    PasswordInput {
        host: HostConfig,
        password: String,
        error: Option<String>,
    },
    Connecting {
        host_name: String,
    },
    HostForm {
        edit_index: Option<usize>,
        name: String,
        host: String,
        port: String,
        user: String,
        auth: AuthMethod,
        group: String,
        field: usize,
        error: Option<String>,
    },
}

/// A single SSH session (one per connection).
pub struct Session {
    pub id: usize,
    pub name: String,
    pub host_config: HostConfig,
    pub manager: Option<SshManager>,
    pub term_screen: TermScreen,
    pub manager_rx: Option<oneshot::Receiver<SshManager>>,
    // SFTP state per session
    pub sftp_entries: Vec<FileEntry>,
    pub sftp_remote_dir: String,
    pub sftp_state: TableState,
    pub sftp_tx: Option<mpsc::UnboundedSender<TransferEvent>>,
    pub sftp_rx: Option<mpsc::UnboundedReceiver<TransferEvent>>,
    pub sftp_op_rx: Option<mpsc::UnboundedReceiver<SftpOp>>,
    pub transfer_state: Option<TransferState>,
}

impl Session {
    pub fn new(id: usize, host_config: HostConfig) -> Self {
        Self {
            id,
            name: host_config.name.clone(),
            host_config,
            manager: None,
            term_screen: TermScreen::new(5000),
            manager_rx: None,
            sftp_entries: Vec::new(),
            sftp_remote_dir: "/".to_string(),
            sftp_state: TableState::default(),
            sftp_tx: None,
            sftp_rx: None,
            sftp_op_rx: None,
            transfer_state: None,
        }
    }
}

/// The main application state.
pub struct App {
    pub config: AppConfig,
    pub panel: Panel,
    pub running: bool,
    pub dialog: Dialog,

    // Host list state
    pub host_list_state: ListState,
    pub host_filter: String,

    // Terminal size (for PTY)
    pub terminal_cols: u16,
    pub terminal_rows: u16,

    // Multi-session support
    pub sessions: Vec<Session>,
    pub active_session: usize,
    pub next_session_id: usize,

    // Global SSH event channel
    pub ssh_rx: Option<mpsc::UnboundedReceiver<SshEvent>>,
    pub ssh_event_tx: mpsc::UnboundedSender<SshEvent>,

    // Status
    pub status_msg: String,
}

impl App {
    pub fn new(mut config: AppConfig) -> Self {
        let (ssh_event_tx, ssh_rx) = mpsc::unbounded_channel();

        // Get current username
        let current_user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string());

        // Add example hosts if config is empty
        if config.hosts.is_empty() {
            config.hosts = vec![
                HostConfig {
                    name: "localhost".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 22,
                    user: current_user,
                    auth: AuthMethod::Password,
                    group: String::new(),
                    tags: vec![],
                    jump_host: None,
                    notes: "Local machine".to_string(),
                },
                HostConfig {
                    name: "example-server".to_string(),
                    host: "example.com".to_string(),
                    port: 22,
                    user: "user".to_string(),
                    auth: AuthMethod::Password,
                    group: String::new(),
                    tags: vec![],
                    jump_host: None,
                    notes: "Example server".to_string(),
                },
            ];
        }

        Self {
            config,
            panel: Panel::HostList,
            running: true,
            dialog: Dialog::None,
            host_list_state: ListState::default(),
            host_filter: String::new(),
            terminal_cols: 80,
            terminal_rows: 24,
            sessions: Vec::new(),
            active_session: 0,
            next_session_id: 1,
            ssh_rx: Some(ssh_rx),
            ssh_event_tx,
            status_msg: "Welcome to ssh-t | Enter: connect | ?: help".to_string(),
        }
    }

    /// Check if a dialog is currently active.
    pub fn has_dialog(&self) -> bool {
        !matches!(self.dialog, Dialog::None)
    }

    /// Get the currently active session, if any.
    pub fn active_session(&self) -> Option<&Session> {
        self.sessions.get(self.active_session)
    }

    /// Get the currently active session mutably, if any.
    pub fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active_session)
    }

    /// Check if there is any connected session.
    pub fn has_active_connection(&self) -> bool {
        self.sessions.get(self.active_session)
            .map(|s| s.manager.is_some())
            .unwrap_or(false)
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if !matches!(self.dialog, Dialog::None) {
            return self.handle_dialog_key(key);
        }

        match self.panel {
            Panel::HostList => self.handle_host_list_key(key),
            Panel::Terminal => self.handle_terminal_key(key),
            Panel::Sftp => self.handle_sftp_key(key),
            Panel::Help => self.handle_help_key(key),
        }
    }

    fn handle_dialog_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;

        let dialog = std::mem::take(&mut self.dialog);

        match dialog {
            Dialog::PasswordInput { host, password, error } => {
                match key.code {
                    KeyCode::Esc => {
                        self.status_msg = "Connection canceled".to_string();
                    }
                    KeyCode::Enter => {
                        let pwd = password.clone();

                        // Store password for future use
                        if let Err(e) = crate::cred::CredentialStore::store_password(
                            &host.host, &host.user, &pwd,
                        ) {
                            eprintln!("Warning: failed to store password: {e}");
                        }

                        let host_name = host.name.clone();
                        self.dialog = Dialog::Connecting { host_name: host_name.clone() };
                        self.status_msg = format!("Connecting to {}...", host.name);

                        // Create new session
                        let session_id = self.next_session_id;
                        self.next_session_id += 1;
                        let mut session = Session::new(session_id, host.clone());

                        let (sftp_tx, sftp_rx) = mpsc::unbounded_channel();
                        session.sftp_tx = Some(sftp_tx);
                        session.sftp_rx = Some(sftp_rx);

                        let (manager_tx, manager_rx) = oneshot::channel();
                        session.manager_rx = Some(manager_rx);

                        self.sessions.push(session);
                        self.active_session = self.sessions.len() - 1;

                        let event_tx = self.ssh_event_tx.clone();
                        let cols = self.terminal_cols;
                        let rows = self.terminal_rows;

                        tokio::spawn(async move {
                            let mut manager = SshManager::with_password(session_id, host, event_tx.clone(), pwd);

                            if let Err(e) = manager.connect().await {
                                event_tx.send(SshEvent::Error { id: session_id, message: e.to_string() }).ok();
                                return;
                            }

                            if let Err(e) = manager.open_shell(cols, rows).await {
                                event_tx.send(SshEvent::Error { id: session_id, message: format!("Shell: {e}") }).ok();
                                return;
                            }

                            // Send manager back to main thread
                            if manager_tx.send(manager).is_err() {
                                event_tx.send(SshEvent::Error { id: session_id, message: "Failed to send manager".to_string() }).ok();
                                return;
                            }

                            event_tx.send(SshEvent::Connected { id: session_id, name: host_name }).ok();
                        });
                    }
                    KeyCode::Char(c) => {
                        self.dialog = Dialog::PasswordInput {
                            host,
                            password: password + &c.to_string(),
                            error,
                        };
                    }
                    KeyCode::Backspace => {
                        let mut pwd = password;
                        pwd.pop();
                        self.dialog = Dialog::PasswordInput { host, password: pwd, error };
                    }
                    _ => {
                        self.dialog = Dialog::PasswordInput { host, password, error };
                    }
                }
            }
            Dialog::Connecting { host_name } => {
                if key.code == KeyCode::Esc {
                    self.status_msg = "Connection canceled".to_string();
                } else {
                    self.dialog = Dialog::Connecting { host_name };
                }
            }
            Dialog::HostForm {
                edit_index,
                name,
                host,
                port,
                user,
                auth,
                group,
                field,
                error,
            } => {
                let max_field = 5;
                match key.code {
                    KeyCode::Esc => {
                        self.dialog = Dialog::None;
                    }
                    KeyCode::Tab => {
                        let next = if field >= max_field { 0 } else { field + 1 };
                        self.dialog = Dialog::HostForm {
                            edit_index, name, host, port, user, auth, group, field: next, error: None,
                        };
                    }
                    KeyCode::BackTab => {
                        let prev = if field == 0 { max_field } else { field - 1 };
                        self.dialog = Dialog::HostForm {
                            edit_index, name, host, port, user, auth, group, field: prev, error: None,
                        };
                    }
                    KeyCode::Enter => {
                        // Validate and save
                        if name.trim().is_empty() {
                            self.dialog = Dialog::HostForm {
                                edit_index, name, host, port, user, auth, group, field,
                                error: Some("Name cannot be empty".to_string()),
                            };
                            return Ok(());
                        }
                        if host.trim().is_empty() {
                            self.dialog = Dialog::HostForm {
                                edit_index, name, host, port, user, auth, group, field,
                                error: Some("Host cannot be empty".to_string()),
                            };
                            return Ok(());
                        }
                        if user.trim().is_empty() {
                            self.dialog = Dialog::HostForm {
                                edit_index, name, host, port, user, auth, group, field,
                                error: Some("User cannot be empty".to_string()),
                            };
                            return Ok(());
                        }
                        let port_val: u16 = match port.parse() {
                            Ok(p) => p,
                            Err(_) => {
                                self.dialog = Dialog::HostForm {
                                    edit_index, name, host, port, user, auth, group, field,
                                    error: Some("Invalid port number".to_string()),
                                };
                                return Ok(());
                            }
                        };

                        let new_host = HostConfig {
                            name: name.trim().to_string(),
                            host: host.trim().to_string(),
                            port: port_val,
                            user: user.trim().to_string(),
                            auth: auth.clone(),
                            group: group.trim().to_string(),
                            tags: vec![],
                            jump_host: None,
                            notes: String::new(),
                        };

                        match edit_index {
                            Some(idx) => {
                                if idx < self.config.hosts.len() {
                                    self.config.hosts[idx] = new_host;
                                }
                            }
                            None => {
                                self.config.hosts.push(new_host);
                            }
                        }

                        if let Err(e) = self.config.save() {
                            self.status_msg = format!("Save failed: {e}");
                        } else {
                            self.status_msg = "Config saved".to_string();
                        }

                        self.dialog = Dialog::None;
                    }
                    KeyCode::Backspace => {
                        let (mut n, mut h, mut p, mut u, mut g) = (name, host, port, user, group);
                        match field {
                            0 => { n.pop(); }
                            1 => { h.pop(); }
                            2 => { p.pop(); }
                            3 => { u.pop(); }
                            5 => { g.pop(); }
                            _ => {}
                        }
                        self.dialog = Dialog::HostForm {
                            edit_index, name: n, host: h, port: p, user: u, auth, group: g, field, error: None,
                        };
                    }
                    KeyCode::Char(' ') if field == 4 => {
                        let new_auth = match auth {
                            AuthMethod::Password => AuthMethod::Key { key_path: String::new() },
                            AuthMethod::Key { .. } => AuthMethod::Agent,
                            AuthMethod::Agent => AuthMethod::Password,
                        };
                        self.dialog = Dialog::HostForm {
                            edit_index, name, host, port, user, auth: new_auth, group, field, error: None,
                        };
                    }
                    KeyCode::Char(c) => {
                        // Field 4 (auth): only space cycles, other chars ignored
                        if field == 4 {
                            self.dialog = Dialog::HostForm {
                                edit_index, name, host, port, user, auth, group, field, error: None,
                            };
                            return Ok(());
                        }
                        let (mut n, mut h, mut p, mut u, mut g) = (name, host, port, user, group);
                        match field {
                            0 => { n.push(c); }
                            1 => { h.push(c); }
                            2 => { p.push(c); }
                            3 => { u.push(c); }
                            5 => { g.push(c); }
                            _ => {}
                        }
                        self.dialog = Dialog::HostForm {
                            edit_index, name: n, host: h, port: p, user: u, auth, group: g, field, error: None,
                        };
                    }
                    _ => {
                        self.dialog = Dialog::HostForm {
                            edit_index, name, host, port, user, auth, group, field, error,
                        };
                    }
                }
            }
            Dialog::None => {}
        }
        Ok(())
    }

    /// Handle a mouse event.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        use crossterm::event::{MouseEventKind, MouseButton};

        if !matches!(self.dialog, Dialog::None) {
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let row = mouse.row as usize;
                match self.panel {
                    Panel::HostList => {
                        let filtered = self.filtered_hosts();
                        if row > 0 && row <= filtered.len() {
                            self.host_list_state.select(Some(row - 1));
                        }
                    }
                    Panel::Sftp => {
                        if let Some(session) = self.active_session() {
                            if row > 0 && row <= session.sftp_entries.len() {
                                if let Some(session) = self.sessions.get_mut(self.active_session) {
                                    session.sftp_state.select(Some(row - 1));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp => {
                match self.panel {
                    Panel::HostList => {
                        let i = self.host_list_state.selected().unwrap_or(0);
                        if i > 0 {
                            self.host_list_state.select(Some(i - 1));
                        }
                    }
                    Panel::Sftp => {
                        if let Some(session) = self.sessions.get_mut(self.active_session) {
                            let i = session.sftp_state.selected().unwrap_or(0);
                            if i > 0 {
                                session.sftp_state.select(Some(i - 1));
                            }
                        }
                    }
                    _ => {}
                }
            }
            MouseEventKind::ScrollDown => {
                match self.panel {
                    Panel::HostList => {
                        let filtered = self.filtered_hosts();
                        let i = self.host_list_state.selected().unwrap_or(0);
                        if i < filtered.len().saturating_sub(1) {
                            self.host_list_state.select(Some(i + 1));
                        }
                    }
                    Panel::Sftp => {
                        if let Some(session) = self.sessions.get_mut(self.active_session) {
                            let i = session.sftp_state.selected().unwrap_or(0);
                            if i < session.sftp_entries.len().saturating_sub(1) {
                                session.sftp_state.select(Some(i + 1));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Poll and process SSH events.
    pub fn poll_ssh_events(&mut self) {
        // Poll manager from oneshot channels for all sessions
        for session in &mut self.sessions {
            if let Some(mut rx) = session.manager_rx.take() {
                match rx.try_recv() {
                    Ok(manager) => {
                        session.manager = Some(manager);
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {
                        // Not ready yet, put it back
                        session.manager_rx = Some(rx);
                    }
                    Err(_) => {
                        // Channel closed, manager was dropped
                    }
                }
            }
        }

        // Poll SSH events and route to sessions
        // Collect events first to avoid borrow conflicts
        let mut events: Vec<SshEvent> = Vec::new();
        if let Some(rx) = &mut self.ssh_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        // Process collected events
        for event in events {
            let id = event.session_id();
            match event {
                SshEvent::Output { data, .. } => {
                    if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
                        session.term_screen.process(&data);
                    }
                }
                SshEvent::Connected { name, .. } => {
                    self.status_msg = format!("Connected to {name}");
                    self.dialog = Dialog::None;
                    self.panel = Panel::Terminal;
                    if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
                        session.term_screen.clear();
                    }
                }
                SshEvent::Disconnected { reason, .. } => {
                    self.status_msg = format!("Disconnected: {reason}");
                    if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
                        session.manager = None;
                    }
                    // Find and remove the session
                    if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
                        self.remove_session(pos);
                    }
                }
                SshEvent::Error { message, .. } => {
                    self.status_msg = format!("Error: {message}");
                    if matches!(self.dialog, Dialog::Connecting { .. }) {
                        self.dialog = Dialog::None;
                        self.panel = Panel::HostList;
                    }
                    // Remove the failed session
                    if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
                        self.remove_session(pos);
                    }
                }
                SshEvent::SftpReady { .. } => {
                    self.status_msg = "SFTP ready".to_string();
                }
            }
        }

        // Poll SFTP operation results for active session
        if let Some(session) = self.sessions.get_mut(self.active_session) {
            if let Some(rx) = &mut session.sftp_op_rx {
                while let Ok(op) = rx.try_recv() {
                    match op {
                        SftpOp::Listed { path, entries } => {
                            session.sftp_remote_dir = path;
                            session.sftp_entries = entries;
                            session.sftp_state.select(None);
                            self.status_msg = format!("Loaded: {}", session.sftp_remote_dir);
                        }
                        SftpOp::Error(e) => {
                            self.status_msg = format!("SFTP Error: {e}");
                        }
                    }
                }
            }

            // Poll SFTP transfer events for active session
            if let Some(rx) = &mut session.sftp_rx {
                while let Ok(event) = rx.try_recv() {
                    match event {
                        TransferEvent::Started { file, total, is_upload } => {
                            session.transfer_state = Some(TransferState {
                                file: file.clone(),
                                transferred: 0,
                                total,
                                is_upload,
                            });
                            let direction = if is_upload { "Uploading" } else { "Downloading" };
                            self.status_msg = format!("{}: {}", direction, file);
                        }
                        TransferEvent::Progress { file, transferred, total } => {
                            if let Some(ref mut state) = session.transfer_state {
                                state.transferred = transferred;
                            }
                            let pct = if total > 0 { (transferred * 100 / total) as u8 } else { 0 };
                            self.status_msg = format!("Transfer: {}% - {}", pct, file);
                        }
                        TransferEvent::Completed { file } => {
                            session.transfer_state = None;
                            self.status_msg = format!("Completed: {}", file);
                        }
                        TransferEvent::Error { file, error } => {
                            session.transfer_state = None;
                            self.status_msg = format!("Error: {} - {}", file, error);
                        }
                    }
                }
            }
        }
    }

    /// Remove a session and adjust active_session index.
    fn remove_session(&mut self, pos: usize) {
        self.sessions.remove(pos);
        if self.sessions.is_empty() {
            self.active_session = 0;
            if self.panel == Panel::Terminal || self.panel == Panel::Sftp {
                self.panel = Panel::HostList;
            }
        } else {
            if self.active_session >= self.sessions.len() {
                self.active_session = self.sessions.len() - 1;
            } else if pos < self.active_session {
                self.active_session -= 1;
            }
        }
    }

    /// Switch to the next session.
    pub fn next_session(&mut self) {
        if self.sessions.len() > 1 {
            self.active_session = (self.active_session + 1) % self.sessions.len();
            self.status_msg = format!("Switched to: {}", self.sessions[self.active_session].name);
            // Resize the new active session
            if let Some(session) = self.sessions.get(self.active_session) {
                if let Some(ref mgr) = session.manager {
                    let _ = mgr.resize(self.terminal_cols, self.terminal_rows);
                }
            }
        }
    }

    /// Switch to the previous session.
    pub fn prev_session(&mut self) {
        if self.sessions.len() > 1 {
            if self.active_session == 0 {
                self.active_session = self.sessions.len() - 1;
            } else {
                self.active_session -= 1;
            }
            self.status_msg = format!("Switched to: {}", self.sessions[self.active_session].name);
            if let Some(session) = self.sessions.get(self.active_session) {
                if let Some(ref mgr) = session.manager {
                    let _ = mgr.resize(self.terminal_cols, self.terminal_rows);
                }
            }
        }
    }

    /// Get filtered host list.
    pub fn filtered_hosts(&self) -> Vec<&HostConfig> {
        self.config.hosts.iter().filter(|h| {
            if self.host_filter.is_empty() {
                true
            } else {
                h.name.contains(&self.host_filter)
                    || h.host.contains(&self.host_filter)
                    || h.user.contains(&self.host_filter)
            }
        }).collect()
    }

    /// Update terminal size.
    pub fn update_terminal_size(&mut self, cols: u16, rows: u16) {
        // In terminal full-screen mode, use full size; otherwise subtract chrome
        let (pty_cols, pty_rows) = if self.panel == Panel::Terminal && self.has_active_connection() {
            (cols, rows)
        } else {
            (cols.saturating_sub(2), rows.saturating_sub(4))
        };

        if self.terminal_cols != pty_cols || self.terminal_rows != pty_rows {
            self.terminal_cols = pty_cols;
            self.terminal_rows = pty_rows;
            // Resize the active session
            if let Some(session) = self.active_session() {
                if let Some(ref mgr) = session.manager {
                    let _ = mgr.resize(pty_cols, pty_rows);
                }
            }
        }
    }

    // -- Key handlers per panel --

    fn handle_host_list_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;
        let filtered = self.filtered_hosts();

        match key.code {
            KeyCode::Char('q') => {
                self.running = false;
            }
            KeyCode::Char('?') => {
                self.panel = Panel::Help;
            }
            KeyCode::Char('a') => {
                // Add new host
                let current_user = std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "root".to_string());
                self.dialog = Dialog::HostForm {
                    edit_index: None,
                    name: String::new(),
                    host: String::new(),
                    port: "22".to_string(),
                    user: current_user,
                    auth: AuthMethod::Password,
                    group: String::new(),
                    field: 0,
                    error: None,
                };
            }
            KeyCode::Char('e') => {
                // Edit selected host
                if let Some(i) = self.host_list_state.selected() {
                    if let Some(host) = self.config.hosts.get(i) {
                        self.dialog = Dialog::HostForm {
                            edit_index: Some(i),
                            name: host.name.clone(),
                            host: host.host.clone(),
                            port: host.port.to_string(),
                            user: host.user.clone(),
                            auth: host.auth.clone(),
                            group: host.group.clone(),
                            field: 0,
                            error: None,
                        };
                    }
                }
            }
            KeyCode::Char('d') => {
                // Delete selected host
                if let Some(i) = self.host_list_state.selected() {
                    if i < self.config.hosts.len() {
                        let name = self.config.hosts[i].name.clone();
                        self.config.hosts.remove(i);
                        if let Err(e) = self.config.save() {
                            self.status_msg = format!("Save failed: {e}");
                        } else {
                            self.status_msg = format!("Deleted: {}", name);
                        }
                        // Adjust selection
                        let len = self.config.hosts.len();
                        if len == 0 {
                            self.host_list_state.select(None);
                        } else if i >= len {
                            self.host_list_state.select(Some(len - 1));
                        }
                    }
                }
            }
            KeyCode::Down => {
                let i = self.host_list_state.selected().unwrap_or(0);
                if i < filtered.len().saturating_sub(1) {
                    self.host_list_state.select(Some(i + 1));
                }
            }
            KeyCode::Up => {
                let i = self.host_list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.host_list_state.select(Some(i - 1));
                }
            }
            KeyCode::Enter => {
                if let Some(i) = self.host_list_state.selected() {
                    if let Some(host) = filtered.get(i) {
                        self.initiate_connection((*host).clone());
                    }
                }
            }
            KeyCode::Backspace => {
                self.host_filter.pop();
            }
            KeyCode::Char(c) => {
                if c != 'q' && c != '?' && c != 'a' && c != 'e' && c != 'd' {
                    self.host_filter.push(c);
                }
            }
            KeyCode::Esc => {
                self.host_filter.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn initiate_connection(&mut self, host: HostConfig) {
        match &host.auth {
            AuthMethod::Password => {
                match crate::cred::CredentialStore::get_password(&host.host, &host.user) {
                    Ok(_) => self.start_connection(host),
                    Err(_) => {
                        self.dialog = Dialog::PasswordInput {
                            host, password: String::new(), error: None,
                        };
                    }
                }
            }
            AuthMethod::Key { key_path } => {
                match russh_keys::load_secret_key(key_path, None) {
                    Ok(_) => self.start_connection(host),
                    Err(_) => {
                        self.dialog = Dialog::PasswordInput {
                            host, password: String::new(),
                            error: Some("Enter key passphrase".to_string()),
                        };
                    }
                }
            }
            AuthMethod::Agent => {
                self.start_connection(host);
            }
        }
    }

    fn start_connection(&mut self, host: HostConfig) {
        let host_name = host.name.clone();
        self.dialog = Dialog::Connecting { host_name: host_name.clone() };
        self.status_msg = format!("Connecting to {}...", host.name);

        // Create new session
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        let mut session = Session::new(session_id, host.clone());

        let (sftp_tx, sftp_rx) = mpsc::unbounded_channel();
        session.sftp_tx = Some(sftp_tx);
        session.sftp_rx = Some(sftp_rx);

        let (manager_tx, manager_rx) = oneshot::channel();
        session.manager_rx = Some(manager_rx);

        self.sessions.push(session);
        self.active_session = self.sessions.len() - 1;

        let event_tx = self.ssh_event_tx.clone();
        let cols = self.terminal_cols;
        let rows = self.terminal_rows;

        tokio::spawn(async move {
            let mut manager = SshManager::new(session_id, host, event_tx.clone());

            if let Err(e) = manager.connect().await {
                event_tx.send(SshEvent::Error { id: session_id, message: e.to_string() }).ok();
                return;
            }

            if let Err(e) = manager.open_shell(cols, rows).await {
                event_tx.send(SshEvent::Error { id: session_id, message: format!("Shell: {e}") }).ok();
                return;
            }

            // Send manager back to main thread
            if manager_tx.send(manager).is_err() {
                event_tx.send(SshEvent::Error { id: session_id, message: "Failed to send manager".to_string() }).ok();
                return;
            }

            event_tx.send(SshEvent::Connected { id: session_id, name: host_name }).ok();
        });
    }

    fn handle_terminal_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => {
                // Disconnect current session and return to host list
                if let Some(session) = self.sessions.get_mut(self.active_session) {
                    if let Some(mut mgr) = session.manager.take() {
                        tokio::spawn(async move {
                            let _ = mgr.disconnect().await;
                        });
                    }
                }
                self.remove_session(self.active_session);
            }
            KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
                // Switch to next session
                self.next_session();
            }
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                // Switch to previous session
                self.prev_session();
            }
            KeyCode::Esc => {
                // Return to host list without disconnecting
                self.panel = Panel::HostList;
            }
            KeyCode::Enter => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\r")?; }
                }
            }
            KeyCode::Backspace => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x7f")?; }
                }
            }
            KeyCode::Delete => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[3~")?; }
                }
            }
            KeyCode::Left => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[D")?; }
                }
            }
            KeyCode::Right => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[C")?; }
                }
            }
            KeyCode::Up => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[A")?; }
                }
            }
            KeyCode::Down => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[B")?; }
                }
            }
            KeyCode::Home => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[H")?; }
                }
            }
            KeyCode::End => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[F")?; }
                }
            }
            KeyCode::PageUp => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[5~")?; }
                }
            }
            KeyCode::PageDown => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\x1b[6~")?; }
                }
            }
            KeyCode::Tab => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager { mgr.send_input(b"\t")?; }
                }
            }
            KeyCode::Char(c) => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager {
                        if key.modifiers == KeyModifiers::CONTROL {
                            mgr.send_input(&[(c as u8) & 0x1F])?;
                        } else {
                            let mut buf = [0u8; 4];
                            mgr.send_input(c.encode_utf8(&mut buf).as_bytes())?;
                        }
                    }
                }
            }
            KeyCode::F(n) => {
                if let Some(session) = self.active_session() {
                    if let Some(ref mgr) = session.manager {
                        let seq = match n {
                            1 => "\x1bOP", 2 => "\x1bOQ", 3 => "\x1bOR", 4 => "\x1bOS",
                            5 => "\x1b[15~", 6 => "\x1b[17~", 7 => "\x1b[18~", 8 => "\x1b[19~",
                            9 => "\x1b[20~", 10 => "\x1b[21~", 11 => "\x1b[23~", 12 => "\x1b[24~",
                            _ => "",
                        };
                        if !seq.is_empty() { mgr.send_input(seq.as_bytes())?; }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_sftp_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;

        // Don't process if transfer in progress
        if self.sessions.get(self.active_session).map(|s| s.transfer_state.is_some()).unwrap_or(false) {
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.panel = Panel::HostList;
            }
            KeyCode::Char('?') => {
                self.panel = Panel::Help;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(session) = self.sessions.get_mut(self.active_session) {
                    let i = session.sftp_state.selected().unwrap_or(0);
                    if i < session.sftp_entries.len().saturating_sub(1) {
                        session.sftp_state.select(Some(i + 1));
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(session) = self.sessions.get_mut(self.active_session) {
                    let i = session.sftp_state.selected().unwrap_or(0);
                    if i > 0 {
                        session.sftp_state.select(Some(i - 1));
                    }
                }
            }
            KeyCode::Enter => {
                // Enter directory
                let entry = self.sessions.get(self.active_session)
                    .and_then(|s| s.sftp_state.selected())
                    .and_then(|i| self.sessions.get(self.active_session)?.sftp_entries.get(i).cloned());
                if let Some(entry) = entry {
                    if entry.is_dir {
                        self.navigate_sftp_dir(&entry.name);
                    }
                }
            }
            KeyCode::Backspace => {
                // Go up one directory
                self.navigate_sftp_up();
            }
            KeyCode::Char('r') => {
                // Refresh
                self.refresh_sftp();
            }
            KeyCode::Char('d') => {
                // Download selected file
                let entry = self.sessions.get(self.active_session)
                    .and_then(|s| s.sftp_state.selected())
                    .and_then(|i| self.sessions.get(self.active_session)?.sftp_entries.get(i).cloned());
                if let Some(entry) = entry {
                    if !entry.is_dir {
                        self.download_file(&entry.name, &entry.path);
                    }
                }
            }
            KeyCode::Char('u') => {
                // Upload - show file dialog (placeholder)
                self.status_msg = "Upload: Enter local file path".to_string();
            }
            _ => {}
        }
        Ok(())
    }

    fn navigate_sftp_dir(&mut self, name: &str) {
        if self.sessions.get(self.active_session).is_none() {
            self.status_msg = "SFTP not connected".to_string();
            return;
        }

        let current = self.sessions.get(self.active_session)
            .map(|s| s.sftp_remote_dir.clone())
            .unwrap_or_default();
        let new_path = if current == "/" {
            format!("/{}", name.trim_start_matches('/'))
        } else {
            format!("{}/{}", current.trim_end_matches('/'), name.trim_start_matches('/'))
        };

        self.list_sftp_dir(new_path);
    }

    fn navigate_sftp_up(&mut self) {
        if self.sessions.get(self.active_session).is_none() {
            return;
        }

        let current = self.sessions.get(self.active_session)
            .map(|s| s.sftp_remote_dir.trim_end_matches('/').to_string())
            .unwrap_or_default();
        if current.is_empty() || current == "/" {
            return;
        }

        if let Some(idx) = current.rfind('/') {
            let parent = if idx == 0 { "/".to_string() } else { current[..idx].to_string() };
            self.list_sftp_dir(parent);
        }
    }

    fn refresh_sftp(&mut self) {
        let path = self.sessions.get(self.active_session)
            .map(|s| s.sftp_remote_dir.clone())
            .unwrap_or_default();
        self.list_sftp_dir(path);
    }

    fn download_file(&mut self, name: &str, remote_path: &str) {
        let local_dir = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let local_path = format!("{}/{}", local_dir, name);
        self.status_msg = format!("Downloading {} to: {}", remote_path, local_path);

        // Get host config for SFTP connection
        let host_config = match self.sessions.get(self.active_session).map(|s| s.host_config.clone()) {
            Some(h) => h,
            None => {
                self.status_msg = "Not connected to SSH".to_string();
                return;
            }
        };

        let (tx, _rx) = mpsc::unbounded_channel();
        let remote = remote_path.to_string();
        let local = local_path.clone();

        tokio::spawn(async move {
            // Create a new SSH connection for SFTP
            let event_tx = mpsc::unbounded_channel().0; // Dummy event tx for the manager
            let mut manager = SshManager::new(0, host_config, event_tx);

            if let Err(e) = manager.connect().await {
                let _ = tx.send(TransferEvent::Error {
                    file: remote,
                    error: format!("SSH connect failed: {e}"),
                });
                return;
            }

            match manager.open_sftp_stream().await {
                Ok(stream) => {
                    let mut engine = SftpEngine::new(tx.clone());
                    if let Err(e) = engine.init(stream).await {
                        let _ = tx.send(TransferEvent::Error {
                            file: remote,
                            error: format!("SFTP init failed: {e}"),
                        });
                        return;
                    }
                    if let Err(e) = engine.download(&remote, &local).await {
                        let _ = tx.send(TransferEvent::Error {
                            file: remote,
                            error: e.to_string(),
                        });
                    }
                }
                Err(e) => {
                    let _ = tx.send(TransferEvent::Error {
                        file: remote,
                        error: format!("Failed to open SFTP: {e}"),
                    });
                }
            }
        });
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.panel = Panel::HostList;
            }
            _ => {}
        }
        Ok(())
    }

    /// Switch panel.
    pub fn switch_panel(&mut self, panel: Panel) {
        self.panel = panel;

        // Initialize SFTP when switching to SFTP panel
        if panel == Panel::Sftp {
            if let Some(session) = self.sessions.get(self.active_session) {
                if session.sftp_entries.is_empty() {
                    self.list_sftp_dir("/".to_string());
                }
            }
        }
    }

    fn list_sftp_dir(&mut self, path: String) {
        // Get host config for SFTP connection
        let host_config = match self.sessions.get(self.active_session).map(|s| s.host_config.clone()) {
            Some(h) => h,
            None => {
                self.status_msg = "Not connected to SSH".to_string();
                return;
            }
        };

        self.status_msg = format!("Loading {}...", path);

        let (tx, rx) = mpsc::unbounded_channel();
        if let Some(session) = self.sessions.get_mut(self.active_session) {
            session.sftp_op_rx = Some(rx);
        }
        let path_clone = path.clone();

        tokio::spawn(async move {
            // Create a new SSH connection for SFTP
            let event_tx = mpsc::unbounded_channel().0; // Dummy event tx
            let mut manager = SshManager::new(0, host_config, event_tx);

            if let Err(e) = manager.connect().await {
                let _ = tx.send(SftpOp::Error(format!("SSH connect failed: {e}")));
                return;
            }

            match manager.open_sftp_stream().await {
                Ok(stream) => {
                    let mut engine = SftpEngine::new(mpsc::unbounded_channel().0);
                    if let Err(e) = engine.init(stream).await {
                        let _ = tx.send(SftpOp::Error(format!("SFTP init failed: {e}")));
                        return;
                    }
                    match engine.list_dir(&path_clone).await {
                        Ok(entries) => {
                            let _ = tx.send(SftpOp::Listed { path: path_clone, entries });
                        }
                        Err(e) => {
                            let _ = tx.send(SftpOp::Error(format!("List failed: {e}")));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(SftpOp::Error(format!("Failed to open SFTP: {e}")));
                }
            }
        });
    }
}

impl Default for Dialog {
    fn default() -> Self {
        Dialog::None
    }
}

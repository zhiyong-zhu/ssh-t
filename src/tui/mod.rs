use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::app::{App, Dialog, Panel};

/// Top-level draw function.
pub fn draw(f: &mut Frame, app: &App) {
    if app.panel == Panel::Terminal && app.has_active_connection() {
        // Full-screen terminal: only show terminal content, no chrome
        draw_fullscreen_terminal(f, app);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // title bar
            Constraint::Min(5),    // main area
            Constraint::Length(1),  // input bar
            Constraint::Length(1),  // status bar
        ])
        .split(f.area());

    draw_title_bar(f, app, chunks[0]);
    draw_main_area(f, app, chunks[1]);
    draw_input_bar(f, app, chunks[2]);
    draw_status_bar(f, app, chunks[3]);

    // Draw dialogs on top
    draw_dialog(f, app);
}

/// Draw full-screen terminal without TUI chrome.
fn draw_fullscreen_terminal(f: &mut Frame, app: &App) {
    let area = f.area();

    // Clear the entire screen first
    let clear = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(clear, area);

    let term_screen = app.active_session().map(|s| &s.term_screen);

    if let Some(term_screen) = term_screen {
        let visible = term_screen.get_visible_lines(area.height as usize);
        let lines: Vec<Line> = visible.iter().map(|l| Line::from(l.clone())).collect();

        let terminal = Paragraph::new(lines)
            .style(Style::default().fg(Color::White).bg(Color::Black));

        f.render_widget(terminal, area);
    }

    // Draw a minimal status hint at bottom-right
    let session_info = if app.sessions.len() > 1 {
        format!(" [{}/{}] {} ", app.active_session + 1, app.sessions.len(), app.sessions[app.active_session].name)
    } else if !app.sessions.is_empty() {
        format!(" {} ", app.sessions[app.active_session].name)
    } else {
        String::new()
    };

    let hint_text = format!("{}Ctrl+Q:disconnect  Ctrl+N/P:switch  Esc:back", session_info);
    let hint_width = hint_text.len() as u16 + 2;
    let hint = Span::styled(
        format!(" {} ", hint_text),
        Style::default().fg(Color::Black).bg(Color::DarkGray),
    );
    let hint_line = Line::from(hint);
    let hint_para = Paragraph::new(hint_line).alignment(Alignment::Right);
    let hint_area = Rect {
        x: area.width.saturating_sub(hint_width),
        y: area.height.saturating_sub(1),
        width: hint_width.min(area.width),
        height: 1,
    };
    f.render_widget(hint_para, hint_area);
}

/// Draw dialogs (modal overlays).
fn draw_dialog(f: &mut Frame, app: &App) {
    match &app.dialog {
        Dialog::None => {}
        Dialog::PasswordInput { host, password, error } => {
            draw_password_dialog(f, host, password, error);
        }
        Dialog::Connecting { host_name } => {
            draw_connecting_dialog(f, host_name);
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
            draw_host_form_dialog(f, edit_index, name, host, port, user, auth, group, *field, error);
        }
    }
}

/// Draw password input dialog.
fn draw_password_dialog(f: &mut Frame, host: &crate::config::HostConfig, password: &str, error: &Option<String>) {
    let area = centered_rect(60, 8, f.area());

    let clear = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(clear, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" Connect to ", Style::default().fg(Color::Cyan)),
            Span::styled(&host.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}@{}:{}", host.user, host.host, host.port)),
        ]),
    ];

    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!(" {err}"),
            Style::default().fg(Color::Red),
        )));
    }

    lines.push(Line::from(vec![
        Span::styled(" Password: ", Style::default().fg(Color::Green)),
        Span::raw("*".repeat(password.len())),
        Span::raw("_"),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Green)),
        Span::raw(" connect  "),
        Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::Red)),
        Span::raw(" cancel"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Password Required ")
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

/// Draw connecting status dialog.
fn draw_connecting_dialog(f: &mut Frame, host_name: &str) {
    let area = centered_rect(50, 5, f.area());

    let clear = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(clear, area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" Connecting to ", Style::default().fg(Color::Cyan)),
            Span::styled(host_name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" ...", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(Span::styled(" Press Esc to cancel", Style::default().fg(Color::DarkGray))),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Please Wait ")
                .title_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Draw host add/edit form dialog.
fn draw_host_form_dialog(
    f: &mut Frame,
    edit_index: &Option<usize>,
    name: &str,
    host: &str,
    port: &str,
    user: &str,
    auth: &crate::config::AuthMethod,
    group: &str,
    field: usize,
    error: &Option<String>,
) {
    let area = centered_rect(60, 12, f.area());

    let clear = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(clear, area);

    let title = match edit_index {
        Some(_) => " Edit Host ",
        None => " Add Host ",
    };

    let auth_label = match auth {
        crate::config::AuthMethod::Password => "Password".to_string(),
        crate::config::AuthMethod::Key { key_path } => format!("Key: {}", if key_path.is_empty() { "(empty)" } else { key_path }),
        crate::config::AuthMethod::Agent => "Agent".to_string(),
    };

    fn field_line(label: &str, value: &str, is_active: bool, idx: usize) -> Line<'static> {
        let cursor = if is_active { "_" } else { "" };
        let label_style = if is_active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let value_style = if is_active {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        Line::from(vec![
            Span::styled(format!(" {:>2}. ", idx), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<8}", label), label_style),
            Span::styled(format!("{}{}", value, cursor), value_style),
        ])
    }

    let mut lines = vec![
        Line::from(""),
        field_line("Name", name, field == 0, 0),
        field_line("Host", host, field == 1, 1),
        field_line("Port", port, field == 2, 2),
        field_line("User", user, field == 3, 3),
        field_line("Auth", &auth_label, field == 4, 4),
        field_line("Group", group, field == 5, 5),
    ];

    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!(" {err}"),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled(" Tab", Style::default().fg(Color::Green)),
        Span::raw(":next "),
        Span::styled("Space", Style::default().fg(Color::Green)),
        Span::raw(":toggle auth "),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::raw(":save "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(":cancel"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black)),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_title_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(
            " ssh-t",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // Session tabs
    if !app.sessions.is_empty() {
        spans.push(Span::raw(" |"));
        for (i, session) in app.sessions.iter().enumerate() {
            let is_active = i == app.active_session;
            let style = if is_active {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            spans.push(Span::styled(format!(" {} ", session.name), style));
            if is_active && session.manager.is_some() {
                // Add a dot to indicate active connection
            }
        }
        spans.push(Span::raw(" |"));
    }

    let tabs = vec![
        Span::styled(" [1] Hosts", Style::default().fg(if app.panel == Panel::HostList { Color::Yellow } else { Color::DarkGray })),
        Span::styled(" [2] Terminal", Style::default().fg(if app.panel == Panel::Terminal { Color::Yellow } else { Color::DarkGray })),
        Span::styled(" [3] SFTP", Style::default().fg(if app.panel == Panel::Sftp { Color::Yellow } else { Color::DarkGray })),
        Span::styled(" [?] Help", Style::default().fg(Color::DarkGray)),
    ];

    spans.extend(tabs);

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(Color::Black));
    f.render_widget(bar, area);
}

fn draw_main_area(f: &mut Frame, app: &App, area: Rect) {
    match app.panel {
        Panel::HostList => draw_host_list(f, app, area),
        Panel::Terminal => draw_terminal(f, app, area),
        Panel::Sftp => draw_sftp(f, app, area),
        Panel::Help => draw_help(f, area),
    }
}

fn draw_host_list(f: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_hosts();

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, host)| {
            let style = if app.host_list_state.selected() == Some(i) {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default()
            };

            let auth_icon = match host.auth {
                crate::config::AuthMethod::Password => "P",
                crate::config::AuthMethod::Key { .. } => "K",
                crate::config::AuthMethod::Agent => "A",
            };

            // Check if this host has an active session
            let connected = app.sessions.iter().any(|s| {
                s.host_config.host == host.host && s.host_config.port == host.port && s.manager.is_some()
            });
            let _conn_marker = if connected { " *" } else { "" };

            let content = Line::from(vec![
                Span::styled(format!("[{}] ", auth_icon), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<20}", host.name), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(" {}@{}:{}", host.user, host.host, host.port)),
                if connected {
                    Span::styled(" (connected)".to_string(), Style::default().fg(Color::Green))
                } else {
                    Span::raw("")
                },
                if !host.group.is_empty() {
                    Span::styled(format!(" [{}]", host.group), Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("")
                },
            ]);

            ListItem::new(content).style(style)
        })
        .collect();

    let session_count = app.sessions.len();
    let title = if session_count > 0 {
        format!(" Host List ({} active session{}) | Enter:connect a:add e:edit d:del ?:help) ", session_count, if session_count > 1 { "s" } else { "" })
    } else {
        " Host List (Enter:connect a:add e:edit d:del ?:help) ".to_string()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(6))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut app.host_list_state.clone());
}

fn draw_terminal(f: &mut Frame, app: &App, area: Rect) {
    if let Some(session) = app.active_session() {
        let visible_height = area.height.saturating_sub(2) as usize;
        let visible = session.term_screen.get_visible_lines(visible_height);
        let lines: Vec<Line> = visible.iter().map(|l| Line::from(l.clone())).collect();

        let terminal = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(format!(" Terminal - {} ({}x{}) ", session.name, app.terminal_cols, app.terminal_rows))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );

        f.render_widget(terminal, area);
    } else {
        let empty_msg = vec![
            Line::from(""),
            Line::from(Span::styled(
                " No active terminal session",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " Connect to a host from the Host List panel",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(empty_msg)
            .block(
                Block::default()
                    .title(" Terminal ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .alignment(Alignment::Center);
        f.render_widget(paragraph, area);
    }
}

fn draw_sftp(f: &mut Frame, app: &App, area: Rect) {
    let session = app.active_session();
    if session.is_none() || session.unwrap().sftp_entries.is_empty() {
        let empty_msg = vec![
            Line::from(""),
            Line::from(Span::styled(
                " No files loaded",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " Connect to a host first, then switch to SFTP panel",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(empty_msg)
            .block(
                Block::default()
                    .title(" SFTP ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .alignment(Alignment::Center);
        f.render_widget(paragraph, area);
        return;
    }

    let session = session.unwrap();

    let rows: Vec<Row> = session
        .sftp_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let style = if session.sftp_state.selected() == Some(i) {
                Style::default().bg(Color::Indexed(6))
            } else if entry.is_dir {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let icon = if entry.is_dir { "D" } else { "F" };
            let size = if entry.is_dir {
                "-".to_string()
            } else {
                format_size(entry.size)
            };

            Row::new(vec![
                Cell::from(icon),
                Cell::from(entry.name.as_str()),
                Cell::from(size),
                Cell::from(entry.modified.as_deref().unwrap_or("-")),
            ])
            .style(style)
        })
        .collect();

    let _local_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(20),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from(""),
            Cell::from("Name"),
            Cell::from("Size"),
            Cell::from("Modified"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1),
    )
    .block(
        Block::default()
            .title(format!(" SFTP - {} - {} ", session.name, session.sftp_remote_dir))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    )
    .row_highlight_style(Style::default().bg(Color::Indexed(6)));

    f.render_stateful_widget(table, area, &mut session.sftp_state.clone());
}

fn draw_help(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            " ssh-t — Keyboard & Mouse Shortcuts",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Global:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   1 / 2 / 3     Switch panel (Hosts / Terminal / SFTP)"),
        Line::from("   ?             Toggle help"),
        Line::from("   q             Quit (on host list)"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Host List:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   Up/Down       Navigate hosts"),
        Line::from("   Enter         Connect to selected host (new session)"),
        Line::from("   a             Add new host"),
        Line::from("   e             Edit selected host"),
        Line::from("   d             Delete selected host"),
        Line::from("   Esc           Clear filter"),
        Line::from("   Type          Filter hosts by name/host/user"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Terminal (PTY Mode):", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   All keys sent directly to remote shell"),
        Line::from("   Ctrl+Q        Disconnect and return to host list"),
        Line::from("   Ctrl+N        Switch to next session"),
        Line::from("   Ctrl+P        Switch to previous session"),
        Line::from("   Esc           Return to host list (keep session)"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Multi-Session:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   Multiple SSH connections supported simultaneously"),
        Line::from("   Each connection creates a new session"),
        Line::from("   Use Ctrl+N/Ctrl+P to switch between sessions"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" SFTP:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   Up/Down       Navigate files"),
        Line::from("   Enter         Open directory"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Mouse:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   Left Click    Select item"),
        Line::from("   Scroll        Navigate list"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Authentication:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   [P] Password  [K] Key file  [A] SSH Agent"),
        Line::from(""),
        Line::from(Span::styled(
            " Press Esc or ? to close this help",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn draw_input_bar(f: &mut Frame, app: &App, area: Rect) {
    let line = match app.panel {
        Panel::HostList => {
            let filter_text = if app.host_filter.is_empty() {
                String::new()
            } else {
                format!(" {}", app.host_filter)
            };
            Line::from(vec![
                Span::styled(" Filter", Style::default().fg(Color::Yellow)),
                Span::raw(filter_text),
                Span::raw("_"),
                Span::styled("  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(":Connect "),
                Span::styled("a", Style::default().fg(Color::Green)),
                Span::raw(":Add "),
                Span::styled("e", Style::default().fg(Color::Green)),
                Span::raw(":Edit "),
                Span::styled("d", Style::default().fg(Color::Green)),
                Span::raw(":Del "),
                Span::styled("?", Style::default().fg(Color::Green)),
                Span::raw(":Help "),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(":Quit"),
            ])
        }
        Panel::Terminal => {
            let session_info = if !app.sessions.is_empty() {
                let session = &app.sessions[app.active_session];
                format!(" [{}]", session.name)
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(" PTY", Style::default().fg(Color::Yellow)),
                Span::raw(format!("{} ", session_info)),
                Span::styled("Ctrl+Q", Style::default().fg(Color::Red)),
                Span::raw(":Disconnect "),
                Span::styled("Ctrl+N/P", Style::default().fg(Color::Green)),
                Span::raw(":Switch "),
                Span::styled("Esc", Style::default().fg(Color::Green)),
                Span::raw(":Back"),
            ])
        }
        Panel::Sftp => {
            Line::from(vec![
                Span::styled(" SFTP", Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled("j/k", Style::default().fg(Color::Green)),
                Span::raw(":Nav "),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(":Open "),
                Span::styled("Backspace", Style::default().fg(Color::Green)),
                Span::raw(":Up "),
                Span::styled("r", Style::default().fg(Color::Green)),
                Span::raw(":Refresh "),
                Span::styled("d", Style::default().fg(Color::Green)),
                Span::raw(":Download "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(":Back"),
            ])
        }
        Panel::Help => {
            Line::from(vec![
                Span::styled(" Help", Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled("Esc/?", Style::default().fg(Color::Green)),
                Span::raw(":Back"),
            ])
        }
    };

    let bar = Paragraph::new(line).style(Style::default().bg(Color::Black));
    f.render_widget(bar, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_style = if app.has_active_connection() {
        Style::default().fg(Color::Black).bg(Color::Green)
    } else {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", &app.status_msg),
            status_style,
        ),
    ]);

    let bar = Paragraph::new(line);
    f.render_widget(bar, area);
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if size >= GB {
        format!("{:.1}G", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1}M", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1}K", size as f64 / KB as f64)
    } else {
        format!("{size}B")
    }
}

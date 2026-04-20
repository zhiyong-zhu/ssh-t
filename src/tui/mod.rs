use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::app::{App, Dialog, Panel};

/// Top-level draw function. Always uses tab-bar layout, no fullscreen mode.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // tab bar
            Constraint::Min(5),    // main area
            Constraint::Length(1),  // input bar
            Constraint::Length(1),  // status bar
        ])
        .split(f.area());

    draw_tab_bar(f, app, chunks[0]);

    match app.panel {
        Panel::Terminal => draw_terminal(f, app, chunks[1]),
        _ => draw_main_area(f, app, chunks[1]),
    }

    draw_input_bar(f, app, chunks[2]);
    draw_status_bar(f, app, chunks[3]);

    // Draw dialogs on top
    draw_dialog(f, app);
}

/// Draw tab bar with session tabs and panel tabs.
fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    // App name
    spans.push(Span::styled(
        " ssh-t ",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));

    // Separator
    spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));

    // Session tabs
    if !app.sessions.is_empty() {
        for (i, session) in app.sessions.iter().enumerate() {
            let is_active_session = i == app.active_session && app.panel == Panel::Terminal;
            let connected = session.manager.is_some();

            let label = if is_active_session {
                format!(" {} {} ", if connected { "●" } else { "○" }, session.name)
            } else {
                format!(" {} {} ", if connected { "●" } else { "○" }, session.name)
            };

            let style = if is_active_session {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if connected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            spans.push(Span::styled(label, style));
        }

        spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
    }

    // Panel tabs (right side) — show Alt prefix as low-conflict app shortcut
    let panel_tabs = [
        (" Alt-H Hosts ", Panel::HostList),
        (" Alt-T Term ", Panel::Terminal),
        (" Alt-S SFTP ", Panel::Sftp),
    ];

    for (label, panel) in panel_tabs {
        let is_active = app.panel == panel;
        let style = if is_active {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        spans.push(Span::styled(label, style));
    }

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

            let connected = app.sessions.iter().any(|s| {
                s.host_config.host == host.host && s.host_config.port == host.port && s.manager.is_some()
            });

            let content = Line::from(vec![
                Span::styled(format!("[{}] ", auth_icon), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<20}", host.name), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(" {}@{}:{}", host.user, host.host, host.port)),
                if connected {
                    Span::styled(" ●".to_string(), Style::default().fg(Color::Green))
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
        format!(" Hosts ({} session{}) ", session_count, if session_count > 1 { "s" } else { "" })
    } else {
        " Hosts ".to_string()
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
        let visible = session.term_screen.get_visible_lines(area.height as usize);
        let text_style = Style::default().fg(Color::White);
        let lines: Vec<Line> = visible
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), text_style)))
            .collect();

        let terminal = Paragraph::new(lines);
        f.render_widget(terminal, area);
    } else {
        let empty_msg = vec![
            Line::from(""),
            Line::from(Span::styled(
                " No active terminal session",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Press Enter on a host to connect",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(empty_msg)
            .block(
                Block::default()
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
            .title(format!(" SFTP - {} ", session.sftp_remote_dir))
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
        Line::from("   Alt-H/T/S     Switch panel (Hosts / Terminal / SFTP)"),
        Line::from("   F1/F2/F3      Switch panel fallback"),
        Line::from("   Alt-Left/Right or Alt-P/N  Switch session tabs"),
        Line::from("   Alt-W         Close current session tab"),
        Line::from("   Tab           Switch to terminal from host list"),
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
        Line::from("   Ctrl keys     Sent directly to remote shell"),
        Line::from("   Esc           Sent directly to remote shell"),
        Line::from("   Alt-H/T/S     Switch panels"),
        Line::from("   Alt-Left/Right or Alt-P/N  Switch sessions"),
        Line::from("   Alt-W         Close current session"),
        Line::from("   Click tabs     Switch session/panel"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Tab Bar:", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from("   Click session tabs to switch sessions"),
        Line::from("   Click panel tabs (Hosts/Terminal/SFTP) to switch view"),
        Line::from("   ● = connected  ○ = disconnected"),
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
            let mut spans = vec![
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
            ];
            if !app.sessions.is_empty() {
                spans.push(Span::styled("Tab", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                spans.push(Span::raw(":Terminal "));
                spans.push(Span::styled("Alt-←/→", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                spans.push(Span::raw(":Tabs "));
            }
            spans.push(Span::styled("Alt-H/T/S", Style::default().fg(Color::Green)));
            spans.push(Span::raw(":Panels "));
            spans.push(Span::styled("?", Style::default().fg(Color::Green)));
            spans.push(Span::raw(":Help "));
            spans.push(Span::styled("q", Style::default().fg(Color::Red)));
            spans.push(Span::raw(":Quit"));
            Line::from(spans)
        }
        Panel::Terminal => {
            if let Some(session) = app.active_session() {
                let session_info = format!(" {} ", session.name);
                Line::from(vec![
                    Span::styled(session_info, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled("Alt-W", Style::default().fg(Color::Red)),
                    Span::raw(":Close "),
                    Span::styled("Alt-←/→", Style::default().fg(Color::Green)),
                    Span::raw(":Tabs "),
                    Span::styled("Alt-H/T/S", Style::default().fg(Color::Green)),
                    Span::raw(":Panels "),
                    if app.sessions.len() > 1 {
                        Span::styled(format!("[{}/{}]", app.active_session + 1, app.sessions.len()), Style::default().fg(Color::DarkGray))
                    } else {
                        Span::raw("")
                    },
                ])
            } else {
                Line::from(vec![
                    Span::styled(" No active session ", Style::default().fg(Color::DarkGray)),
                ])
            }
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
                Span::styled("Alt-H/T/S", Style::default().fg(Color::Green)),
                Span::raw(":Panels "),
                Span::styled("Alt-←/→", Style::default().fg(Color::Green)),
                Span::raw(":Tabs "),
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

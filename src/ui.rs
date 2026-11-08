use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Tabs, Wrap,
    },
    Frame,
};
use crate::types::{FoundServer, ScanType};
use crate::AppState;

pub const CYBER_CYAN: Color = Color::Rgb(0, 255, 240);
pub const CYBER_GREEN: Color = Color::Rgb(57, 255, 20);
pub const CYBER_MAGENTA: Color = Color::Rgb(255, 0, 128);
pub const CYBER_YELLOW: Color = Color::Rgb(255, 230, 0);
pub const CYBER_RED: Color = Color::Rgb(255, 49, 49);
#[allow(dead_code)]
pub const CYBER_DARK: Color = Color::Rgb(15, 15, 25);
pub const CYBER_GRAY: Color = Color::Rgb(100, 110, 130);

pub fn render(f: &mut Frame, state: &mut AppState) {
    let size = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Banner & Tabs
            Constraint::Min(10),   // Main Content Area
            Constraint::Length(5), // Bottom Status & Progress Bar & Telemetry
        ])
        .split(size);

    render_header(f, state, chunks[0]);

    match state.selected_tab {
        0 => render_scanner_tab(f, state, chunks[1]),
        1 => render_analytics_tab(f, state, chunks[1]),
        2 => render_database_tab(f, state, chunks[1]),
        3 => render_bot_control_tab(f, state, chunks[1]),
        4 => render_users_admin_tab(f, state, chunks[1]),
        5 => render_proxy_tab(f, state, chunks[1]),
        6 => render_help_tab(f, chunks[1]),
        _ => {}
    }

    render_footer(f, state, chunks[2]);

    if state.editing_field.is_some() {
        render_input_modal(f, state);
    }
}

fn render_header(f: &mut Frame, state: &AppState, area: Rect) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32),
            Constraint::Min(10),
        ])
        .split(area);

    let logo = Paragraph::new(Line::from(vec![
        Span::styled(" ⚡ PIS SCANNER ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("v2.0 ", Style::default().fg(CYBER_MAGENTA)),
        Span::styled("[CYBER]", Style::default().fg(CYBER_CYAN)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYBER_CYAN)),
    );
    f.render_widget(logo, header_chunks[0]);

    let tab_titles = vec![
        "[1] SCANNER",
        "[2] ANALYTICS",
        "[3] DATABASE",
        "[4] BOT CONTROL",
        "[5] USERS & PROMO",
        "[6] PROXY & NET",
        "[7] HELP",
    ];

    let tabs = Tabs::new(tab_titles)
        .select(state.selected_tab)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(CYBER_CYAN)),
        )
        .style(Style::default().fg(CYBER_GRAY))
        .highlight_style(
            Style::default()
                .fg(CYBER_GREEN)
                .add_modifier(Modifier::BOLD)
                .bg(Color::Rgb(20, 40, 30)),
        )
        .divider(Span::styled(" │ ", Style::default().fg(CYBER_GRAY)));

    f.render_widget(tabs, header_chunks[1]);
}

fn render_scanner_tab(f: &mut Frame, state: &mut AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(44),
            Constraint::Min(20),
        ])
        .split(area);

    // Left Panel: Configuration
    render_config_panel(f, state, chunks[0]);

    // Right Panel: Live Feed
    render_feed_panel(f, state, chunks[1]);
}

fn render_config_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let is_scanning = state.is_scanning.load(std::sync::atomic::Ordering::Relaxed);

    let config_block = Block::default()
        .title(Span::styled(" ⚙ SCAN CONTROLS & PARAMS ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if is_scanning { CYBER_GREEN } else { CYBER_CYAN }));

    let mut lines = Vec::new();

    // Start / Stop prompt
    let action_line = if is_scanning {
        Line::from(vec![
            Span::styled(" ▶ [SPACE] ", Style::default().bg(CYBER_RED).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(" STOP SCANNING", Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ▶ [SPACE] ", Style::default().bg(CYBER_GREEN).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(" LAUNCH SCANNER", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        ])
    };
    lines.push(action_line);
    lines.push(Line::from(""));

    // Scan Mode
    lines.push(Line::from(vec![
        Span::styled("MODE: ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled(format!("[M] {:?}", state.scan_type), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]));

    let mode_desc = match state.scan_type {
        ScanType::MultiEngine => "↳ 🚀 Мульти-движок (Параллельно по всем базам)",
        ScanType::Default => "↳ Random IPv4 address search",
        ScanType::Targeted => "↳ Datacenter subnets (/16, /24)",
        ScanType::PlayerTracker => "↳ Global nickname tracker",
        ScanType::Playit => "↳ Playit.gg tunnels across ports",
        ScanType::Ngrok => "↳ Ngrok multi-region TCP tunnels",
        ScanType::DnsBrute => "↳ DNS subdomain bruteforce",
    };
    lines.push(Line::from(Span::styled(format!("  {}", mode_desc), Style::default().fg(CYBER_GRAY))));

    if state.scan_type == ScanType::MultiEngine {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("── PARALLEL STREAMS (1-100) ──", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD))));
        
        let t_color = if state.multi_targeted_threads > 0 { CYBER_GREEN } else { CYBER_GRAY };
        lines.push(Line::from(vec![
            Span::styled("[T] 🎯 Targeted: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("[ {:>3} ] ", state.multi_targeted_threads), Style::default().fg(t_color).add_modifier(Modifier::BOLD)),
            Span::styled("потоков", Style::default().fg(CYBER_GRAY)),
        ]));

        let s_color = if state.multi_standard_threads > 0 { CYBER_GREEN } else { CYBER_GRAY };
        lines.push(Line::from(vec![
            Span::styled("[S] 🌐 Standard: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("[ {:>3} ] ", state.multi_standard_threads), Style::default().fg(s_color).add_modifier(Modifier::BOLD)),
            Span::styled("потоков", Style::default().fg(CYBER_GRAY)),
        ]));

        let y_color = if state.multi_playit_threads > 0 { CYBER_GREEN } else { CYBER_GRAY };
        lines.push(Line::from(vec![
            Span::styled("[Y] 🎮 Playit:   ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("[ {:>3} ] ", state.multi_playit_threads), Style::default().fg(y_color).add_modifier(Modifier::BOLD)),
            Span::styled("потоков", Style::default().fg(CYBER_GRAY)),
        ]));

        let o_color = if state.multi_ngrok_threads > 0 { CYBER_GREEN } else { CYBER_GRAY };
        lines.push(Line::from(vec![
            Span::styled("[O] 🔗 Ngrok:    ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("[ {:>3} ] ", state.multi_ngrok_threads), Style::default().fg(o_color).add_modifier(Modifier::BOLD)),
            Span::styled("потоков", Style::default().fg(CYBER_GRAY)),
        ]));

        let total_workers = state.multi_targeted_threads + state.multi_standard_threads + state.multi_playit_threads + state.multi_ngrok_threads;
        lines.push(Line::from(vec![
            Span::styled("⚡ Всего: ", Style::default().fg(CYBER_GREEN)),
            Span::styled(format!("{} воркеров", total_workers), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" | 🔒 Anti-Collision: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled("ВКЛ", Style::default().fg(CYBER_GREEN)),
        ]));
    } else {
        if state.scan_type == ScanType::Targeted {
            lines.push(Line::from(vec![
                Span::styled("TARGET DB: ", Style::default().fg(CYBER_YELLOW)),
                Span::styled(format!("[B] {:?}", state.targeted_db), Style::default().fg(CYBER_CYAN)),
            ]));
        }

        if state.scan_type == ScanType::PlayerTracker {
            let nick_val = if state.target_nick.is_empty() { "<Auto Random>" } else { &state.target_nick };
            lines.push(Line::from(vec![
                Span::styled("TARGET NICK: ", Style::default().fg(CYBER_YELLOW)),
                Span::styled(format!("[N] {}", nick_val), Style::default().fg(CYBER_GREEN)),
            ]));
        }

        if state.scan_type == ScanType::DnsBrute {
            let domain_val = if state.dns_target_domain.is_empty() { "aternos.me" } else { &state.dns_target_domain };
            lines.push(Line::from(vec![
                Span::styled("BASE DOMAIN: ", Style::default().fg(CYBER_YELLOW)),
                Span::styled(format!("[D] {}", domain_val), Style::default().fg(CYBER_CYAN)),
            ]));
        }

        lines.push(Line::from(""));

        // Threads & Target Amount
        lines.push(Line::from(vec![
            Span::styled("THREADS: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("[-] {} [+] ", state.threads), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" (10-1000)", Style::default().fg(CYBER_GRAY)),
        ]));
    }

    let amt_str = if state.infinite_scan && state.scan_type != ScanType::PlayerTracker {
        "∞ INFINITE".to_string()
    } else {
        format!("{}", state.amount)
    };
    lines.push(Line::from(vec![
        Span::styled("LIMIT:   ", Style::default().fg(CYBER_YELLOW)),
        Span::styled(format!("[I] {}", amt_str), Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
    ]));

    let ver_str = if state.target_version.is_empty() { "ALL" } else { &state.target_version };
    lines.push(Line::from(vec![
        Span::styled("VERSION: ", Style::default().fg(CYBER_YELLOW)),
        Span::styled(format!("[V] {}", ver_str), Style::default().fg(CYBER_CYAN)),
    ]));

    lines.push(Line::from(""));

    // Deep scan & Auto RCON
    let deep_box = if state.deep_scan { "[X]" } else { "[ ]" };
    lines.push(Line::from(vec![
        Span::styled(format!("{} [P] Deep Scan (Зеленые, 100 потоков)", deep_box), Style::default().fg(if state.deep_scan { CYBER_GREEN } else { CYBER_GRAY })),
    ]));

    let rcon_box = if state.auto_rcon_enabled { "[X]" } else { "[ ]" };
    let rcon_cmd_disp = if state.auto_rcon_cmd.is_empty() { "op your_nick" } else { &state.auto_rcon_cmd };
    lines.push(Line::from(vec![
        Span::styled(format!("{} [R] Auto RCON: ", rcon_box), Style::default().fg(if state.auto_rcon_enabled { CYBER_RED } else { CYBER_GRAY })),
        Span::styled(format!("\"{}\"", rcon_cmd_disp), Style::default().fg(CYBER_YELLOW)),
    ]));

    lines.push(Line::from(""));

    // UI Filters
    lines.push(Line::from(Span::styled("── LIVE FILTERS ──", Style::default().fg(CYBER_GRAY))));
    let f_green = if state.filter_green { "[X] Free" } else { "[ ] Free" };
    let f_yellow = if state.filter_yellow { "[X] Whitelist" } else { "[ ] Whitelist" };
    let f_blue = if state.filter_blue { "[X] Lic" } else { "[ ] Lic" };

    lines.push(Line::from(vec![
        Span::styled(format!("[F1]{} ", f_green), Style::default().fg(CYBER_GREEN)),
        Span::styled(format!("[F2]{} ", f_yellow), Style::default().fg(CYBER_YELLOW)),
        Span::styled(format!("[F3]{}", f_blue), Style::default().fg(CYBER_CYAN)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(config_block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_feed_panel(f: &mut Frame, state: &mut AppState, area: Rect) {
    let total_found = state.stats_total;

    let block = Block::default()
        .title(Span::styled(
            format!(" ⚡ LIVE SERVERS FEED (Total Found: {}) [↑/↓ Navigate | 'c' Copy IP] ", total_found),
            Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CYBER_CYAN));

    let visible_servers: Vec<&FoundServer> = state
        .found
        .iter()
        .filter(|s| {
            let is_free = s.info.contains("[Вход свободен]");
            let is_cracked = s.is_cracked || s.needs_mods;
            let is_lic = !s.is_cracked;

            if is_free && !state.filter_green { return false; }
            if !is_free && is_cracked && !state.filter_yellow { return false; }
            if is_lic && !state.filter_blue { return false; }
            true
        })
        .collect();

    let items: Vec<ListItem> = visible_servers
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_selected = idx == state.selected_server_idx;
            
            let prefix = if is_selected { "▶ " } else { "  " };

            let (ip_color, badge) = if s.is_op {
                (Color::White, "[★ CREATIVE/OP]")
            } else if s.is_bungee_vulnerable {
                (CYBER_RED, "[🚨 BUNGEE VULN]")
            } else if s.is_velocity_vulnerable {
                (CYBER_RED, "[🚨 VELOCITY VULN]")
            } else if s.info.contains("[Вход свободен]") {
                (CYBER_GREEN, "[FREE]")
            } else if s.is_cracked {
                (CYBER_YELLOW, "[CRACKED]")
            } else {
                (CYBER_CYAN, "[LIC]")
            };

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(&s.ip, Style::default().fg(ip_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {} ", badge), Style::default().fg(ip_color).add_modifier(Modifier::BOLD)),
            ];

            if let Some(ref pass) = s.rcon_pass {
                spans.push(Span::styled(format!("[🔓 RCON: {}] ", pass), Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)));
            }

            if s.is_deep_scan {
                spans.push(Span::styled("[DEEP] ", Style::default().fg(CYBER_YELLOW)));
            }

            let mut lines = vec![Line::from(spans)];
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(&s.info, Style::default().fg(Color::Rgb(180, 190, 200))),
            ]));

            if let Some(ref pl) = s.plugins {
                lines.push(Line::from(vec![
                    Span::styled("    📦 Plugins: ", Style::default().fg(CYBER_GRAY)),
                    Span::styled(pl, Style::default().fg(Color::Rgb(160, 220, 255))),
                ]));
            }

            let style = if is_selected {
                Style::default().bg(Color::Rgb(25, 35, 45))
            } else {
                Style::default()
            };

            ListItem::new(lines).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut state.server_list_state);
}

fn render_analytics_tab(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(10),
        ])
        .split(area);

    // Summary Cards (Cracked vs Lic, Threats)
    let card_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[0]);

    // Card 1: Auth Split
    let total = state.stats_total.max(1) as f64;
    let cracked_pct = (state.stats_cracked as f64 / total) * 100.0;
    let lic_pct = (state.stats_lic as f64 / total) * 100.0;

    let auth_block = Block::default()
        .title(Span::styled(" 📊 CRACKED VS LICENSED DISTRIBUTION ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let auth_text = vec![
        Line::from(vec![
            Span::styled("TOTAL SERVERS ANALYZED: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(state.stats_total.to_string(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(format!("Cracked / Offline: {} ({:.1}%)  ", state.stats_cracked, cracked_pct), Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("Licensed / Mojang: {} ({:.1}%)", state.stats_lic, lic_pct), Style::default().fg(CYBER_CYAN)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Ratio: [", Style::default().fg(CYBER_GRAY)),
            Span::styled("█".repeat((cracked_pct / 3.0) as usize), Style::default().fg(CYBER_YELLOW)),
            Span::styled("█".repeat((lic_pct / 3.0) as usize), Style::default().fg(CYBER_CYAN)),
            Span::styled("]", Style::default().fg(CYBER_GRAY)),
        ]),
    ];
    f.render_widget(Paragraph::new(auth_text).block(auth_block), card_chunks[0]);

    // Card 2: Security Vulnerabilities
    let sec_block = Block::default()
        .title(Span::styled(" 🚨 DETECTED VULNERABILITIES & EXPLOITS ", Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let sec_text = vec![
        Line::from(vec![
            Span::styled("BungeeCord IP-Forwarding Bypass: ", Style::default().fg(Color::White)),
            Span::styled(format!("{} SERVERS", state.stats_bungee_vuln), Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Velocity Modern Forwarding Bypass: ", Style::default().fg(Color::White)),
            Span::styled(format!("{} SERVERS", state.stats_velocity_vuln), Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("All compromised and vulnerable servers are automatically saved into SQLite servers.db", Style::default().fg(CYBER_GRAY))),
    ];
    f.render_widget(Paragraph::new(sec_text).block(sec_block), card_chunks[1]);

    // Bottom Histogram: Top Versions
    let ver_block = Block::default()
        .title(Span::styled(" 📈 TOP 20 MINECRAFT SERVER VERSIONS (HISTOGRAM) ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let mut sorted_versions: Vec<(&String, &usize)> = state.stats_versions.iter().collect();
    sorted_versions.sort_by(|a, b| b.1.cmp(a.1));

    let max_count = sorted_versions.first().map(|(_, c)| **c).unwrap_or(1) as f32;

    let mut ver_lines = Vec::new();
    if sorted_versions.is_empty() {
        ver_lines.push(Line::from(Span::styled("Waiting for scanner data to compile version telemetry...", Style::default().fg(CYBER_GRAY))));
    } else {
        for (ver, count) in sorted_versions.iter().take(15) {
            let bar_len = ((**count as f32 / max_count) * 45.0).max(1.0) as usize;
            let bar = "█".repeat(bar_len);

            ver_lines.push(Line::from(vec![
                Span::styled(format!("{:>14} │ ", ver), Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
                Span::styled(bar, Style::default().fg(CYBER_GREEN)),
                Span::styled(format!(" {} servers", count), Style::default().fg(Color::White)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(ver_lines).block(ver_block), chunks[1]);
}

fn render_database_tab(f: &mut Frame, state: &mut AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
        ])
        .split(area);

    // Search bar
    let search_block = Block::default()
        .title(Span::styled(" 🔍 SEARCH SQLITE DATABASE (Press 'e' or Enter to edit query) ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let q = if state.db_search_query.is_empty() { "<All Servers>" } else { &state.db_search_query };
    let search_bar = Paragraph::new(Line::from(vec![
        Span::styled(" Query: ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled(q, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ])).block(search_block);
    f.render_widget(search_bar, chunks[0]);

    // DB List
    let db_block = Block::default()
        .title(Span::styled(
            format!(" 💾 SAVED SERVERS (Found in DB: {}) [↑/↓ Navigate | 'c' Copy IP] ", state.db_results.len()),
            Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let items: Vec<ListItem> = state.db_results
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_selected = idx == state.selected_db_idx;
            let prefix = if is_selected { "▶ " } else { "  " };

            let title_color = if s.is_op {
                Color::White
            } else if s.info.contains("[Вход свободен]") {
                CYBER_GREEN
            } else if s.is_cracked {
                CYBER_YELLOW
            } else {
                CYBER_CYAN
            };

            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(&s.ip, Style::default().fg(title_color).add_modifier(Modifier::BOLD)),
            ];

            if let Some(ref p) = s.rcon_pass {
                spans.push(Span::styled(format!(" [🔓 RCON: {}]", p), Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)));
            }

            let lines = vec![
                Line::from(spans),
                Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(&s.info, Style::default().fg(CYBER_GRAY)),
                ]),
            ];

            let style = if is_selected {
                Style::default().bg(Color::Rgb(25, 35, 45))
            } else {
                Style::default()
            };

            ListItem::new(lines).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(db_block)
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, chunks[1], &mut state.db_list_state);
}

fn render_bot_control_tab(f: &mut Frame, state: &mut AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Top stats & controls
            Constraint::Min(10),   // Bottom activity logs
        ])
        .split(area);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(chunks[0]);

    // 1. Status Panel
    let status_block = Block::default()
        .title(Span::styled(" 🤖 BOT ENGINE STATUS ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CYBER_CYAN));

    let bot_status = crate::telegram::get_status();
    let is_enabled = crate::telegram::is_bot_enabled();
    let updates_cnt = crate::telegram::get_updates_count();
    let status_lines = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(bot_status, Style::default().fg(if is_enabled { CYBER_GREEN } else { CYBER_RED }).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Engine: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(if is_enabled { "🟢 Polling Active" } else { "🔴 Paused" }, Style::default().fg(if is_enabled { CYBER_GREEN } else { CYBER_RED })),
            Span::styled(format!(" (Updates: {})", updates_cnt), Style::default().fg(CYBER_GRAY)),
        ]),
        Line::from(vec![
            Span::styled("Token:   ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(if state.tg_bot_token.is_empty() { "Not Set".to_string() } else { format!("{}...{}", &state.tg_bot_token.chars().take(8).collect::<String>(), &state.tg_bot_token.chars().rev().take(4).collect::<String>()) }, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("Gateway: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled("⚡ Cloudflare Worker Proxy", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
        ]),
    ];
    f.render_widget(Paragraph::new(status_lines).block(status_block), top_chunks[0]);

    // 2. SaaS Audience Metrics
    let saas_block = Block::default()
        .title(Span::styled(" 💎 SAAS AUDIENCE & REVENUE ", Style::default().fg(CYBER_MAGENTA).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let total_users = state.tg_users.len();
    let active_prime = state.tg_users.iter().filter(|u| u.active_tier() == "Prime").count();
    let active_std = state.tg_users.iter().filter(|u| u.active_tier() == "Standard").count();
    let total_balance: i64 = state.tg_users.iter().map(|u| u.balance).sum();

    let saas_lines = vec![
        Line::from(vec![
            Span::styled("Total Users: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("{} users", total_users), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" │ Prime: ", Style::default().fg(CYBER_RED)),
            Span::styled(format!("{} ⭐", active_prime), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Standard:    ", Style::default().fg(CYBER_GREEN)),
            Span::styled(format!("{} 🟢", active_std), Style::default().fg(Color::White)),
            Span::styled(" │ Free: ", Style::default().fg(CYBER_CYAN)),
            Span::styled(format!("{}", total_users.saturating_sub(active_prime + active_std)), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("User Balance Sum: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(format!("{} ₽", total_balance), Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        ]),
    ];
    f.render_widget(Paragraph::new(saas_lines).block(saas_block), top_chunks[1]);

    // 3. Quick Hotkeys
    let hotkeys_block = Block::default()
        .title(Span::styled(" ⚡ BOT CONTROLS ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let hotkey_lines = vec![
        Line::from(vec![
            Span::styled(" [T] ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("Toggle Pause / Resume Bot Engine", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [A] ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("Broadcast Announcement", Style::default().fg(Color::White)),
            Span::styled(" │ [K] Edit Token", Style::default().fg(CYBER_YELLOW)),
        ]),
        Line::from(vec![
            Span::styled(" [C] ", Style::default().fg(CYBER_MAGENTA).add_modifier(Modifier::BOLD)),
            Span::styled("Clear Activity Logs", Style::default().fg(Color::White)),
        ]),
    ];
    f.render_widget(Paragraph::new(hotkey_lines).block(hotkeys_block), top_chunks[2]);

    // Bottom Logs Panel
    let logs = crate::telegram::get_bot_logs();
    let logs_block = Block::default()
        .title(Span::styled(
            format!(" 📡 LIVE TELEGRAM ACTIVITY STREAM (Events: {}) [↑/↓ Scroll | 'c' Clear | 'a' Broadcast] ", logs.len()),
            Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CYBER_CYAN));

    let items: Vec<ListItem> = logs
        .iter()
        .rev()
        .map(|entry| {
            let mut color = Color::White;
            if entry.contains("[EXCLUSIVE") || entry.contains("PRIME") {
                color = CYBER_RED;
            } else if entry.contains("STARS") || entry.contains("⭐") || entry.contains("PROMO") {
                color = CYBER_YELLOW;
            } else if entry.contains("STANDARD") || entry.contains("🟢") {
                color = CYBER_GREEN;
            } else if entry.contains("Входящее") || entry.contains("MSG") {
                color = CYBER_CYAN;
            }
            ListItem::new(Line::from(Span::styled(entry, Style::default().fg(color))))
        })
        .collect();

    let list = List::new(items)
        .block(logs_block)
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, chunks[1], &mut state.bot_logs_state);
}

fn render_proxy_tab(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // Public IP Status
            Constraint::Min(8),    // SOCKS5 Proxy Management
        ])
        .split(area);

    // 1. Public IP
    let ip_block = Block::default()
        .title(Span::styled(" 🌐 NETWORK & ANONYMITY CHECK ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let ip_lines = vec![
        Line::from(vec![
            Span::styled("Public IP Address: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(&state.public_ip, Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("  [U] Refresh Public IP", Style::default().fg(CYBER_CYAN)),
        ]),
        Line::from(vec![
            Span::styled("Routing mode: ", Style::default().fg(CYBER_YELLOW)),
            Span::styled(if state.use_proxy { "SOCKS5 PROXY ACTIVE" } else { "DIRECT CONNECTION" }, Style::default().fg(if state.use_proxy { CYBER_GREEN } else { CYBER_YELLOW })),
        ]),
    ];
    f.render_widget(Paragraph::new(ip_lines).block(ip_block), chunks[0]);

    // 2. Proxies
    let proxy_block = Block::default()
        .title(Span::styled(" 🛡 SOCKS5 PROXY POOL ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let proxy_count = if let Ok(lock) = state.proxy_list.lock() { lock.len() } else { 0 };

    let p_use_box = if state.use_proxy { "[X]" } else { "[ ]" };
    let p_scrape_box = if state.auto_scrape_proxies { "[X]" } else { "[ ]" };

    let mut proxy_lines = vec![
        Line::from(vec![
            Span::styled(format!("{} [X] Route through SOCKS5  ", p_use_box), Style::default().fg(if state.use_proxy { CYBER_GREEN } else { CYBER_GRAY })),
            Span::styled(format!("{} [A] Auto-scrape on launch  ", p_scrape_box), Style::default().fg(if state.auto_scrape_proxies { CYBER_GREEN } else { CYBER_GRAY })),
            Span::styled("[S] Scrape & Validate Now", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(format!("Verified Working Proxies in Memory: {} ", proxy_count), Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("[M] Add manual proxies / [K] Clear pool", Style::default().fg(CYBER_GRAY)),
        ]),
        Line::from(""),
    ];

    if let Ok(lock) = state.proxy_list.lock() {
        for (i, p) in lock.iter().take(12).enumerate() {
            proxy_lines.push(Line::from(Span::styled(format!("  {:02}. SOCKS5 -> {}", i + 1, p), Style::default().fg(Color::Rgb(150, 200, 230)))));
        }
        if lock.len() > 12 {
            proxy_lines.push(Line::from(Span::styled(format!("  ... and {} more proxies", lock.len() - 12), Style::default().fg(CYBER_GRAY))));
        }
    }

    f.render_widget(Paragraph::new(proxy_lines).block(proxy_block), chunks[1]);
}

fn render_users_admin_tab(f: &mut Frame, state: &mut AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(58),
            Constraint::Percentage(42),
        ])
        .split(area);

    // Left: Users List
    let users_block = Block::default()
        .title(Span::styled(
            format!(" 👥 TELEGRAM USERS ({}) [↑/↓ Navigate | 'f' Search ID | 'x' Ban/Unban | 'g' Sub | 'b' Balance] ", state.tg_users.len()),
            Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CYBER_CYAN));

    let user_items: Vec<ListItem> = state.tg_users
        .iter()
        .enumerate()
        .map(|(idx, u)| {
            let is_selected = idx == state.selected_user_idx;
            let prefix = if is_selected { "▶ " } else { "  " };

            let (tier_color, tier_badge) = match u.active_tier().as_str() {
                "Prime" => (CYBER_RED, "[⭐ PRIME]"),
                "Plus" | "Standard" => (CYBER_GREEN, "[🟢 PLUS]"),
                _ => (CYBER_CYAN, "[🔷 FREE]"),
            };

            let uname = if u.username.is_empty() { "No Username" } else { &u.username };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            
            let expiry_info = if u.is_sub_active() {
                let days = (u.sub_expires_at - now) / 86400;
                format!("Подписка: {} дн.", days.max(1))
            } else {
                "Подписка: нет".to_string()
            };

            let mut line1_spans = vec![
                Span::styled(prefix, Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(format!("@{} ", uname), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!("(ID: {}) ", u.chat_id), Style::default().fg(CYBER_GRAY)),
                Span::styled(tier_badge, Style::default().fg(tier_color).add_modifier(Modifier::BOLD)),
            ];

            if u.is_banned {
                line1_spans.push(Span::styled(" [⛔ ЗАБАНЕН]", Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)));
            }

            let line1 = Line::from(line1_spans);

            let line2 = Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(format!("💰 Баланс: {} руб. │ ⏳ {} │ 🔔 Рассылка: {}", u.balance, expiry_info, if u.active { "ВКЛ" } else { "ВЫКЛ" }), Style::default().fg(Color::Rgb(180, 190, 200))),
            ]);

            let style = if is_selected {
                Style::default().bg(Color::Rgb(25, 35, 45))
            } else {
                Style::default()
            };

            ListItem::new(vec![line1, line2]).style(style)
        })
        .collect();

    let users_list = List::new(user_items)
        .block(users_block)
        .highlight_symbol("▶ ");

    f.render_stateful_widget(users_list, chunks[0], &mut state.user_list_state);

    // Right: Promocodes and Admin actions
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[1]);

    // Promocodes List
    let promo_block = Block::default()
        .title(Span::styled(
            format!(" 🎟 PROMOCODES ({}) [ 'p' Create | 'd' Delete ] ", state.promocodes.len()),
            Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let promo_items: Vec<ListItem> = state.promocodes
        .iter()
        .map(|(code, tier, days, uses)| {
            let tier_col = if tier == "Prime" { CYBER_RED } else { CYBER_GREEN };
            let line1 = Line::from(vec![
                Span::styled("🎟 ", Style::default().fg(CYBER_YELLOW)),
                Span::styled(code, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" [{} / {} дн.]", tier, days), Style::default().fg(tier_col).add_modifier(Modifier::BOLD)),
            ]);
            let line2 = Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(format!("Осталось активаций: {}", uses), Style::default().fg(CYBER_CYAN)),
            ]);
            ListItem::new(vec![line1, line2])
        })
        .collect();

    f.render_widget(List::new(promo_items).block(promo_block), right_chunks[0]);

    // Actions & Info Panel
    let actions_block = Block::default()
        .title(Span::styled(" 👑 ADMIN CONTROLS ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let (selected_user_str, ban_btn_label, ban_btn_color) = if let Some(u) = state.tg_users.get(state.selected_user_idx) {
        let name_str = format!("@{} (ID: {})", if u.username.is_empty() { "No Username" } else { &u.username }, u.chat_id);
        if u.is_banned {
            (format!("{} [⛔ ЗАБАНЕН]", name_str), "Разбанить пользователя (UNBAN)", CYBER_GREEN)
        } else {
            (format!("{} [✓ АКТИВЕН]", name_str), "Забанить пользователя (BAN)", CYBER_RED)
        }
    } else {
        ("None selected".to_string(), "Забанить/Разбанить", CYBER_RED)
    };

    let actions_text = vec![
        Line::from(vec![
            Span::styled(" Выбран юзер: ", Style::default().fg(CYBER_GRAY)),
            Span::styled(selected_user_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [F] ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("Поиск пользователя по ID / никнейму", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [X] ", Style::default().fg(ban_btn_color).add_modifier(Modifier::BOLD)),
            Span::styled(ban_btn_label, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" [G] ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("Выдать/продлить подписку (Plus/Prime)", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [B] ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled("Пополнить баланс в рублях/Stars", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [P] ", Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("Создать новый промокод", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [D] ", Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)),
            Span::styled("Удалить промокод", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" [R] ", Style::default().fg(CYBER_MAGENTA).add_modifier(Modifier::BOLD)),
            Span::styled("Обновить списки из базы SQLite", Style::default().fg(Color::White)),
        ]),
    ];

    f.render_widget(Paragraph::new(actions_text).block(actions_block), right_chunks[1]);
}

fn render_help_tab(f: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(Span::styled(" 📖 COMMANDS & KEYBOARD CHEATSHEET ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let lines = vec![
        Line::from(vec![Span::styled("GLOBAL NAVIGATION:", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))]),
        Line::from("  [1] - [7] or [Tab] / [Shift+Tab] : Switch between tabs"),
        Line::from("  [q] or [Esc]                     : Quit scanner / Close modal popup"),
        Line::from(""),
        Line::from(vec![Span::styled("SCANNER CONTROLS (TAB 1):", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))]),
        Line::from("  [Space]                          : Start / Stop scanning"),
        Line::from("  [m]                              : Cycle scan mode (MultiEngine, Targeted, Default, PlayerTracker, Playit, Ngrok, DNS)"),
        Line::from("  [t] / [s] / [y] / [o]            : Set threads (1-1000000) for Targeted, Standard, Playit, Ngrok streams"),
        Line::from("  [b]                              : Cycle Targeted DB (NoLic, Lic, All)"),
        Line::from("  [+] / [-]                        : Increase / Decrease active thread pool"),
        Line::from("  [i]                              : Edit scan limit amount / Toggle infinite scan"),
        Line::from("  [v]                              : Filter target Minecraft version"),
        Line::from("  [n]                              : Set target nickname (PlayerTracker mode)"),
        Line::from("  [d]                              : Set target DNS domain (DNS Bruteforce mode)"),
        Line::from("  [p]                              : Toggle Deep Scan (all ports 1-65535)"),
        Line::from("  [r]                              : Toggle & edit Auto RCON command"),
        Line::from("  [F1] / [F2] / [F3]               : Toggle Free / Whitelist / License filters"),
        Line::from("  [↑] / [↓]                        : Scroll through found servers live feed"),
        Line::from("  [c]                              : Copy IP address of currently selected server to clipboard"),
        Line::from(""),
        Line::from(vec![Span::styled("BOT CONTROL (TAB 4):", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))]),
        Line::from("  [t]                              : Toggle Pause / Resume Telegram Bot engine"),
        Line::from("  [a]                              : Broadcast announcement message to all users"),
        Line::from("  [k]                              : Edit Telegram Bot Token"),
        Line::from("  [c]                              : Clear live activity event logs"),
        Line::from("  [↑] / [↓]                        : Scroll real-time bot event logs"),
        Line::from(""),
        Line::from(vec![Span::styled("USERS & PROMO MANAGEMENT (TAB 5):", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))]),
        Line::from("  [g]                              : Give / Extend subscription to selected user (e.g. 'prime 30')"),
        Line::from("  [b]                              : Add balance to selected user (e.g. '500')"),
        Line::from("  [p]                              : Create new promo code (e.g. 'prime 30 10 VIP2026')"),
        Line::from("  [d]                              : Delete promo code"),
        Line::from("  [r]                              : Refresh users & promocodes list from SQLite"),
        Line::from("  [↑] / [↓]                        : Navigate registered bot users"),
        Line::from(""),
        Line::from(vec![Span::styled("DATABASE & PROXY (TABS 3 & 6):", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD))]),
        Line::from("  [e] or [Enter]                   : Edit search query in Database tab"),
        Line::from("  [x]                              : Toggle SOCKS5 proxy routing"),
        Line::from("  [s]                              : Scrape and concurrently validate public SOCKS5 proxies"),
        Line::from("  [u]                              : Refresh and check public IP address"),
    ];

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_footer(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Progress Bar
            Constraint::Length(1), // Scan status line
            Constraint::Length(1), // Hardware & Network Telemetry
        ])
        .split(area);

    let is_scanning = state.is_scanning.load(std::sync::atomic::Ordering::Relaxed);
    let deep_active = state.deep_scans_active.load(std::sync::atomic::Ordering::Relaxed);

    // Progress Bar
    let (ratio, label) = if deep_active > 0 {
        let d_scanned = state.deep_scanned_ports.load(std::sync::atomic::Ordering::Relaxed);
        let d_total = state.deep_total_ports.load(std::sync::atomic::Ordering::Relaxed);
        let frac = if d_total > 0 { (d_scanned as f64 / d_total as f64).clamp(0.0, 1.0) } else { 0.0 };
        (frac, format!("DEEP PORT SCAN ACTIVE: {} / {} ports ({:.1}%)", d_scanned, d_total, frac * 100.0))
    } else if is_scanning {
        let scanned = state.scanned_ips.load(std::sync::atomic::Ordering::Relaxed);
        if state.infinite_scan && state.scan_type != ScanType::PlayerTracker {
            (1.0, format!("GLOBAL INFINITE SCAN ACTIVE: {} IP addresses checked", scanned))
        } else {
            let total = state.total_ips_current_scan.max(1);
            let frac = (scanned as f64 / total as f64).clamp(0.0, 1.0);
            (frac, format!("PROGRESS: {} / {} IPs ({:.1}%)", scanned, total, frac * 100.0))
        }
    } else {
        (0.0, "SCANNER IDLE // READY TO LAUNCH [PRESS SPACE]".to_string())
    };

    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(if is_scanning { CYBER_GREEN } else { CYBER_GRAY })
                .bg(Color::Rgb(20, 25, 30)),
        )
        .ratio(ratio)
        .label(Span::styled(label, Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)));

    f.render_widget(gauge, chunks[0]);

    // Bottom Stats text (Line 1)
    let status_line = Line::from(vec![
        Span::styled(" STATUS: ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled(if is_scanning { "● RUNNING " } else { "○ IDLE " }, Style::default().fg(if is_scanning { CYBER_GREEN } else { CYBER_GRAY }).add_modifier(Modifier::BOLD)),
        Span::styled("│ FOUND: ", Style::default().fg(CYBER_CYAN)),
        Span::styled(format!("{} ", state.stats_total), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("│ CRACKED: ", Style::default().fg(CYBER_YELLOW)),
        Span::styled(format!("{} ", state.stats_cracked), Style::default().fg(Color::White)),
        Span::styled("│ VULNS: ", Style::default().fg(CYBER_RED)),
        Span::styled(format!("{} ", state.stats_bungee_vuln + state.stats_velocity_vuln), Style::default().fg(CYBER_RED).add_modifier(Modifier::BOLD)),
        Span::styled("│ PROXIES: ", Style::default().fg(CYBER_CYAN)),
        Span::styled(format!("{}", if let Ok(lock) = state.proxy_list.lock() { lock.len() } else { 0 }), Style::default().fg(Color::White)),
    ]);

    f.render_widget(Paragraph::new(status_line), chunks[1]);

    // System Telemetry text (Line 2)
    let cpu_color = if state.sys_metrics.cpu_pct > 85.0 {
        CYBER_RED
    } else if state.sys_metrics.cpu_pct > 60.0 {
        CYBER_YELLOW
    } else {
        CYBER_GREEN
    };

    let ram_color = if state.sys_metrics.ram_pct > 85.0 {
        CYBER_RED
    } else if state.sys_metrics.ram_pct > 65.0 {
        CYBER_YELLOW
    } else {
        CYBER_GREEN
    };

    let rx_str = if state.sys_metrics.net_rx_kbs >= 1024.0 {
        format!("{:.1} MB/s", state.sys_metrics.net_rx_kbs / 1024.0)
    } else {
        format!("{:.0} KB/s", state.sys_metrics.net_rx_kbs)
    };

    let tx_str = if state.sys_metrics.net_tx_kbs >= 1024.0 {
        format!("{:.1} MB/s", state.sys_metrics.net_tx_kbs / 1024.0)
    } else {
        format!("{:.0} KB/s", state.sys_metrics.net_tx_kbs)
    };

    let telemetry_line = Line::from(vec![
        Span::styled(" ⚡ CPU: ", Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:.1}% ", state.sys_metrics.cpu_pct), Style::default().fg(cpu_color).add_modifier(Modifier::BOLD)),
        Span::styled("│ 🧠 RAM: ", Style::default().fg(CYBER_CYAN)),
        Span::styled(format!("{} MB / {} MB ({:.1}%) ", state.sys_metrics.ram_used_mb, state.sys_metrics.ram_total_mb, state.sys_metrics.ram_pct), Style::default().fg(ram_color).add_modifier(Modifier::BOLD)),
        Span::styled("│ 🚀 SPEED: ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} pps ", state.sys_metrics.scan_pps), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("│ 🌐 NET: ", Style::default().fg(CYBER_CYAN)),
        Span::styled(format!("↓ {}  ↑ {} ", rx_str, tx_str), Style::default().fg(Color::White)),
    ]);

    f.render_widget(Paragraph::new(telemetry_line), chunks[2]);
}

fn render_input_modal(f: &mut Frame, state: &AppState) {
    if let Some((ref title, ref current_val)) = state.editing_field {
        let area = centered_rect(60, 20, f.size());
        f.render_widget(Clear, area);

        let block = Block::default()
            .title(Span::styled(format!(" ✍ EDIT: {} (Enter to confirm | Esc to cancel) ", title), Style::default().fg(CYBER_YELLOW).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(CYBER_GREEN))
            .style(Style::default().bg(Color::Rgb(10, 15, 25)));

        let input_text = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(" > ", Style::default().fg(CYBER_GREEN).add_modifier(Modifier::BOLD)),
                Span::styled(current_val, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("█", Style::default().fg(CYBER_GREEN)),
            ]),
        ])
        .block(block);

        f.render_widget(input_text, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
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

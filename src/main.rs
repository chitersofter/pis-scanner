mod db;
mod types;
mod utils;
mod minecraft;
mod scanner;
mod nick_generator;
mod playit;
mod ngrok;
mod proxy;
mod donatepay;
mod telegram;
mod ui;
mod system_monitor;

use crossbeam_channel::{unbounded, Receiver, Sender};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::ListState, Terminal};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use types::{FoundServer, ScanType, TargetedDbType};

pub struct AppState {
    pub scan_type: ScanType,
    pub targeted_db: TargetedDbType,
    pub threads: u32,
    pub amount: u32,
    pub target_version: String,
    pub target_nick: String,
    pub dns_target_domain: String,
    pub infinite_scan: bool,
    pub deep_scan: bool,
    pub filter_green: bool,
    pub filter_yellow: bool,
    pub filter_blue: bool,
    pub is_scanning: Arc<AtomicBool>,
    pub scanned_ips: Arc<AtomicU32>,
    pub total_ips_current_scan: u32,
    pub deep_scans_active: Arc<AtomicU32>,
    pub deep_scanned_ports: Arc<AtomicU32>,
    pub deep_total_ports: Arc<AtomicU32>,
    pub found: Vec<FoundServer>,
    pub rx: Receiver<FoundServer>,
    pub tx: Sender<FoundServer>,

    pub multi_targeted_threads: u32,
    pub multi_standard_threads: u32,
    pub multi_playit_threads: u32,
    pub multi_ngrok_threads: u32,
    
    pub selected_tab: usize,
    pub selected_server_idx: usize,
    pub selected_db_idx: usize,
    pub server_list_state: ListState,
    pub db_list_state: ListState,
    pub stats_total: usize,
    pub stats_cracked: usize,
    pub stats_lic: usize,
    pub stats_bungee_vuln: usize,
    pub stats_velocity_vuln: usize,
    pub stats_versions: HashMap<String, usize>,
    
    pub auto_rcon_enabled: bool,
    pub auto_rcon_cmd: String,
    
    pub db_search_query: String,
    pub db_results: Vec<db::DbServer>,

    pub tg_users: Vec<db::TgDbUser>,
    pub promocodes: Vec<(String, String, i64, i32)>,
    pub selected_user_idx: usize,
    pub user_list_state: ListState,
    pub bot_logs_state: ListState,

    pub use_proxy: bool,
    pub auto_scrape_proxies: bool,
    pub proxy_list: Arc<Mutex<Vec<String>>>,
    pub public_ip: String,
    pub ip_check_tx: crossbeam_channel::Sender<String>,
    pub ip_check_rx: crossbeam_channel::Receiver<String>,
    
    pub tg_bot_token: String,
    pub editing_field: Option<(String, String)>,
    pub sys_metrics: system_monitor::SystemMetrics,
}

impl AppState {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let (ip_check_tx, ip_check_rx) = crossbeam_channel::bounded(1);
        
        // 1. Try SQLite database config
        let mut tg_bot_token = db::get_config("tg_bot_token").unwrap_or_default().trim().to_string();

        // 2. Try tg_config.txt in current directory
        if tg_bot_token.is_empty() {
            tg_bot_token = fs::read_to_string("tg_config.txt").unwrap_or_default().trim().to_string();
        }

        // 3. Try tg_config.txt next to executable binary
        if tg_bot_token.is_empty() {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    let next_to_exe = parent.join("tg_config.txt");
                    if let Ok(content) = fs::read_to_string(&next_to_exe) {
                        tg_bot_token = content.trim().to_string();
                    }
                }
            }
        }

        // 4. Default fallback token - keep empty if not provided
        if !tg_bot_token.is_empty() {
            // Save persistently to SQLite and file
            db::set_config("tg_bot_token", &tg_bot_token);
            let _ = fs::write("tg_config.txt", &tg_bot_token);
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    let _ = fs::write(parent.join("tg_config.txt"), &tg_bot_token);
                }
            }
        }

        telegram::init(tg_bot_token.clone());
        donatepay::init();

        let initial_db = db::search_servers("");
        let tg_users = db::get_all_tg_users();
        let promocodes = db::get_all_promocodes();

        let mut server_list_state = ListState::default();
        server_list_state.select(Some(0));
        let mut db_list_state = ListState::default();
        db_list_state.select(Some(0));
        let mut user_list_state = ListState::default();
        user_list_state.select(Some(0));
        let mut bot_logs_state = ListState::default();
        bot_logs_state.select(Some(0));

        Self {
            scan_type: ScanType::Targeted,
            targeted_db: TargetedDbType::NoLic,
            infinite_scan: true,
            threads: 500,
            amount: 5000,
            target_version: String::new(),
            target_nick: String::new(),
            dns_target_domain: String::new(),
            deep_scan: false,
            filter_green: true,
            filter_yellow: true,
            filter_blue: true,
            is_scanning: Arc::new(AtomicBool::new(false)),
            scanned_ips: Arc::new(AtomicU32::new(0)),
            total_ips_current_scan: 0,
            deep_scans_active: Arc::new(AtomicU32::new(0)),
            deep_scanned_ports: Arc::new(AtomicU32::new(0)),
            deep_total_ports: Arc::new(AtomicU32::new(0)),
            found: Vec::new(),
            rx,
            tx,
            multi_targeted_threads: 50,
            multi_standard_threads: 50,
            multi_playit_threads: 20,
            multi_ngrok_threads: 20,
            selected_tab: 0,
            selected_server_idx: 0,
            selected_db_idx: 0,
            server_list_state,
            db_list_state,
            tg_users,
            promocodes,
            selected_user_idx: 0,
            user_list_state,
            bot_logs_state,
            stats_total: db::get_total_server_count() as usize,
            stats_cracked: 0,
            stats_lic: 0,
            stats_bungee_vuln: 0,
            stats_velocity_vuln: 0,
            stats_versions: HashMap::new(),
            auto_rcon_enabled: false,
            auto_rcon_cmd: String::new(),
            db_search_query: String::new(),
            db_results: initial_db,
            use_proxy: false,
            auto_scrape_proxies: false,
            proxy_list: Arc::new(Mutex::new({
                if let Ok(content) = std::fs::read_to_string("proxies.txt") {
                    content
                        .lines()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty() && s.contains(':'))
                        .collect()
                } else {
                    Vec::new()
                }
            })),
            public_ip: String::from("Not Checked"),
            ip_check_tx,
            ip_check_rx,
            tg_bot_token,
            editing_field: None,
            sys_metrics: system_monitor::SystemMetrics::default(),
        }
    }

    pub fn reload_admin_data(&mut self) {
        self.tg_users = db::get_all_tg_users();
        self.promocodes = db::get_all_promocodes();
        if self.selected_user_idx >= self.tg_users.len() && !self.tg_users.is_empty() {
            self.selected_user_idx = self.tg_users.len() - 1;
        }
        self.user_list_state.select(Some(self.selected_user_idx));
    }

    pub fn start_scan(&mut self) {
        if self.is_scanning.load(Ordering::Relaxed) {
            return;
        }

        self.is_scanning.store(true, Ordering::SeqCst);
        self.scanned_ips.store(0, Ordering::SeqCst);
        self.total_ips_current_scan = self.amount;
        self.deep_scans_active.store(0, Ordering::SeqCst);
        self.deep_scanned_ports.store(0, Ordering::SeqCst);
        self.deep_total_ports.store(0, Ordering::SeqCst);
        self.found.clear();
        self.selected_server_idx = 0;

        scanner::run_scan(
            self.scan_type.clone(),
            self.targeted_db.clone(),
            self.threads,
            self.amount,
            self.infinite_scan,
            self.target_version.clone(),
            self.deep_scan,
            self.target_nick.clone(),
            self.auto_rcon_enabled,
            self.auto_rcon_cmd.clone(),
            self.dns_target_domain.clone(),
            self.use_proxy,
            self.auto_scrape_proxies,
            self.proxy_list.clone(),
            self.tx.clone(),
            self.is_scanning.clone(),
            self.scanned_ips.clone(),
            self.deep_scans_active.clone(),
            self.deep_scanned_ports.clone(),
            self.deep_total_ports.clone(),
            self.multi_targeted_threads,
            self.multi_standard_threads,
            self.multi_playit_threads,
            self.multi_ngrok_threads,
        );
    }

    pub fn stop_scan(&mut self) {
        self.is_scanning.store(false, Ordering::SeqCst);
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Limit the blocking thread pool (used for DNS resolution) to prevent
    // hundreds of blocking threads from consuming gigabytes of stack memory.
    std::env::set_var("TOKIO_BLOCKING_THREADS", "16");
    db::init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Panic hook to restore terminal
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_panic(info);
    }));

    let mut state = AppState::new();
    let mut sys_mon = system_monitor::SysMonitor::new();
    let mut clipboard = arboard::Clipboard::new().ok();
    let mut last_draw = std::time::Instant::now();

    loop {
        // Drain newly found servers (Ring buffer of 250 in RAM for ultra-fast rendering, SQLite keeps everything)
        while let Ok(s) = state.rx.try_recv() {
            state.stats_total += 1;
            if s.is_cracked {
                state.stats_cracked += 1;
            } else {
                state.stats_lic += 1;
            }
            if s.is_bungee_vulnerable {
                state.stats_bungee_vuln += 1;
            }
            if s.is_velocity_vulnerable {
                state.stats_velocity_vuln += 1;
            }

            let v_str = s
                .info
                .split(" | ")
                .nth(1)
                .unwrap_or("Unknown")
                .split(' ')
                .next()
                .unwrap_or("Unknown")
                .to_string();
            *state.stats_versions.entry(v_str).or_insert(0) += 1;

            if state.found.len() >= 250 {
                state.found.remove(0);
                state.selected_server_idx = state.selected_server_idx.saturating_sub(1);
                state.server_list_state.select(Some(state.selected_server_idx));
            }
            state.found.push(s);
        }

        if let Ok(ip) = state.ip_check_rx.try_recv() {
            state.public_ip = ip;
        }

        // Strictly throttle drawing to at most 10 FPS (100ms) to prevent 100% CPU spin over SSH
        if last_draw.elapsed() >= Duration::from_millis(100) {
            state.sys_metrics = sys_mon.update(&state.scanned_ips);
            terminal.draw(|f| ui::render(f, &mut state))?;
            last_draw = std::time::Instant::now();
        }

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Force immediate redraw upon user interaction
                let should_redraw = true;

                // If editing field modal is open
                if let Some((ref field_name, ref mut buffer)) = state.editing_field {
                    match key.code {
                        KeyCode::Enter => {
                            let val = buffer.trim().to_string();
                            let field_name_clone = field_name.clone();
                            state.editing_field = None;
                            match field_name_clone.as_str() {
                                "Targeted Streams (1-1000000)" => {
                                    if let Ok(num) = val.parse::<u32>() {
                                        state.multi_targeted_threads = num.clamp(0, 1_000_000);
                                    }
                                }
                                "Standard IPv4 Streams (1-1000000)" => {
                                    if let Ok(num) = val.parse::<u32>() {
                                        state.multi_standard_threads = num.clamp(0, 1_000_000);
                                    }
                                }
                                "Playit Streams (1-1000000)" => {
                                    if let Ok(num) = val.parse::<u32>() {
                                        state.multi_playit_threads = num.clamp(0, 1_000_000);
                                    }
                                }
                                "Ngrok Streams (1-1000000)" => {
                                    if let Ok(num) = val.parse::<u32>() {
                                        state.multi_ngrok_threads = num.clamp(0, 1_000_000);
                                    }
                                }
                                "Version" => state.target_version = val,
                                "Target Nickname" => state.target_nick = val,
                                "Base DNS Domain" => state.dns_target_domain = val,
                                "Auto RCON Command" => state.auto_rcon_cmd = val,
                                "Scan Amount / Limit" => {
                                    if let Ok(num) = val.parse::<u32>() {
                                        state.amount = num;
                                        state.infinite_scan = false;
                                    }
                                }
                                "Database Query" => {
                                    state.db_search_query = val.clone();
                                    state.db_results = db::search_servers(&val);
                                    state.selected_db_idx = 0;
                                    state.db_list_state.select(Some(0));
                                }
                                "Telegram Bot Token" => {
                                    state.tg_bot_token = val.clone();
                                    telegram::update_token(val.clone());
                                    db::set_config("tg_bot_token", &val);
                                    let _ = fs::write("tg_config.txt", &val);
                                    if let Ok(exe_path) = std::env::current_exe() {
                                        if let Some(parent) = exe_path.parent() {
                                            let _ = fs::write(parent.join("tg_config.txt"), &val);
                                        }
                                    }
                                }
                                "Broadcast Announcement to All Users" => {
                                    let msg = val.clone();
                                    tokio::spawn(async move {
                                        telegram::send_admin_broadcast(&msg).await;
                                    });
                                }
                                "Manual Proxies (ip:port)" | "Manual Proxies (ip:port or ip:port:user:pass)" => {
                                    let lines: Vec<String> = val
                                        .lines()
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty() && s.contains(':'))
                                        .collect();
                                    if let Ok(mut lock) = state.proxy_list.lock() {
                                        lock.extend(lines.clone());
                                        lock.sort();
                                        lock.dedup();
                                    }
                                    tokio::spawn(async move {
                                        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("proxies.txt") {
                                            use std::io::Write;
                                            for line in lines {
                                                let _ = writeln!(file, "{}", line);
                                            }
                                        }
                                    });
                                }
                                "Give Subscription (format: <plus|prime> <days>)" | "Give Subscription (format: <standard|prime> <days>)" => {
                                    let parts: Vec<&str> = val.split_whitespace().collect();
                                    if let Some(user) = state.tg_users.get(state.selected_user_idx) {
                                        let target_id = user.chat_id;
                                        let tier = if parts.first().map(|s| s.to_lowercase()).as_deref() == Some("prime") { "Prime" } else { "Plus" };
                                        let days = if parts.len() >= 2 {
                                            parts[1].parse::<i64>().unwrap_or(30)
                                        } else if let Ok(d) = val.parse::<i64>() {
                                            d
                                        } else {
                                            30
                                        };
                                        let _ = db::grant_subscription(target_id, tier, days);
                                        state.reload_admin_data();
                                    }
                                }
                                "Add Balance (amount in rubles/stars)" => {
                                    if let Ok(amount) = val.parse::<i64>() {
                                        if let Some(user) = state.tg_users.get(state.selected_user_idx) {
                                            let _ = db::add_user_balance(user.chat_id, amount);
                                            state.reload_admin_data();
                                        }
                                    }
                                }
                                "Create Promocode (format: <plus|prime> <days> <uses> <CODE>)" => {
                                    let parts: Vec<&str> = val.split_whitespace().collect();
                                    if parts.len() >= 4 {
                                        let tier = if parts[0].to_lowercase() == "prime" { "Prime" } else { "Plus" };
                                        let days = parts[1].parse::<i64>().unwrap_or(30);
                                        let uses = parts[2].parse::<i32>().unwrap_or(1);
                                        let code = parts[3];
                                        let _ = db::create_promocode(code, tier, days, uses);
                                        state.reload_admin_data();
                                    }
                                }
                                "Delete Promocode (enter code)" => {
                                    let _ = db::delete_promocode(&val);
                                    state.reload_admin_data();
                                }
                                "Search User by ID / Username" => {
                                    let q = val.trim();
                                    if !q.is_empty() {
                                        if let Ok(target_id) = q.parse::<i64>() {
                                            if let Some(pos) = state.tg_users.iter().position(|u| u.chat_id == target_id) {
                                                state.selected_user_idx = pos;
                                                state.user_list_state.select(Some(pos));
                                            }
                                        } else {
                                            let q_lower = q.to_lowercase();
                                            if let Some(pos) = state.tg_users.iter().position(|u| u.username.to_lowercase().contains(&q_lower)) {
                                                state.selected_user_idx = pos;
                                                state.user_list_state.select(Some(pos));
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                            state.editing_field = None;
                        }
                        KeyCode::Esc => {
                            state.editing_field = None;
                        }
                        KeyCode::Backspace => {
                            buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            buffer.push(c);
                        }
                        _ => {}
                    }
                } else {
                    // Normal Navigation
                    match key.code {
                        KeyCode::Char('q') => {
                            break;
                        }
                        KeyCode::Char('1') => state.selected_tab = 0,
                        KeyCode::Char('2') => state.selected_tab = 1,
                        KeyCode::Char('3') => {
                            state.selected_tab = 2;
                            state.db_results = db::search_servers(&state.db_search_query);
                            state.selected_db_idx = 0;
                            state.db_list_state.select(Some(0));
                        }
                        KeyCode::Char('4') => state.selected_tab = 3,
                        KeyCode::Char('5') => {
                            state.selected_tab = 4;
                            state.reload_admin_data();
                        }
                        KeyCode::Char('6') => state.selected_tab = 5,
                        KeyCode::Char('7') => state.selected_tab = 6,
                        KeyCode::Tab => {
                            state.selected_tab = (state.selected_tab + 1) % 7;
                            if state.selected_tab == 2 {
                                state.db_results = db::search_servers(&state.db_search_query);
                                state.selected_db_idx = 0;
                                state.db_list_state.select(Some(0));
                            } else if state.selected_tab == 4 {
                                state.reload_admin_data();
                            }
                        }
                        KeyCode::BackTab => {
                            state.selected_tab = (state.selected_tab + 6) % 7;
                            if state.selected_tab == 2 {
                                state.db_results = db::search_servers(&state.db_search_query);
                                state.selected_db_idx = 0;
                                state.db_list_state.select(Some(0));
                            } else if state.selected_tab == 4 {
                                state.reload_admin_data();
                            }
                        }
                        KeyCode::Char(' ') => {
                            if state.is_scanning.load(Ordering::Relaxed) {
                                state.stop_scan();
                            } else {
                                state.start_scan();
                            }
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            if state.selected_tab == 5 {
                                state.editing_field = Some(("Manual Proxies (ip:port or ip:port:user:pass)".to_string(), String::new()));
                            } else {
                                state.scan_type = match state.scan_type {
                                    ScanType::MultiEngine => ScanType::Targeted,
                                    ScanType::Targeted => ScanType::Default,
                                    ScanType::Default => ScanType::PlayerTracker,
                                    ScanType::PlayerTracker => ScanType::Playit,
                                    ScanType::Playit => ScanType::Ngrok,
                                    ScanType::Ngrok => ScanType::DnsBrute,
                                    ScanType::DnsBrute => ScanType::MultiEngine,
                                };
                            }
                        }
                        KeyCode::Char('b') | KeyCode::Char('B') => {
                            if state.selected_tab == 4 {
                                if let Some(_u) = state.tg_users.get(state.selected_user_idx) {
                                    state.editing_field = Some(("Add Balance (amount in rubles/stars)".to_string(), "500".to_string()));
                                }
                            } else {
                                state.targeted_db = match state.targeted_db {
                                    TargetedDbType::NoLic => TargetedDbType::Lic,
                                    TargetedDbType::Lic => TargetedDbType::All,
                                    TargetedDbType::All => TargetedDbType::NoLic,
                                };
                            }
                        }
                        KeyCode::Char('g') | KeyCode::Char('G') => {
                            if state.selected_tab == 4 && !state.tg_users.is_empty() {
                                state.editing_field = Some(("Give Subscription (format: <plus|prime> <days>)".to_string(), "prime 30".to_string()));
                            }
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            state.threads = (state.threads + 50).min(1_000_000);
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            state.threads = state.threads.saturating_sub(50).max(10);
                        }
                        KeyCode::Char('i') | KeyCode::Char('I') => {
                            if state.infinite_scan {
                                state.infinite_scan = false;
                                state.editing_field = Some(("Scan Amount / Limit".to_string(), state.amount.to_string()));
                            } else {
                                state.infinite_scan = true;
                            }
                        }
                        KeyCode::Char('v') | KeyCode::Char('V') => {
                            state.editing_field = Some(("Version".to_string(), state.target_version.clone()));
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            state.editing_field = Some(("Target Nickname".to_string(), state.target_nick.clone()));
                        }
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            if state.selected_tab == 4 {
                                state.editing_field = Some(("Delete Promocode (enter code)".to_string(), "".to_string()));
                            } else {
                                state.editing_field = Some(("Base DNS Domain".to_string(), state.dns_target_domain.clone()));
                            }
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            if state.selected_tab == 4 {
                                state.editing_field = Some(("Create Promocode (format: <plus|prime> <days> <uses> <CODE>)".to_string(), "prime 30 10 VIP2026".to_string()));
                            } else {
                                state.deep_scan = !state.deep_scan;
                            }
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            if state.selected_tab == 4 {
                                state.reload_admin_data();
                            } else {
                                state.auto_rcon_enabled = !state.auto_rcon_enabled;
                                if state.auto_rcon_enabled && state.auto_rcon_cmd.is_empty() {
                                    state.editing_field = Some(("Auto RCON Command".to_string(), "op your_nick".to_string()));
                                }
                            }
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') => {
                            if state.selected_tab == 0 {
                                state.editing_field = Some(("Targeted Streams (1-100)".to_string(), state.multi_targeted_threads.to_string()));
                            } else if state.selected_tab == 3 {
                                telegram::toggle_bot();
                            } else {
                                state.editing_field = Some(("Telegram Bot Token".to_string(), state.tg_bot_token.clone()));
                            }
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            if state.selected_tab == 0 {
                                state.editing_field = Some(("Standard IPv4 Streams (1-100)".to_string(), state.multi_standard_threads.to_string()));
                            } else if state.selected_tab == 5 {
                                let proxy_list_clone = state.proxy_list.clone();
                                tokio::spawn(async move {
                                    let scraped = proxy::scrape_proxies().await;
                                    if let Ok(mut lock) = proxy_list_clone.lock() {
                                        lock.extend(scraped);
                                        lock.sort();
                                        lock.dedup();
                                    }
                                });
                            }
                        }
                        KeyCode::Char('e') | KeyCode::Char('E') => {
                            if state.selected_tab == 2 {
                                state.editing_field = Some(("Database Query".to_string(), state.db_search_query.clone()));
                            }
                        }
                        KeyCode::Enter => {
                            if state.selected_tab == 2 {
                                state.editing_field = Some(("Database Query".to_string(), state.db_search_query.clone()));
                            }
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            if state.selected_tab == 0 {
                                state.editing_field = Some(("Playit Streams (1-100)".to_string(), state.multi_playit_threads.to_string()));
                            }
                        }
                        KeyCode::Char('o') | KeyCode::Char('O') => {
                            if state.selected_tab == 0 {
                                state.editing_field = Some(("Ngrok Streams (1-100)".to_string(), state.multi_ngrok_threads.to_string()));
                            }
                        }
                        KeyCode::F(1) => state.filter_green = !state.filter_green,
                        KeyCode::F(2) => state.filter_yellow = !state.filter_yellow,
                        KeyCode::F(3) => state.filter_blue = !state.filter_blue,
                        KeyCode::Up => {
                            if state.selected_tab == 0 && !state.found.is_empty() {
                                state.selected_server_idx = state.selected_server_idx.saturating_sub(1);
                                state.server_list_state.select(Some(state.selected_server_idx));
                            } else if state.selected_tab == 2 && !state.db_results.is_empty() {
                                state.selected_db_idx = state.selected_db_idx.saturating_sub(1);
                                state.db_list_state.select(Some(state.selected_db_idx));
                            } else if state.selected_tab == 4 && !state.tg_users.is_empty() {
                                state.selected_user_idx = state.selected_user_idx.saturating_sub(1);
                                state.user_list_state.select(Some(state.selected_user_idx));
                            }
                        }
                        KeyCode::Down => {
                            if state.selected_tab == 0 && !state.found.is_empty() {
                                if state.selected_server_idx + 1 < state.found.len() {
                                    state.selected_server_idx += 1;
                                    state.server_list_state.select(Some(state.selected_server_idx));
                                }
                            } else if state.selected_tab == 2 && !state.db_results.is_empty() {
                                if state.selected_db_idx + 1 < state.db_results.len() {
                                    state.selected_db_idx += 1;
                                    state.db_list_state.select(Some(state.selected_db_idx));
                                }
                            } else if state.selected_tab == 4 && !state.tg_users.is_empty() {
                                if state.selected_user_idx + 1 < state.tg_users.len() {
                                    state.selected_user_idx += 1;
                                    state.user_list_state.select(Some(state.selected_user_idx));
                                }
                            }
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            if state.selected_tab == 3 {
                                telegram::clear_bot_logs();
                            } else {
                                let ip_to_copy = if state.selected_tab == 0 {
                                    state.found.get(state.selected_server_idx).map(|s| s.ip.clone())
                                } else if state.selected_tab == 2 {
                                    state.db_results.get(state.selected_db_idx).map(|s| s.ip.clone())
                                } else {
                                    None
                                };

                                if let Some(ip) = ip_to_copy {
                                    if let Some(ref mut cb) = clipboard {
                                        let _ = cb.set_text(ip);
                                    }
                                }
                            }
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            if state.selected_tab == 3 {
                                state.editing_field = Some(("Broadcast Announcement to All Users".to_string(), "".to_string()));
                            } else if state.selected_tab == 5 {
                                state.auto_scrape_proxies = !state.auto_scrape_proxies;
                            }
                        }
                        KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::Char('/') => {
                            if state.selected_tab == 4 {
                                state.editing_field = Some(("Search User by ID / Username".to_string(), "".to_string()));
                            }
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') => {
                            if state.selected_tab == 4 {
                                if let Some(user) = state.tg_users.get(state.selected_user_idx) {
                                    if user.is_banned {
                                        let _ = db::unban_user(user.chat_id);
                                    } else {
                                        let _ = db::ban_user(user.chat_id);
                                    }
                                    state.reload_admin_data();
                                }
                            } else if state.selected_tab == 5 {
                                state.use_proxy = !state.use_proxy;
                            }
                        }
                        KeyCode::Char('u') | KeyCode::Char('U') => {
                            if state.selected_tab == 5 {
                                let proxy_to_use = if state.use_proxy {
                                    state.proxy_list.lock().ok().and_then(|l| l.first().cloned())
                                } else {
                                    None
                                };
                                let tx_ip = state.ip_check_tx.clone();
                                tokio::spawn(async move {
                                    let ip = proxy::check_public_ip(proxy_to_use.as_deref()).await;
                                    let _ = tx_ip.send(ip);
                                });
                                state.public_ip = "⏳ Проверка...".to_string();
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Char('K') => {
                            if state.selected_tab == 3 {
                                state.editing_field = Some(("Telegram Bot Token".to_string(), state.tg_bot_token.clone()));
                            } else if state.selected_tab == 5 {
                                if let Ok(mut lock) = state.proxy_list.lock() {
                                    lock.clear();
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if should_redraw {
                    state.sys_metrics = sys_mon.update(&state.scanned_ips);
                    terminal.draw(|f| ui::render(f, &mut state))?;
                    last_draw = std::time::Instant::now();
                }
            }
        }
    }

    // Cleanup & restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!("\n[+] PIS Scanner terminated safely.");
    Ok(())
}
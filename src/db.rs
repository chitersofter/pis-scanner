use rusqlite::Connection;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct DbServer {
    pub ip: String,
    pub info: String,
    pub is_cracked: bool,
    pub is_registered: bool,
    pub is_op: bool,
    pub needs_mods: bool,
    pub has_builds: bool,
    pub movement_restricted: bool,
    pub rcon_pass: Option<String>,
}

use crate::types::FoundServer;

#[derive(Clone, Debug)]
pub struct TgDbUser {
    pub chat_id: i64,
    pub username: String,
    pub tier: String, // "Free", "Plus", "Prime"
    pub balance: i64, // rubles or credits
    pub sub_expires_at: i64, // unix timestamp in seconds
    pub active: bool,
    pub filter_green: bool,
    pub filter_yellow: bool,
    pub filter_blue: bool,
    pub pool_mode: String, // "auto", "exclusive", "plus", "free"
    pub src_default: bool,
    pub src_ngrok: bool,
    pub src_playit: bool,
    pub src_e4mc: bool,
    pub ai_enabled: bool,
    pub created_at: i64,
    pub hunt_ver: String,
    pub hunt_online: i32,
    pub hunt_builds: bool,
    pub hunt_noreg: bool,
    pub hunt_op: bool,
    pub hunt_geo: String,
    pub referrer_id: i64,
    pub ref_count: i64,
    pub ref_earned: i64,
    pub last_daily_case: i64,
    pub is_banned: bool,
}

impl TgDbUser {
    pub fn is_sub_active(&self) -> bool {
        if self.tier == "Free" {
            return false;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.sub_expires_at > now
    }

    pub fn active_tier(&self) -> String {
        if self.is_sub_active() {
            if self.tier == "Standard" {
                "Plus".to_string()
            } else {
                self.tier.clone()
            }
        } else {
            "Free".to_string()
        }
    }

    pub fn has_pool(&self, pool: &str) -> bool {
        let clean_pool = if pool == "standard" { "plus" } else { pool };
        if self.pool_mode == "auto" {
            return match clean_pool {
                "exclusive" => self.active_tier() == "Prime",
                "plus" => self.active_tier() == "Plus" || self.active_tier() == "Prime",
                "free" => self.active_tier() == "Free",
                _ => true,
            };
        }
        self.pool_mode.split(',').any(|p| {
            let p_clean = if p.trim() == "standard" { "plus" } else { p.trim() };
            p_clean == clean_pool
        })
    }

    pub fn toggle_pool(&mut self, pool: &str) {
        let clean_pool = if pool == "standard" { "plus" } else { pool };
        let mut pools: std::collections::HashSet<String> = if self.pool_mode == "auto" || self.pool_mode == "all" {
            let mut s = std::collections::HashSet::new();
            if self.active_tier() == "Prime" {
                s.insert("exclusive".to_string());
                s.insert("plus".to_string());
            } else if self.active_tier() == "Plus" {
                s.insert("plus".to_string());
            } else {
                s.insert("free".to_string());
            }
            s
        } else {
            self.pool_mode
                .split(',')
                .map(|s| {
                    let t = s.trim();
                    if t == "standard" { "plus".to_string() } else { t.to_string() }
                })
                .filter(|s| !s.is_empty())
                .collect()
        };

        if pools.contains(clean_pool) {
            pools.remove(clean_pool);
        } else {
            pools.insert(clean_pool.to_string());
        }

        if pools.is_empty() {
            if self.active_tier() == "Prime" {
                pools.insert("exclusive".to_string());
            } else if self.active_tier() == "Plus" {
                pools.insert("plus".to_string());
            } else {
                pools.insert("free".to_string());
            }
        }

        let mut list: Vec<String> = pools.into_iter().collect();
        list.sort();
        self.pool_mode = list.join(",");
    }

    pub fn matches_hunt_filters(&self, server: &FoundServer) -> bool {
        if self.hunt_ver != "all" && !self.hunt_ver.is_empty() {
            if !server.version.to_lowercase().contains(&self.hunt_ver.to_lowercase()) {
                return false;
            }
        }
        if self.hunt_online == 1 && server.online_players <= 0 {
            return false;
        } else if self.hunt_online == 2 && (server.online_players < 1 || server.online_players > 20) {
            return false;
        } else if self.hunt_online == 3 && server.online_players <= 20 {
            return false;
        }
        if self.hunt_builds && !server.has_builds {
            return false;
        }
        if self.hunt_noreg && server.is_registered {
            return false;
        }
        if self.hunt_op && !server.is_op {
            return false;
        }
        if self.hunt_geo != "all" && !self.hunt_geo.is_empty() {
            let s_geo = server.geo.to_lowercase();
            let h_geo = self.hunt_geo.to_lowercase();
            if h_geo == "ru" {
                if !s_geo.contains("russia") && !s_geo.contains("росси") && !s_geo.contains("ru") && !s_geo.contains("moscow") {
                    return false;
                }
            } else if h_geo == "eu" {
                if !s_geo.contains("germany") && !s_geo.contains("france") && !s_geo.contains("finland") && !s_geo.contains("netherlands") && !s_geo.contains("poland") && !s_geo.contains("europe") {
                    return false;
                }
            } else if h_geo == "us" {
                if !s_geo.contains("united states") && !s_geo.contains("usa") && !s_geo.contains("us") && !s_geo.contains("canada") {
                    return false;
                }
            } else if !s_geo.contains(&h_geo) {
                return false;
            }
        }
        true
    }
}

pub fn init() {
    let conn = Connection::open("servers.db").unwrap();
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    let _ = conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;"
    );

    conn.execute(
        "CREATE TABLE IF NOT EXISTS servers (
            ip TEXT PRIMARY KEY,
            info TEXT,
            is_cracked INTEGER,
            is_registered INTEGER DEFAULT 0,
            is_op INTEGER,
            needs_mods INTEGER,
            has_builds INTEGER DEFAULT 0,
            movement_restricted INTEGER DEFAULT 0,
            rcon_pass TEXT
        )",
        (),
    ).unwrap();

    let _ = conn.execute("ALTER TABLE servers ADD COLUMN has_builds INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE servers ADD COLUMN is_registered INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE servers ADD COLUMN movement_restricted INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE servers ADD COLUMN created_at INTEGER DEFAULT 0", ());
    let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_servers_created_at ON servers(created_at)", ());

    conn.execute(
        "CREATE TABLE IF NOT EXISTS players (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            nickname TEXT,
            server_ip TEXT,
            UNIQUE(nickname, server_ip)
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS tg_users (
            chat_id INTEGER PRIMARY KEY,
            username TEXT,
            tier TEXT,
            balance INTEGER,
            sub_expires_at INTEGER,
            active INTEGER,
            filter_green INTEGER,
            filter_yellow INTEGER,
            filter_blue INTEGER,
            pool_mode TEXT DEFAULT 'auto',
            src_default INTEGER DEFAULT 1,
            src_ngrok INTEGER DEFAULT 1,
            src_playit INTEGER DEFAULT 1,
            src_e4mc INTEGER DEFAULT 1,
            ai_enabled INTEGER DEFAULT 1,
            created_at INTEGER
        )",
        (),
    ).unwrap();

    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN pool_mode TEXT DEFAULT 'auto'", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN src_default INTEGER DEFAULT 1", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN src_ngrok INTEGER DEFAULT 1", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN src_playit INTEGER DEFAULT 1", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN src_e4mc INTEGER DEFAULT 1", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN ai_enabled INTEGER DEFAULT 1", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_ver TEXT DEFAULT 'all'", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_online INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_builds INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_noreg INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_op INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN hunt_geo TEXT DEFAULT 'all'", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN referrer_id INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN ref_count INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN ref_earned INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN last_daily_case INTEGER DEFAULT 0", ());
    let _ = conn.execute("ALTER TABLE tg_users ADD COLUMN is_banned INTEGER DEFAULT 0", ());

    conn.execute(
        "CREATE TABLE IF NOT EXISTS exclusive_deliveries (
            server_ip TEXT PRIMARY KEY,
            claimed_by INTEGER,
            delivered_at INTEGER
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS daily_user_deliveries (
            chat_id INTEGER,
            day_key TEXT,
            count INTEGER,
            PRIMARY KEY (chat_id, day_key)
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS promocodes (
            code TEXT PRIMARY KEY,
            tier TEXT,
            days INTEGER,
            uses_left INTEGER
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS processed_payments (
            tx_id INTEGER PRIMARY KEY,
            chat_id INTEGER,
            amount REAL,
            comment TEXT,
            system TEXT DEFAULT 'donatepay',
            processed_at INTEGER
        )",
        (),
    ).unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS user_sent_servers (
            chat_id INTEGER,
            server_ip TEXT,
            sent_at INTEGER,
            PRIMARY KEY(chat_id, server_ip)
        )",
        (),
    ).unwrap();
    let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_user_sent_servers_ts ON user_sent_servers(chat_id, sent_at)", ());

    let _ = conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_players_unique ON players(nickname, server_ip)", ());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let _ = conn.execute("UPDATE servers SET created_at = ?1 WHERE created_at IS NULL OR created_at = 0", [now]);
    let _ = std::fs::create_dir_all("cheats");

    DB.set(Mutex::new(conn)).unwrap();
}

pub fn save_server(
    ip: &str, info: &str, is_cracked: bool, is_registered: bool, is_op: bool, 
    needs_mods: bool, has_builds: bool, movement_restricted: bool, rcon_pass: Option<String>
) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "INSERT INTO servers (ip, info, is_cracked, is_registered, is_op, needs_mods, has_builds, movement_restricted, rcon_pass, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(ip) DO UPDATE SET
                     info = excluded.info,
                     is_cracked = excluded.is_cracked,
                     is_registered = excluded.is_registered,
                     is_op = excluded.is_op,
                     needs_mods = excluded.needs_mods,
                     has_builds = excluded.has_builds,
                     movement_restricted = excluded.movement_restricted,
                     rcon_pass = COALESCE(excluded.rcon_pass, servers.rcon_pass),
                     created_at = CASE WHEN servers.created_at > 0 THEN servers.created_at ELSE excluded.created_at END",
                (ip, info, is_cracked as i32, is_registered as i32, is_op as i32, needs_mods as i32, has_builds as i32, movement_restricted as i32, rcon_pass, now),
            );
        }
    }
}

pub fn record_sent_server(chat_id: i64, server_ip: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO user_sent_servers (chat_id, server_ip, sent_at) VALUES (?1, ?2, ?3)",
                (chat_id, server_ip, now),
            );
        }
    }
}

pub fn export_user_servers_txt(chat_id: i64, period: &str, user: &TgDbUser) -> (String, usize) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let min_ts: i64 = match period {
        "1h" | "hour" => now.saturating_sub(3600),
        "24h" | "day" => now.saturating_sub(86400),
        "30d" | "month" => now.saturating_sub(86400 * 30),
        _ => 0, // "all"
    };

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            // 1. First priority: servers actually sent to this specific user
            let sql_sent = "SELECT s.ip, COALESCE(GROUP_CONCAT(p.nickname, '/'), '')
                            FROM user_sent_servers us
                            JOIN servers s ON us.server_ip = s.ip
                            LEFT JOIN players p ON s.ip = p.server_ip
                            WHERE us.chat_id = ?1 AND (?2 = 0 OR us.sent_at >= ?2)
                            GROUP BY s.ip
                            ORDER BY us.sent_at DESC";

            if let Ok(mut stmt) = conn.prepare(sql_sent) {
                if let Ok(mapped) = stmt.query_map((chat_id, min_ts), |row| {
                    let ip: String = row.get(0)?;
                    let players: String = row.get(1)?;
                    Ok((ip, players))
                }) {
                    let list: Vec<(String, String)> = mapped.flatten().collect();
                    if !list.is_empty() {
                        let mut output = String::new();
                        let mut count = 0;
                        for (ip, players_str) in list {
                            count += 1;
                            let trimmed_players = players_str.trim();
                            if trimmed_players.is_empty() {
                                output.push_str(&ip);
                            } else {
                                output.push_str(&format!("{}/{}", ip, trimmed_players));
                            }
                            output.push('\n');
                        }
                        return (output, count);
                    }
                }
            }

            // 2. Fallback: filter all database servers strictly by the user's active filters
            let sql_all = "SELECT s.ip, s.info, s.is_cracked, s.is_registered, s.is_op, s.has_builds, COALESCE(GROUP_CONCAT(p.nickname, '/'), '')
                           FROM servers s
                           LEFT JOIN players p ON s.ip = p.server_ip
                           WHERE ?1 = 0 OR s.created_at >= ?1
                           GROUP BY s.ip
                           ORDER BY s.created_at DESC";

            if let Ok(mut stmt) = conn.prepare(sql_all) {
                if let Ok(mapped) = stmt.query_map([min_ts], |row| {
                    let ip: String = row.get(0)?;
                    let info: String = row.get(1)?;
                    let is_cracked: bool = row.get::<_, i32>(2)? == 1;
                    let is_registered: bool = row.get::<_, i32>(3)? == 1;
                    let is_op: bool = row.get::<_, i32>(4)? == 1;
                    let has_builds: bool = row.get::<_, i32>(5)? == 1;
                    let players: String = row.get(6)?;
                    Ok((ip, info, is_cracked, is_registered, is_op, has_builds, players))
                }) {
                    let mut output = String::new();
                    let mut count = 0;
                    for (ip, info, is_cracked, is_registered, is_op, has_builds, players_str) in mapped.flatten() {
                        let ip_lower = ip.to_lowercase();
                        let info_lower = info.to_lowercase();
                        let is_playit = ip_lower.contains("playit") || info_lower.contains("playit");
                        let is_ngrok = ip_lower.contains("ngrok") || info_lower.contains("ngrok") || ip.contains(".tcp.");
                        let is_default = !is_playit && !is_ngrok;

                        if is_playit && !user.src_playit { continue; }
                        if is_ngrok && !user.src_ngrok { continue; }
                        if is_default && !user.src_default { continue; }

                        // Color filters:
                        if !is_cracked && !user.filter_blue { continue; }
                        if is_cracked && is_registered && !user.filter_yellow { continue; }
                        if is_cracked && !is_registered && !user.filter_green { continue; }

                        // Hunt filters:
                        if user.hunt_builds && !has_builds { continue; }
                        if user.hunt_noreg && is_registered { continue; }
                        if user.hunt_op && !is_op { continue; }

                        if user.hunt_ver != "all" && !user.hunt_ver.is_empty() {
                            if !info_lower.contains(&user.hunt_ver.to_lowercase()) {
                                continue;
                            }
                        }

                        count += 1;
                        let trimmed_players = players_str.trim();
                        if trimmed_players.is_empty() {
                            output.push_str(&ip);
                        } else {
                            output.push_str(&format!("{}/{}", ip, trimmed_players));
                        }
                        output.push('\n');
                    }
                    return (output, count);
                }
            }
        }
    }
    (String::new(), 0)
}

#[allow(dead_code)]
pub fn export_servers_txt(period: &str) -> (String, usize) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let min_ts: i64 = match period {
        "1h" | "hour" => now.saturating_sub(3600),
        "24h" | "day" => now.saturating_sub(86400),
        "30d" | "month" => now.saturating_sub(86400 * 30),
        _ => 0, // "all"
    };

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let sql = "SELECT s.ip, COALESCE(GROUP_CONCAT(p.nickname, '/'), '')
                       FROM servers s
                       LEFT JOIN players p ON s.ip = p.server_ip
                       WHERE ?1 = 0 OR s.created_at >= ?1
                       GROUP BY s.ip
                       ORDER BY s.created_at DESC";

            if let Ok(mut stmt) = conn.prepare(sql) {
                let rows_res = stmt.query_map([min_ts], |row| {
                    let ip: String = row.get(0)?;
                    let players: String = row.get(1)?;
                    Ok((ip, players))
                });

                if let Ok(mapped) = rows_res {
                    let mut output = String::new();
                    let mut count = 0;
                    for item in mapped.flatten() {
                        count += 1;
                        let (ip, players_str) = item;
                        let trimmed_players = players_str.trim();
                        if trimmed_players.is_empty() {
                            output.push_str(&ip);
                        } else {
                            output.push_str(&format!("{}/{}", ip, trimmed_players));
                        }
                        output.push('\n');
                    }
                    return (output, count);
                }
            }
        }
    }
    (String::new(), 0)
}

pub fn save_player(nickname: &str, server_ip: &str) {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO players (nickname, server_ip) VALUES (?1, ?2)",
                (nickname, server_ip),
            );
        }
    }
}

pub fn search_servers(query: &str) -> Vec<DbServer> {
    let mut results = Vec::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let trimmed = query.trim();
            if trimmed.is_empty() {
                let sql = "SELECT ip, info, is_cracked, is_registered, is_op, needs_mods, has_builds, movement_restricted, rcon_pass FROM servers ORDER BY rowid DESC LIMIT 200";
                if let Ok(mut stmt) = conn.prepare(sql) {
                    let rows = stmt.query_map([], |row| {
                        Ok(DbServer {
                            ip: row.get(0)?,
                            info: row.get(1)?,
                            is_cracked: row.get::<_, i32>(2)? != 0,
                            is_registered: row.get::<_, i32>(3).unwrap_or(0) != 0,
                            is_op: row.get::<_, i32>(4)? != 0,
                            needs_mods: row.get::<_, i32>(5)? != 0,
                            has_builds: row.get::<_, i32>(6).unwrap_or(0) != 0,
                            movement_restricted: row.get::<_, i32>(7).unwrap_or(0) != 0,
                            rcon_pass: row.get(8)?,
                        })
                    });

                    if let Ok(rows) = rows {
                        for r in rows.flatten() {
                            results.push(r);
                        }
                    }
                }
            } else {
                let sql = "SELECT ip, info, is_cracked, is_registered, is_op, needs_mods, has_builds, movement_restricted, rcon_pass FROM servers WHERE ip LIKE ?1 ESCAPE '\\' OR info LIKE ?1 ESCAPE '\\' ORDER BY rowid DESC LIMIT 200";
                if let Ok(mut stmt) = conn.prepare(sql) {
                    let escaped = trimmed.replace('%', "\\%").replace('_', "\\_");
                    let pattern = format!("%{}%", escaped);
                    let rows = stmt.query_map([&pattern], |row| {
                        Ok(DbServer {
                            ip: row.get(0)?,
                            info: row.get(1)?,
                            is_cracked: row.get::<_, i32>(2)? != 0,
                            is_registered: row.get::<_, i32>(3).unwrap_or(0) != 0,
                            is_op: row.get::<_, i32>(4)? != 0,
                            needs_mods: row.get::<_, i32>(5)? != 0,
                            has_builds: row.get::<_, i32>(6).unwrap_or(0) != 0,
                            movement_restricted: row.get::<_, i32>(7).unwrap_or(0) != 0,
                            rcon_pass: row.get(8)?,
                        })
                    });

                    if let Ok(rows) = rows {
                        for r in rows.flatten() {
                            results.push(r);
                        }
                    }
                }
            }
        }
    }
    results
}

#[allow(dead_code)]
pub fn get_all_server_ips() -> std::collections::HashSet<String> {
    let mut ips = std::collections::HashSet::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT ip FROM servers") {
                let rows = stmt.query_map([], |row| row.get::<_, String>(0));
                if let Ok(rows) = rows {
                    for r in rows.flatten() {
                        ips.insert(r);
                    }
                }
            }
        }
    }
    ips
}

pub fn get_or_create_tg_user(chat_id: i64, username: &str) -> TgDbUser {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let user_opt = if let Ok(mut stmt) = conn.prepare("SELECT chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at, hunt_ver, hunt_online, hunt_builds, hunt_noreg, hunt_op, hunt_geo, referrer_id, ref_count, ref_earned, last_daily_case, is_banned FROM tg_users WHERE chat_id = ?1") {
                stmt.query_row([chat_id], |row| {
                    Ok(TgDbUser {
                        chat_id: row.get(0)?,
                        username: row.get(1)?,
                        tier: row.get(2)?,
                        balance: row.get(3)?,
                        sub_expires_at: row.get(4)?,
                        active: row.get::<_, i32>(5)? != 0,
                        filter_green: row.get::<_, i32>(6)? != 0,
                        filter_yellow: row.get::<_, i32>(7)? != 0,
                        filter_blue: row.get::<_, i32>(8)? != 0,
                        pool_mode: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "auto".to_string()),
                        src_default: row.get::<_, Option<i32>>(10)?.unwrap_or(1) != 0,
                        src_ngrok: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                        src_playit: row.get::<_, Option<i32>>(12)?.unwrap_or(1) != 0,
                        src_e4mc: row.get::<_, Option<i32>>(13)?.unwrap_or(1) != 0,
                        ai_enabled: row.get::<_, Option<i32>>(14)?.unwrap_or(1) != 0,
                        created_at: row.get(15)?,
                        hunt_ver: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "all".to_string()),
                        hunt_online: row.get::<_, Option<i32>>(17)?.unwrap_or(0),
                        hunt_builds: row.get::<_, Option<i32>>(18)?.unwrap_or(0) != 0,
                        hunt_noreg: row.get::<_, Option<i32>>(19)?.unwrap_or(0) != 0,
                        hunt_op: row.get::<_, Option<i32>>(20)?.unwrap_or(0) != 0,
                        hunt_geo: row.get::<_, Option<String>>(21)?.unwrap_or_else(|| "all".to_string()),
                        referrer_id: row.get::<_, Option<i64>>(22)?.unwrap_or(0),
                        ref_count: row.get::<_, Option<i64>>(23)?.unwrap_or(0),
                        ref_earned: row.get::<_, Option<i64>>(24)?.unwrap_or(0),
                        last_daily_case: row.get::<_, Option<i64>>(25)?.unwrap_or(0),
                        is_banned: row.get::<_, Option<i32>>(26)?.unwrap_or(0) != 0,
                    })
                }).ok()
            } else {
                None
            };

            if let Some(mut user) = user_opt {
                if !username.is_empty() && user.username != username {
                    user.username = username.to_string();
                    let _ = conn.execute(
                        "UPDATE tg_users SET username = ?1 WHERE chat_id = ?2",
                        (&user.username, chat_id),
                    );
                }
                return user;
            }

            let new_user = TgDbUser {
                chat_id,
                username: username.to_string(),
                tier: "Free".to_string(),
                balance: 0,
                sub_expires_at: 0,
                active: false,
                filter_green: true,
                filter_yellow: true,
                filter_blue: true,
                pool_mode: "auto".to_string(),
                src_default: true,
                src_ngrok: true,
                src_playit: true,
                src_e4mc: true,
                ai_enabled: true,
                created_at: now,
                hunt_ver: "all".to_string(),
                hunt_online: 0,
                hunt_builds: false,
                hunt_noreg: false,
                hunt_op: false,
                hunt_geo: "all".to_string(),
                referrer_id: 0,
                ref_count: 0,
                ref_earned: 0,
                last_daily_case: 0,
                is_banned: false,
            };

            let _ = conn.execute(
                "INSERT INTO tg_users (chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at, hunt_ver, hunt_online, hunt_builds, hunt_noreg, hunt_op, hunt_geo, referrer_id, ref_count, ref_earned, last_daily_case, is_banned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
                rusqlite::params![
                    new_user.chat_id,
                    &new_user.username,
                    &new_user.tier,
                    new_user.balance,
                    new_user.sub_expires_at,
                    new_user.active as i32,
                    new_user.filter_green as i32,
                    new_user.filter_yellow as i32,
                    new_user.filter_blue as i32,
                    &new_user.pool_mode,
                    new_user.src_default as i32,
                    new_user.src_ngrok as i32,
                    new_user.src_playit as i32,
                    new_user.src_e4mc as i32,
                    new_user.ai_enabled as i32,
                    new_user.created_at,
                    &new_user.hunt_ver,
                    new_user.hunt_online,
                    new_user.hunt_builds as i32,
                    new_user.hunt_noreg as i32,
                    new_user.hunt_op as i32,
                    &new_user.hunt_geo,
                    new_user.referrer_id,
                    new_user.ref_count,
                    new_user.ref_earned,
                    new_user.last_daily_case,
                    new_user.is_banned as i32,
                ],
            );

            return new_user;
        }
    }

    TgDbUser {
        chat_id,
        username: username.to_string(),
        tier: "Free".to_string(),
        balance: 0,
        sub_expires_at: 0,
        active: false,
        filter_green: true,
        filter_yellow: true,
        filter_blue: true,
        pool_mode: "auto".to_string(),
        src_default: true,
        src_ngrok: true,
        src_playit: true,
        src_e4mc: true,
        ai_enabled: true,
        created_at: now,
        hunt_ver: "all".to_string(),
        hunt_online: 0,
        hunt_builds: false,
        hunt_noreg: false,
        hunt_op: false,
        hunt_geo: "all".to_string(),
        referrer_id: 0,
        ref_count: 0,
        ref_earned: 0,
        last_daily_case: 0,
        is_banned: false,
    }
}

pub fn save_tg_user(user: &TgDbUser) {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "UPDATE tg_users SET username = ?1, tier = ?2, balance = ?3, sub_expires_at = ?4, active = ?5, filter_green = ?6, filter_yellow = ?7, filter_blue = ?8, pool_mode = ?9, src_default = ?10, src_ngrok = ?11, src_playit = ?12, src_e4mc = ?13, ai_enabled = ?14, hunt_ver = ?15, hunt_online = ?16, hunt_builds = ?17, hunt_noreg = ?18, hunt_op = ?19, hunt_geo = ?20, referrer_id = ?21, ref_count = ?22, ref_earned = ?23, last_daily_case = ?24, is_banned = ?25 WHERE chat_id = ?26",
                rusqlite::params![
                    &user.username,
                    &user.tier,
                    user.balance,
                    user.sub_expires_at,
                    user.active as i32,
                    user.filter_green as i32,
                    user.filter_yellow as i32,
                    user.filter_blue as i32,
                    &user.pool_mode,
                    user.src_default as i32,
                    user.src_ngrok as i32,
                    user.src_playit as i32,
                    user.src_e4mc as i32,
                    user.ai_enabled as i32,
                    &user.hunt_ver,
                    user.hunt_online,
                    user.hunt_builds as i32,
                    user.hunt_noreg as i32,
                    user.hunt_op as i32,
                    &user.hunt_geo,
                    user.referrer_id,
                    user.ref_count,
                    user.ref_earned,
                    user.last_daily_case,
                    user.is_banned as i32,
                    user.chat_id,
                ],
            );
        }
    }
}

pub fn get_all_tg_users() -> Vec<TgDbUser> {
    let mut users = Vec::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at, hunt_ver, hunt_online, hunt_builds, hunt_noreg, hunt_op, hunt_geo, referrer_id, ref_count, ref_earned, last_daily_case, is_banned FROM tg_users") {
                let rows = stmt.query_map([], |row| {
                    Ok(TgDbUser {
                        chat_id: row.get(0)?,
                        username: row.get(1)?,
                        tier: row.get(2)?,
                        balance: row.get(3)?,
                        sub_expires_at: row.get(4)?,
                        active: row.get::<_, i32>(5)? != 0,
                        filter_green: row.get::<_, i32>(6)? != 0,
                        filter_yellow: row.get::<_, i32>(7)? != 0,
                        filter_blue: row.get::<_, i32>(8)? != 0,
                        pool_mode: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "auto".to_string()),
                        src_default: row.get::<_, Option<i32>>(10)?.unwrap_or(1) != 0,
                        src_ngrok: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                        src_playit: row.get::<_, Option<i32>>(12)?.unwrap_or(1) != 0,
                        src_e4mc: row.get::<_, Option<i32>>(13)?.unwrap_or(1) != 0,
                        ai_enabled: row.get::<_, Option<i32>>(14)?.unwrap_or(1) != 0,
                        created_at: row.get(15)?,
                        hunt_ver: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "all".to_string()),
                        hunt_online: row.get::<_, Option<i32>>(17)?.unwrap_or(0),
                        hunt_builds: row.get::<_, Option<i32>>(18)?.unwrap_or(0) != 0,
                        hunt_noreg: row.get::<_, Option<i32>>(19)?.unwrap_or(0) != 0,
                        hunt_op: row.get::<_, Option<i32>>(20)?.unwrap_or(0) != 0,
                        hunt_geo: row.get::<_, Option<String>>(21)?.unwrap_or_else(|| "all".to_string()),
                        referrer_id: row.get::<_, Option<i64>>(22)?.unwrap_or(0),
                        ref_count: row.get::<_, Option<i64>>(23)?.unwrap_or(0),
                        ref_earned: row.get::<_, Option<i64>>(24)?.unwrap_or(0),
                        last_daily_case: row.get::<_, Option<i64>>(25)?.unwrap_or(0),
                        is_banned: row.get::<_, Option<i32>>(26)?.unwrap_or(0) != 0,
                    })
                });
                if let Ok(rows) = rows {
                    for r in rows.flatten() {
                        users.push(r);
                    }
                }
            }
        }
    }
    users
}

pub fn ban_user(chat_id: i64) -> bool {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let res = conn.execute("UPDATE tg_users SET is_banned = 1 WHERE chat_id = ?1", [chat_id]);
            return res.is_ok();
        }
    }
    false
}

pub fn unban_user(chat_id: i64) -> bool {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let res = conn.execute("UPDATE tg_users SET is_banned = 0 WHERE chat_id = ?1", [chat_id]);
            return res.is_ok();
        }
    }
    false
}

pub fn is_user_banned(chat_id: i64) -> bool {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row(
                "SELECT is_banned FROM tg_users WHERE chat_id = ?1",
                [chat_id],
                |row| row.get::<_, Option<i32>>(0),
            ).unwrap_or(None).unwrap_or(0) != 0;
        }
    }
    false
}

#[allow(dead_code)]
pub fn get_user_by_chat_id(chat_id: i64) -> Option<TgDbUser> {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at, hunt_ver, hunt_online, hunt_builds, hunt_noreg, hunt_op, hunt_geo, referrer_id, ref_count, ref_earned, last_daily_case, is_banned FROM tg_users WHERE chat_id = ?1") {
                return stmt.query_row([chat_id], |row| {
                    Ok(TgDbUser {
                        chat_id: row.get(0)?,
                        username: row.get(1)?,
                        tier: row.get(2)?,
                        balance: row.get(3)?,
                        sub_expires_at: row.get(4)?,
                        active: row.get::<_, i32>(5)? != 0,
                        filter_green: row.get::<_, i32>(6)? != 0,
                        filter_yellow: row.get::<_, i32>(7)? != 0,
                        filter_blue: row.get::<_, i32>(8)? != 0,
                        pool_mode: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "auto".to_string()),
                        src_default: row.get::<_, Option<i32>>(10)?.unwrap_or(1) != 0,
                        src_ngrok: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                        src_playit: row.get::<_, Option<i32>>(12)?.unwrap_or(1) != 0,
                        src_e4mc: row.get::<_, Option<i32>>(13)?.unwrap_or(1) != 0,
                        ai_enabled: row.get::<_, Option<i32>>(14)?.unwrap_or(1) != 0,
                        created_at: row.get(15)?,
                        hunt_ver: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "all".to_string()),
                        hunt_online: row.get::<_, Option<i32>>(17)?.unwrap_or(0),
                        hunt_builds: row.get::<_, Option<i32>>(18)?.unwrap_or(0) != 0,
                        hunt_noreg: row.get::<_, Option<i32>>(19)?.unwrap_or(0) != 0,
                        hunt_op: row.get::<_, Option<i32>>(20)?.unwrap_or(0) != 0,
                        hunt_geo: row.get::<_, Option<String>>(21)?.unwrap_or_else(|| "all".to_string()),
                        referrer_id: row.get::<_, Option<i64>>(22)?.unwrap_or(0),
                        ref_count: row.get::<_, Option<i64>>(23)?.unwrap_or(0),
                        ref_earned: row.get::<_, Option<i64>>(24)?.unwrap_or(0),
                        last_daily_case: row.get::<_, Option<i64>>(25)?.unwrap_or(0),
                        is_banned: row.get::<_, Option<i32>>(26)?.unwrap_or(0) != 0,
                    })
                }).ok();
            }
        }
    }
    None
}

pub fn get_total_user_count() -> i64 {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row("SELECT COUNT(*) FROM tg_users", [], |row| row.get(0)).unwrap_or(0);
        }
    }
    0
}

pub fn get_active_feed_count() -> i64 {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row("SELECT COUNT(*) FROM tg_users WHERE active = 1", [], |row| row.get(0)).unwrap_or(0);
        }
    }
    0
}

pub fn get_active_sub_count() -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row(
                "SELECT COUNT(*) FROM tg_users WHERE tier != 'Free' AND sub_expires_at > ?1",
                [now],
                |row| row.get(0)
            ).unwrap_or(0);
        }
    }
    0
}

pub fn get_total_server_count() -> i64 {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row("SELECT COUNT(*) FROM servers", [], |row| row.get(0)).unwrap_or(0);
        }
    }
    0
}

pub fn check_and_increment_daily_limit(chat_id: i64, tier: &str) -> bool {
    let clean_tier = if tier == "Standard" { "Plus" } else { tier };
    let is_prime = clean_tier == "Prime";
    let max_limit: i64 = match clean_tier {
        "Prime" => i64::MAX,
        "Plus" => 666,
        _ => 100,
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let day_key = format!("{}", now / 86400);

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let count: i64 = conn.query_row(
                "SELECT count FROM daily_user_deliveries WHERE chat_id = ?1 AND day_key = ?2",
                (chat_id, &day_key),
                |row| row.get(0),
            ).unwrap_or(0);

            if !is_prime && count >= max_limit {
                return false;
            }

            let _ = conn.execute(
                "INSERT INTO daily_user_deliveries (chat_id, day_key, count) VALUES (?1, ?2, 1)
                 ON CONFLICT(chat_id, day_key) DO UPDATE SET count = count + 1",
                (chat_id, &day_key),
            );
            return true;
        }
    }
    true
}

pub fn get_user_daily_count(chat_id: i64) -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let day_key = format!("{}", now / 86400);
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn.query_row(
                "SELECT count FROM daily_user_deliveries WHERE chat_id = ?1 AND day_key = ?2",
                (chat_id, &day_key),
                |row| row.get(0),
            ).unwrap_or(0);
        }
    }
    0
}

pub fn grant_subscription(chat_id: i64, tier: &str, days: i64) -> Result<i64, String> {
    let clean_tier = match tier.to_lowercase().as_str() {
        "prime" | "exclusive" => "Prime",
        "plus" | "standard" => "Plus",
        _ => "Free",
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let current_sub: i64 = conn
                .query_row(
                    "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                    [chat_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let base_time = if current_sub > now { current_sub } else { now };
            let new_expires = base_time + (days * 86400);

            let res = conn.execute(
                "UPDATE tg_users SET tier = ?1, sub_expires_at = ?2 WHERE chat_id = ?3",
                (clean_tier, new_expires, chat_id),
            );

            if res.is_ok() {
                return Ok(new_expires);
            }
        }
    }
    Err("Ошибка обновления подписки в БД".to_string())
}

pub fn add_user_balance(chat_id: i64, amount: i64) -> Result<i64, String> {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let current_bal: i64 = conn
                .query_row(
                    "SELECT balance FROM tg_users WHERE chat_id = ?1",
                    [chat_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            
            if current_bal.checked_add(amount).is_none() {
                return Err("Переполнение баланса".to_string());
            }

            let res = conn.execute(
                "UPDATE tg_users SET balance = balance + ?1 WHERE chat_id = ?2",
                (amount, chat_id),
            );
            if res.is_ok() {
                let new_bal: i64 = conn
                    .query_row(
                        "SELECT balance FROM tg_users WHERE chat_id = ?1",
                        [chat_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                return Ok(new_bal);
            }
        }
    }
    Err("Ошибка обновления баланса".to_string())
}

pub fn claim_exclusive_server(server_ip: &str, chat_id: i64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let res = conn.execute(
                "INSERT INTO exclusive_deliveries (server_ip, claimed_by, delivered_at) VALUES (?1, ?2, ?3)",
                (server_ip, chat_id, now),
            );
            return res.is_ok();
        }
    }
    false
}

pub fn create_promocode(code: &str, tier: &str, days: i64, uses: i32) -> Result<(), String> {
    let clean_tier = match tier.to_lowercase().as_str() {
        "prime" | "exclusive" => "Prime",
        "plus" | "standard" => "Plus",
        _ => "Free",
    };

    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let res = conn.execute(
                "INSERT OR REPLACE INTO promocodes (code, tier, days, uses_left) VALUES (?1, ?2, ?3, ?4)",
                (code.trim().to_uppercase(), clean_tier, days, uses),
            );
            if res.is_ok() {
                return Ok(());
            }
        }
    }
    Err("Не удалось сохранить промокод в БД".to_string())
}

pub fn purchase_subscription_with_balance(chat_id: i64, tier: &str, days: i64, cost: i64) -> Result<i64, String> {
    if cost <= 0 {
        return Err("Недопустимая стоимость".to_string());
    }
    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let updated = tx.execute(
                "UPDATE tg_users SET balance = balance - ?1 WHERE chat_id = ?2 AND balance >= ?1",
                (cost, chat_id),
            ).map_err(|e| e.to_string())?;
            if updated == 0 {
                return Err("Недостаточно средств на балансе".to_string());
            }

            let clean_tier = match tier.to_lowercase().as_str() {
                "prime" | "exclusive" => "Prime",
                "plus" | "standard" => "Plus",
                _ => "Free",
            };
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let current_sub: i64 = tx
                .query_row(
                    "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                    [chat_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let base_time = if current_sub > now { current_sub } else { now };
            let new_expires = base_time + (days * 86400);

            tx.execute(
                "UPDATE tg_users SET tier = ?1, sub_expires_at = ?2 WHERE chat_id = ?3",
                (clean_tier, new_expires, chat_id),
            ).map_err(|e| e.to_string())?;

            tx.commit().map_err(|e| e.to_string())?;
            return Ok(new_expires);
        }
    }
    Err("Ошибка базы данных".to_string())
}

#[allow(dead_code)]
pub fn buy_market_server_with_balance(chat_id: i64, cost: i64) -> Result<DbServer, String> {
    if cost <= 0 {
        return Err("Недопустимая стоимость".to_string());
    }
    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            let updated = tx.execute(
                "UPDATE tg_users SET balance = balance - ?1 WHERE chat_id = ?2 AND balance >= ?1",
                (cost, chat_id),
            ).map_err(|e| e.to_string())?;
            if updated == 0 {
                return Err("Недостаточно средств на балансе".to_string());
            }

            let sql = "SELECT s.ip, s.info, s.is_cracked, s.is_registered, s.is_op, s.needs_mods, s.has_builds, s.movement_restricted, s.rcon_pass 
                       FROM servers s
                       LEFT JOIN exclusive_deliveries d ON s.ip = d.server_ip
                       WHERE d.server_ip IS NULL 
                         AND (s.is_op = 1 OR s.rcon_pass IS NOT NULL OR s.info LIKE '%BUNGEE%' OR s.info LIKE '%VELOCITY%')
                       ORDER BY s.rowid DESC LIMIT 1";

            let srv_opt: Option<DbServer> = tx.query_row(sql, [], |row| {
                Ok(DbServer {
                    ip: row.get(0)?,
                    info: row.get(1)?,
                    is_cracked: row.get::<_, i32>(2)? != 0,
                    is_registered: row.get::<_, i32>(3).unwrap_or(0) != 0,
                    is_op: row.get::<_, i32>(4)? != 0,
                    needs_mods: row.get::<_, i32>(5)? != 0,
                    has_builds: row.get::<_, i32>(6).unwrap_or(0) != 0,
                    movement_restricted: row.get::<_, i32>(7).unwrap_or(0) != 0,
                    rcon_pass: row.get(8)?,
                })
            }).ok();

            let srv = match srv_opt {
                Some(s) => s,
                None => return Err("Нет доступных серверов в маркете".to_string()),
            };

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            tx.execute(
                "INSERT INTO exclusive_deliveries (server_ip, claimed_by, delivered_at) VALUES (?1, ?2, ?3)",
                (&srv.ip, chat_id, now),
            ).map_err(|e| e.to_string())?;

            tx.commit().map_err(|e| e.to_string())?;
            return Ok(srv);
        }
    }
    Err("Ошибка базы данных".to_string())
}

pub fn redeem_promocode(chat_id: i64, code: &str) -> Result<(String, i64), String> {
    let clean_code = code.trim().to_uppercase();
    if clean_code.is_empty() {
        return Err("Укажите промокод".to_string());
    }

    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            let promo_opt: Option<(String, i64, i32)> = tx
                .query_row(
                    "SELECT tier, days, uses_left FROM promocodes WHERE code = ?1",
                    [&clean_code],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .ok();

            if let Some((raw_tier, days, _uses_left)) = promo_opt {
                let updated = tx.execute(
                    "UPDATE promocodes SET uses_left = uses_left - 1 WHERE code = ?1 AND uses_left > 0",
                    [&clean_code],
                ).map_err(|e| e.to_string())?;

                if updated == 0 {
                    return Err("Этот промокод уже исчерпал лимит использований".to_string());
                }

                let tier = if raw_tier == "Standard" { "Plus".to_string() } else { raw_tier };

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                let current_sub: i64 = tx
                    .query_row(
                        "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                        [chat_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                let base_time = if current_sub > now { current_sub } else { now };
                let new_expires = base_time + (days * 86400);

                tx.execute(
                    "UPDATE tg_users SET tier = ?1, sub_expires_at = ?2 WHERE chat_id = ?3",
                    (&tier, new_expires, chat_id),
                ).map_err(|e| e.to_string())?;

                tx.commit().map_err(|e| e.to_string())?;
                return Ok((tier, days));
            } else {
                return Err("Промокод не найден или недействителен".to_string());
            }
        }
    }
    Err("Ошибка базы данных".to_string())
}

#[allow(dead_code)]
pub fn get_exclusive_market_servers(limit: usize) -> Vec<DbServer> {
    let mut list = Vec::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let sql = "SELECT s.ip, s.info, s.is_cracked, s.is_registered, s.is_op, s.needs_mods, s.has_builds, s.movement_restricted, s.rcon_pass 
                       FROM servers s
                       LEFT JOIN exclusive_deliveries d ON s.ip = d.server_ip
                       WHERE d.server_ip IS NULL 
                         AND (s.is_op = 1 OR s.rcon_pass IS NOT NULL OR s.info LIKE '%BUNGEE%' OR s.info LIKE '%VELOCITY%')
                       ORDER BY s.rowid DESC LIMIT ?1";
            if let Ok(mut stmt) = conn.prepare(sql) {
                let rows = stmt.query_map([limit as i64], |row| {
                    Ok(DbServer {
                        ip: row.get(0)?,
                        info: row.get(1)?,
                        is_cracked: row.get::<_, i32>(2)? != 0,
                        is_registered: row.get::<_, i32>(3).unwrap_or(0) != 0,
                        is_op: row.get::<_, i32>(4)? != 0,
                        needs_mods: row.get::<_, i32>(5)? != 0,
                        has_builds: row.get::<_, i32>(6).unwrap_or(0) != 0,
                        movement_restricted: row.get::<_, i32>(7).unwrap_or(0) != 0,
                        rcon_pass: row.get(8)?,
                    })
                });
                if let Ok(rows) = rows {
                    for r in rows.flatten() {
                        list.push(r);
                    }
                }
            }
        }
    }
    list
}

pub fn get_all_promocodes() -> Vec<(String, String, i64, i32)> {
    let mut list = Vec::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT code, tier, days, uses_left FROM promocodes ORDER BY rowid DESC") {
                let rows = stmt.query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                });
                if let Ok(rows) = rows {
                    for r in rows.flatten() {
                        list.push(r);
                    }
                }
            }
        }
    }
    list
}

pub fn delete_promocode(code: &str) -> bool {
    let clean = code.trim().to_uppercase();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let res = conn.execute("DELETE FROM promocodes WHERE code = ?1", [&clean]);
            return res.map(|cnt| cnt > 0).unwrap_or(false);
        }
    }
    false
}

#[allow(dead_code)]
pub fn get_tg_user_by_id(chat_id: i64) -> Option<TgDbUser> {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let mut stmt = conn
                .prepare("SELECT chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at, hunt_ver, hunt_online, hunt_builds, hunt_noreg, hunt_op, hunt_geo, referrer_id, ref_count, ref_earned, last_daily_case, is_banned FROM tg_users WHERE chat_id = ?1")
                .ok()?;
            return stmt
                .query_row([chat_id], |row| {
                    Ok(TgDbUser {
                        chat_id: row.get(0)?,
                        username: row.get(1)?,
                        tier: row.get(2)?,
                        balance: row.get(3)?,
                        sub_expires_at: row.get(4)?,
                        active: row.get::<_, i32>(5)? != 0,
                        filter_green: row.get::<_, i32>(6)? != 0,
                        filter_yellow: row.get::<_, i32>(7)? != 0,
                        filter_blue: row.get::<_, i32>(8)? != 0,
                        pool_mode: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "auto".to_string()),
                        src_default: row.get::<_, Option<i32>>(10)?.unwrap_or(1) != 0,
                        src_ngrok: row.get::<_, Option<i32>>(11)?.unwrap_or(1) != 0,
                        src_playit: row.get::<_, Option<i32>>(12)?.unwrap_or(1) != 0,
                        src_e4mc: row.get::<_, Option<i32>>(13)?.unwrap_or(1) != 0,
                        ai_enabled: row.get::<_, Option<i32>>(14)?.unwrap_or(1) != 0,
                        created_at: row.get(15)?,
                        hunt_ver: row.get::<_, Option<String>>(16)?.unwrap_or_else(|| "all".to_string()),
                        hunt_online: row.get::<_, Option<i32>>(17)?.unwrap_or(0),
                        hunt_builds: row.get::<_, Option<i32>>(18)?.unwrap_or(0) != 0,
                        hunt_noreg: row.get::<_, Option<i32>>(19)?.unwrap_or(0) != 0,
                        hunt_op: row.get::<_, Option<i32>>(20)?.unwrap_or(0) != 0,
                        hunt_geo: row.get::<_, Option<String>>(21)?.unwrap_or_else(|| "all".to_string()),
                        referrer_id: row.get::<_, Option<i64>>(22)?.unwrap_or(0),
                        ref_count: row.get::<_, Option<i64>>(23)?.unwrap_or(0),
                        ref_earned: row.get::<_, Option<i64>>(24)?.unwrap_or(0),
                        last_daily_case: row.get::<_, Option<i64>>(25)?.unwrap_or(0),
                        is_banned: row.get::<_, Option<i32>>(26)?.unwrap_or(0) != 0,
                    })
                })
                .ok();
        }
    }
    None
}

pub fn get_players_for_server(server_ip: &str) -> Vec<String> {
    let mut players = Vec::new();
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT DISTINCT nickname FROM players WHERE server_ip = ?1 ORDER BY id DESC LIMIT 50") {
                let rows = stmt.query_map([server_ip], |row| row.get::<_, String>(0));
                if let Ok(rows) = rows {
                    for r in rows.flatten() {
                        let clean = r.trim().to_string();
                        if !clean.is_empty() {
                            players.push(clean);
                        }
                    }
                }
            }
        }
    }
    players
}

fn get_random_plus_server_conn(conn: &rusqlite::Connection) -> Option<DbServer> {
    let sql = "SELECT ip, info, is_cracked, is_registered, is_op, needs_mods, has_builds, movement_restricted, rcon_pass 
               FROM servers 
               WHERE is_cracked = 1 AND needs_mods = 0 AND info NOT LIKE '%[Вайтлист]%' AND info NOT LIKE '%(bs,%'
               ORDER BY RANDOM() LIMIT 30";
    if let Ok(mut stmt) = conn.prepare(sql) {
        let rows = stmt.query_map([], |row| {
            Ok(DbServer {
                ip: row.get(0)?,
                info: row.get(1)?,
                is_cracked: row.get::<_, i32>(2)? != 0,
                is_registered: row.get::<_, i32>(3).unwrap_or(0) != 0,
                is_op: row.get::<_, i32>(4)? != 0,
                needs_mods: row.get::<_, i32>(5)? != 0,
                has_builds: row.get::<_, i32>(6).unwrap_or(0) != 0,
                movement_restricted: row.get::<_, i32>(7).unwrap_or(0) != 0,
                rcon_pass: row.get(8)?,
            })
        });

        if let Ok(rows) = rows {
            for srv in rows.flatten() {
                // Extract online and max_players from info (e.g. "0/20 online")
                if let Some(pos) = srv.info.find(" online") {
                    let prefix = &srv.info[..pos];
                    if let Some(slash_pos) = prefix.rfind('/') {
                        let after_slash = &prefix[slash_pos + 1..].trim();
                        if let Ok(max_p) = after_slash.parse::<i32>() {
                            if max_p > 49 {
                                continue;
                            }
                        }
                    }
                }
                return Some(srv);
            }
        }
    }
    None
}

pub fn get_random_plus_server() -> Option<DbServer> {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return get_random_plus_server_conn(&conn);
        }
    }
    None
}

pub fn buy_single_server_with_balance(chat_id: i64, cost: i64) -> Result<DbServer, String> {
    if cost <= 0 {
        return Err("Недопустимая стоимость".to_string());
    }
    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            let cur_bal: i64 = tx
                .query_row(
                    "SELECT balance FROM tg_users WHERE chat_id = ?1",
                    [chat_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if cur_bal < cost {
                return Err(format!("Недостаточно средств. Ваш баланс: {} руб., требуется: {} руб.", cur_bal, cost));
            }

            let srv = match get_random_plus_server_conn(&tx) {
                Some(s) => s,
                None => return Err("На данный момент нет доступных серверов в наличии.".to_string()),
            };

            let updated = tx.execute(
                "UPDATE tg_users SET balance = balance - ?1 WHERE chat_id = ?2",
                (cost, chat_id),
            ).map_err(|e| e.to_string())?;

            if updated == 0 {
                return Err("Не удалось списать средства".to_string());
            }

            tx.commit().map_err(|e| e.to_string())?;
            return Ok(srv);
        }
    }
    Err("Ошибка базы данных".to_string())
}

pub fn process_referral(new_chat_id: i64, referrer_id: i64) -> (bool, i64) {
    if referrer_id == 0 || referrer_id == new_chat_id {
        return (false, 0);
    }
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let cur_referrer: i64 = conn.query_row(
                "SELECT COALESCE(referrer_id, 0) FROM tg_users WHERE chat_id = ?1",
                [new_chat_id],
                |row| row.get(0)
            ).unwrap_or(0);

            if cur_referrer != 0 {
                return (false, 0);
            }

            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            let one_day_sec = 86400;
            let _ = conn.execute(
                "UPDATE tg_users SET referrer_id = ?1 WHERE chat_id = ?2",
                (referrer_id, new_chat_id),
            );

            let current_sub: i64 = conn.query_row(
                "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                [new_chat_id],
                |row| row.get(0)
            ).unwrap_or(0);
            let base_time = if current_sub > now { current_sub } else { now };
            let new_sub = base_time + one_day_sec;
            let _ = conn.execute(
                "UPDATE tg_users SET tier = 'Plus', sub_expires_at = ?1 WHERE chat_id = ?2",
                (new_sub, new_chat_id),
            );

            let _ = conn.execute(
                "UPDATE tg_users SET balance = balance + 10, ref_count = COALESCE(ref_count, 0) + 1, ref_earned = COALESCE(ref_earned, 0) + 10 WHERE chat_id = ?1",
                [referrer_id],
            );

            return (true, referrer_id);
        }
    }
    (false, 0)
}

#[allow(dead_code)]
pub fn can_claim_daily_case(chat_id: i64) -> (bool, i64) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let last_claim: i64 = conn.query_row(
                "SELECT COALESCE(last_daily_case, 0) FROM tg_users WHERE chat_id = ?1",
                [chat_id],
                |row| row.get(0)
            ).unwrap_or(0);

            let passed = now - last_claim;
            if passed >= 86400 {
                return (true, 0);
            } else {
                return (false, 86400 - passed);
            }
        }
    }
    (true, 0)
}

pub fn claim_daily_case(chat_id: i64) -> Result<DbServer, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            let last_claim: i64 = tx.query_row(
                "SELECT COALESCE(last_daily_case, 0) FROM tg_users WHERE chat_id = ?1",
                [chat_id],
                |row| row.get(0)
            ).unwrap_or(0);

            let passed = now - last_claim;
            if passed < 86400 {
                let remaining = 86400 - passed;
                let hours = remaining / 3600;
                let mins = (remaining % 3600) / 60;
                return Err(format!("Кейс уже открыт сегодня! Следующий будет доступен через {} ч. {} мин.", hours, mins));
            }

            let srv = match get_random_plus_server_conn(&tx) {
                Some(s) => s,
                None => return Err("В базе пока нет подходящих серверов для кейса. Попробуйте чуть позже!".to_string()),
            };

            tx.execute(
                "UPDATE tg_users SET last_daily_case = ?1 WHERE chat_id = ?2",
                (now, chat_id),
            ).map_err(|e| e.to_string())?;

            tx.commit().map_err(|e| e.to_string())?;
            return Ok(srv);
        }
    }
    Err("Ошибка подключения к БД".to_string())
}

pub fn reset_hunt_filters(chat_id: i64) {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "UPDATE tg_users SET hunt_ver = 'all', hunt_online = 0, hunt_builds = 0, hunt_noreg = 0, hunt_op = 0, hunt_geo = 'all' WHERE chat_id = ?1",
                [chat_id],
            );
        }
    }
}

pub fn get_config(key: &str) -> Option<String> {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1").ok()?;
            return stmt.query_row([key], |row| row.get(0)).ok();
        }
    }
    None
}

pub fn set_config(key: &str, value: &str) {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            let _ = conn.execute(
                "INSERT INTO config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
                [key, value],
            );
        }
    }
}

pub fn is_admin(chat_id: i64) -> bool {
    if let Ok(admin_env) = std::env::var("ADMIN_CHAT_ID") {
        for id_str in admin_env.split(',') {
            if let Ok(id) = id_str.trim().parse::<i64>() {
                if id == chat_id { return true; }
            }
        }
    }
    if let Some(admin_list) = get_config("admin_chat_ids") {
        for id_str in admin_list.split(',') {
            if let Ok(id) = id_str.trim().parse::<i64>() {
                if id == chat_id { return true; }
            }
        }
    }
    false
}

pub fn add_admin_id(chat_id: i64) {
    let current = get_config("admin_chat_ids").unwrap_or_default();
    let mut ids: Vec<String> = current
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let s_id = chat_id.to_string();
    if !ids.contains(&s_id) {
        ids.push(s_id);
        set_config("admin_chat_ids", &ids.join(","));
    }
}

pub fn get_admin_ids() -> Vec<i64> {
    let mut result = Vec::new();
    if let Ok(admin_env) = std::env::var("ADMIN_CHAT_ID") {
        for id_str in admin_env.split(',') {
            if let Ok(id) = id_str.trim().parse::<i64>() {
                if !result.contains(&id) { result.push(id); }
            }
        }
    }
    if let Some(admin_list) = get_config("admin_chat_ids") {
        for id_str in admin_list.split(',') {
            if let Ok(id) = id_str.trim().parse::<i64>() {
                if !result.contains(&id) { result.push(id); }
            }
        }
    }
    result
}

pub fn is_payment_processed(tx_id: i64) -> bool {
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn
                .query_row(
                    "SELECT 1 FROM processed_payments WHERE tx_id = ?1",
                    [tx_id],
                    |_| Ok(()),
                )
                .is_ok();
        }
    }
    false
}

pub fn record_processed_payment(tx_id: i64, chat_id: i64, amount: f64, comment: &str, system: &str) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if let Some(mutex) = DB.get() {
        if let Ok(conn) = mutex.lock() {
            return conn
                .execute(
                    "INSERT OR IGNORE INTO processed_payments (tx_id, chat_id, amount, comment, system, processed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![tx_id, chat_id, amount, comment, system, now],
                )
                .is_ok();
        }
    }
    false
}

#[derive(Debug, Clone)]
pub enum DonatePayApplyResult {
    SubscriptionGranted { tier: String, days: i64, new_expires_at: i64 },
    BalanceTopup { added_rubles: i64, new_balance: i64 },
    UnderpaymentTopup { required: i64, paid: i64, new_balance: i64 },
}

pub fn apply_donatepay_payment(
    tx_id: i64,
    chat_id: i64,
    amount: f64,
    comment: &str,
) -> Result<DonatePayApplyResult, String> {
    if amount <= 0.0 {
        return Err("Сумма платежа должна быть больше 0".to_string());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(mutex) = DB.get() {
        if let Ok(mut conn) = mutex.lock() {
            let tx = conn.transaction().map_err(|e| e.to_string())?;

            // 1. Check if tx_id was already processed (anti-replay check)
            let exists: bool = tx
                .query_row(
                    "SELECT 1 FROM processed_payments WHERE tx_id = ?1",
                    [tx_id],
                    |_| Ok(()),
                )
                .is_ok();

            if exists {
                return Err(format!("Транзакция #{} уже была обработана ранее", tx_id));
            }

            // 2. Ensure user exists in tg_users
            let user_exists: bool = tx
                .query_row(
                    "SELECT 1 FROM tg_users WHERE chat_id = ?1",
                    [chat_id],
                    |_| Ok(()),
                )
                .is_ok();

            if !user_exists {
                let _ = tx.execute(
                    "INSERT INTO tg_users (chat_id, username, tier, balance, sub_expires_at, active, filter_green, filter_yellow, filter_blue, pool_mode, src_default, src_ngrok, src_playit, src_e4mc, ai_enabled, created_at)
                     VALUES (?1, '', 'Free', 0, 0, 0, 1, 1, 1, 'auto', 1, 1, 1, 1, 1, ?2)",
                    (chat_id, now),
                );
            }

            // 3. Record the transaction in processed_payments immediately inside atomic transaction
            tx.execute(
                "INSERT INTO processed_payments (tx_id, chat_id, amount, comment, system, processed_at) VALUES (?1, ?2, ?3, ?4, 'donatepay', ?5)",
                rusqlite::params![tx_id, chat_id, amount, comment, now],
            ).map_err(|e| e.to_string())?;

            let c_lower = comment.to_lowercase();
            let paid_rubles = amount.floor() as i64;

            if c_lower.contains("prime") {
                // Strict check: Prime costs 400 rub.
                if paid_rubles >= 400 {
                    let current_sub: i64 = tx
                        .query_row(
                            "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                            [chat_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(0);
                    let base_time = if current_sub > now { current_sub } else { now };
                    let new_expires = base_time + (30 * 86400);

                    tx.execute(
                        "UPDATE tg_users SET tier = 'Prime', sub_expires_at = ?1 WHERE chat_id = ?2",
                        (new_expires, chat_id),
                    ).map_err(|e| e.to_string())?;

                    tx.commit().map_err(|e| e.to_string())?;
                    return Ok(DonatePayApplyResult::SubscriptionGranted {
                        tier: "Prime".to_string(),
                        days: 30,
                        new_expires_at: new_expires,
                    });
                } else {
                    // Underpayment: credit to balance instead of granting Prime for cheap
                    tx.execute(
                        "UPDATE tg_users SET balance = balance + ?1 WHERE chat_id = ?2",
                        (paid_rubles, chat_id),
                    ).map_err(|e| e.to_string())?;

                    let new_bal: i64 = tx
                        .query_row("SELECT balance FROM tg_users WHERE chat_id = ?1", [chat_id], |row| row.get(0))
                        .unwrap_or(paid_rubles);

                    tx.commit().map_err(|e| e.to_string())?;
                    return Ok(DonatePayApplyResult::UnderpaymentTopup {
                        required: 400,
                        paid: paid_rubles,
                        new_balance: new_bal,
                    });
                }
            } else if c_lower.contains("plus") || c_lower.contains("standard") {
                // Strict check: Plus costs 200 rub.
                if paid_rubles >= 200 {
                    let current_sub: i64 = tx
                        .query_row(
                            "SELECT sub_expires_at FROM tg_users WHERE chat_id = ?1",
                            [chat_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(0);
                    let base_time = if current_sub > now { current_sub } else { now };
                    let new_expires = base_time + (30 * 86400);

                    tx.execute(
                        "UPDATE tg_users SET tier = 'Plus', sub_expires_at = ?1 WHERE chat_id = ?2",
                        (new_expires, chat_id),
                    ).map_err(|e| e.to_string())?;

                    tx.commit().map_err(|e| e.to_string())?;
                    return Ok(DonatePayApplyResult::SubscriptionGranted {
                        tier: "Plus".to_string(),
                        days: 30,
                        new_expires_at: new_expires,
                    });
                } else {
                    // Underpayment: credit to balance
                    tx.execute(
                        "UPDATE tg_users SET balance = balance + ?1 WHERE chat_id = ?2",
                        (paid_rubles, chat_id),
                    ).map_err(|e| e.to_string())?;

                    let new_bal: i64 = tx
                        .query_row("SELECT balance FROM tg_users WHERE chat_id = ?1", [chat_id], |row| row.get(0))
                        .unwrap_or(paid_rubles);

                    tx.commit().map_err(|e| e.to_string())?;
                    return Ok(DonatePayApplyResult::UnderpaymentTopup {
                        required: 200,
                        paid: paid_rubles,
                        new_balance: new_bal,
                    });
                }
            } else {
                // General balance topup
                tx.execute(
                    "UPDATE tg_users SET balance = balance + ?1 WHERE chat_id = ?2",
                    (paid_rubles, chat_id),
                ).map_err(|e| e.to_string())?;

                let new_bal: i64 = tx
                    .query_row("SELECT balance FROM tg_users WHERE chat_id = ?1", [chat_id], |row| row.get(0))
                    .unwrap_or(paid_rubles);

                tx.commit().map_err(|e| e.to_string())?;
                return Ok(DonatePayApplyResult::BalanceTopup {
                    added_rubles: paid_rubles,
                    new_balance: new_bal,
                });
            }
        }
    }
    Err("Ошибка подключения к базе данных".to_string())
}
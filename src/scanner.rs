use crate::{minecraft, utils, nick_generator, playit, ngrok, db, proxy, telegram};
use crate::types::{FoundServer, ScanType, TargetedDbType};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use rand::prelude::*;
use std::time::Duration;

const SUBDOMAINS: &[&str] = &[
    "play", "mc", "server", "vanilla", "survival", "dev", "test", "lobby", "bungee", 
    "s1", "s2", "node1", "host", "api", "eu", "us", "ru", "ua", "hub", "proxy", 
    "auth", "login", "bedwars", "skywars", "factions", "creative", "smp", "games", "network", "join", "go"
];

#[derive(Clone)]
struct DeepScanTask {
    pub ip: String,
    pub domain: String,
    pub version: String,
    pub test_nick: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn save_and_send(
    ip: &str, info: &str, version: &str, online_players: i32, max_players: i32, geo: &str, req_reason: &str,
    is_cracked: bool, is_registered: bool, is_deep_scan: bool, is_op: bool, 
    needs_mods: bool, is_bungee_vulnerable: bool, is_velocity_vulnerable: bool, has_builds: bool,
    movement_restricted: bool, players: Vec<String>, rcon_pass: Option<String>, plugins: Option<String>, 
    tx: Sender<FoundServer>
) {
    let mut db_info = info.to_string();
    if let Some(ref p) = plugins {
        db_info = format!("{} | Plugins: {}", db_info, p);
    }
    if is_bungee_vulnerable { db_info = format!("{} | BUNGEE VULN", db_info); }
    if is_velocity_vulnerable { db_info = format!("{} | VELOCITY VULN", db_info); }
    
    let server = FoundServer { 
        ip: ip.to_string(),
        info: info.to_string(),
        version: version.to_string(),
        online_players,
        max_players,
        geo: geo.to_string(),
        req_reason: req_reason.to_string(),
        is_cracked,
        is_registered,
        is_deep_scan, 
        is_op,
        needs_mods,
        is_bungee_vulnerable,
        is_velocity_vulnerable,
        has_builds,
        movement_restricted,
        players: players.clone(),
        rcon_pass: rcon_pass.clone(),
        plugins: plugins.clone()
    };

    // 1. Мгновенно отправляем в UI терминала без задержек
    let _ = tx.send(server.clone());

    // 2. Асинхронно и не блокируя поток сканера сохраняем в SQLite и рассылаем в Telegram
    let db_ip = ip.to_string();
    let db_info_c = db_info;
    let rcon_c = rcon_pass;
    let players_c = players;

    tokio::spawn(async move {
        db::save_server(&db_ip, &db_info_c, is_cracked, is_registered, is_op, needs_mods, has_builds, movement_restricted, rcon_c);
        for player in &players_c {
            db::save_player(player, &db_ip);
        }
        telegram::broadcast_server(&server).await;
    });
}

#[allow(clippy::too_many_arguments)]
async fn probe_target(
    target_connect_ip: String,
    target_port: u16,
    target_domain: String,
    tv: String,
    nick_to_check: String,
    scn_type: ScanType,
    current_proxy: Option<String>,
    do_deep_scan: bool,
    rcon_enabled: bool,
    rcon_cmd: String,
    tx: Sender<FoundServer>,
    is_scan_task: Arc<AtomicBool>,
    deep_sender: Option<tokio::sync::mpsc::Sender<DeepScanTask>>,
) {
    if !is_scan_task.load(Ordering::Relaxed) {
        return;
    }
    let display_address = if target_port == 25565 {
        target_domain.clone()
    } else {
        format!("{}:{}", target_domain, target_port)
    };

    if let Ok(Some(res)) = minecraft::check_java(
        &target_connect_ip,
        target_port,
        &tv,
        &nick_to_check,
        &target_domain,
        current_proxy,
    )
    .await
    {
        let is_whitelist = res.info.contains("bs,") && !res.info.contains("nobs,");
        let is_modded = res.needs_mods
            || res.info.to_lowercase().contains("neoforge")
            || res.info.to_lowercase().contains("forge")
            || res.info.to_lowercase().contains("fabric");
        let is_green = (res.is_cracked && !is_modded && !is_whitelist) || res.info.contains("[Вход свободен]");

        if scn_type == ScanType::Default || scn_type == ScanType::MultiEngine {
            let db_to_save = if is_green {
                TargetedDbType::NoLic
            } else {
                TargetedDbType::Lic
            };
            utils::add_subnet_to_db(&target_connect_ip, db_to_save);
        }

        if let Some(pass) = &res.rcon_pass {
            if rcon_enabled && !rcon_cmd.is_empty() {
                let ip_clone_rcon = target_connect_ip.clone();
                let pass_clone = pass.clone();
                let cmd_clone = rcon_cmd.clone();
                tokio::spawn(async move {
                    minecraft::send_rcon_command(&ip_clone_rcon, &pass_clone, &cmd_clone).await;
                });
            }
        }

        let mut formatted_info = res.info.clone();
        if scn_type == ScanType::PlayerTracker {
            if res.is_registered {
                formatted_info = format!("[NICK: {}] {}", nick_to_check, formatted_info);
                save_and_send(
                    &display_address,
                    &formatted_info,
                    &res.version,
                    res.online_players,
                    res.max_players,
                    &res.geo,
                    &res.req_reason,
                    res.is_cracked,
                    res.is_registered,
                    false,
                    res.is_op,
                    res.needs_mods,
                    res.is_bungee_vulnerable,
                    res.is_velocity_vulnerable,
                    res.has_builds,
                    res.movement_restricted,
                    res.players,
                    res.rcon_pass,
                    res.plugins,
                    tx.clone(),
                )
                .await;
            }
        } else {
            save_and_send(
                &display_address,
                &formatted_info,
                &res.version,
                res.online_players,
                res.max_players,
                &res.geo,
                &res.req_reason,
                res.is_cracked,
                res.is_registered,
                false,
                res.is_op,
                res.needs_mods,
                res.is_bungee_vulnerable,
                res.is_velocity_vulnerable,
                res.has_builds,
                res.movement_restricted,
                res.players,
                res.rcon_pass,
                res.plugins,
                tx.clone(),
            )
            .await;

            // Deep scan runs EXCLUSIVELY for "green" (free/cracked, no-whitelist, no-mods) servers on base port 25565!
            if do_deep_scan && is_green && target_port == 25565 {
                if let Some(ref d_tx) = deep_sender {
                    let _ = d_tx.try_send(DeepScanTask {
                        ip: target_connect_ip.clone(),
                        domain: target_domain.clone(),
                        version: tv.clone(),
                        test_nick: nick_to_check.clone(),
                    });
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_scan(
    scn_type: ScanType, targeted_db: TargetedDbType, threads: u32, _amount: u32,
    _infinite_scan: bool, target_version: String, do_deep_scan: bool, target_nick: String, 
    auto_rcon: bool, auto_rcon_cmd: String, dns_target_domain: String,
    use_proxy: bool, auto_scrape: bool, proxy_list: Arc<Mutex<Vec<String>>>,
    tx: Sender<FoundServer>, is_scanning: Arc<AtomicBool>, scanned_ips: Arc<AtomicU32>,
    ds_active: Arc<AtomicU32>, ds_scanned: Arc<AtomicU32>, ds_total: Arc<AtomicU32>,
    multi_targeted_threads: u32, multi_standard_threads: u32,
    multi_playit_threads: u32, multi_ngrok_threads: u32,
) {
    let safe_threads = threads.clamp(1, 1_000_000);

    tokio::spawn(async move {
        if use_proxy && auto_scrape {
            let scraped = proxy::scrape_proxies().await;
            if let Ok(mut lock) = proxy_list.lock() {
                lock.extend(scraped);
                lock.sort();
                lock.dedup();
            }
        }

        let targeted_subnets = utils::get_ram_subnets(targeted_db.clone());

        let current_nick = if scn_type == ScanType::PlayerTracker {
            if target_nick.is_empty() { nick_generator::generate_nick() } else { target_nick.clone() }
        } else { "DankScanner".to_string() };

        // ═══════════════════════════════════════════════════════════════
        // DEDICATED 100-THREAD NON-BLOCKING DEEP SCAN WORKER POOL
        // ═══════════════════════════════════════════════════════════════
        let (deep_tx, mut deep_rx) = tokio::sync::mpsc::channel::<DeepScanTask>(500);
        let deep_sender_opt = if do_deep_scan { Some(deep_tx) } else { None };

        if do_deep_scan {
            let is_scan_ds = is_scanning.clone();
            let tx_ds = tx.clone();
            let p_list_ds = proxy_list.clone();
            let ds_a = ds_active.clone();
            let ds_s = ds_scanned.clone();
            let ds_t = ds_total.clone();
            let r_cmd_ds = auto_rcon_cmd.clone();

            tokio::spawn(async move {
                while is_scan_ds.load(Ordering::Relaxed) {
                    let task = match deep_rx.recv().await {
                        Some(t) => t,
                        None => break,
                    };

                    ds_a.fetch_add(1, Ordering::Relaxed);
                    // Check ports 1..=65535 except 25565
                    ds_t.fetch_add(65534, Ordering::Relaxed);

                    let (port_tx, port_rx) = tokio::sync::mpsc::channel::<u16>(1000);
                    let port_rx = std::sync::Arc::new(tokio::sync::Mutex::new(port_rx));
                    let is_scan_p = is_scan_ds.clone();

                    // Feed ports prioritized by likelihood of active servers
                    tokio::spawn(async move {
                        // 1. High-frequency neighboring ports (25560..=25585)
                        for port in 25560..=25585u16 {
                            if port == 25565 { continue; }
                            if !is_scan_p.load(Ordering::Relaxed) || port_tx.send(port).await.is_err() { return; }
                        }
                        // 2. Secondary standard range (25500..=25600)
                        for port in 25500..=25600u16 {
                            if (25560..=25585).contains(&port) || port == 25565 { continue; }
                            if !is_scan_p.load(Ordering::Relaxed) || port_tx.send(port).await.is_err() { return; }
                        }
                        // 3. Hosting allocation blocks (10000..=30000)
                        for port in 10000..=30000u16 {
                            if (25500..=25600).contains(&port) { continue; }
                            if !is_scan_p.load(Ordering::Relaxed) || port_tx.send(port).await.is_err() { return; }
                        }
                        // 4. Remaining ports
                        for port in 1..=65535u16 {
                            if port == 25565 || (10000..=30000).contains(&port) { continue; }
                            if !is_scan_p.load(Ordering::Relaxed) || port_tx.send(port).await.is_err() { return; }
                        }
                    });

                    // 100 dedicated asynchronous deep-scan tasks for this server
                    let mut ds_handles = vec![];
                    for _ in 0..100 {
                        let rx = port_rx.clone();
                        let is_scan_w = is_scan_ds.clone();
                        let ip_c = task.ip.clone();
                        let dom_c = task.domain.clone();
                        let ver_c = task.version.clone();
                        let nick_c = task.test_nick.clone();
                        let p_list_w = p_list_ds.clone();
                        let tx_w = tx_ds.clone();
                        let sc_ports = ds_s.clone();
                        let r_cmd_w = r_cmd_ds.clone();

                        ds_handles.push(tokio::spawn(async move {
                            while is_scan_w.load(Ordering::Relaxed) {
                                let port = {
                                    let mut guard = rx.lock().await;
                                    match guard.recv().await {
                                        Some(p) => p,
                                        None => break,
                                    }
                                };

                                let cur_proxy = if use_proxy {
                                    p_list_w.lock().ok().and_then(|l| {
                                        if !l.is_empty() {
                                            let mut rng = rand::thread_rng();
                                            l.choose(&mut rng).cloned()
                                        } else { None }
                                    })
                                } else { None };

                                probe_target(
                                    ip_c.clone(), port, dom_c.clone(), ver_c.clone(), nick_c.clone(),
                                    ScanType::MultiEngine, cur_proxy, false, auto_rcon, r_cmd_w.clone(),
                                    tx_w.clone(), is_scan_w.clone(), None,
                                ).await;

                                sc_ports.fetch_add(1, Ordering::Relaxed);
                            }
                        }));
                    }

                    for h in ds_handles {
                        let _ = h.await;
                    }
                    ds_a.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }

        let mut worker_handles = vec![];

        // ═══════════════════════════════════════════════════════════════
        // MULTI-ENGINE CONCURRENT WORKER POOLS
        // ═══════════════════════════════════════════════════════════════
        if scn_type == ScanType::MultiEngine {
            // 1. Targeted Subnets Pool (High-Density /24 and /16 Ranges)
            for _ in 0..multi_targeted_threads.clamp(0, 100) {
                let subnets = targeted_subnets.clone();
                let is_scan = is_scanning.clone();
                let scan_cnt = scanned_ips.clone();
                let tx = tx.clone();
                let tv = target_version.clone();
                let nick = current_nick.clone();
                let r_cmd = auto_rcon_cmd.clone();
                let p_list = proxy_list.clone();
                let d_sender = deep_sender_opt.clone();

                worker_handles.push(tokio::spawn(async move {
                    while is_scan.load(Ordering::Relaxed) {
                        let ip = {
                            let mut rng = rand::thread_rng();
                            let sub = &subnets[rng.gen_range(0..subnets.len())];
                            let ip_u32 = if sub.start == sub.end { sub.start } else { rng.gen_range(sub.start..=sub.end) };
                            format!("{}.{}.{}.{}", (ip_u32 >> 24) as u8, (ip_u32 >> 16) as u8, (ip_u32 >> 8) as u8, ip_u32 as u8)
                        };

                        let cur_proxy = if use_proxy {
                            p_list.lock().ok().and_then(|l| {
                                if !l.is_empty() {
                                    let mut rng = rand::thread_rng();
                                    l.choose(&mut rng).cloned()
                                } else { None }
                            })
                        } else { None };

                        probe_target(
                            ip.clone(), 25565, ip, tv.clone(), nick.clone(), ScanType::MultiEngine,
                            cur_proxy, do_deep_scan, auto_rcon, r_cmd.clone(), tx.clone(), is_scan.clone(),
                            d_sender.clone(),
                        ).await;

                        scan_cnt.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }));
            }

            // 2. Standard IPv4 Pool
            for _ in 0..multi_standard_threads.clamp(0, 100) {
                let is_scan = is_scanning.clone();
                let scan_cnt = scanned_ips.clone();
                let tx = tx.clone();
                let tv = target_version.clone();
                let nick = current_nick.clone();
                let r_cmd = auto_rcon_cmd.clone();
                let p_list = proxy_list.clone();
                let d_sender = deep_sender_opt.clone();

                worker_handles.push(tokio::spawn(async move {
                    while is_scan.load(Ordering::Relaxed) {
                        let ip = {
                            let mut rng = rand::thread_rng();
                            format!("{}.{}.{}.{}", rng.gen_range(1..=255), rng.gen_range(0..=255), rng.gen_range(0..=255), rng.gen_range(0..=255))
                        };

                        let cur_proxy = if use_proxy {
                            p_list.lock().ok().and_then(|l| {
                                if !l.is_empty() {
                                    let mut rng = rand::thread_rng();
                                    l.choose(&mut rng).cloned()
                                } else { None }
                            })
                        } else { None };

                        probe_target(
                            ip.clone(), 25565, ip, tv.clone(), nick.clone(), ScanType::MultiEngine,
                            cur_proxy, do_deep_scan, auto_rcon, r_cmd.clone(), tx.clone(), is_scan.clone(),
                            d_sender.clone(),
                        ).await;

                        scan_cnt.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }));
            }

            // 3. Playit.gg Pool
            for _ in 0..multi_playit_threads.clamp(0, 100) {
                let is_scan = is_scanning.clone();
                let scan_cnt = scanned_ips.clone();
                let tx = tx.clone();
                let tv = target_version.clone();
                let nick = current_nick.clone();
                let r_cmd = auto_rcon_cmd.clone();
                let p_list = proxy_list.clone();

                worker_handles.push(tokio::spawn(async move {
                    while is_scan.load(Ordering::Relaxed) {
                        let (ip, port) = {
                            let mut rng = rand::thread_rng();
                            let count = rng.gen_range(0..100_000);
                            playit::get_target(count)
                        };

                        let cur_proxy = if use_proxy {
                            p_list.lock().ok().and_then(|l| {
                                if !l.is_empty() {
                                    let mut rng = rand::thread_rng();
                                    l.choose(&mut rng).cloned()
                                } else { None }
                            })
                        } else { None };

                        probe_target(
                            ip.clone(), port, ip, tv.clone(), nick.clone(), ScanType::MultiEngine,
                            cur_proxy, false, auto_rcon, r_cmd.clone(), tx.clone(), is_scan.clone(),
                            None,
                        ).await;

                        scan_cnt.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }));
            }

            // 5. Ngrok Pool (Zero-DNS, Direct Region IP)
            for _ in 0..multi_ngrok_threads.clamp(0, 100) {
                let is_scan = is_scanning.clone();
                let scan_cnt = scanned_ips.clone();
                let tx = tx.clone();
                let tv = target_version.clone();
                let nick = current_nick.clone();
                let r_cmd = auto_rcon_cmd.clone();
                let p_list = proxy_list.clone();

                worker_handles.push(tokio::spawn(async move {
                    while is_scan.load(Ordering::Relaxed) {
                        let (connect_ip, port, domain) = ngrok::get_random_target();

                        let cur_proxy = if use_proxy {
                            p_list.lock().ok().and_then(|l| {
                                if !l.is_empty() {
                                    let mut rng = rand::thread_rng();
                                    l.choose(&mut rng).cloned()
                                } else { None }
                            })
                        } else { None };

                        probe_target(
                            connect_ip, port, domain, tv.clone(), nick.clone(), ScanType::MultiEngine,
                            cur_proxy, false, auto_rcon, r_cmd.clone(), tx.clone(), is_scan.clone(),
                            None,
                        ).await;

                        scan_cnt.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }));
            }

            for handle in worker_handles {
                let _ = handle.await;
            }
            is_scanning.store(false, Ordering::SeqCst);
            return;
        }

        // ═══════════════════════════════════════════════════════════════
        // SINGLE ENGINE PERSISTENT WORKER POOL
        // ═══════════════════════════════════════════════════════════════
        for _ in 0..safe_threads {
            let scn = scn_type.clone();
            let is_scan = is_scanning.clone();
            let scan_cnt = scanned_ips.clone();
            let tx = tx.clone();
            let tv = target_version.clone();
            let nick = current_nick.clone();
            let r_cmd = auto_rcon_cmd.clone();
            let p_list = proxy_list.clone();
            let subnets = targeted_subnets.clone();
            let dns_domain = dns_target_domain.clone();
            let d_sender = deep_sender_opt.clone();

            worker_handles.push(tokio::spawn(async move {
                while is_scan.load(Ordering::Relaxed) {
                    let cur_proxy = if use_proxy {
                        p_list.lock().ok().and_then(|lock| {
                            if !lock.is_empty() {
                                let mut rng = rand::thread_rng();
                                Some(lock.choose(&mut rng).unwrap().clone())
                            } else { None }
                        })
                    } else { None };

                    let maybe_target = {
                        let mut rng = rand::thread_rng();
                        match scn {
                            ScanType::MultiEngine => None,
                            ScanType::Ngrok => {
                                let (connect_ip, port, domain) = ngrok::get_random_target();
                                Some((connect_ip, port, domain))
                            }
                            ScanType::DnsBrute => {
                                let sub = SUBDOMAINS.choose(&mut rng).unwrap_or(&"play");
                                let domain = format!("{}.{}", sub, dns_domain);
                                Some((domain.clone(), 25565, domain))
                            }
                            ScanType::Targeted | ScanType::PlayerTracker => {
                                let sub = &subnets[rng.gen_range(0..subnets.len())];
                                let ip_u32 = if sub.start == sub.end { sub.start } else { rng.gen_range(sub.start..=sub.end) };
                                let ip = format!("{}.{}.{}.{}", (ip_u32 >> 24) as u8, (ip_u32 >> 16) as u8, (ip_u32 >> 8) as u8, ip_u32 as u8);
                                Some((ip.clone(), 25565, ip))
                            }
                            ScanType::Playit => {
                                let count = rng.gen_range(0..100_000);
                                let (ip, port) = playit::get_target(count);
                                Some((ip.clone(), port, ip))
                            }
                            ScanType::Default => {
                                let ip = format!("{}.{}.{}.{}", rng.gen_range(1..=255), rng.gen_range(0..=255), rng.gen_range(0..=255), rng.gen_range(0..=255));
                                Some((ip.clone(), 25565, ip))
                            }
                        }
                    };

                    let (target_connect_ip, target_port, target_domain) = match maybe_target {
                        Some(t) => t,
                        None => {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        }
                    };

                    probe_target(
                        target_connect_ip, target_port, target_domain, tv.clone(), nick.clone(),
                        scn.clone(), cur_proxy, do_deep_scan, auto_rcon, r_cmd.clone(),
                        tx.clone(), is_scan.clone(), d_sender.clone(),
                    ).await;

                    scan_cnt.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }));
        }

        for handle in worker_handles {
            let _ = handle.await;
        }
        
        is_scanning.store(false, Ordering::SeqCst);
    });
}
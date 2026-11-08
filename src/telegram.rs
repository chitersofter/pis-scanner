use crate::db::{self, TgDbUser};
use crate::donatepay;
use crate::types::FoundServer;
use reqwest::Client;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::time::{sleep, Duration};

static BOT_TOKEN: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static BOT_STATUS: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static BOT_LOGS: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();
static BOT_ENABLED: AtomicBool = AtomicBool::new(true);
static BOT_UPDATES_COUNT: AtomicU32 = AtomicU32::new(0);
static PRIME_ROTATION_IDX: AtomicUsize = AtomicUsize::new(0);
static RATE_LIMITER: OnceLock<Mutex<std::collections::HashMap<i64, (u64, u32)>>> = OnceLock::new();

fn is_rate_limited(chat_id: i64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let limiter = RATE_LIMITER.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut map) = limiter.lock() {
        if map.len() > 10_000 {
            map.retain(|_, (ts, _)| *ts + 60 >= now);
        }
        let entry = map.entry(chat_id).or_insert((now, 0));
        if entry.0 + 2 >= now {
            entry.1 += 1;
            if entry.1 > 7 {
                return true;
            }
        } else {
            *entry = (now, 1);
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerColor {
    Green,  // [FREE] Open vanilla/paper cracked server without restrictions
    Yellow, // [CRACKED] Cracked server with restrictions (whitelist, mods, auth/reg)
    Blue,   // [LIC] Licensed Mojang server
}

pub fn get_server_color(server: &FoundServer) -> ServerColor {
    if !server.is_cracked {
        ServerColor::Blue
    } else if server.info.contains("[Вход свободен]") {
        ServerColor::Green
    } else {
        ServerColor::Yellow
    }
}

pub fn init(token: String) {
    BOT_TOKEN.get_or_init(|| Arc::new(Mutex::new(token)));
    BOT_STATUS.get_or_init(|| Arc::new(Mutex::new("🔴 Ожидание запуска...".to_string())));
    BOT_LOGS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));

    log_event("🚀 Telegram Bot Engine инициализирован");

    tokio::spawn(async move {
        poll_updates().await;
    });
}

pub fn update_token(token: String) {
    if let Some(token_store) = BOT_TOKEN.get() {
        if let Ok(mut lock) = token_store.lock() {
            *lock = token;
            log_event("🔑 Токен Telegram бота обновлен через панель управления");
        }
    }
}

pub fn get_status() -> String {
    if !BOT_ENABLED.load(Ordering::Relaxed) {
        return "⏸ ПРИОСТАНОВЛЕН (Выключен в панели)".to_string();
    }
    if let Some(status) = BOT_STATUS.get() {
        if let Ok(lock) = status.lock() {
            return lock.clone();
        }
    }
    "🔴 Ожидание...".to_string()
}

fn set_status(new_status: &str) {
    if let Some(status) = BOT_STATUS.get() {
        if let Ok(mut lock) = status.lock() {
            *lock = new_status.to_string();
        }
    }
}

pub fn is_bot_enabled() -> bool {
    BOT_ENABLED.load(Ordering::Relaxed)
}

pub fn toggle_bot() -> bool {
    let prev = BOT_ENABLED.fetch_xor(true, Ordering::SeqCst);
    let new_state = !prev;
    if new_state {
        log_event("🟢 Telegram Bot возобновил работу");
    } else {
        log_event("⏸ Telegram Bot приостановлен администратором");
    }
    new_state
}

pub fn get_updates_count() -> u32 {
    BOT_UPDATES_COUNT.load(Ordering::Relaxed)
}

fn get_time_str() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sec = now % 60;
    let min = (now / 60) % 60;
    let hour = (now / 3600 + 3) % 24; // UTC+3 / Local offset
    format!("{:02}:{:02}:{:02}", hour, min, sec)
}

pub fn log_event(msg: &str) {
    let entry = format!("{} │ {}", get_time_str(), msg);
    if let Some(logs) = BOT_LOGS.get() {
        if let Ok(mut lock) = logs.lock() {
            if lock.len() >= 300 {
                lock.drain(0..50);
            }
            lock.push(entry);
        }
    }
}

pub fn get_bot_logs() -> Vec<String> {
    if let Some(logs) = BOT_LOGS.get() {
        if let Ok(lock) = logs.lock() {
            return lock.clone();
        }
    }
    Vec::new()
}

pub fn clear_bot_logs() {
    if let Some(logs) = BOT_LOGS.get() {
        if let Ok(mut lock) = logs.lock() {
            lock.clear();
            lock.push(format!("{} │ 🧹 Логи активности очищены", get_time_str()));
        }
    }
}

fn clean_token(token: &str) -> String {
    let t = token.trim();
    if let Some(rest) = t.strip_prefix("bot") {
        rest.trim().to_string()
    } else if let Some(rest) = t.strip_prefix("BOT") {
        rest.trim().to_string()
    } else if let Some(rest) = t.strip_prefix("Bot") {
        rest.trim().to_string()
    } else {
        t.to_string()
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

static BOT_API_BASE: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static BOT_USERNAME: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
const DEFAULT_WORKER_URL: &str = "https://api.telegram.org";

pub fn get_bot_username() -> String {
    if let Some(store) = BOT_USERNAME.get() {
        if let Ok(lock) = store.lock() {
            if !lock.is_empty() {
                return lock.clone();
            }
        }
    }
    "dank_scanner_bot".to_string()
}

#[allow(dead_code)]
pub fn set_api_base(base: String) {
    if let Some(store) = BOT_API_BASE.get() {
        if let Ok(mut lock) = store.lock() {
            *lock = base.trim().trim_end_matches('/').to_string();
        }
    } else {
        let _ = BOT_API_BASE.set(Arc::new(Mutex::new(base.trim().trim_end_matches('/').to_string())));
    }
}

pub fn get_api_base() -> String {
    if let Some(store) = BOT_API_BASE.get() {
        if let Ok(lock) = store.lock() {
            if !lock.is_empty() {
                return lock.clone();
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string("tg_api.txt") {
        let clean = content
            .trim_start_matches('\u{feff}')
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('/')
            .to_string();
        if !clean.is_empty() {
            let u = if clean.starts_with("http://") || clean.starts_with("https://") {
                clean
            } else if clean.contains("://") {
                clean
            } else {
                format!("https://{}", clean)
            };
            let _ = BOT_API_BASE.set(Arc::new(Mutex::new(u.clone())));
            return u;
        }
    }
    DEFAULT_WORKER_URL.to_string()
}

pub fn get_http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

async fn set_bot_commands(client: &Client, token: &str) {
    let url = format!("{}/bot{}/setMyCommands", get_api_base(), token);
    let body = json!({
        "commands": [
            {"command": "start", "description": "🚀 Главное меню и статус рассылки"},
            {"command": "menu", "description": "⚡ Главное меню бота"},
            {"command": "radar", "description": "🎯 Охота по фильтрам (радар)"},
            {"command": "daily", "description": "🎁 Бесплатный ежедневный кейс"},
            {"command": "ref", "description": "👥 Реферальная программа (+1 день Plus)"},
            {"command": "market", "description": "🛒 Купить сервер (15⭐ / 20₽)"},
            {"command": "api", "description": "👑 API доступ для кланов"},
            {"command": "search", "description": "🔍 Поиск серверов в базе"},
            {"command": "export", "description": "Выгрузка серверов (TXT) по вашим фильтрам"},
            {"command": "settings", "description": "⚙ Настройки фильтров и источников"},
            {"command": "profile", "description": "👤 Личный кабинет и подписка"},
            {"command": "plans", "description": "💳 Тарифные планы (Plus / Prime)"},
            {"command": "promo", "description": "🎁 Активация промокода"},
            {"command": "support", "description": "💬 Поддержка @pisscaner_support"},
            {"command": "status", "description": "📊 Статус сканера и база"}
        ]
    });
    let _ = client.post(&url).json(&body).send().await;
}

async fn answer_callback_query(
    client: &Client,
    token: &str,
    callback_id: &str,
    text: Option<&str>,
    show_alert: bool,
) {
    if callback_id.is_empty() {
        return;
    }
    let url = format!("{}/bot{}/answerCallbackQuery", get_api_base(), token);
    let mut body = json!({
        "callback_query_id": callback_id,
        "show_alert": show_alert,
    });
    if let Some(t) = text {
        body["text"] = json!(t);
    }
    let _ = client.post(&url).json(&body).send().await;
}

async fn poll_updates() {
    let client = get_http_client();
    let mut offset = 0;
    let mut last_checked_token = String::new();
    let mut is_token_valid = false;

    loop {
        if !BOT_ENABLED.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        let raw_token = {
            if let Some(t) = BOT_TOKEN.get() {
                t.lock().map(|l| l.clone()).unwrap_or_default()
            } else {
                String::new()
            }
        };

        let token = clean_token(&raw_token);

        if token.is_empty() {
            set_status("🔴 Токен не указан");
            is_token_valid = false;
            last_checked_token = String::new();
            sleep(Duration::from_secs(3)).await;
            continue;
        }

        let api_base = get_api_base();

        if token != last_checked_token || !is_token_valid {
            set_status("🟡 Проверка токена (Cloudflare Worker)...");
            let me_url = format!("{}/bot{}/getMe", api_base, token);
            match client.get(&me_url).send().await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if json["ok"].as_bool().unwrap_or(false) {
                            let uname = json["result"]["username"].as_str().unwrap_or("");
                            if let Some(store) = BOT_USERNAME.get() {
                                if let Ok(mut lock) = store.lock() {
                                    *lock = uname.to_string();
                                }
                            } else {
                                let _ = BOT_USERNAME.set(Arc::new(Mutex::new(uname.to_string())));
                            }
                            let status_str = format!("🟢 @{} [Cloudflare Worker]", uname);
                            set_status(&status_str);
                            log_event(&format!("🟢 Бот успешно подключен через Cloudflare Worker (@{})", uname));
                            is_token_valid = true;
                            last_checked_token = token.clone();
                            set_bot_commands(&client, &token).await;
                        } else {
                            set_status("🔴 Неверный токен!");
                            log_event("❌ Ошибка: Telegram отклонил токен бота");
                            is_token_valid = false;
                            sleep(Duration::from_secs(3)).await;
                            continue;
                        }
                    } else {
                        log_event("⚠️ Ошибка формата ответа от Cloudflare Worker. Повтор...");
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }
                Err(e) => {
                    log_event(&format!("⚠️ Ошибка соединения с Cloudflare Worker: {:?}", e));
                    set_status("🔴 Ошибка соединения с воркером");
                    sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
        }

        let url = format!(
            "{}/bot{}/getUpdates?offset={}&timeout=10",
            api_base, token, offset
        );

        match client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(result) = json.get("result").and_then(|r| r.as_array()) {
                        for update in result {
                            if let Some(update_id) = update.get("update_id").and_then(|id| id.as_i64()) {
                                offset = update_id + 1;
                            }

                            BOT_UPDATES_COUNT.fetch_add(1, Ordering::Relaxed);

                            // 1. Handle Telegram Stars Pre-Checkout Query
                            if let Some(pcq) = update.get("pre_checkout_query") {
                                if let Some(pcq_id) = pcq.get("id").and_then(|id| id.as_str()) {
                                    log_event("⭐ Получен запрос на оплату Telegram Stars (PreCheckout)");
                                    let answer_url = format!("{}/bot{}/answerPreCheckoutQuery", api_base, token);
                                    let _ = client.post(&answer_url).json(&json!({
                                        "pre_checkout_query_id": pcq_id,
                                        "ok": true
                                    })).send().await;
                                }
                            }

                            // 2. Handle Normal Message & Successful Payments
                            if let Some(msg) = update.get("message") {
                                handle_message(&client, &token, msg).await;
                            } else if let Some(callback) = update.get("callback_query") {
                                handle_callback(&client, &token, callback).await;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                sleep(Duration::from_secs(2)).await;
            }
        }

        sleep(Duration::from_millis(150)).await;
    }
}

async fn handle_message(client: &Client, token: &str, msg: &serde_json::Value) {
    let chat_id = match msg.get("chat").and_then(|c| c.get("id")).and_then(|id| id.as_i64()) {
        Some(id) => id,
        None => return,
    };

    if db::is_user_banned(chat_id) {
        return;
    }

    if is_rate_limited(chat_id) {
        return;
    }

    let username = msg
        .get("from")
        .and_then(|f| f.get("username"))
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    let mut user = db::get_or_create_tg_user(chat_id, &username);

    // Check for Successful Telegram Stars Payment
    if let Some(payment) = msg.get("successful_payment") {
        let payload = payment.get("invoice_payload").and_then(|p| p.as_str()).unwrap_or("");
        let total_stars = payment.get("total_amount").and_then(|a| a.as_i64()).unwrap_or(0);

        log_event(&format!("⭐ ОПЛАТА STARS: @{} (ID: {}) оплатил payload: '{}' ({} ⭐)", username, chat_id, payload, total_stars));

        if payload == "sub_plus_30" || payload == "sub_standard_30" {
            if total_stars >= 120 {
                let _ = db::grant_subscription(chat_id, "Plus", 30);
                send_message(
                    client,
                    token,
                    chat_id,
                    "🟢 <b>Оплата получена!</b>\n\nВам активирована подписка <b>Plus</b> на <b>30 дней</b> (666 серверов/день).\nДоступ к открытым серверам активен.",
                ).await;
            } else {
                log_event(&format!("⚠️ Недостаточная сумма Stars для Plus: {} от ID {}", total_stars, chat_id));
            }
        } else if payload == "sub_prime_30" {
            if total_stars >= 240 {
                let _ = db::grant_subscription(chat_id, "Prime", 30);
                send_message(
                    client,
                    token,
                    chat_id,
                    "⭐ <b>Оплата получена!</b>\n\nВам активирована подписка <b>Prime</b> на <b>30 дней</b> (∞ безлимит).\nЭксклюзивный поток серверов в одни руки с никами игроков и уязвимостями открыт.",
                ).await;
            } else {
                log_event(&format!("⚠️ Недостаточная сумма Stars для Prime: {} от ID {}", total_stars, chat_id));
            }
        } else if payload == "buy_cheats_stars" || payload == "buy_client_stars" {
            if total_stars >= 240 {
                log_event(&format!("⚡ ЧИТЫ STARS: @{} (ID: {}) оплатил чит-пак ({} ⭐)", username, chat_id, total_stars));
                deliver_cheats_file(client, token, chat_id).await;
            } else {
                log_event(&format!("⚠️ Недостаточная сумма Stars для читов: {} от ID {}", total_stars, chat_id));
            }
        } else if payload.starts_with("topup_stars_") {
            if total_stars > 0 {
                let _ = db::add_user_balance(chat_id, total_stars);
                send_message(
                    client,
                    token,
                    chat_id,
                    &format!("💰 <b>Баланс успешно пополнен на {} Stars (руб.)!</b>\nСредства зачислены на ваш аккаунт.", total_stars),
                ).await;
            }
        } else if payload == "buy_single_server_stars" {
            if total_stars >= 15 {
                if let Some(srv) = db::get_random_plus_server() {
                    let (version, online, location) = clean_parse_server_info(&srv.info);
                    let mut online_p = 0;
                    let mut max_p = 20;
                    if let Some(slash_idx) = online.find('/') {
                        online_p = online[..slash_idx].trim().parse::<i32>().unwrap_or(0);
                        max_p = online[slash_idx + 1..].trim().parse::<i32>().unwrap_or(20);
                    }
                    let found_server = FoundServer {
                        ip: srv.ip.clone(),
                        info: srv.info.clone(),
                        version: version.clone(),
                        online_players: online_p,
                        max_players: max_p,
                        geo: location.clone(),
                        req_reason: String::new(),
                        is_cracked: srv.is_cracked,
                        is_registered: srv.is_registered,
                        is_deep_scan: false,
                        is_op: srv.is_op,
                        needs_mods: srv.needs_mods,
                        is_bungee_vulnerable: srv.info.contains("Bungee"),
                        is_velocity_vulnerable: srv.info.contains("Velocity"),
                        has_builds: srv.has_builds,
                        movement_restricted: srv.movement_restricted,
                        players: db::get_players_for_server(&srv.ip),
                        rcon_pass: srv.rcon_pass.clone(),
                        plugins: None,
                    };
                    let card = format_server_card_html(&found_server, false);
                    let deliver_msg = format!("🎉 <b>Оплата 15 Stars получена!</b>\nВам выдан сервер из Plus-пула:\n\n{}", card);
                    let markup = json!({
                        "inline_keyboard": [
                            [{"text": "Скопировать IP", "callback_data": format!("copy_ip:{}", srv.ip)}],
                            [{"text": "Купить ещё (15 Stars)", "callback_data": "buy_single_stars"}, {"text": "Главное меню", "callback_data": "menu"}]
                        ]
                    });
                    send_message_with_markup(client, token, chat_id, &deliver_msg, markup).await;
                } else {
                    let _ = db::add_user_balance(chat_id, 20);
                    send_message(client, token, chat_id, "⚠️ На данный момент нет свободных серверов в наличии. 20 руб. зачислены на ваш баланс.").await;
                }
            }
        }
        return;
    }

    let text = msg.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
    log_event(&format!("📨 Входящее сообщение от @{} (ID: {}): '{}'", username, chat_id, text));

    if text.starts_with("/start") {
        let param = text.trim_start_matches("/start").trim();
        if !param.is_empty() {
            let ref_id_str = if let Some(r) = param.strip_prefix("r_") {
                r
            } else if let Some(r) = param.strip_prefix("ref_") {
                r
            } else {
                param
            };
            if let Ok(ref_id) = ref_id_str.parse::<i64>() {
                let (rewarded, referrer) = db::process_referral(chat_id, ref_id);
                if rewarded {
                    user = db::get_or_create_tg_user(chat_id, &username);
                    send_message(
                        client,
                        token,
                        chat_id,
                        "🎁 <b>Добро пожаловать по приглашению друга!</b>\n\nВам начислен <b>1 день Plus-подписки</b>! Все функции поиска и рассылки серверов уже активны.",
                    ).await;

                    send_message(
                        client,
                        token,
                        referrer,
                        &format!("🎉 <b>Новый реферал!</b>\n\nПользователь @{} (ID: <code>{}</code>) зарегистрировался по вашей ссылке. Вам начислено <b>+10 ₽</b> на баланс!", escape_html(&username), chat_id),
                    ).await;
                }
            }
        }
        send_bottom_keyboard_if_needed(client, token, chat_id).await;
        send_start_menu(client, token, &user).await;
    } else if text.starts_with("/cheats") || text.starts_with("/client") || text.starts_with("/cheat") || text.contains("Читы") || text.contains("Клиент") {
        send_cheats_menu(client, token, chat_id).await;
    } else if text.starts_with("/radar") || text.starts_with("/hunt") || text == "Охота по фильтрам" || text == "🎯 Охота по фильтрам" || text == "🎯 Радар" || text == "Радар" {
        send_radar_menu_direct(client, token, chat_id, &user).await;
    } else if text.starts_with("/daily") || text.starts_with("/case") || text == "Ежедневный кейс" || text == "🎁 Ежедневный кейс" || text == "🎁 Бесплатный кейс" {
        handle_daily_case_action(client, token, chat_id, None).await;
    } else if text.starts_with("/ref") || text.starts_with("/referral") || text == "Рефералы" || text == "👥 Рефералы" || text == "👥 Реферальная программа" {
        send_referral_menu_direct(client, token, chat_id, &user).await;
    } else if text.starts_with("/api") || text.starts_with("/clan") || text.starts_with("/clans") || text == "API для кланов" || text == "👑 API & Кланы" || text == "🏰 Для кланов" {
        send_clan_api_menu_direct(client, token, chat_id).await;
    } else if text.starts_with("/menu") || text == "Главное меню" || text == "◀ В главное меню" || text == "⚡ Главное меню" || text == "◀ Меню" {
        send_main_menu(client, token, &user).await;
    } else if text.starts_with("/profile") || text == "Профиль" || text == "👤 Профиль" || text == "👤 Мой Профиль" {
        send_profile(client, token, &user).await;
    } else if text.starts_with("/buy") || text.starts_with("/plans") || text == "Тарифы" || text == "💳 Тарифы и подписка" || text == "💳 Тарифы" {
        send_plans_menu(client, token, chat_id).await;
    } else if text.starts_with("/market") || text == "Купить сервер" || text == "🛒 Купить сервер" || text == "🛒 Купить сервер штучно" || text == "🛒 VIP-маркет" {
        send_market_menu_direct(client, token, chat_id, &user).await;
    } else if text == "Промокод" || text == "🎁 Промокод" || text == "🎁 Ввести промокод" || text == "/promo" {
        send_promo_prompt(client, token, chat_id).await;
    } else if text.starts_with("/promo ") {
        let code = text.trim_start_matches("/promo").trim();
        if code.is_empty() {
            send_promo_prompt(client, token, chat_id).await;
        } else {
            match db::redeem_promocode(chat_id, code) {
                Ok((tier, days)) => {
                    log_event(&format!("🎁 ПРОМОКОД: @{} активировал код '{}' (Тариф: {}, {} дн.)", username, code, tier, days));
                    let markup = json!({
                        "inline_keyboard": [[{"text": "Главное меню", "callback_data": "menu"}]]
                    });
                    send_message_with_markup(
                        client,
                        token,
                        chat_id,
                        &format!(
                            "🎉 <b>Промокод успешно активирован!</b>\n\nВам начислена подписка <b>{}</b> на <b>{} дней</b>.\nПриятного пользования сканером!",
                            tier, days
                        ),
                        markup,
                    )
                    .await;
                }
                Err(err) => {
                    let markup = json!({
                        "inline_keyboard": [[{"text": "Главное меню", "callback_data": "menu"}]]
                    });
                    send_message_with_markup(client, token, chat_id, &format!("❌ <b>Ошибка:</b> {}", err), markup).await;
                }
            }
        }
    } else if text.starts_with("/support") || text == "Поддержка" || text == "💬 Поддержка" {
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Связаться с поддержкой", "url": "https://t.me/pisscaner_support"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        send_message_with_markup(
            client,
            token,
            chat_id,
            "<b>Служба поддержки пользователей:</b>\n\nЕсли у вас есть вопросы по работе бота, оплате, тарифам или поиску серверов, напишите нам:\n👉 <b>@pisscaner_support</b>",
            markup,
        ).await;
    } else if text.starts_with("/settings") || text == "Фильтры" || text == "🎯 Фильтры" || text == "⚙ Фильтры" || text == "⚙ Настройки" || text == "Настройки" {
        send_settings_menu_direct(client, token, chat_id, &user).await;
    } else if text == "Вкл/Выкл рассылку" || text == "Включить/выключить рассылку" || text.starts_with("/toggle_feed") || text.starts_with("/feed_") || text.starts_with("/start_feed") || text.starts_with("/stop_feed") {
        user.active = !user.active;
        db::save_tg_user(&user);
        let status_msg = if user.active {
            "<b>Рассылка серверов ВКЛЮЧЕНА!</b>\nНовые серверы будут поступать в этот чат."
        } else {
            "<b>Рассылка серверов ПРИОСТАНОВЛЕНА.</b>\nСерверы временно не отправляются."
        };
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Настройки фильтров", "callback_data": "settings"}, {"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        send_message_with_markup(client, token, chat_id, status_msg, markup).await;
    } else if text.starts_with("/search") || text.starts_with("/find") || text == "Найти сервер" || text == "🔍 Найти сервер" || text == "🔍 Найти сервер в базе" {
        let query = if text.starts_with("/search") {
            text.trim_start_matches("/search").trim()
        } else if text.starts_with("/find") {
            text.trim_start_matches("/find").trim()
        } else {
            ""
        };

        if query.is_empty() {
            let markup = json!({
                "inline_keyboard": [
                    [{"text": "1.20.1", "callback_data": "quick_search:1.20.1"}, {"text": "RCON", "callback_data": "quick_search:rcon"}],
                    [{"text": "Anarchy", "callback_data": "quick_search:anarchy"}, {"text": "Survival", "callback_data": "quick_search:survival"}],
                    [{"text": "Главное меню", "callback_data": "menu"}]
                ]
            });
            send_message_with_markup(
                client,
                token,
                chat_id,
                "🔍 <b>Поиск серверов в базе данных</b>\n\nВведите запрос после команды, например:\n• <code>/search 1.20.1</code>\n• <code>/search rcon</code>\n• <code>/search anarchy</code>\n• <code>/search paper</code>\n• <code>/search playit</code>\n\n<i>Или выберите быстрый поисковый запрос ниже:</i>",
                markup,
            ).await;
        } else {
            let results = db::search_servers(query);
            if results.is_empty() {
                let markup = json!({
                    "inline_keyboard": [[{"text": "Главное меню", "callback_data": "menu"}]]
                });
                send_message_with_markup(
                    client,
                    token,
                    chat_id,
                    &format!("🔍 По запросу «<code>{}</code>» ничего не найдено в текущей базе.", escape_html(query)),
                    markup,
                ).await;
            } else {
                let top_results = &results[..results.len().min(5)];
                let mut resp_text = format!("🔍 <b>Результаты поиска по «{}» (Найдено: {}):</b>\n───────────────────────\n", escape_html(query), results.len());
                for (i, s) in top_results.iter().enumerate() {
                    let (version, online, loc) = clean_parse_server_info(&s.info);
                    let tag = if s.is_cracked { "[OPEN]" } else { "[LICENSE]" };
                    resp_text.push_str(&format!(
                        "{}. <code>{}</code>\n   <b>{}</b> | {} — {}\n   Локация: {}\n",
                        i + 1,
                        escape_html(&s.ip),
                        tag,
                        escape_html(&version),
                        escape_html(&online),
                        escape_html(&loc)
                    ));
                    if s.rcon_pass.is_some() {
                        if user.active_tier() == "Prime" || db::is_admin(chat_id) {
                            resp_text.push_str("   <b>RCON: Открыт</b>\n");
                        } else {
                            resp_text.push_str("   <i>RCON: Доступен в тарифе Prime</i>\n");
                        }
                    }
                    if s.is_op {
                        resp_text.push_str("   <b>OP / Креатив: Включен</b>\n");
                    }
                    resp_text.push_str("\n");
                }
                if results.len() > 5 {
                    resp_text.push_str(&format!("<i>...и ещё {} серверов в базе.</i>\n", results.len() - 5));
                }
                resp_text.push_str("───────────────────────");
                let markup = json!({
                    "inline_keyboard": [
                        [{"text": "Настройки фильтров", "callback_data": "settings"}, {"text": "Главное меню", "callback_data": "menu"}]
                    ]
                });
                send_message_with_markup(client, token, chat_id, &resp_text, markup).await;
            }
        }
    } else if text.starts_with("/info") || text.starts_with("/faq") || text.starts_with("/help") || text.starts_with("/guide") || text == "Справка" || text == "📖 Справка" || text == "📖 Справка & FAQ" {
        send_help_menu(client, token, chat_id).await;
    } else if text.starts_with("/status") || text.starts_with("/ping") {
        let total_users = db::get_total_user_count();
        let active_feed_users = db::get_active_feed_count();
        let active_subs = db::get_active_sub_count();
        let all_servers_cnt = db::get_total_server_count();
        let bot_status_str = get_status();
        let status_card = format!(
            "<b>ДИАГНОСТИКА СИСТЕМЫ</b>\n\n<b>Статус сканера:</b> {}\n<b>Пользователей:</b> <b>{}</b>\n<b>Активных подписчиков:</b> <b>{}</b>\n<b>Премиум (Plus/Prime):</b> <b>{}</b>\n<b>База серверов:</b> <b>{}+</b>\n───────────────────────\n<i>Все подсистемы и сетевые узлы работают в штатном режиме.</i>",
            bot_status_str,
            total_users,
            active_feed_users,
            active_subs,
            all_servers_cnt
        );
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}, {"text": "Фильтры", "callback_data": "settings"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        send_message_with_markup(client, token, chat_id, &status_card, markup).await;
    } else if text.starts_with("/terms") || text.starts_with("/rules") || text.starts_with("/agreement") || text == "Пользовательское соглашение" || text == "📜 Пользовательское соглашение" || text == "Соглашение" {
        send_terms_menu(client, token, chat_id).await;
    } else if text.starts_with("/export") || text.starts_with("/dump") || text == "Выгрузить сервера" || text == "Выгрузка" {
        send_export_menu(client, token, chat_id).await;
    } else if text.starts_with("/admin_add") {
        if !db::is_admin(chat_id) && !db::get_admin_ids().is_empty() {
            send_message(client, token, chat_id, "❌ <b>Доступ запрещен.</b>").await;
            return;
        }
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(new_admin_id) = parts[1].parse::<i64>() {
                db::add_admin_id(new_admin_id);
                send_message(client, token, chat_id, &format!("✅ Пользователь <code>{}</code> добавлен в список администраторов.", new_admin_id)).await;
                log_event(&format!("👑 Добавлен новый администратор: ID {}", new_admin_id));
            }
        } else {
            send_message(client, token, chat_id, "ℹ️ Формат: <code>/admin_add &lt;chat_id&gt;</code>").await;
        }
    } else if text.starts_with("/sub") {
        if !db::is_admin(chat_id) {
            send_message(client, token, chat_id, "❌ <b>Доступ запрещен.</b> Команда доступна только администраторам.").await;
            return;
        }
        // Admin command: /sub <chat_id> <plus|prime> <days>
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 4 {
            if let (Ok(target_id), Ok(days)) = (parts[1].parse::<i64>(), parts[3].parse::<i64>()) {
                let tier = match parts[2].to_lowercase().as_str() {
                    "prime" | "exclusive" => "Prime",
                    "plus" | "standard" => "Plus",
                    _ => "Free",
                };
                match db::grant_subscription(target_id, tier, days) {
                    Ok(_) => {
                        log_event(&format!("👑 АДМИН: Выдана подписка {} на {} дн. юзеру ID {}", tier, days, target_id));
                        send_message(
                            client,
                            token,
                            chat_id,
                            &format!("✅ Подписка <b>{}</b> на <b>{} дней</b> успешно выдана пользователю <code>{}</code>", tier, days, target_id),
                        ).await;
                        send_message(
                            client,
                            token,
                            target_id,
                            &format!("🎁 <b>Администратор активировал вам подписку {} на {} дней!</b>", tier, days),
                        ).await;
                    }
                    Err(e) => {
                        send_message(client, token, chat_id, &format!("❌ Ошибка: {}", e)).await;
                    }
                }
            }
        } else {
            send_message(client, token, chat_id, "ℹ️ <b>Формат:</b> <code>/sub &lt;chat_id&gt; &lt;plus|prime&gt; &lt;days&gt;</code>").await;
        }
    } else if text.starts_with("/genpromo") {
        if !db::is_admin(chat_id) {
            send_message(client, token, chat_id, "❌ <b>Доступ запрещен.</b> Команда доступна только администраторам.").await;
            return;
        }
        // Admin command: /genpromo <plus|prime> <days> <uses> <code>
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 5 {
            let tier = match parts[1].to_lowercase().as_str() {
                "prime" | "exclusive" => "Prime",
                "plus" | "standard" => "Plus",
                _ => "Free",
            };
            if let (Ok(days), Ok(uses)) = (parts[2].parse::<i64>(), parts[3].parse::<i32>()) {
                let code = parts[4];
                match db::create_promocode(code, tier, days, uses) {
                    Ok(_) => {
                        log_event(&format!("🎟 АДМИН: Создан промокод '{}' ({}, {} дн., {} исп.)", code.to_uppercase(), tier, days, uses));
                        send_message(
                            client,
                            token,
                            chat_id,
                            &format!("✅ Промокод <code>{}</code> ({}, {} дн., {} исп.) успешно создан!", code.to_uppercase(), tier, days, uses),
                        ).await;
                    }
                    Err(e) => {
                        send_message(client, token, chat_id, &format!("❌ Ошибка создания: {}", e)).await;
                    }
                }
            }
        } else {
            send_message(client, token, chat_id, "ℹ️ <b>Формат:</b> <code>/genpromo &lt;plus|prime&gt; &lt;days&gt; &lt;uses&gt; &lt;CODE&gt;</code>").await;
        }
    } else if text.starts_with("/addbalance") {
        if !db::is_admin(chat_id) {
            send_message(client, token, chat_id, "❌ <b>Доступ запрещен.</b> Команда доступна только администраторам.").await;
            return;
        }
        // Admin command: /addbalance <chat_id> <amount>
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 3 {
            if let (Ok(target_id), Ok(amount)) = (parts[1].parse::<i64>(), parts[2].parse::<i64>()) {
                match db::add_user_balance(target_id, amount) {
                    Ok(new_bal) => {
                        log_event(&format!("💰 АДМИН: Пополнен баланс юзеру ID {} на {} руб. (Баланс: {})", target_id, amount, new_bal));
                        send_message(
                            client,
                            token,
                            chat_id,
                            &format!("✅ Баланс пользователя <code>{}</code> пополнен на {} руб. (Текущий баланс: {} руб.)", target_id, amount, new_bal),
                        ).await;
                        send_message(
                            client,
                            token,
                            target_id,
                            &format!("💰 <b>Ваш баланс пополнен на {} руб.!</b> Текущий баланс: <b>{} руб.</b>", amount, new_bal),
                        ).await;
                    }
                    Err(e) => {
                        send_message(client, token, chat_id, &format!("❌ Ошибка: {}", e)).await;
                    }
                }
            }
        } else {
            send_message(client, token, chat_id, "ℹ️ <b>Формат:</b> <code>/addbalance &lt;chat_id&gt; &lt;amount&gt;</code>").await;
        }
    } else if text.starts_with("/stars") || text.starts_with("/topup") {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(amount) = parts[1].parse::<i64>() {
                if amount >= 1 && amount <= 250000 {
                    log_event(&format!("⭐ Запрос произвольного инвойса Stars на {} ⭐ от ID {}", amount, chat_id));
                    send_stars_invoice(
                        client,
                        token,
                        chat_id,
                        &format!("Пополнение баланса (+{} ⭐)", amount),
                        &format!("Зачисление {} рублей/звезд на баланс", amount),
                        &format!("topup_stars_{}", amount),
                        amount,
                    )
                    .await;
                    return;
                }
            }
        }
        send_message(
            client,
            token,
            chat_id,
            "⭐ <b>Пополнение баланса Telegram Stars</b>\n\nВведите команду вместе с желаемой суммой:\n<code>/stars КОЛИЧЕСТВО</code> (например: <code>/stars 150</code> или <code>/stars 500</code>)\n\n<i>1 Star (⭐) = 1 рубль на баланс</i>",
        )
        .await;
    } else if text.starts_with("/stats") {
        let users = db::get_all_tg_users();
        let total = users.len();
        let active_subs: usize = users.iter().filter(|u| u.is_sub_active()).count();
        let prime_count: usize = users.iter().filter(|u| u.active_tier() == "Prime").count();
        let plus_count: usize = users.iter().filter(|u| u.active_tier() == "Plus").count();
        let free_count = total.saturating_sub(active_subs);

        let stats_msg = format!(
            "📊 <b>Статистика Telegram Бота</b>\n\n            👥 Всего пользователей: <b>{}</b>\n            ⭐ Активных Prime: <b>{}</b>\n            🟢 Активных Plus: <b>{}</b>\n            🔷 Бесплатных (Free): <b>{}</b>\n            🚀 Всего с платной подпиской: <b>{}</b>",
            total, prime_count, plus_count, free_count, active_subs
        );
        send_message(client, token, chat_id, &stats_msg).await;
    } else if let Ok(amount) = text.parse::<i64>() {
        // User typed a raw number -> treat as custom Stars amount topup!
        if amount >= 1 && amount <= 250000 {
            log_event(&format!("⭐ Запрос произвольного инвойса Stars на {} ⭐ от ID {}", amount, chat_id));
            send_stars_invoice(
                client,
                token,
                chat_id,
                &format!("Пополнение баланса (+{} ⭐)", amount),
                &format!("Зачисление {} рублей/звезд на баланс", amount),
                &format!("topup_stars_{}", amount),
                amount,
            )
            .await;
        } else {
            send_main_menu(client, token, &user).await;
        }
    } else {
        // Any text / greeting -> show main menu
        send_main_menu(client, token, &user).await;
    }
}

async fn handle_callback(client: &Client, token: &str, callback: &serde_json::Value) {
    let msg = match callback.get("message") {
        Some(m) => m,
        None => return,
    };
    let chat_id = match msg.get("chat").and_then(|c| c.get("id")).and_then(|id| id.as_i64()) {
        Some(id) => id,
        None => return,
    };
    let from_id = callback.get("from").and_then(|f| f.get("id")).and_then(|id| id.as_i64()).unwrap_or(0);

    if db::is_user_banned(chat_id) || (from_id != 0 && db::is_user_banned(from_id)) {
        return;
    }

    let callback_id = callback.get("id").and_then(|id| id.as_str()).unwrap_or("");
    if is_rate_limited(chat_id) {
        answer_callback_query(client, token, callback_id, Some("⏳ Пожалуйста, подождите..."), false).await;
        return;
    }
    let msg_id = msg.get("message_id").and_then(|id| id.as_i64()).unwrap_or(0);
    let data = callback.get("data").and_then(|d| d.as_str()).unwrap_or("");

    let mut user = db::get_or_create_tg_user(chat_id, "");
    log_event(&format!("🔘 Нажатие кнопки от ID {}: '{}'", chat_id, data));

    if data == "menu" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_main_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "start_menu" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_start_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "profile" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_profile(client, token, chat_id, msg_id, &user).await;
    } else if data == "plans" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_plans_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "plans_info" || data == "help" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_help_menu(client, token, chat_id, msg_id).await;
    } else if data == "terms" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_terms_menu(client, token, chat_id, msg_id).await;
    } else if data == "export_menu" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_export_menu(client, token, chat_id, msg_id).await;
    } else if data.starts_with("export:") {
        let period = data.strip_prefix("export:").unwrap_or("1h");
        answer_callback_query(client, token, callback_id, Some("Формируем выгрузку..."), false).await;
        handle_export_servers(client, token, chat_id, period).await;
    } else if data == "market" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_market_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "settings" || data == "filters" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data.starts_with("toggle_pool:") {
        let pool = data.strip_prefix("toggle_pool:").unwrap_or("");
        user.toggle_pool(pool);
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_free" {
        user.toggle_pool("free");
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_src_default" {
        user.src_default = !user.src_default;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_src_ngrok" {
        user.src_ngrok = !user.src_ngrok;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_src_playit" {
        user.src_playit = !user.src_playit;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_green" {
        user.filter_green = !user.filter_green;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_yellow" {
        user.filter_yellow = !user.filter_yellow;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_blue" {
        user.filter_blue = !user.filter_blue;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "buy_single_stars" {
        answer_callback_query(client, token, callback_id, Some("⭐ Выставлен счет на 15 Stars"), false).await;
        send_stars_invoice(
            client,
            token,
            chat_id,
            "1 сервер из Plus-пула",
            "Случайный проверенный сервер с игроками и постройками без вайтлиста",
            "buy_single_server_stars",
            15,
        ).await;
    } else if data == "buy_single_balance" {
        let cost = 20;
        match db::buy_single_server_with_balance(chat_id, cost) {
            Ok(srv) => {
                log_event(&format!("🛒 ШТУЧНЫЙ СЕРВЕР: ID {} купил сервер {} за 20 руб.", chat_id, srv.ip));
                answer_callback_query(client, token, callback_id, Some("🎉 Сервер успешно приобретен!"), false).await;

                let (version, online, location) = clean_parse_server_info(&srv.info);
                let mut online_p = 0;
                let mut max_p = 20;
                if let Some(slash_idx) = online.find('/') {
                    online_p = online[..slash_idx].trim().parse::<i32>().unwrap_or(0);
                    max_p = online[slash_idx + 1..].trim().parse::<i32>().unwrap_or(20);
                }
                let found_server = FoundServer {
                    ip: srv.ip.clone(),
                    info: srv.info.clone(),
                    version: version.clone(),
                    online_players: online_p,
                    max_players: max_p,
                    geo: location.clone(),
                    req_reason: String::new(),
                    is_cracked: srv.is_cracked,
                    is_registered: srv.is_registered,
                    is_deep_scan: false,
                    is_op: srv.is_op,
                    needs_mods: srv.needs_mods,
                    is_bungee_vulnerable: srv.info.contains("Bungee"),
                    is_velocity_vulnerable: srv.info.contains("Velocity"),
                    has_builds: srv.has_builds,
                    movement_restricted: srv.movement_restricted,
                    players: db::get_players_for_server(&srv.ip),
                    rcon_pass: srv.rcon_pass.clone(),
                    plugins: None,
                };

                let card = format_server_card_html(&found_server, false);
                let deliver_msg = format!("🎉 <b>ПОКУПКА УСПЕШНА (-20 ₽)!</b>\nВам выдан сервер из Plus-пула:\n\n{}", card);
                let markup = json!({
                    "inline_keyboard": [
                        [{"text": "📋 Скопировать IP", "callback_data": format!("copy_ip:{}", srv.ip)}],
                        [{"text": "🛒 Купить ещё (20₽)", "callback_data": "buy_single_balance"}, {"text": "◀ В главное меню", "callback_data": "menu"}]
                    ]
                });
                edit_message(client, token, chat_id, msg_id, &deliver_msg, markup).await;
            }
            Err(e) => {
                if e.contains("Недостаточно средств") {
                    let cur_bal = db::get_or_create_tg_user(chat_id, "").balance;
                    answer_callback_query(client, token, callback_id, Some("❌ Недостаточно средств на балансе!"), true).await;
                    let text = format!("❌ <b>Недостаточно средств на балансе!</b>\n\nВаш баланс: <b>{} руб.</b>\nСтоимость сервера: <b>20 руб.</b>\n\nПополните баланс через Stars, СБП или приобретите сервер за 15 Stars напрямую.", cur_bal);
                    let markup = json!({
                        "inline_keyboard": [
                            [{"text": "⭐ Купить за 15 Stars напрямую", "callback_data": "buy_single_stars"}],
                            [{"text": "💰 Пополнить баланс", "callback_data": "topup"}],
                            [{"text": "◀ Назад", "callback_data": "market"}]
                        ]
                    });
                    edit_message(client, token, chat_id, msg_id, &text, markup).await;
                } else {
                    answer_callback_query(client, token, callback_id, Some("⚠️ Нет свободных серверов в наличии."), true).await;
                }
            }
        }
    } else if data == "radar" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:ver" {
        user.hunt_ver = match user.hunt_ver.as_str() {
            "all" => "1.16.5".to_string(),
            "1.16.5" => "1.20".to_string(),
            "1.20" => "1.12.2".to_string(),
            _ => "all".to_string(),
        };
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:online" {
        user.hunt_online = (user.hunt_online + 1) % 4;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:builds" {
        user.hunt_builds = !user.hunt_builds;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:noreg" {
        user.hunt_noreg = !user.hunt_noreg;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:op" {
        user.hunt_op = !user.hunt_op;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_toggle:geo" {
        user.hunt_geo = match user.hunt_geo.as_str() {
            "all" => "ru".to_string(),
            "ru" => "eu".to_string(),
            "eu" => "us".to_string(),
            _ => "all".to_string(),
        };
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "hunt_reset" {
        db::reset_hunt_filters(chat_id);
        user = db::get_or_create_tg_user(chat_id, "");
        answer_callback_query(client, token, callback_id, Some("🔄 Фильтры сброшены!"), false).await;
        edit_radar_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "clan_api" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_clan_api_menu(client, token, chat_id, msg_id).await;
    } else if data == "ref" || data == "referral" {
        answer_callback_query(client, token, callback_id, None, false).await;
        edit_referral_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "daily_case" {
        answer_callback_query(client, token, callback_id, None, false).await;
        handle_daily_case_action(client, token, chat_id, Some(msg_id)).await;
    } else if data.starts_with("show_all_players:") {
        let srv_ip = data.trim_start_matches("show_all_players:").trim();
        let players = db::get_players_for_server(srv_ip);
        answer_callback_query(client, token, callback_id, None, false).await;
        if players.is_empty() {
            send_message(client, token, chat_id, &format!("👥 <b>Игроки сервера <code>{}</code> не найдены в базе.</b>", escape_html(srv_ip))).await;
        } else {
            let mut text = format!("👥 <b>СПИСОК ВСЕХ НАЙДЕННЫХ ИГРОКОВ (Всего: {})</b>\n<code>{}</code>\n━━━━━━━━━━\n", players.len(), escape_html(srv_ip));
            for (i, p) in players.iter().enumerate() {
                text.push_str(&format!("{}. <code>{}</code>\n", i + 1, escape_html(p)));
                if i >= 99 {
                    text.push_str(&format!("<i>...и ещё {} игроков</i>\n", players.len() - 100));
                    break;
                }
            }
            text.push_str("━━━━━━━━━━");
            let markup = json!({
                "inline_keyboard": [
                    [{"text": "📋 Скопировать IP", "callback_data": format!("copy_ip:{}", srv_ip)}],
                    [{"text": "◀ Закрыть", "callback_data": "delete_msg"}]
                ]
            });
            send_message_with_markup(client, token, chat_id, &text, markup).await;
        }
    } else if data == "delete_msg" {
        answer_callback_query(client, token, callback_id, None, false).await;
        let _ = client.post(format!("{}/bot{}/deleteMessage", get_api_base(), token))
            .json(&json!({"chat_id": chat_id, "message_id": msg_id}))
            .send().await;
    } else if data == "buy_sub:plus:30" || data == "buy_sub:standard:30" {
        let cost = 200;
        match db::purchase_subscription_with_balance(chat_id, "Plus", 30, cost) {
            Ok(_) => {
                log_event(&format!("🟢 ПОДПИСКА: ID {} оформил Plus (30д) с баланса", chat_id));
                answer_callback_query(client, token, callback_id, Some("Подписка Plus активирована!"), false).await;
                let text = "<b>ПОДПИСКА АКТИВИРОВАНА</b>\n\nПодписка <b>Plus</b> на 30 дней успешно активирована с вашего баланса (666 серверов/день).\nОткрытые серверы поступают в штатном режиме.\n───────────────────────";
                let markup = json!({
                    "inline_keyboard": [[{"text": "В профиль", "callback_data": "profile"}], [{"text": "Главное меню", "callback_data": "menu"}]]
                });
                edit_menu_with_photo_or_text(client, token, chat_id, msg_id, text, markup, "plans").await;
            }
            Err(_) => {
                let cur_bal = db::get_or_create_tg_user(chat_id, "").balance;
                answer_callback_query(client, token, callback_id, None, false).await;
                send_topup_prompt(client, token, chat_id, msg_id, cost, cur_bal).await;
            }
        }
    } else if data == "buy_sub:prime:30" {
        let cost = 400;
        match db::purchase_subscription_with_balance(chat_id, "Prime", 30, cost) {
            Ok(_) => {
                log_event(&format!("⭐ ПОДПИСКА: ID {} оформил Prime (30д) с баланса", chat_id));
                answer_callback_query(client, token, callback_id, Some("Подписка Prime активирована!"), false).await;
                let text = "<b>ПОДПИСКА АКТИВИРОВАНА</b>\n\nПодписка <b>Prime</b> на 30 дней успешно активирована с вашего баланса (безлимит).\nЭксклюзивный поток серверов строго в одни руки и чит-клиент открыты.\n───────────────────────";
                let markup = json!({
                    "inline_keyboard": [[{"text": "В профиль", "callback_data": "profile"}], [{"text": "Главное меню", "callback_data": "menu"}]]
                });
                edit_menu_with_photo_or_text(client, token, chat_id, msg_id, text, markup, "plans").await;
            }
            Err(_) => {
                let cur_bal = db::get_or_create_tg_user(chat_id, "").balance;
                answer_callback_query(client, token, callback_id, None, false).await;
                send_topup_prompt(client, token, chat_id, msg_id, cost, cur_bal).await;
            }
        }
    } else if data == "cheats_menu" || data == "client_menu" || data == "buy_cheats_balance" || data == "buy_cheats_stars" {
        answer_callback_query(client, token, callback_id, None, false).await;
        if user.active_tier() == "Prime" {
            deliver_cheats_file(client, token, chat_id).await;
        } else {
            edit_cheats_menu(client, token, chat_id, msg_id).await;
        }
    } else if data.starts_with("donatepay_pay:plus:") {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_donatepay_plan_invoice(
            client,
            token,
            chat_id,
            msg_id,
            "Plus",
            200,
            "plus",
        ).await;
    } else if data.starts_with("donatepay_pay:prime:") {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_donatepay_plan_invoice(
            client,
            token,
            chat_id,
            msg_id,
            "Prime",
            400,
            "prime",
        ).await;
    } else if data == "donatepay_topup:custom" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_donatepay_topup_invoice(client, token, chat_id, msg_id, None).await;
    } else if data.starts_with("donatepay_topup:") {
        let amt_str = data.strip_prefix("donatepay_topup:").unwrap_or("100");
        let amt = amt_str.parse::<i64>().unwrap_or(100);
        answer_callback_query(client, token, callback_id, None, false).await;
        send_donatepay_topup_invoice(client, token, chat_id, msg_id, Some(amt)).await;
    } else if data.starts_with("donatepay_check:") {
        answer_callback_query(client, token, callback_id, Some("Проверяем платежи в DonatePay..."), false).await;
        match donatepay::check_user_payments_now(chat_id).await {
            Ok(found) => {
                if found {
                    answer_callback_query(client, token, callback_id, Some("Оплата найдена и зачислена!"), true).await;
                    let u = db::get_or_create_tg_user(chat_id, "");
                    edit_profile(client, token, chat_id, msg_id, &u).await;
                } else {
                    answer_callback_query(
                        client,
                        token,
                        callback_id,
                        Some("Платеж пока не найден. Если вы уже оплатили, подождите 10-20 секунд и нажмите снова."),
                        true,
                    ).await;
                }
            }
            Err(e) => {
                answer_callback_query(client, token, callback_id, Some(&format!("Ошибка проверки: {}", e)), true).await;
            }
        }
    } else if data == "buy_stars:plus:30" || data == "buy_stars:standard:30" {
        answer_callback_query(client, token, callback_id, Some("Выставлен счет в Telegram Stars"), false).await;
        send_stars_invoice(
            client,
            token,
            chat_id,
            "Подписка Plus (30 дней)",
            "Поток открытых серверов (666 серверов/день) на 30 дней",
            "sub_plus_30",
            120,
        )
        .await;
    } else if data == "buy_stars:prime:30" {
        answer_callback_query(client, token, callback_id, Some("Выставлен счет в Telegram Stars"), false).await;
        send_stars_invoice(
            client,
            token,
            chat_id,
            "Подписка Prime (30 дней)",
            "Безлимитный поток серверов и эксклюзивные сервера в одни руки на 30 дней",
            "sub_prime_30",
            240,
        )
        .await;
    } else if data == "buy_stars:topup:50" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_stars_invoice(client, token, chat_id, "Пополнение баланса (+50 Stars)", "Зачисление 50 руб. на баланс бота", "topup_stars_50", 50).await;
    } else if data == "buy_stars:topup:100" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_stars_invoice(client, token, chat_id, "Пополнение баланса (+100 Stars)", "Зачисление 100 руб. на баланс бота", "topup_stars_100", 100).await;
    } else if data == "buy_stars:topup:250" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_stars_invoice(client, token, chat_id, "Пополнение баланса (+250 Stars)", "Зачисление 250 руб. на баланс бота", "topup_stars_250", 250).await;
    } else if data == "buy_stars:topup:500" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_stars_invoice(client, token, chat_id, "Пополнение баланса (+500 Stars)", "Зачисление 500 руб. на баланс бота", "topup_stars_500", 500).await;
    } else if data == "buy_stars:topup:1000" {
        answer_callback_query(client, token, callback_id, None, false).await;
        send_stars_invoice(client, token, chat_id, "Пополнение баланса (+1000 Stars)", "Зачисление 1000 руб. на баланс бота", "topup_stars_1000", 1000).await;
    } else if data == "topup_custom" {
        answer_callback_query(client, token, callback_id, None, false).await;
        let text = format_topup_custom_text(&user);
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Все варианты пополнения", "callback_data": "topup"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
    } else if data == "topup" {
        answer_callback_query(client, token, callback_id, None, false).await;
        let text = format_topup_text(&user);
        let markup = generate_topup_markup();
        edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
    } else if data == "promo_prompt" {
        answer_callback_query(client, token, callback_id, None, false).await;
        let text = "<b>АКТИВАЦИЯ ПРОМОКОДА</b>\n\nОтправьте в чат команду в формате:\n<code>/promo ВАШ_ПРОМОКОД</code>\n\nПодписка или баланс будут начислены моментально!\n───────────────────────\nПоддержка: @pisscaner_support";
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Тарифы", "callback_data": "plans"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
    } else if data == "toggle_feed" {
        user.active = !user.active;
        db::save_tg_user(&user);
        let toast = if user.active {
            "🟢 Рассылка включена!"
        } else {
            "🔴 Рассылка приостановлена"
        };
        answer_callback_query(client, token, callback_id, Some(toast), false).await;
        edit_settings_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "toggle_feed_start" {
        user.active = !user.active;
        db::save_tg_user(&user);
        let toast = if user.active {
            "🟢 Рассылка включена!"
        } else {
            "🔴 Рассылка приостановлена"
        };
        answer_callback_query(client, token, callback_id, Some(toast), false).await;
        edit_start_menu(client, token, chat_id, msg_id, &user).await;
    } else if data == "search_prompt" {
        answer_callback_query(client, token, callback_id, None, false).await;
        let text = "🔍 <b>Поиск по базе серверов</b>\n\nОтправьте команду с запросом:\n• <code>/search 1.20.1</code>\n• <code>/search rcon</code>\n• <code>/search anarchy</code>\n• <code>/search survival</code>\n\n<i>Или выберите быстрый поисковый запрос ниже:</i>";
        let markup = json!({
            "inline_keyboard": [
                [{"text": "1.20.1", "callback_data": "quick_search:1.20.1"}, {"text": "RCON", "callback_data": "quick_search:rcon"}],
                [{"text": "Anarchy", "callback_data": "quick_search:anarchy"}, {"text": "Survival", "callback_data": "quick_search:survival"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        send_message_with_markup(client, token, chat_id, text, markup).await;
    } else if data.starts_with("quick_search:") {
        let q = data.strip_prefix("quick_search:").unwrap_or("");
        answer_callback_query(client, token, callback_id, Some(&format!("🔍 Поиск «{}»...", q)), false).await;
        let results = db::search_servers(q);
        if results.is_empty() {
            let markup = json!({
                "inline_keyboard": [[{"text": "Главное меню", "callback_data": "menu"}]]
            });
            send_message_with_markup(
                client,
                token,
                chat_id,
                &format!("🔍 По запросу «<code>{}</code>» ничего не найдено в базе.", escape_html(q)),
                markup,
            ).await;
        } else {
            let top_results = &results[..results.len().min(5)];
            let mut resp_text = format!("🔍 <b>Результаты поиска по «{}» (Найдено: {}):</b>\n───────────────────────\n", escape_html(q), results.len());
            for (i, s) in top_results.iter().enumerate() {
                let (version, online, loc) = clean_parse_server_info(&s.info);
                let tag = if s.is_cracked { "[OPEN]" } else { "[LICENSE]" };
                resp_text.push_str(&format!(
                    "{}. <code>{}</code>\n   <b>{}</b> | {} — {}\n   Локация: {}\n",
                    i + 1,
                    escape_html(&s.ip),
                    tag,
                    escape_html(&version),
                    escape_html(&online),
                    escape_html(&loc)
                ));
                if s.rcon_pass.is_some() {
                    if user.active_tier() == "Prime" || db::is_admin(chat_id) {
                        resp_text.push_str("   <b>RCON: Открыт</b>\n");
                    } else {
                        resp_text.push_str("   <i>RCON: Доступен в тарифе Prime</i>\n");
                    }
                }
                if s.is_op {
                    resp_text.push_str("   <b>OP / Креатив: Включен</b>\n");
                }
                resp_text.push_str("\n");
            }
            if results.len() > 5 {
                resp_text.push_str(&format!("<i>...и ещё {} серверов в базе.</i>\n", results.len() - 5));
            }
            resp_text.push_str("───────────────────────");
            let markup = json!({
                "inline_keyboard": [
                    [{"text": "Настройки фильтров", "callback_data": "settings"}, {"text": "Главное меню", "callback_data": "menu"}]
                ]
            });
            send_message_with_markup(client, token, chat_id, &resp_text, markup).await;
        }
    } else if data == "pause_feed_card" {
        user.active = false;
        db::save_tg_user(&user);
        answer_callback_query(client, token, callback_id, Some("Рассылка приостановлена. Возобновить можно в настройках"), true).await;
    } else if data.starts_with("copy_ip:") {
        let ip = data.strip_prefix("copy_ip:").unwrap_or("");
        answer_callback_query(client, token, callback_id, Some(&format!("Скопировано: {}", ip)), false).await;
        send_message(
            client,
            token,
            chat_id,
            &format!("<b>IP адрес для копирования:</b>\n<code>{}</code>", escape_html(ip)),
        ).await;
    }
}

fn find_image_for_menu(menu_name: &str) -> Option<std::path::PathBuf> {
    let candidate_names = match menu_name {
        "start" => vec!["start.jpg", "start.png", "1.jpg", "1.png"],
        "main" => vec!["main.jpg", "main.png", "2.jpg", "2.png"],
        "profile" => vec!["profile.jpg", "profile.png", "3.jpg", "3.png"],
        "plans" => vec!["plans.jpg", "plans.png", "4.jpg", "4.png"],
        "settings" => vec!["settings.jpg", "settings.png", "5.jpg", "5.png"],
        "help" => vec!["help.jpg", "help.png", "6.jpg", "6.png"],
        "cheats" => vec!["cheats.jpg", "cheats.png", "7.jpg", "7.png"],
        "market" => vec!["market.jpg", "market.png", "8.jpg", "8.png"],
        "daily_case" => vec!["daily_case.jpg", "daily_case.png", "9.jpg", "9.png"],
        "referral" => vec!["referral.jpg", "referral.png", "10.jpg", "10.png"],
        "radar" => vec!["settings.jpg", "settings.png", "config.jpg", "radar.jpg", "radar.png", "5.jpg"],
        "terms" => vec!["terms.jpg", "terms.png", "12.jpg", "12.png"],
        "clan_api" => vec!["clan_api.jpg", "clan_api.png", "13.jpg", "13.png"],
        "banner" => vec!["banner.jpg", "banner.png", "14.jpg", "14.png"],
        _ => vec!["main.jpg", "1.jpg"],
    };

    let search_dirs = [
        std::path::PathBuf::from("2"),
        std::path::PathBuf::from("../2"),
        std::path::PathBuf::from("dank_scanner/2"),
    ];

    for dir in &search_dirs {
        for name in &candidate_names {
            let p = dir.join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

async fn send_stars_invoice(
    client: &Client,
    token: &str,
    chat_id: i64,
    title: &str,
    description: &str,
    payload: &str,
    amount_stars: i64,
) {
    let api_base = get_api_base();
    let url = format!("{}/bot{}/sendInvoice", api_base, token);
    let invoice_body = json!({
        "chat_id": chat_id,
        "title": title,
        "description": description,
        "payload": payload,
        "currency": "XTR",
        "prices": [
            { "label": title, "amount": amount_stars }
        ]
    });
    let _ = client.post(&url).json(&invoice_body).send().await;
}

async fn send_topup_prompt(
    client: &Client,
    token: &str,
    chat_id: i64,
    msg_id: i64,
    cost: i64,
    cur_bal: i64,
) {
    let diff = cost - cur_bal;
    let text = format!(
        "<b>НЕДОСТАТОЧНО СРЕДСТВ</b>\n\nСтоимость: <b>{} руб.</b>\nВаш баланс: <b>{} руб.</b>\nНе хватает: <b>{} руб.</b>\n\nПополните баланс удобным способом:\n───────────────────────",
        cost, cur_bal, diff
    );
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Пополнить баланс", "callback_data": "topup"}],
            [{"text": "Тарифы", "callback_data": "plans"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
}

fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

async fn send_photo_menu(
    client: &Client,
    token: &str,
    chat_id: i64,
    photo_path: &std::path::Path,
    caption: &str,
    markup: serde_json::Value,
) -> bool {
    let api_base = get_api_base();
    let url = format!("{}/bot{}/sendPhoto", api_base, token);

    if let Ok(photo_bytes) = tokio::fs::read(photo_path).await {
        let file_name = photo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image.jpg")
            .to_string();

        let mime = if file_name.ends_with(".png") {
            "image/png"
        } else if file_name.ends_with(".webp") {
            "image/webp"
        } else {
            "image/jpeg"
        };

        let safe_caption = safe_truncate_str(caption, 1020);

        let part = reqwest::multipart::Part::bytes(photo_bytes)
            .file_name(file_name)
            .mime_str(mime)
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", safe_caption.to_string())
            .text("parse_mode", "HTML")
            .text("reply_markup", markup.to_string())
            .part("photo", part);

        if let Ok(resp) = client.post(&url).multipart(form).send().await {
            if resp.status().is_success() {
                return true;
            }
        }
    }
    false
}

async fn send_menu_with_photo_or_text(
    client: &Client,
    token: &str,
    chat_id: i64,
    text: &str,
    markup: serde_json::Value,
    menu_name: &str,
) {
    if let Some(photo_path) = find_image_for_menu(menu_name) {
        if send_photo_menu(client, token, chat_id, &photo_path, text, markup.clone()).await {
            return;
        }
    }
    send_message_with_markup(client, token, chat_id, text, markup).await;
}

async fn edit_photo_media(
    client: &Client,
    token: &str,
    chat_id: i64,
    msg_id: i64,
    photo_path: &std::path::Path,
    caption: &str,
    markup: serde_json::Value,
) -> bool {
    let api_base = get_api_base();
    let url = format!("{}/bot{}/editMessageMedia", api_base, token);

    if let Ok(photo_bytes) = tokio::fs::read(photo_path).await {
        let file_name = photo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image.jpg")
            .to_string();

        let mime = if file_name.ends_with(".png") {
            "image/png"
        } else if file_name.ends_with(".webp") {
            "image/webp"
        } else {
            "image/jpeg"
        };

        let safe_caption = safe_truncate_str(caption, 1020);

        let media_obj = json!({
            "type": "photo",
            "media": "attach://photo_file",
            "caption": safe_caption,
            "parse_mode": "HTML"
        });

        let part = reqwest::multipart::Part::bytes(photo_bytes)
            .file_name(file_name)
            .mime_str(mime)
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("message_id", msg_id.to_string())
            .text("media", media_obj.to_string())
            .text("reply_markup", markup.to_string())
            .part("photo_file", part);

        if let Ok(resp) = client.post(&url).multipart(form).send().await {
            if resp.status().is_success() {
                return true;
            }
        }
    }
    false
}

async fn edit_menu_with_photo_or_text(
    client: &Client,
    token: &str,
    chat_id: i64,
    msg_id: i64,
    text: &str,
    markup: serde_json::Value,
    menu_name: &str,
) {
    if let Some(photo_path) = find_image_for_menu(menu_name) {
        if edit_photo_media(client, token, chat_id, msg_id, &photo_path, text, markup.clone()).await {
            return;
        }
    }
    edit_message(client, token, chat_id, msg_id, text, markup).await;
}

fn format_start_menu_text(user: &TgDbUser) -> String {
    let safe_uname = if user.username.is_empty() {
        "Аноним".to_string()
    } else {
        format!("@{}", escape_html(&user.username))
    };

    let feed_status = if user.active {
        "Активна (Включена)"
    } else {
        "На паузе (Отключена)"
    };

    let daily_cnt = db::get_user_daily_count(user.chat_id);
    let daily_limit_str = match user.active_tier().as_str() {
        "Prime" => format!("{}/∞", daily_cnt),
        "Plus" | "Standard" => format!("{}/666", daily_cnt),
        _ => format!("{}/100", daily_cnt),
    };

    format!(
        "<b>PIS SCANNER</b> — Система мониторинга и анализа серверов Minecraft\n\n<b>Пользователь:</b> {} [ID: <code>{}</code>]\n<b>Тариф:</b> <b>{}</b>\n<b>Получено сегодня:</b> <b>{}</b>\n<b>Рассылка:</b> {}\n───────────────────────\nАвтоматический перебор и анализ серверов в реальном времени.\n\nРассылка изначально отключена. Включите поток кнопкой ниже:\n───────────────────────\nПоддержка: @pisscaner_support",
        safe_uname,
        user.chat_id,
        user.active_tier(),
        daily_limit_str,
        feed_status
    )
}

fn generate_start_menu_markup(user: &TgDbUser) -> serde_json::Value {
    let feed_btn = if user.active {
        json!({"text": "Приостановить поток (Пауза)", "callback_data": "toggle_feed_start"})
    } else {
        json!({"text": "Запустить поток серверов", "callback_data": "toggle_feed_start"})
    };

    json!({
        "inline_keyboard": [
            [feed_btn],
            [{"text": "Профиль", "callback_data": "profile"}, {"text": "Тарифы", "callback_data": "plans"}],
            [{"text": "Фильтры", "callback_data": "settings"}, {"text": "Справка", "callback_data": "help"}]
        ]
    })
}

async fn send_start_menu(client: &Client, token: &str, user: &TgDbUser) {
    let text = format_start_menu_text(user);
    let markup = generate_start_menu_markup(user);
    send_menu_with_photo_or_text(client, token, user.chat_id, &text, markup, "start").await;
}

async fn edit_start_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_start_menu_text(user);
    let markup = generate_start_menu_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "start").await;
}

async fn send_main_menu(client: &Client, token: &str, user: &TgDbUser) {
    let text = format_main_menu_text(user);
    let markup = generate_main_menu_markup(user);
    send_menu_with_photo_or_text(client, token, user.chat_id, &text, markup, "main").await;
}

async fn edit_main_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_main_menu_text(user);
    let markup = generate_main_menu_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "main").await;
}

fn format_main_menu_text(user: &TgDbUser) -> String {
    let tier_badge = match user.active_tier().as_str() {
        "Prime" => "Prime [Безлимит]",
        "Plus" | "Standard" => "Plus [666/день]",
        _ => "Free [100/день]",
    };

    let safe_uname = if user.username.is_empty() {
        "Аноним".to_string()
    } else {
        format!("@{}", escape_html(&user.username))
    };

    let status_str = if user.active {
        "Активна (Включена)"
    } else {
        "На паузе (Отключена)"
    };

    let daily_cnt = db::get_user_daily_count(user.chat_id);
    let daily_limit_str = match user.active_tier().as_str() {
        "Prime" => format!("{}/∞", daily_cnt),
        "Plus" | "Standard" => format!("{}/666", daily_cnt),
        _ => format!("{}/100", daily_cnt),
    };

    format!(
        "<b>PIS SCANNER</b> — Главное меню\n\n<b>Пользователь:</b> <b>{}</b> [ID: <code>{}</code>]\n<b>Тариф:</b> <b>{}</b>\n<b>Получено сегодня:</b> <b>{}</b>\n<b>Баланс:</b> <b>{} руб.</b>\n<b>Рассылка:</b> {}\n───────────────────────\nВыберите раздел:",
        safe_uname, user.chat_id, tier_badge, daily_limit_str, user.balance, status_str
    )
}

fn generate_main_menu_markup(user: &TgDbUser) -> serde_json::Value {
    let feed_toggle_btn = if user.active {
        json!({"text": "Приостановить рассылку", "callback_data": "toggle_feed"})
    } else {
        json!({"text": "Включить рассылку", "callback_data": "toggle_feed"})
    };

    json!({
        "inline_keyboard": [
            [{"text": "Профиль", "callback_data": "profile"}, {"text": "Тарифы", "callback_data": "plans"}],
            [{"text": "Фильтры", "callback_data": "settings"}, {"text": "Справка", "callback_data": "help"}],
            [feed_toggle_btn]
        ]
    })
}

fn generate_profile_markup(user: &TgDbUser) -> serde_json::Value {
    let mut kb = vec![
        vec![
            json!({"text": "Пополнить баланс", "callback_data": "topup"}),
            json!({"text": "Ежедневный кейс", "callback_data": "daily_case"}),
        ],
        vec![
            json!({"text": "Купить сервер (20 руб)", "callback_data": "market"}),
            json!({"text": "Рефералы", "callback_data": "ref"}),
        ],
        vec![
            json!({"text": "Ввести промокод", "callback_data": "promo_prompt"}),
            json!({"text": "Тарифы", "callback_data": "plans"}),
        ],
        vec![
            json!({"text": "Выгрузить сервера (TXT)", "callback_data": "export_menu"}),
        ],
    ];

    if user.active_tier() == "Prime" {
        kb.push(vec![
            json!({"text": "Получить чит-клиент (Prime)", "callback_data": "cheats_menu"})
        ]);
    }

    kb.push(vec![
        json!({"text": "Поддержка", "url": "https://t.me/pisscaner_support"}),
        json!({"text": "Главное меню", "callback_data": "menu"}),
    ]);

    json!({"inline_keyboard": kb})
}

async fn send_profile(client: &Client, token: &str, user: &TgDbUser) {
    let text = format_profile_text(user);
    let markup = generate_profile_markup(user);
    send_menu_with_photo_or_text(client, token, user.chat_id, &text, markup, "profile").await;
}

async fn edit_profile(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_profile_text(user);
    let markup = generate_profile_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "profile").await;
}

fn format_profile_text(user: &TgDbUser) -> String {
    let active_tier = user.active_tier();
    let expiry_str = if user.is_sub_active() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days_left = (user.sub_expires_at - now) / 86400;
        format!("Активна (осталось {} дн.)", days_left.max(1))
    } else {
        "Бесплатный доступ".to_string()
    };

    let safe_uname = if user.username.is_empty() {
        "Аноним".to_string()
    } else {
        format!("@{}", escape_html(&user.username))
    };

    let daily_cnt = db::get_user_daily_count(user.chat_id);
    let limit_str = match active_tier.as_str() {
        "Prime" => format!("{}/∞", daily_cnt),
        "Plus" => format!("{}/666", daily_cnt),
        _ => format!("{}/100", daily_cnt),
    };

    format!(
        "<b>ПРОФИЛЬ ПОЛЬЗОВАТЕЛЯ</b>\n\nID: <code>{}</code>\nИмя: {}\nТариф: <b>{}</b>\nСрок действия: {}\nПолучено сегодня: <b>{}</b>\nБаланс: <b>{} руб.</b>\nРассылка: {}\n\n<b>Реферальная программа:</b>\n• Приглашено друзей: <b>{}</b> чел.\n• Заработано: <b>{} руб.</b>\n───────────────────────",
        user.chat_id,
        safe_uname,
        active_tier,
        expiry_str,
        limit_str,
        user.balance,
        if user.active { "Включена" } else { "Отключена (Пауза)" },
        user.ref_count,
        user.ref_earned
    )
}

async fn send_plans_menu(client: &Client, token: &str, chat_id: i64) {
    let user = db::get_or_create_tg_user(chat_id, "");
    let text = format_plans_text(&user);
    let markup = generate_plans_markup(&user);
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "plans").await;
}

async fn edit_plans_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_plans_text(user);
    let markup = generate_plans_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
}

fn format_plans_text(user: &TgDbUser) -> String {
    format!(
        "<b>ТАРИФНЫЕ ПЛАНЫ И ПОДПИСКА</b>\nБаланс: <b>{} руб.</b>\n\n<b>[FREE] — 0 руб. (100 серверов/день)</b>\n• Общий поток серверов (ограничения, лицензии, 50+ слотов)\n\n<b>[PLUS] — 200 руб. (120 Stars) / 30 дней (666 серверов/день)</b>\n• Plus-пул: <b>ТОЛЬКО открытые серверы</b> со слотами <b>до 49 игроков</b>\n• Настраиваемые фильтры туннелей IPv4 / Ngrok / Playit\n\n<b>[PRIME] — 400 руб. (240 Stars) / 30 дней (Безлимит)</b>\n• <b>Эксклюзивные открытые серверы строго в одни руки</b> (слоты до 49)\n• Серверы 1-5 онлайн, ники игроков, RCON, OP, форвардинг\n• Доступ ко всем пулам одновременно\n• <b>В стоимость включен боевой чит-клиент и софт для грифа</b>\n───────────────────────\nПоддержка: @pisscaner_support",
        user.balance
    )
}

fn generate_plans_markup(user: &TgDbUser) -> serde_json::Value {
    let mut kb = vec![
        vec![
            json!({"text": "Plus (200 руб - СБП/Карта)", "callback_data": "donatepay_pay:plus:200"}),
            json!({"text": "Plus (120 Stars)", "callback_data": "buy_stars:plus:30"}),
        ],
        vec![
            json!({"text": "Prime (400 руб - СБП/Карта)", "callback_data": "donatepay_pay:prime:400"}),
            json!({"text": "Prime (240 Stars)", "callback_data": "buy_stars:prime:30"}),
        ],
        vec![
            json!({"text": "Plus с баланса (200 руб)", "callback_data": "buy_sub:plus:30"}),
            json!({"text": "Prime с баланса (400 руб)", "callback_data": "buy_sub:prime:30"}),
        ],
    ];

    if user.active_tier() == "Prime" {
        kb.push(vec![
            json!({"text": "Скачать чит-клиент (Включен в Prime)", "callback_data": "cheats_menu"})
        ]);
    } else {
        kb.push(vec![
            json!({"text": "Чит-клиент (Включен в Prime)", "callback_data": "cheats_menu"})
        ]);
    }

    kb.push(vec![
        json!({"text": "API для кланов", "callback_data": "clan_api"}),
        json!({"text": "Промокод", "callback_data": "promo_prompt"}),
    ]);
    kb.push(vec![
        json!({"text": "Пополнить баланс", "callback_data": "topup"}),
        json!({"text": "Главное меню", "callback_data": "menu"}),
    ]);

    json!({"inline_keyboard": kb})
}

fn format_topup_text(user: &TgDbUser) -> String {
    format!(
        "<b>ПОПОЛНЕНИЕ БАЛАНСА</b>\nБаланс: <b>{} руб.</b>\n\nВыберите способ пополнения:\n\n• <b>СБП / Карты РФ / Кошельки:</b> через DonatePay\n• <b>Telegram Stars:</b> моментальная оплата в приложении\n• <b>Промокоды:</b> команда <code>/promo КОД</code>\n• <b>Поддержка:</b> @pisscaner_support\n───────────────────────",
        user.balance
    )
}

fn generate_topup_markup() -> serde_json::Value {
    json!({
        "inline_keyboard": [
            [{"text": "100 руб (СБП/Карта)", "callback_data": "donatepay_topup:100"}, {"text": "100 Stars (100 руб)", "callback_data": "buy_stars:topup:100"}],
            [{"text": "250 руб (СБП/Карта)", "callback_data": "donatepay_topup:250"}, {"text": "250 Stars (250 руб)", "callback_data": "buy_stars:topup:250"}],
            [{"text": "500 руб (СБП/Карта)", "callback_data": "donatepay_topup:500"}, {"text": "500 Stars (500 руб)", "callback_data": "buy_stars:topup:500"}],
            [{"text": "1000 руб (СБП/Карта)", "callback_data": "donatepay_topup:1000"}, {"text": "1000 Stars (1000 руб)", "callback_data": "buy_stars:topup:1000"}],
            [{"text": "Любая сумма (СБП/Карта)", "callback_data": "donatepay_topup:custom"}, {"text": "Своя сумма Stars", "callback_data": "topup_custom"}],
            [{"text": "Активировать промокод", "callback_data": "promo_prompt"}],
            [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

fn format_topup_custom_text(user: &TgDbUser) -> String {
    format!(
        "<b>ПОПОЛНЕНИЕ БАЛАНСА // СВОЯ СУММА</b>\nБаланс: <b>{} руб.</b>\n\nОтправьте в этот чат желаемое количество Telegram Stars (например, <code>50</code>, <code>150</code>, <code>777</code>, <code>2500</code>) или команду:\n<code>/stars 150</code>\n\n<i>Курс: 1 Telegram Star = 1 рубль на баланс бота.\nВы можете указать любую сумму от 1 до 250 000 Stars.</i>\n───────────────────────",
        user.balance
    )
}

async fn send_cheats_menu(client: &Client, token: &str, chat_id: i64) {
    let user = db::get_or_create_tg_user(chat_id, "");
    if user.active_tier() == "Prime" {
        deliver_cheats_file(client, token, chat_id).await;
        return;
    }

    let text = format!(
        "<b>ЧИТ-КЛИЕНТ И МОДЫ ДЛЯ ГРИФА</b>\n\nБоевая сборка клиентского софта и инструментов для эффективного исследования и грифа серверов Minecraft.\n\n<b>Содержимое сборки:</b>\n• Настроенный клиент под радар PIS\n• Моды для смены ников и обхода проверок\n• Инструкция по быстрой установке\n\n<b>Доступ:</b> <b>Включен в тариф Prime</b> (400 руб. / 240 Stars на 30 дней).\n\nВаш баланс: <b>{} руб.</b>\nОформите подписку Prime, чтобы мгновенно получить чит-клиент и эксклюзивные серверы!",
        user.balance
    );
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Оформить Prime (240 Stars)", "callback_data": "buy_stars:prime:30"}, {"text": "Оформить Prime (400 руб)", "callback_data": "buy_sub:prime:30"}],
            [{"text": "Все тарифы", "callback_data": "plans"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "cheats").await;
}

async fn edit_cheats_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64) {
    let user = db::get_or_create_tg_user(chat_id, "");
    if user.active_tier() == "Prime" {
        deliver_cheats_file(client, token, chat_id).await;
        return;
    }

    let text = format!(
        "<b>ЧИТ-КЛИЕНТ И МОДЫ ДЛЯ ГРИФА</b>\n\nБоевая сборка клиентского софта и инструментов для эффективного исследования и грифа серверов Minecraft.\n\n<b>Содержимое сборки:</b>\n• Настроенный клиент под радар PIS\n• Моды для смены ников и обхода проверок\n• Инструкция по быстрой установке\n\n<b>Доступ:</b> <b>Включен в тариф Prime</b> (400 руб. / 240 Stars на 30 дней).\n\nВаш баланс: <b>{} руб.</b>\nОформите подписку Prime, чтобы мгновенно получить чит-клиент и эксклюзивные серверы!",
        user.balance
    );
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Оформить Prime (240 Stars)", "callback_data": "buy_stars:prime:30"}, {"text": "Оформить Prime (400 руб)", "callback_data": "buy_sub:prime:30"}],
            [{"text": "Все тарифы", "callback_data": "plans"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "cheats").await;
}

async fn deliver_cheats_file(client: &Client, token: &str, chat_id: i64) {
    let candidate_dirs = [
        std::path::PathBuf::from("cheats"),
        std::path::PathBuf::from("../cheats"),
        std::path::PathBuf::from("dank_scanner/cheats"),
    ];

    let mut found_file = None;
    for cheats_dir in &candidate_dirs {
        if cheats_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(cheats_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        found_file = Some(path);
                        break;
                    }
                }
            }
        }
        if found_file.is_some() {
            break;
        }
    }

    if let Some(file_path) = found_file {
        if let Ok(file_bytes) = std::fs::read(&file_path) {
            let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("grief_pack.zip").to_string();
            let part = reqwest::multipart::Part::bytes(file_bytes)
                .file_name(file_name);
            let form = reqwest::multipart::Form::new()
                .text("chat_id", chat_id.to_string())
                .text("caption", "<b>Чит-клиент успешно отправлен!</b>\nАрхив включен в вашу подписку Prime.\nРаспакуйте архив и следуйте инструкции.")
                .text("parse_mode", "HTML")
                .part("document", part);

            let url = format!("{}/bot{}/sendDocument", get_api_base(), token);
            let _ = client.post(&url).multipart(form).send().await;
            return;
        }
    }

    send_message(
        client,
        token,
        chat_id,
        "<b>Чит-клиент включен в вашу подписку Prime!</b>\n\n📁 Архив с читами в данный момент загружается на сервер.\nКак только файл появится в папке, он будет отправлен вам, либо напишите в поддержку: @pisscaner_support",
    ).await;
}

fn format_help_text() -> String {
    "<b>СПРАВКА И ДОКУМЕНТАЦИЯ</b>\n\n<b>PIS SCANNER</b> — система поиска, мониторинга и анализа серверов Minecraft.\n\n<b>Полезные моды:</b>\n• <b>ViaFabricPlus</b> — вход на любые версии 1.0–1.21.x\n• <b>Account Switcher</b> — быстрая смена ников\n• <b>Meteor Client</b> — навигация и поиск баз\n• <b>FreeCam</b> — свободная камера\n\n<b>Типы серверов:</b>\n• <b>Открытый:</b> свободный вход без вайтлиста и модов\n• <b>Ограничения:</b> вайтлист, регистрация или моды\n• <b>Лицензия:</b> вход только с аккаунта Mojang\n\n<b>Пулы:</b>\n• <b>Plus:</b> проверенные открытые серверы (до 49 слотов)\n• <b>Prime:</b> эксклюзивные серверы строго в одни руки\n• <b>Free:</b> общий поток серверов\n\n<b>Уязвимости:</b>\n• <b>RCON пароль:</b> прямой доступ к админ-консоли\n• <b>Bungee/Velocity bypass:</b> вход под любым ником\n───────────────────────\nПоддержка: @pisscaner_support".to_string()
}

fn generate_help_markup() -> serde_json::Value {
    json!({
        "inline_keyboard": [
            [{"text": "Пользовательское соглашение", "callback_data": "terms"}],
            [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

async fn send_help_menu(client: &Client, token: &str, chat_id: i64) {
    let text = format_help_text();
    let markup = generate_help_markup();
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "help").await;
}

async fn edit_help_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64) {
    let text = format_help_text();
    let markup = generate_help_markup();
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "help").await;
}

fn format_settings_text(user: &TgDbUser) -> String {
    let tier = user.active_tier();
    let mut active_pools = Vec::new();
    if user.has_pool("exclusive") { active_pools.push("Prime Exclusive"); }
    if user.has_pool("plus") { active_pools.push("Plus пул"); }
    if user.has_pool("free") { active_pools.push("Free пул"); }
    let pool_desc = if active_pools.is_empty() {
        "Free пул".to_string()
    } else {
        active_pools.join(" + ")
    };

    let mut text = format!(
        "<b>НАСТРОЙКИ ФИЛЬТРОВ И РАССЫЛКИ</b>\n\n<b>Тариф:</b> <b>{}</b>\n<b>Активные пулы:</b> <b>{}</b>\n<b>Статус рассылки:</b> {}\n───────────────────────\n",
        tier,
        pool_desc,
        if user.active { "Включена" } else { "На паузе" }
    );

    let is_prime = tier == "Prime";
    let is_paid = tier != "Free";

    if is_prime {
        text.push_str("\n<b>Выбор пулов серверов (Prime):</b>\n<i>(Prime и Plus пулы выдают ТОЛЬКО открытые серверы до 49 слотов)</i>\n");
    } else if is_paid {
        text.push_str("\n<b>Выбор пулов серверов (Plus):</b>\n<i>(Plus пул выдает ТОЛЬКО открытые серверы до 49 слотов)</i>\n");
    } else {
        text.push_str("\n<i>На тарифе Free выдается общий Free пул. Для доступа к Plus и Prime перейдите в /plans</i>\n");
    }

    if is_paid {
        text.push_str("\n<b>Источники и туннели:</b>\n");
    }

    text.push_str("\n<b>Фильтры типов серверов:</b>\n");
    text
}

fn generate_settings_markup(user: &TgDbUser) -> serde_json::Value {
    let tier = user.active_tier();
    let is_paid = tier != "Free";
    let is_prime = tier == "Prime";

    let mut kb: Vec<Vec<serde_json::Value>> = Vec::new();

    // 1. Master toggle
    let active_btn = if user.active { "[✓] Рассылка: ВКЛ" } else { "[✗] Рассылка: ВЫКЛ (Пауза)" };
    kb.push(vec![json!({"text": active_btn, "callback_data": "toggle_feed"})]);

    // 2. Custom radar & search
    kb.push(vec![
        json!({"text": "Охота по фильтрам (Радар)", "callback_data": "radar"}),
        json!({"text": "Поиск по базе", "callback_data": "search_prompt"})
    ]);

    // 3. Pool Selection (Multi-select)
    if is_prime {
        let btn_excl = if user.has_pool("exclusive") { "[✓] Prime Exclusive (до 49)" } else { "[✗] Prime Exclusive (до 49)" };
        let btn_plus = if user.has_pool("plus") { "[✓] Plus пул (до 49)" } else { "[✗] Plus пул (до 49)" };
        let btn_free = if user.has_pool("free") { "[✓] Free пул (Все)" } else { "[✗] Free пул (Все)" };

        kb.push(vec![json!({"text": btn_excl, "callback_data": "toggle_pool:exclusive"})]);
        kb.push(vec![
            json!({"text": btn_plus, "callback_data": "toggle_pool:plus"}),
            json!({"text": btn_free, "callback_data": "toggle_pool:free"})
        ]);
    } else if is_paid {
        let btn_plus = if user.has_pool("plus") { "[✓] Plus пул (до 49)" } else { "[✗] Plus пул (до 49)" };
        let btn_free = if user.has_pool("free") { "[✓] Free пул (Все)" } else { "[✗] Free пул (Все)" };

        kb.push(vec![
            json!({"text": btn_plus, "callback_data": "toggle_pool:plus"}),
            json!({"text": btn_free, "callback_data": "toggle_free"})
        ]);
    }

    // 4. Source & Tunnel Selection
    if is_paid {
        let def_btn = if user.src_default { "[✓] IPv4" } else { "[✗] IPv4" };
        let ngr_btn = if user.src_ngrok { "[✓] Ngrok" } else { "[✗] Ngrok" };
        let ply_btn = if user.src_playit { "[✓] Playit.gg" } else { "[✗] Playit.gg" };

        kb.push(vec![
            json!({"text": def_btn, "callback_data": "toggle_src_default"}),
            json!({"text": ngr_btn, "callback_data": "toggle_src_ngrok"}),
            json!({"text": ply_btn, "callback_data": "toggle_src_playit"})
        ]);
    }

    // 5. Content Filters
    let green_btn = if user.filter_green { "[✓] Открытые серверы" } else { "[✗] Открытые серверы" };
    let yellow_btn = if user.filter_yellow { "[✓] С ограничениями" } else { "[✗] С ограничениями" };
    let blue_btn = if user.filter_blue { "[✓] Лицензионные" } else { "[✗] Лицензионные" };

    kb.push(vec![json!({"text": green_btn, "callback_data": "toggle_green"})]);
    kb.push(vec![
        json!({"text": yellow_btn, "callback_data": "toggle_yellow"}),
        json!({"text": blue_btn, "callback_data": "toggle_blue"})
    ]);

    kb.push(vec![
        json!({"text": "Главное меню", "callback_data": "menu"})
    ]);

    json!({"inline_keyboard": kb})
}

async fn edit_settings_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_settings_text(user);
    let markup = generate_settings_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "settings").await;
}

async fn send_settings_menu_direct(client: &Client, token: &str, chat_id: i64, user: &TgDbUser) {
    let text = format_settings_text(user);
    let markup = generate_settings_markup(user);
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "settings").await;
}

async fn send_promo_prompt(client: &Client, token: &str, chat_id: i64) {
    let text = "<b>АКТИВАЦИЯ ПРОМОКОДА</b>\n\nВведите промокод в чат командой:\n<code>/promo ВАШ_ПРОМОКОД</code>\n\nПодписка или баланс будут начислены моментально!\n───────────────────────\nПоддержка: @pisscaner_support";
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Тарифы", "callback_data": "plans"}],
            [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    send_menu_with_photo_or_text(client, token, chat_id, text, markup, "plans").await;
}

async fn edit_message(client: &Client, token: &str, chat_id: i64, msg_id: i64, text: &str, markup: serde_json::Value) {
    let api_base = get_api_base();
    let safe_caption = safe_truncate_str(text, 1020);

    let url_caption = format!("{}/bot{}/editMessageCaption", api_base, token);
    if let Ok(resp) = client
        .post(&url_caption)
        .json(&json!({
            "chat_id": chat_id,
            "message_id": msg_id,
            "caption": safe_caption,
            "parse_mode": "HTML",
            "reply_markup": markup
        }))
        .send()
        .await
    {
        if resp.status().is_success() {
            return;
        }
    }

    let url_text = format!("{}/bot{}/editMessageText", api_base, token);
    if let Ok(resp) = client
        .post(&url_text)
        .json(&json!({
            "chat_id": chat_id,
            "message_id": msg_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": markup
        }))
        .send()
        .await
    {
        if resp.status().is_success() {
            return;
        }
    }

    // Ultimate fallback: delete old message and send fresh one so buttons never get stuck!
    let _ = client.post(format!("{}/bot{}/deleteMessage", api_base, token))
        .json(&json!({"chat_id": chat_id, "message_id": msg_id}))
        .send().await;
    send_message_with_markup(client, token, chat_id, text, markup).await;
}

async fn send_message(client: &Client, token: &str, chat_id: i64, text: &str) {
    let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
    let _ = client
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML"
        }))
        .send()
        .await;
}

async fn send_message_with_markup(client: &Client, token: &str, chat_id: i64, text: &str, markup: serde_json::Value) {
    let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
    let _ = client
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "reply_markup": markup
        }))
        .send()
        .await;
}

pub async fn send_direct_message(chat_id: i64, text: &str) {
    let raw_token = {
        if let Some(t) = BOT_TOKEN.get() {
            t.lock().map(|l| l.clone()).unwrap_or_default()
        } else {
            return;
        }
    };
    let token = clean_token(&raw_token);
    if token.is_empty() {
        return;
    }
    let client = get_http_client();
    send_message(&client, &token, chat_id, text).await;
}

#[allow(dead_code)]
pub async fn send_direct_message_with_markup(chat_id: i64, text: &str, markup: serde_json::Value) {
    let raw_token = {
        if let Some(t) = BOT_TOKEN.get() {
            t.lock().map(|l| l.clone()).unwrap_or_default()
        } else {
            return;
        }
    };
    let token = clean_token(&raw_token);
    if token.is_empty() {
        return;
    }
    let client = get_http_client();
    send_message_with_markup(&client, &token, chat_id, text, markup).await;
}

async fn send_donatepay_plan_invoice(
    client: &Client,
    token: &str,
    chat_id: i64,
    msg_id: i64,
    tier_name: &str,
    amount_rub: i64,
    action: &str,
) {
    let pay_url = donatepay::get_payment_url(chat_id, action, amount_rub as f64);
    let comment_str = format!("tg_{}_{}", chat_id, action);

    let text = format!(
        "<b>ОПЛАТА ПОДПИСКИ ЧЕРЕЗ DONATEPAY</b>\n\
        Способы: СБП, Банковские карты РФ, Кошельки\n\n\
        Тариф: <b>{}</b>\n\
        Срок действия: <b>30 дней</b>\n\
        Сумма к оплате: <b>{} руб.</b>\n\n\
        <b>ИНСТРУКЦИЯ ПО ОПЛАТЕ:</b>\n\
        1. Нажмите кнопку <b>«Перейти к оплате ({} руб)»</b> ниже.\n\
        2. В поле <b>«Сумма пожертвования»</b> укажите:\n\
        <code>{}</code>\n\
        3. В поле <b>«Сообщение стримеру»</b> вставьте:\n\
        <code>{}</code>\n\n\
        <i>(По этому сообщению бот распознает ваш перевод и сразу выдаст подписку).</i>\n\n\
        4. После оплаты на сайте нажмите кнопку <b>«Проверить оплату»</b> (или подождите 10 секунд для автозачисления).\n\
        ───────────────────────",
        tier_name, amount_rub, amount_rub, amount_rub, comment_str
    );

    let markup = json!({
        "inline_keyboard": [
            [{"text": format!("Перейти к оплате ({} руб)", amount_rub), "url": pay_url}],
            [{"text": "Проверить оплату", "callback_data": format!("donatepay_check:{}", chat_id)}],
            [{"text": "Назад к тарифам", "callback_data": "plans"}]
        ]
    });

    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
}

async fn send_donatepay_topup_invoice(
    client: &Client,
    token: &str,
    chat_id: i64,
    msg_id: i64,
    suggested_amount: Option<i64>,
) {
    let pay_url = donatepay::get_payment_url(chat_id, "topup", suggested_amount.unwrap_or(100) as f64);
    let comment_str = format!("tg_{}", chat_id);

    let amount_desc = if let Some(amt) = suggested_amount {
        format!("укажите <code>{}</code> (или любую другую сумму)", amt)
    } else {
        "укажите любую желаемую сумму (например, <code>100</code>, <code>250</code>, <code>500</code>)".to_string()
    };

    let btn_text = if let Some(amt) = suggested_amount {
        format!("Перейти к оплате ({} руб)", amt)
    } else {
        "Перейти к оплате".to_string()
    };

    let text = format!(
        "<b>ПОПОЛНЕНИЕ БАЛАНСА ЧЕРЕЗ DONATEPAY</b>\n\
        Способы: СБП, Банковские карты РФ, Кошельки\n\n\
        <b>ИНСТРУКЦИЯ ПО ПОПОЛНЕНИЮ:</b>\n\
        1. Нажмите кнопку <b>«{}»</b> ниже (откроется страница оплаты DonatePay).\n\
        2. В поле <b>«Сумма пожертвования»</b> {}.\n\
        3. В поле <b>«Сообщение стримеру»</b> ОБЯЗАТЕЛЬНО вставьте ваш ID:\n\
        <code>{}</code>\n\n\
        <i>(Бот зачислит переведенную сумму 1 к 1 на ваш баланс в боте).</i>\n\n\
        4. После оплаты вернитесь в бот и нажмите <b>«Проверить оплату»</b>.\n\
        ───────────────────────",
        btn_text, amount_desc, comment_str
    );

    let markup = json!({
        "inline_keyboard": [
            [{"text": btn_text, "url": pay_url}],
            [{"text": "Проверить оплату", "callback_data": format!("donatepay_check:{}", chat_id)}],
            [{"text": "Назад к пополнению", "callback_data": "topup"}]
        ]
    });

    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "plans").await;
}

fn get_bottom_reply_keyboard() -> serde_json::Value {
    json!({
        "keyboard": [
            [{"text": "Главное меню"}, {"text": "Профиль"}],
            [{"text": "Вкл/Выкл рассылку"}, {"text": "Фильтры"}]
        ],
        "resize_keyboard": true,
        "is_persistent": true
    })
}

async fn send_bottom_keyboard_if_needed(client: &Client, token: &str, chat_id: i64) {
    let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
    let _ = client
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": "<b>PIS SCANNER</b> — Меню управления:",
            "parse_mode": "HTML",
            "reply_markup": get_bottom_reply_keyboard()
        }))
        .send()
        .await;
}

// ── ОХОТА ПО ФИЛЬТРАМ (РАДАР) ──

fn format_radar_text(user: &TgDbUser) -> String {
    let ver_label = if user.hunt_ver == "all" { "Любая" } else { &user.hunt_ver };
    let online_label = match user.hunt_online {
        1 => "> 0 игроков",
        2 => "1 — 20 игроков",
        3 => "20+ игроков",
        _ => "Любой",
    };
    let builds_label = if user.hunt_builds { "[✓] Только с постройками" } else { "Все" };
    let noreg_label = if user.hunt_noreg { "[✓] Без регистрации" } else { "Все" };
    let op_label = if user.hunt_op { "[✓] Только OP / Креатив" } else { "Все" };
    let geo_label = match user.hunt_geo.as_str() {
        "ru" | "RU" => "RU & СНГ",
        "eu" | "EU" => "Европа",
        "us" | "US" => "США & Америка",
        _ => "Весь мир",
    };

    format!(
        "<b>ОХОТА ПО ФИЛЬТРАМ (РАДАР)</b>\n\n<b>Версия:</b> <code>{}</code>\n<b>Онлайн:</b> <code>{}</code>\n<b>Постройки:</b> <code>{}</code>\n<b>Регистрация:</b> <code>{}</code>\n<b>Права OP:</b> <code>{}</code>\n<b>Локация:</b> <code>{}</code>\n───────────────────────\n<i>Радар фильтрует входящий поток серверов по выбранным критериям:</i>",
        ver_label, online_label, builds_label, noreg_label, op_label, geo_label
    )
}

fn generate_radar_markup(user: &TgDbUser) -> serde_json::Value {
    let ver_btn = match user.hunt_ver.as_str() {
        "1.16.5" => "Версия: 1.16.5",
        "1.20" => "Версия: 1.20+",
        "1.12.2" => "Версия: 1.12.2",
        _ => "Версия: Любая",
    };
    let online_btn = match user.hunt_online {
        1 => "Онлайн: > 0",
        2 => "Онлайн: 1–20",
        3 => "Онлайн: 20+",
        _ => "Онлайн: Любой",
    };
    let builds_btn = if user.hunt_builds { "[✓] Постройки: ВКЛ" } else { "[✗] Постройки: ВЫКЛ" };
    let noreg_btn = if user.hunt_noreg { "[✓] Без рег.: ВКЛ" } else { "[✗] Без рег.: ВЫКЛ" };
    let op_btn = if user.hunt_op { "[✓] Только OP: ВКЛ" } else { "[✗] Только OP: ВЫКЛ" };
    let geo_btn = match user.hunt_geo.as_str() {
        "ru" => "Гео: RU",
        "eu" => "Гео: EU",
        "us" => "Гео: US",
        _ => "Гео: Все",
    };

    json!({
        "inline_keyboard": [
            [{"text": ver_btn, "callback_data": "hunt_toggle:ver"}, {"text": online_btn, "callback_data": "hunt_toggle:online"}],
            [{"text": builds_btn, "callback_data": "hunt_toggle:builds"}, {"text": noreg_btn, "callback_data": "hunt_toggle:noreg"}],
            [{"text": op_btn, "callback_data": "hunt_toggle:op"}, {"text": geo_btn, "callback_data": "hunt_toggle:geo"}],
            [{"text": "Сбросить фильтры", "callback_data": "hunt_reset"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

async fn send_radar_menu_direct(client: &Client, token: &str, chat_id: i64, user: &TgDbUser) {
    let text = format_radar_text(user);
    let markup = generate_radar_markup(user);
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "radar").await;
}

async fn edit_radar_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_radar_text(user);
    let markup = generate_radar_markup(user);
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "radar").await;
}

// ── API ДЛЯ КЛАНОВ ──

fn format_clan_api_text() -> String {
    "<b>API ДЛЯ КЛАНОВ И КОМАНД</b>\n\nПрямой поток данных для кланов, гриферских команд и разработчиков:\n\n• <b>Webhook / WebSocket:</b> моментальная доставка серверов прямо в Discord / Telegram / базу\n• <b>Экспорт данных:</b> автоматическая генерация <code>servers.dat</code>, списков Meteor/Wurst\n• <b>База игроков:</b> история ников и уязвимостей со всех просканированных нод\n• <b>Выделенный канал:</b> приватный пул без задержек\n\n───────────────────────\n<b>Подключение:</b>\nВыдача API-ключей и вебхуков осуществляется через поддержку:\n👉 <b>@pisscaner_support</b>".to_string()
}

async fn send_clan_api_menu_direct(client: &Client, token: &str, chat_id: i64) {
    let text = format_clan_api_text();
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Связаться с поддержкой для подключения", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "clan_api").await;
}

async fn edit_clan_api_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64) {
    let text = format_clan_api_text();
    let markup = json!({
        "inline_keyboard": [
            [{"text": "Связаться с поддержкой для подключения", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "clan_api").await;
}

// ── РЕФЕРАЛЬНАЯ СИСТЕМА ──

fn format_referral_text(user: &TgDbUser) -> String {
    let uname = get_bot_username();
    let ref_link = format!("https://t.me/{}?start=r_{}", uname, user.chat_id);
    format!(
        "<b>РЕФЕРАЛЬНАЯ ПРОГРАММА</b>\n\nПриглашайте друзей и получайте бесплатные подписки и рубли на баланс!\n\n<b>Ваша ссылка для приглашения:</b>\n<code>{}</code>\n\n<b>Бонусы:</b>\n• <b>Друг:</b> <b>+1 день Plus-подписки</b> сразу при переходе\n• <b>Вы:</b> <b>+10 руб. на баланс</b> за каждого приглашённого\n\n<b>Статистика:</b>\n• Приглашено друзей: <b>{}</b> чел.\n• Заработано: <b>{} руб.</b>\n───────────────────────",
        ref_link, user.ref_count, user.ref_earned
    )
}

fn generate_referral_markup() -> serde_json::Value {
    json!({
        "inline_keyboard": [
            [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

async fn send_referral_menu_direct(client: &Client, token: &str, chat_id: i64, user: &TgDbUser) {
    let text = format_referral_text(user);
    let markup = generate_referral_markup();
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "referral").await;
}

async fn edit_referral_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_referral_text(user);
    let markup = generate_referral_markup();
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "referral").await;
}

// ── ШТУЧНАЯ ПОКУПКА СЕРВЕРОВ (15 STARS / 20 RUB) ──

fn format_market_text(user: &TgDbUser) -> String {
    format!(
        "<b>ШТУЧНАЯ ПОКУПКА СЕРВЕРОВ</b>\n\nПокупка <b>1 проверенного сервера из Plus-пула</b>:\n\n• Открытый пиратский сервер (без вайтлиста)\n• Слоты до 49 игроков, наличие построек и игроков\n• Ники игроков и проверка уязвимостей\n\n<b>Стоимость:</b>\n• <b>15 Stars</b> (в приложении Telegram)\n• <b>20 руб.</b> (с баланса бота: <b>{} руб.</b>)\n───────────────────────",
        user.balance
    )
}

fn generate_market_markup() -> serde_json::Value {
    json!({
        "inline_keyboard": [
            [{"text": "Купить за 15 Stars", "callback_data": "buy_single_stars"}],
            [{"text": "Купить за 20 руб (с баланса)", "callback_data": "buy_single_balance"}],
            [{"text": "Пополнить баланс", "callback_data": "topup"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

async fn send_market_menu_direct(client: &Client, token: &str, chat_id: i64, user: &TgDbUser) {
    let text = format_market_text(user);
    let markup = generate_market_markup();
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "market").await;
}

async fn edit_market_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64, user: &TgDbUser) {
    let text = format_market_text(user);
    let markup = generate_market_markup();
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "market").await;
}

// ── ЕЖЕДНЕВНЫЙ БЕСПЛАТНЫЙ КЕЙС ──

async fn handle_daily_case_action(client: &Client, token: &str, chat_id: i64, msg_id: Option<i64>) {
    match db::claim_daily_case(chat_id) {
        Ok(srv) => {
            log_event(&format!("🎁 ЕЖЕДНЕВНЫЙ КЕЙС: ID {} открыл бесплатный кейс и получил {}", chat_id, srv.ip));
            let (version, online, location) = clean_parse_server_info(&srv.info);
            let mut online_p = 0;
            let mut max_p = 20;
            if let Some(slash_idx) = online.find('/') {
                online_p = online[..slash_idx].trim().parse::<i32>().unwrap_or(0);
                max_p = online[slash_idx + 1..].trim().parse::<i32>().unwrap_or(20);
            }
            let found_server = FoundServer {
                ip: srv.ip.clone(),
                info: srv.info.clone(),
                version: version.clone(),
                online_players: online_p,
                max_players: max_p,
                geo: location.clone(),
                req_reason: String::new(),
                is_cracked: srv.is_cracked,
                is_registered: srv.is_registered,
                is_deep_scan: false,
                is_op: srv.is_op,
                needs_mods: srv.needs_mods,
                is_bungee_vulnerable: srv.info.contains("Bungee"),
                is_velocity_vulnerable: srv.info.contains("Velocity"),
                has_builds: srv.has_builds,
                movement_restricted: srv.movement_restricted,
                players: db::get_players_for_server(&srv.ip),
                rcon_pass: srv.rcon_pass.clone(),
                plugins: None,
            };

            let card = format_server_card_html(&found_server, false);
            let message_text = format!("<b>ЕЖЕДНЕВНЫЙ БЕСПЛАТНЫЙ КЕЙС</b>\n\nВам выдан проверенный сервер из Plus-пула:\n\n{}", card);
            let markup = json!({
                "inline_keyboard": [
                    [{"text": "Скопировать IP", "callback_data": format!("copy_ip:{}", srv.ip)}],
                    [{"text": "Главное меню", "callback_data": "menu"}]
                ]
            });
            if let Some(mid) = msg_id {
                edit_menu_with_photo_or_text(client, token, chat_id, mid, &message_text, markup, "daily_case").await;
            } else {
                send_menu_with_photo_or_text(client, token, chat_id, &message_text, markup, "daily_case").await;
            }
        }
        Err(err_msg) => {
            let message_text = format!("<b>ЕЖЕДНЕВНЫЙ БЕСПЛАТНЫЙ КЕЙС</b>\n\n{}\n\n<i>Возвращайтесь каждый день за новым бесплатным сервером!</i>", err_msg);
            let markup = json!({
                "inline_keyboard": [
                    [{"text": "Купить сервер штучно (20 руб)", "callback_data": "market"}],
                    [{"text": "Главное меню", "callback_data": "menu"}]
                ]
            });
            if let Some(mid) = msg_id {
                edit_menu_with_photo_or_text(client, token, chat_id, mid, &message_text, markup, "daily_case").await;
            } else {
                send_menu_with_photo_or_text(client, token, chat_id, &message_text, markup, "daily_case").await;
            }
        }
    }
}

fn get_full_terms_document() -> String {
    "==============================================================================
                ПОЛЬЗОВАТЕЛЬСКОЕ СОГЛАШЕНИЕ И ПУБЛИЧНАЯ ОФЕРТА
                         СЕРВИСА «PIS SCANNER»
==============================================================================
Дата последнего обновления: 2026 год
Редакция: 2.4-RELEASE

ВНИМАНИЕ: ПЕРЕД ИСПОЛЬЗОВАНИЕМ СЕРВИСА, ТЕЛЕГРАМ-БОТА ИЛИ ОФОРМЛЕНИЕМ ЛЮБЫХ
ПОДПИСОК ВНИМАТЕЛЬНО ОЗНАКОМЬТЕСЬ С НАСТОЯЩИМ СОГЛАШЕНИЕМ. НАЖАТИЕ КНОПКИ «СТАРТ»,
ПОПОЛНЕНИЕ БАЛАНСА, ВВОД ПРОМОКОДА ИЛИ ИСПОЛЬЗОВАНИЕ ПОЛУЧЕННЫХ ДАННЫХ ЯВЛЯЕТСЯ
ПОЛНЫМ И БЕЗОГОВОРОЧНЫМ АКЦЕПТОМ (ПРИНЯТИЕМ) НАСТОЯЩЕЙ ПУБЛИЧНОЙ ОФЕРТЫ.

------------------------------------------------------------------------------
1. ПРЕДМЕТ СОГЛАШЕНИЯ И ПРИРОДА СЕРВИСА
------------------------------------------------------------------------------
1.1. Сервис «PIS Scanner» (далее — «Сервис», «Исполнитель») представляет собой
автоматизированную информационную систему сетевого мониторинга общедоступных
публичных IPv4-адресов в сети Интернет.
1.2. Сервис осуществляет стандартные запросы опроса состояния (Server List Ping)
к открытым сетевым портам протокола Minecraft Java Edition в строгом соответствии
с общепринятыми сетевыми спецификациями.
1.3. Сервис не производит взлом, модификацию стороннего программного обеспечения,
несанкционированный доступ или нарушение целостности чужих компьютерных систем.
Вся информация собирается исключительно из открытых сетевых ответов.

------------------------------------------------------------------------------
2. ЦИФРОВОЙ КОНТЕНТ И УСЛОВИЯ ОПЛАТЫ (NO REFUND POLICY)
------------------------------------------------------------------------------
2.1. Все услуги Сервиса (тарифные планы Plus, Prime, штучная покупка серверов,
API-доступ, подписки за Telegram Stars и рубли) являются нематериальным цифровым
контентом мгновенного исполнения.
2.2. Обязательства Сервиса считаются полностью исполненными в момент зачисления
цифровой подписки или выдачи данных в диалоге Телеграм-бота.
2.3. ПОЛИТИКА ОТСУТСТВИЯ ВОЗВРАТОВ (NO REFUND):
     Ввиду мгновенного потребления цифрового контента, возврат денежных средств
     (Refund), отмена транзакций или перерасчёт не производятся ни при каких
     обстоятельствах, включая:
     - Ошибочный выбор тарифа Пользователем;
     - Неудовлетворённость внутриигровым наполнением найденных серверов;
     - Изменение планов Пользователя;
     - Блокировку аккаунта Пользователя за нарушение правил.
2.4. Пополнение баланса через сторонние платежные шлюзы (DonatePay, Crypto,
Telegram Stars) осуществляется Пользователем добровольно.

------------------------------------------------------------------------------
3. ТЕХНИЧЕСКИЕ ОСОБЕННОСТИ И СТАТУСЫ СЕРВЕРОВ
------------------------------------------------------------------------------
3.1. Категоризация серверов («Открытый», «Вайтлист», «Лицензия», «Моды»,
«Bungee/Velocity bypass», «RCON») формируется автоматически алгоритмом сканера.
3.2. В связи с наличием на сторонних серверах кастомных плагинов, прокси-авторизаций,
BungeeCord-эмуляторов и нестандартных античитов, Сервис допускает наличие
статистической погрешности и не гарантирует 100% точность внутриигровых статусов.
3.3. Сервис не гарантирует постоянное наличие конкретных игроков или построек
на обнаруженных серверах в момент фактического входа Пользователя в игру.

------------------------------------------------------------------------------
4. ОГРАНИЧЕНИЕ ОТВЕТСТВЕННОСТИ (DISCLAIMER)
------------------------------------------------------------------------------
4.1. Сервис предоставляет информацию по принципу «КАК ЕСТЬ» («AS IS»).
4.2. Исполнитель не несёт ответственности за любые прямые или косвенные действия
Пользователя на найденных серверах, а также за любые внутриигровые конфликты,
баны аккаунтов или блокировки IP-адресов сторонними администраторами.
4.3. Пользователь единолично несёт полную ответственность за соблюдение
законодательства своей юрисдикции при использовании полученных IP-адресов.

------------------------------------------------------------------------------
5. ПРАВИЛА ПОЛЬЗОВАНИЯ И ЗАПРЕТЫ
------------------------------------------------------------------------------
5.1. Пользователю категорически запрещается:
     - Осуществлять попытки дестабилизации, DDoS-атак или спама на инфраструктуру бота;
     - Перепродавать доступ к боту без предварительного согласования с администрацией;
     - Использовать автоматизированные скрипты для парсинга вывода бота в обход лимитов.
5.2. При нарушении п. 5.1 администрация вправе немедленно заблокировать доступ
Пользователя без компенсации оплаченных услуг.

------------------------------------------------------------------------------
6. ЗАКЛЮЧИТЕЛЬНЫЕ ПОЛОЖЕНИЯ И ПОДДЕРЖКА
------------------------------------------------------------------------------
6.1. Сервис оставляет за собой право вносить изменения в настоящее соглашение.
Актуальная версия документа всегда доступна в Telegram-боте.
6.2. Официальный контакт технической поддержки: @pisscaner_support
==============================================================================".to_string()
}

async fn send_terms_menu(client: &Client, token: &str, chat_id: i64) {
    send_terms_document(client, token, chat_id).await;
}

async fn edit_terms_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64) {
    let api_base = get_api_base();
    let _ = client.post(format!("{}/bot{}/deleteMessage", api_base, token))
        .json(&json!({"chat_id": chat_id, "message_id": msg_id}))
        .send().await;
    send_terms_document(client, token, chat_id).await;
}

async fn send_terms_document(client: &Client, token: &str, chat_id: i64) {
    let doc_content = get_full_terms_document();
    let api_base = get_api_base();
    let url = format!("{}/bot{}/sendDocument", api_base, token);

    let part = reqwest::multipart::Part::bytes(doc_content.into_bytes())
        .file_name("PIS_SCANNER_USER_AGREEMENT.txt")
        .mime_str("text/plain; charset=utf-8")
        .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

    let markup = json!({
        "inline_keyboard": [
            [{"text": "Тарифные планы", "callback_data": "plans"}],
            [{"text": "Поддержка", "url": "https://t.me/pisscaner_support"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });

    let caption = "📄 <b>ПОЛЬЗОВАТЕЛЬСКОЕ СОГЛАШЕНИЕ И ОФЕРТА</b>\n\nПолный юридический текст соглашения сервиса <b>PIS SCANNER</b> прикреплён в текстовом файле выше.\n\nИспользуя сервис, бота или приобретая подписку, вы безоговорочно принимаете все пункты данного соглашения.\n───────────────────────\nПоддержка: @pisscaner_support";

    let form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", caption.to_string())
        .text("parse_mode", "HTML".to_string())
        .text("reply_markup", markup.to_string())
        .part("document", part);

    let _ = client.post(&url).multipart(form).send().await;
}

async fn send_export_menu(client: &Client, token: &str, chat_id: i64) {
    let text = format_export_menu_text();
    let markup = generate_export_markup();
    send_menu_with_photo_or_text(client, token, chat_id, &text, markup, "export").await;
}

async fn edit_export_menu(client: &Client, token: &str, chat_id: i64, msg_id: i64) {
    let text = format_export_menu_text();
    let markup = generate_export_markup();
    edit_menu_with_photo_or_text(client, token, chat_id, msg_id, &text, markup, "export").await;
}

fn format_export_menu_text() -> String {
    "<b>ВЫГРУЗКА НАЙДЕННЫХ СЕРВЕРОВ (TXT)</b>\n\nСерверы экспортируются в текстовый файл строго по вашим активным фильтрам (только те, которые подходят под ваши настройки и отправлялись вам).\n\n<b>Формат строк в файле:</b>\n<code>IP:PORT/Ник_1/Ник_2/Ник_n...</code>\n\nВыберите период выгрузки:".to_string()
}

fn generate_export_markup() -> serde_json::Value {
    json!({
        "inline_keyboard": [
            [{"text": "За последний 1 час", "callback_data": "export:1h"}, {"text": "За последние 24 часа", "callback_data": "export:24h"}],
            [{"text": "За последние 30 дней", "callback_data": "export:30d"}, {"text": "За все время", "callback_data": "export:all"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    })
}

async fn handle_export_servers(client: &Client, token: &str, chat_id: i64, period: &str) {
    let user = db::get_or_create_tg_user(chat_id, "");
    let (content, count) = db::export_user_servers_txt(chat_id, period, &user);
    if count == 0 {
        let msg = "<b>Серверы не найдены</b>\n\nЗа выбранный период по вашим активным фильтрам нет записей. Попробуйте выбрать более широкий интервал (например, 24 часа или за все время).";
        let markup = json!({
            "inline_keyboard": [
                [{"text": "Выбрать другой период", "callback_data": "export_menu"}],
                [{"text": "Главное меню", "callback_data": "menu"}]
            ]
        });
        send_menu_with_photo_or_text(client, token, chat_id, msg, markup, "export").await;
        return;
    }

    let (period_label, filename) = match period {
        "1h" => ("Последний 1 час", "servers_1hour.txt"),
        "24h" => ("Последние 24 часа", "servers_24hours.txt"),
        "30d" => ("Последние 30 дней", "servers_30days.txt"),
        _ => ("За все время", "servers_all_time.txt"),
    };

    let caption = format!(
        "<b>ВЫГРУЗКА СЕРВЕРОВ MINECRAFT</b>\n\nПериод: <b>{}</b>\nКоличество серверов: <b>{}</b>\nФильтры: <b>по вашим настройкам</b>\nФормат строк: <code>IP/Ник1/Ник2...</code>\n───────────────────────\nПоддержка: @pisscaner_support",
        period_label, count
    );

    let markup = json!({
        "inline_keyboard": [
            [{"text": "Выгрузить другой период", "callback_data": "export_menu"}],
            [{"text": "Главное меню", "callback_data": "menu"}]
        ]
    });

    let api_base = get_api_base();
    let url = format!("{}/bot{}/sendDocument", api_base, token);

    let part = reqwest::multipart::Part::bytes(content.into_bytes())
        .file_name(filename.to_string())
        .mime_str("text/plain; charset=utf-8")
        .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

    let form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", caption)
        .text("parse_mode", "HTML".to_string())
        .text("reply_markup", markup.to_string())
        .part("document", part);

    let _ = client.post(&url).multipart(form).send().await;
}

fn user_accepts_source(user: &TgDbUser, is_playit: bool, is_ngrok: bool, is_default: bool) -> bool {
    if user.active_tier() == "Free" {
        return true;
    }
    if is_playit && !user.src_playit {
        return false;
    }
    if is_ngrok && !user.src_ngrok {
        return false;
    }
    if is_default && !user.src_default {
        return false;
    }
    true
}

pub fn clean_parse_server_info(info: &str) -> (String, String, String) {
    let parts: Vec<&str> = info.split('|').map(|s| s.trim()).collect();

    let mut version = "Java".to_string();
    let mut online = "0/20".to_string();
    let mut location = "Global".to_string();

    if parts.len() >= 2 {
        let v_part = parts[1];
        if let Some(idx) = v_part.find('(') {
            let v = v_part[..idx].trim();
            if !v.is_empty() {
                version = v.to_string();
            }
        } else {
            let v = v_part.trim();
            if !v.is_empty() {
                version = v.to_string();
            }
        }
    }

    if parts.len() >= 3 {
        let on_part = parts[2];
        let cleaned_online = on_part
            .replace("online", "")
            .replace("players", "")
            .replace(' ', "")
            .trim()
            .to_string();
        if !cleaned_online.is_empty() {
            online = cleaned_online;
        }
    }

    if parts.len() >= 4 {
        let loc_part = parts[3];
        if !loc_part.trim().is_empty() {
            location = loc_part.trim().to_string();
        }
    }

    (version, online, location)
}

fn format_server_card_html(
    server: &FoundServer,
    is_exclusive_prime: bool,
) -> String {
    let color = get_server_color(server);

    let mut msg = String::new();

    // ── 1 БЛОК: Статус и IP ──
    match color {
        ServerColor::Green => {
            if is_exclusive_prime {
                msg.push_str(&format!("[PRIME ЭКСКЛЮЗИВ] <code>{}</code>\n<b>СТАТУС:</b> <b>ОТКРЫТЫЙ СЕРВЕР [ЭКСКЛЮЗИВ]</b>\n", escape_html(&server.ip)));
            } else {
                msg.push_str(&format!("[ОТКРЫТЫЙ СЕРВЕР] <code>{}</code>\n<b>СТАТУС:</b> <b>ОТКРЫТЫЙ СЕРВЕР</b>\n", escape_html(&server.ip)));
            }
        }
        ServerColor::Yellow => {
            msg.push_str(&format!("[С ОГРАНИЧЕНИЯМИ] <code>{}</code>\n<b>СТАТУС:</b> <b>СЕРВЕР С ОГРАНИЧЕНИЯМИ</b>\n", escape_html(&server.ip)));
        }
        ServerColor::Blue => {
            msg.push_str(&format!("[ЛИЦЕНЗИЯ] <code>{}</code>\n<b>СТАТУС:</b> <b>ЛИЦЕНЗИОННЫЙ СЕРВЕР</b>\n", escape_html(&server.ip)));
        }
    }

    msg.push_str("───────────────────────\n");

    // ── 2 БЛОК: Версия и Онлайн ──
    let clean_ver = if server.version.is_empty() { "Java" } else { &server.version };
    msg.push_str(&format!("<b>Версия:</b> {}\n", escape_html(clean_ver)));
    msg.push_str(&format!("<b>Онлайн:</b> {}/{}\n", server.online_players, server.max_players));

    msg.push_str("───────────────────────\n");

    // ── 3 БЛОК: Игроки ──
    if !server.players.is_empty() {
        let first_5: Vec<&str> = server.players.iter().take(5).map(|s| s.as_str()).collect();
        let players_str = first_5.join(", ");
        if server.players.len() > 5 {
            msg.push_str(&format!("<b>Игроки:</b> {} <i>(+ ещё {})</i>\n", escape_html(&players_str), server.players.len() - 5));
        } else {
            msg.push_str(&format!("<b>Игроки:</b> {}\n", escape_html(&players_str)));
        }
    } else {
        msg.push_str("<b>Игроки:</b> null\n");
    }

    if server.is_op {
        msg.push_str("<b>Режим OP / Creative: Обнаружен</b>\n");
    }

    if is_exclusive_prime && server.online_players >= 1 && server.online_players <= 5 {
        msg.push_str("<i>Подсказка: заходите под никами этих игроков</i>\n");
    }

    msg.push_str("───────────────────────\n");

    // ── 4 БЛОК: Теги и Локация ──
    let tags_bracket = if let Some(start) = server.info.find('(') {
        if let Some(end) = server.info.find(')') {
            if end > start {
                &server.info[start..=end]
            } else {
                ""
            }
        } else {
            ""
        }
    } else {
        ""
    };

    let loc = if server.geo.is_empty() { "Global" } else { &server.geo };
    if !tags_bracket.is_empty() {
        msg.push_str(&format!("<b>Локация:</b> {} {}\n", escape_html(loc), escape_html(tags_bracket)));
    } else {
        msg.push_str(&format!("<b>Локация:</b> {}\n", escape_html(loc)));
    }

    // Причины для restrictions / license
    match color {
        ServerColor::Green => {}
        ServerColor::Yellow => {
            let mut reasons = Vec::new();
            let is_whitelist = (server.info.contains("bs,") && !server.info.contains("nobs,"))
                || server.info.contains("[Вайтлист]");
            let is_modded = server.needs_mods
                || server.info.contains("[Требуются моды")
                || server.info.to_lowercase().contains("neoforge")
                || server.info.to_lowercase().contains("forge")
                || server.info.to_lowercase().contains("fabric");
            let is_reg = server.info.contains("(reg,")
                || server.info.contains(", reg,")
                || server.info.contains("требуется регистрация")
                || (server.is_registered && !server.info.contains("noreg"));
            let is_mov_rest = server.movement_restricted || server.info.contains("movrest");

            if is_whitelist {
                reasons.push("Вайтлист");
            }
            if is_modded {
                reasons.push("Моды");
            }
            if is_reg {
                reasons.push("Регистрация");
            }
            if is_mov_rest {
                reasons.push("Передвижение ограничено");
            }
            if reasons.is_empty() {
                if server.info.contains("?bs") {
                    reasons.push("Не подтвержден вход");
                } else {
                    reasons.push("Ограничения входа");
                }
            }
            msg.push_str(&format!("<b>Причина:</b> {}\n", escape_html(&reasons.join(", "))));
        }
        ServerColor::Blue => {
            msg.push_str("<b>Причина:</b> Лицензия Mojang\n");
        }
    }

    // Уязвимости
    if let Some(ref pass) = server.rcon_pass {
        msg.push_str(&format!("<b>RCON пароль:</b> <code>{}</code>\n", escape_html(pass)));
        msg.push_str("<i>Подключитесь через RCON-клиент (mcrcon) на IP сервера для ввода команд админа.</i>\n");
    }
    if server.is_bungee_vulnerable {
        msg.push_str("<b>Уязвимость: BungeeCord IP-Forwarding Bypass</b>\n");
        msg.push_str("<i>Отключен прокси-файрвол. Доступен вход под ником админа без пароля.</i>\n");
    }
    if server.is_velocity_vulnerable {
        msg.push_str("<b>Уязвимость: Velocity Forwarding Bypass</b>\n");
        msg.push_str("<i>Уязвимость форвардинга Velocity. Доступен обход авторизации.</i>\n");
    }
    if server.is_op {
        msg.push_str("<b>Креатив режим: Активен</b>\n");
    }
    if let Some(ref p) = server.plugins {
        msg.push_str(&format!("<b>Плагины:</b> <code>{}</code>\n", escape_html(p)));
    }

    msg
}

fn generate_server_card_markup(ip: &str, players_len: usize) -> serde_json::Value {
    let mut keyboard = Vec::new();
    if players_len > 5 {
        keyboard.push(vec![
            json!({"text": format!("Показать всех игроков ({})", players_len), "callback_data": format!("show_all_players:{}", ip)})
        ]);
    }
    keyboard.push(vec![
        json!({"text": "Скопировать IP", "callback_data": format!("copy_ip:{}", ip)}),
        json!({"text": "Пауза", "callback_data": "pause_feed_card"}),
        json!({"text": "Настройки", "callback_data": "settings"})
    ]);
    json!({ "inline_keyboard": keyboard })
}

pub async fn broadcast_server(server: &FoundServer) {
    if !BOT_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let raw_token = {
        if let Some(t) = BOT_TOKEN.get() {
            t.lock().map(|l| l.clone()).unwrap_or_default()
        } else {
            return;
        }
    };

    let token = clean_token(&raw_token);
    if token.is_empty() {
        return;
    }

    let color = get_server_color(server);

    let is_prime_exclusive_candidate = color == ServerColor::Green
        && server.max_players <= 49
        && (server.is_op
            || server.rcon_pass.is_some()
            || server.is_bungee_vulnerable
            || server.is_velocity_vulnerable
            || (server.online_players >= 1 && server.online_players <= 5));

    let ip_lower = server.ip.to_lowercase();
    let info_lower = server.info.to_lowercase();
    let is_playit = ip_lower.contains("playit") || info_lower.contains("playit");
    let is_ngrok = ip_lower.contains("ngrok") || info_lower.contains("ngrok") || server.ip.contains(".tcp.");
    let is_default = !is_playit && !is_ngrok;

    let all_users = db::get_all_tg_users();
    let client = get_http_client();
    let card_markup = generate_server_card_markup(&server.ip, server.players.len());

    // ═══════════════════════════════════════════════════════════════
    // 1. PRIME EXCLUSIVE DISTRIBUTION (Strictly GREEN servers, in ONE pair of hands)
    // ═══════════════════════════════════════════════════════════════
    if color == ServerColor::Green && is_prime_exclusive_candidate {

        let prime_users: Vec<TgDbUser> = all_users
            .iter()
            .filter(|u| {
                !u.is_banned
                    && u.active
                    && u.filter_green
                    && u.active_tier() == "Prime"
                    && u.has_pool("exclusive")
                    && user_accepts_source(u, is_playit, is_ngrok, is_default)
                    && u.matches_hunt_filters(server)
            })
            .cloned()
            .collect();

        if !prime_users.is_empty() {
            let idx = PRIME_ROTATION_IDX.fetch_add(1, Ordering::Relaxed) % prime_users.len();
            let target_user = &prime_users[idx];

            if db::claim_exclusive_server(&server.ip, target_user.chat_id) {
                db::record_sent_server(target_user.chat_id, &server.ip);
                let _ = db::check_and_increment_daily_limit(target_user.chat_id, "Prime");
                log_event(&format!("★ [PRIME EXCLUSIVE В ОДНИ РУКИ] Зеленый сервер {} выдан юзеру @{} (ID: {})", server.ip, target_user.username, target_user.chat_id));

                let text = format_server_card_html(server, true);

                let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
                let _ = client
                    .post(&url)
                    .json(&json!({
                        "chat_id": target_user.chat_id,
                        "text": text,
                        "parse_mode": "HTML",
                        "reply_markup": card_markup
                    }))
                    .send()
                    .await;
                return;
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // 2. PLUS POOL DISTRIBUTION (Strictly GREEN servers with max_players <= 49)
    // ═══════════════════════════════════════════════════════════════
    if color == ServerColor::Green && server.max_players <= 49 {
        let plus_users: Vec<TgDbUser> = all_users
            .iter()
            .filter(|u| {
                if u.is_banned || !u.active || !u.filter_green {
                    return false;
                }
                if !user_accepts_source(u, is_playit, is_ngrok, is_default) {
                    return false;
                }
                if !u.matches_hunt_filters(server) {
                    return false;
                }
                let tier = u.active_tier();
                if tier == "Prime" || tier == "Plus" {
                    u.has_pool("plus")
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        if !plus_users.is_empty() {
            let text = format_server_card_html(server, false);

            for user in plus_users {
                if db::check_and_increment_daily_limit(user.chat_id, &user.active_tier()) {
                    db::record_sent_server(user.chat_id, &server.ip);
                    let client_c = client.clone();
                    let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
                    let text_c = text.clone();
                    let markup_c = card_markup.clone();
                    let chat_id = user.chat_id;
                    tokio::spawn(async move {
                        let _ = client_c
                            .post(&url)
                            .json(&json!({
                                "chat_id": chat_id,
                                "text": text_c,
                                "parse_mode": "HTML",
                                "reply_markup": markup_c
                            }))
                            .send()
                            .await;
                    });
                }
            }
            return;
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // 3. FREE POOL DISTRIBUTION (Yellow, Blue, and Green servers with 50+ max_players, or Green <= 49 if Free pool chosen)
    // ═══════════════════════════════════════════════════════════════
    let target_free_pool_users: Vec<TgDbUser> = all_users
        .into_iter()
        .filter(|u| {
            if u.is_banned || !u.active {
                return false;
            }
            if !user_accepts_source(&u, is_playit, is_ngrok, is_default) {
                return false;
            }
            if !u.matches_hunt_filters(server) {
                return false;
            }

            let tier = u.active_tier();
            let pool_ok = if tier == "Free" { true } else { u.has_pool("free") };
            if !pool_ok {
                return false;
            }

            match color {
                ServerColor::Green => u.filter_green,
                ServerColor::Yellow => u.filter_yellow,
                ServerColor::Blue => u.filter_blue,
            }
        })
        .collect();

    if !target_free_pool_users.is_empty() {
        let text = format_server_card_html(server, false);

        for user in target_free_pool_users {
            if db::check_and_increment_daily_limit(user.chat_id, &user.active_tier()) {
                db::record_sent_server(user.chat_id, &server.ip);
                let client_c = client.clone();
                let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
                let text_c = text.clone();
                let markup_c = card_markup.clone();
                let chat_id = user.chat_id;
                tokio::spawn(async move {
                    let _ = client_c
                        .post(&url)
                        .json(&json!({
                            "chat_id": chat_id,
                            "text": text_c,
                            "parse_mode": "HTML",
                            "reply_markup": markup_c
                        }))
                        .send()
                        .await;
                });
            }
        }
    }
}

pub async fn send_admin_broadcast(broadcast_text: &str) -> usize {
    let raw_token = {
        if let Some(t) = BOT_TOKEN.get() {
            t.lock().map(|l| l.clone()).unwrap_or_default()
        } else {
            return 0;
        }
    };

    let token = clean_token(&raw_token);
    if token.is_empty() {
        return 0;
    }

    let all_users = db::get_all_tg_users();
    let client = get_http_client();
    let mut sent = 0;

    let text = format!("<b>ОБЪЯВЛЕНИЕ ОТ АДМИНИСТРАЦИИ</b>\n───────────────────────\n\n{}\n\n───────────────────────\nПоддержка: @pisscaner_support", broadcast_text);

    for user in all_users {
        if user.is_banned {
            continue;
        }
        let url = format!("{}/bot{}/sendMessage", get_api_base(), token);
        let resp = client
            .post(&url)
            .json(&json!({
                "chat_id": user.chat_id,
                "text": text,
                "parse_mode": "HTML"
            }))
            .send()
            .await;

        if let Ok(r) = resp {
            if r.status().is_success() {
                sent += 1;
            }
        }
    }

    log_event(&format!("Рассылка администратора успешно отправлена {} пользователям", sent));
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_open_server_card() {
        let server = FoundServer {
            ip: "192.0.2.10:25565".to_string(),
            info: "Java | 1.20.4 (nolic, nobs, noreg, movfree, noop, buildno) [Вход свободен] | 58/100 online | Frankfurt am Main".to_string(),
            version: "1.20.4".to_string(),
            online_players: 58,
            max_players: 100,
            geo: "Frankfurt am Main".to_string(),
            req_reason: "".to_string(),
            is_cracked: true,
            is_registered: false,
            is_deep_scan: false,
            is_op: false,
            needs_mods: false,
            is_bungee_vulnerable: false,
            is_velocity_vulnerable: false,
            has_builds: false,
            movement_restricted: false,
            players: vec!["Steve".to_string(), "Alex".to_string()],
            rcon_pass: None,
            plugins: None,
        };

        assert_eq!(get_server_color(&server), ServerColor::Green);
        let card = format_server_card_html(&server, false);
        assert!(card.contains("[ОТКРЫТЫЙ СЕРВЕР] <code>192.0.2.10:25565</code>"));
        assert!(card.contains("<b>ОТКРЫТЫЙ СЕРВЕР</b>"));
        assert!(card.contains("1.20.4"));
        assert!(card.contains("58/100"));
        assert!(card.contains("Steve, Alex"));
        assert!(card.contains("Frankfurt am Main (nolic, nobs, noreg, movfree, noop, buildno)"));
        assert!(!card.contains("Причина:"));
    }

    #[test]
    fn test_format_yellow_server_card() {
        let server = FoundServer {
            ip: "192.0.2.20:25565".to_string(),
            info: "Java | Gate 1.7.2-1.7.5-26.2 (nolic, ?bs, noreg, movfree, noop, buildno) ?admin | 0/20 online | Frankfurt am Main".to_string(),
            version: "Gate 1.7.2-1.7.5-26.2".to_string(),
            online_players: 0,
            max_players: 20,
            geo: "Frankfurt am Main".to_string(),
            req_reason: "".to_string(),
            is_cracked: true,
            is_registered: false,
            is_deep_scan: false,
            is_op: false,
            needs_mods: false,
            is_bungee_vulnerable: false,
            is_velocity_vulnerable: false,
            has_builds: false,
            movement_restricted: false,
            players: Vec::new(),
            rcon_pass: None,
            plugins: None,
        };

        assert_eq!(get_server_color(&server), ServerColor::Yellow);
        let card = format_server_card_html(&server, false);
        assert!(card.contains("[С ОГРАНИЧЕНИЯМИ] <code>192.0.2.20:25565</code>"));
        assert!(card.contains("<b>СЕРВЕР С ОГРАНИЧЕНИЯМИ</b>"));
    }

    #[test]
    fn test_format_license_server_card() {
        let server = FoundServer {
            ip: "192.0.2.30:25565".to_string(),
            info: "Java | 1.21.1 (Lic, ?bs, noreg, movfree, noop, buildyes) PlayerOne, PlayerTwo | 2/20 online | Gravelines".to_string(),
            version: "1.21.1".to_string(),
            online_players: 2,
            max_players: 20,
            geo: "Gravelines".to_string(),
            req_reason: "".to_string(),
            is_cracked: false,
            is_registered: false,
            is_deep_scan: false,
            is_op: false,
            needs_mods: false,
            is_bungee_vulnerable: false,
            is_velocity_vulnerable: false,
            has_builds: true,
            movement_restricted: false,
            players: vec!["PlayerOne".to_string(), "PlayerTwo".to_string()],
            rcon_pass: None,
            plugins: None,
        };

        assert_eq!(get_server_color(&server), ServerColor::Blue);
        let card = format_server_card_html(&server, false);
        assert!(card.contains("[ЛИЦЕНЗИЯ] <code>192.0.2.30:25565</code>"));
        assert!(card.contains("<b>ЛИЦЕНЗИОННЫЙ СЕРВЕР</b>"));
        assert!(card.contains("Лицензия Mojang"));
    }

    #[test]
    fn test_format_prime_exclusive_card_with_exploit() {
        let server = FoundServer {
            ip: "192.0.2.40:25565".to_string(),
            info: "Java | 1.20.1 (nolic, nobs, noreg, movfree, noop, buildyes) [Вход свободен] | 3/50 online | Moscow".to_string(),
            version: "1.20.1".to_string(),
            online_players: 3,
            max_players: 50,
            geo: "Moscow".to_string(),
            req_reason: "".to_string(),
            is_cracked: true,
            is_registered: false,
            is_deep_scan: false,
            is_op: false,
            needs_mods: false,
            is_bungee_vulnerable: true,
            is_velocity_vulnerable: false,
            has_builds: true,
            movement_restricted: false,
            players: vec!["Notch".to_string(), "Jeb_".to_string()],
            rcon_pass: Some("root123".to_string()),
            plugins: None,
        };

        assert_eq!(get_server_color(&server), ServerColor::Green);
        let card = format_server_card_html(&server, true);
        assert!(card.contains("[PRIME ЭКСКЛЮЗИВ] <code>192.0.2.40:25565</code>"));
        assert!(card.contains("<b>ОТКРЫТЫЙ СЕРВЕР [ЭКСКЛЮЗИВ]</b>"));
        assert!(card.contains("Подсказка: заходите под никами этих игроков"));
        assert!(card.contains("<b>RCON пароль:</b> <code>root123</code>"));
        assert!(card.contains("<b>Уязвимость: BungeeCord IP-Forwarding Bypass</b>"));
    }
}

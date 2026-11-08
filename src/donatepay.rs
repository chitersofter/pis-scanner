use crate::db::{self, DonatePayApplyResult};
use crate::telegram::{self, log_event};
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::time::sleep;

static DONATEPAY_TOKEN: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static DONATEPAY_USER_ID: OnceLock<Arc<Mutex<String>>> = OnceLock::new();
static IS_INITIALIZED: AtomicBool = AtomicBool::new(false);
static LAST_CHECK_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

const DEFAULT_DONATEPAY_TOKEN: &str = "";
const DEFAULT_USER_ID: &str = "";

#[derive(Debug, Deserialize)]
struct DonatePayUserResponse {
    #[allow(dead_code)]
    status: Option<String>,
    data: Option<DonatePayUserData>,
}

#[derive(Debug, Deserialize)]
struct DonatePayUserData {
    id: Option<i64>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DonatePayTxResponse {
    #[allow(dead_code)]
    status: Option<String>,
    data: Option<Vec<DonatePayTransaction>>,
}

#[derive(Debug, Deserialize, Clone)]
struct DonatePayTransaction {
    id: i64,
    sum: serde_json::Value,
    what: Option<String>,
    status: Option<String>,
    #[serde(rename = "type")]
    tx_type: Option<String>,
    comment: Option<String>,
    vars: Option<serde_json::Value>,
    #[allow(dead_code)]
    created_at: Option<String>,
}

impl DonatePayTransaction {
    pub fn parse_sum(&self) -> f64 {
        match &self.sum {
            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
            serde_json::Value::String(s) => s.trim().parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn get_effective_comment(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref c) = self.comment {
            if !c.trim().is_empty() {
                parts.push(c.trim());
            }
        }
        if let Some(ref v) = self.vars {
            if let Some(c) = v.get("comment").and_then(|c| c.as_str()) {
                if !c.trim().is_empty() {
                    parts.push(c.trim());
                }
            }
            if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
                if !n.trim().is_empty() {
                    parts.push(n.trim());
                }
            }
        }
        if let Some(ref w) = self.what {
            if !w.trim().is_empty() {
                parts.push(w.trim());
            }
        }
        parts.join(" ")
    }
}

pub fn get_token() -> String {
    if let Some(store) = DONATEPAY_TOKEN.get() {
        if let Ok(lock) = store.lock() {
            if !lock.trim().is_empty() {
                return lock.trim().to_string();
            }
        }
    }
    if let Some(t) = db::get_config("donatepay_token") {
        if !t.trim().is_empty() {
            return t.trim().to_string();
        }
    }
    if let Ok(content) = fs::read_to_string("donatepay_api.txt") {
        let t = content.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    DEFAULT_DONATEPAY_TOKEN.to_string()
}

pub fn get_user_id() -> String {
    if let Some(store) = DONATEPAY_USER_ID.get() {
        if let Ok(lock) = store.lock() {
            if !lock.trim().is_empty() {
                return lock.trim().to_string();
            }
        }
    }
    if let Some(u) = db::get_config("donatepay_user_id") {
        if !u.trim().is_empty() {
            return u.trim().to_string();
        }
    }
    DEFAULT_USER_ID.to_string()
}

pub fn get_payment_url(_chat_id: i64, _action: &str, _sum: f64) -> String {
    let user_id = get_user_id();
    format!("https://donatepay.ru/don/{}", user_id)
}

pub fn init() {
    if IS_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    let token = get_token();
    let _ = DONATEPAY_TOKEN.set(Arc::new(Mutex::new(token.clone())));
    let _ = DONATEPAY_USER_ID.set(Arc::new(Mutex::new(DEFAULT_USER_ID.to_string())));
    let _ = LAST_CHECK_MUTEX.set(tokio::sync::Mutex::new(()));

    tokio::spawn(async move {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        let token = get_token();
        let user_url = format!("https://donatepay.ru/api/v1/user?access_token={}", token);
        if let Ok(resp) = client.get(&user_url).send().await {
            if let Ok(user_resp) = resp.json::<DonatePayUserResponse>().await {
                if let Some(data) = user_resp.data {
                    if let Some(id) = data.id {
                        let id_str = id.to_string();
                        if let Some(store) = DONATEPAY_USER_ID.get() {
                            if let Ok(mut lock) = store.lock() {
                                *lock = id_str.clone();
                            }
                        }
                        db::set_config("donatepay_user_id", &id_str);
                        log_event(&format!("[DONATEPAY] Сервис подключен. Аккаунт: {} (ID: {})", data.name.unwrap_or_default(), id));
                    }
                }
            }
        }

        seed_existing_transactions_if_empty(&client).await;

        loop {
            sleep(Duration::from_secs(14)).await;
            let _ = process_recent_transactions(&client, None).await;
        }
    });
}

async fn seed_existing_transactions_if_empty(client: &Client) {
    let token = get_token();
    let url = format!(
        "https://donatepay.ru/api/v1/transactions?access_token={}&limit=50&status=success&type=donation",
        token
    );

    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(tx_resp) = resp.json::<DonatePayTxResponse>().await {
            if let Some(txs) = tx_resp.data {
                for tx in txs {
                    let comment = tx.get_effective_comment();
                    let sum = tx.parse_sum();
                    db::record_processed_payment(tx.id, 0, sum, &comment, "donatepay_seed");
                }
            }
        }
    }
}

pub async fn check_user_payments_now(chat_id: i64) -> Result<bool, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default();
    process_recent_transactions(&client, Some(chat_id)).await
}

async fn process_recent_transactions(client: &Client, manual_check_chat_id: Option<i64>) -> Result<bool, String> {
    let lock_guard = match LAST_CHECK_MUTEX.get() {
        Some(m) => m,
        None => return Ok(false),
    };
    let _lock = lock_guard.lock().await;

    let token = get_token();
    let url = format!(
        "https://donatepay.ru/api/v1/transactions?access_token={}&limit=25&status=success&type=donation",
        token
    );

    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let tx_resp = resp.json::<DonatePayTxResponse>().await.map_err(|e| e.to_string())?;

    let mut found_for_user = false;

    if let Some(txs) = tx_resp.data {
        for tx in txs {
            if tx.status.as_deref() != Some("success") {
                continue;
            }
            if tx.tx_type.as_deref() != Some("donation") {
                continue;
            }
            let sum = tx.parse_sum();
            if sum <= 0.0 {
                continue;
            }

            if db::is_payment_processed(tx.id) {
                continue;
            }

            let comment = tx.get_effective_comment();
            let parsed_chat_id = parse_chat_id_from_comment(&comment);

            if let Some(chat_id) = parsed_chat_id {
                if let Some(target_id) = manual_check_chat_id {
                    if chat_id == target_id {
                        found_for_user = true;
                    }
                }

                match db::apply_donatepay_payment(tx.id, chat_id, sum, &comment) {
                    Ok(result) => {
                        log_event(&format!(
                            "[DONATEPAY] Оплата #{} ({} руб.) от ID {}: {:?}",
                            tx.id, sum, chat_id, result
                        ));

                        let notification_text = match result {
                            DonatePayApplyResult::SubscriptionGranted { tier, days, new_expires_at } => {
                                let days_left = (new_expires_at - (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64)) / 86400;
                                format!(
                                    "<b>ОПЛАТА УСПЕШНО ПОЛУЧЕНА</b>\n───────────────────────\n\nВам активирована подписка <b>{}</b> на <b>{} дней</b>.\nВсего активно: <b>{} дней</b>.\n\nСумма: <b>{} руб.</b>\nТранзакция: <code>#{}</code>\n\n<i>Поток серверов обновлен.</i>",
                                    tier, days, days_left.max(1), sum as i64, tx.id
                                )
                            }
                            DonatePayApplyResult::BalanceTopup { added_rubles, new_balance } => {
                                format!(
                                    "<b>БАЛАНС УСПЕШНО ПОПОЛНЕН</b>\n───────────────────────\n\nЗачислено: <b>+{} руб.</b>\nТекущий баланс: <b>{} руб.</b>\nТранзакция: <code>#{}</code>\n\n<i>Средства доступны для использования в боте.</i>",
                                    added_rubles, new_balance, tx.id
                                )
                            }
                            DonatePayApplyResult::UnderpaymentTopup { required, paid, new_balance } => {
                                format!(
                                    "<b>ОПЛАТА ПОЛУЧЕНА (НЕДОСТАТОЧНО ДЛЯ ТАРИФА)</b>\n───────────────────────\n\nВы оплатили: <b>{} руб.</b>\nДля активации тарифа требовалось: <b>{} руб.</b>\n\nСумма <b>+{} руб.</b> зачислена на ваш баланс (Всего на балансе: <b>{} руб.</b>).\nВы можете доплатить остаток и активировать тариф из меню /plans.",
                                    paid, required, paid, new_balance
                                )
                            }
                        };

                        telegram::send_direct_message(chat_id, &notification_text).await;
                    }
                    Err(e) => {
                        log_event(&format!("[DONATEPAY] Ошибка применения оплаты #{}: {}", tx.id, e));
                    }
                }
            } else {
                db::record_processed_payment(tx.id, 0, sum, &comment, "donatepay_unidentified");
                log_event(&format!(
                    "[DONATEPAY] Донат #{} на {} руб. без корректного ID в комментарии ('{}')",
                    tx.id, sum, comment
                ));
            }
        }
    }

    Ok(found_for_user)
}

fn parse_chat_id_from_comment(comment: &str) -> Option<i64> {
    let clean = comment.trim();
    if clean.is_empty() {
        return None;
    }

    if let Some(rest) = clean.strip_prefix("tg_").or_else(|| clean.strip_prefix("TG_")).or_else(|| clean.strip_prefix("tg")) {
        let parts: Vec<&str> = rest.split('_').collect();
        if let Some(first) = parts.first() {
            if let Ok(id) = first.parse::<i64>() {
                if id > 0 {
                    return Some(id);
                }
            }
        }
    }

    if let Ok(id) = clean.parse::<i64>() {
        if id > 0 {
            return Some(id);
        }
    }

    for word in clean.split_whitespace() {
        let word_clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if let Some(rest) = word_clean.strip_prefix("tg_").or_else(|| word_clean.strip_prefix("tg")) {
            let id_str = rest.split('_').next().unwrap_or("");
            if let Ok(id) = id_str.parse::<i64>() {
                if id > 0 {
                    return Some(id);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chat_id_from_comment() {
        assert_eq!(parse_chat_id_from_comment("tg_123456789_plus"), Some(123456789));
        assert_eq!(parse_chat_id_from_comment("tg_987654321_prime"), Some(987654321));
        assert_eq!(parse_chat_id_from_comment("TG_555666777_topup"), Some(555666777));
        assert_eq!(parse_chat_id_from_comment("tg_111222333"), Some(111222333));
        assert_eq!(parse_chat_id_from_comment("123456789"), Some(123456789));
        assert_eq!(parse_chat_id_from_comment("Привет от tg_999888777"), Some(999888777));
        assert_eq!(parse_chat_id_from_comment(""), None);
        assert_eq!(parse_chat_id_from_comment("some random text"), None);
    }

    #[test]
    fn test_get_payment_url() {
        let url = get_payment_url(123456789, "plus", 200.0);
        assert!(url.starts_with("https://donatepay.ru/don/"));
    }
}

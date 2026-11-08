use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;
use rand::prelude::*;

pub const DEFAULT_ADJECTIVES: &[&str] = &[
    "cool", "epic", "wild", "fast", "dark", "blue", "red", "gold", "iron", "mega",
    "super", "hyper", "crazy", "brave", "magic", "cyber", "happy", "lucky", "fresh", "sweet",
    "clean", "sharp", "smart", "quick", "quiet", "grand", "royal", "prime", "solid", "vivid",
    "sunny", "windy", "frost", "amber", "green", "white", "black", "neon", "retro", "heavy",
    "light", "storm", "blaze", "chill", "swift", "giant", "tiny", "bold", "calm", "crisp",
    "deep", "fair", "fine", "free", "glad", "keen", "loud", "neat", "proud", "pure",
    "rich", "safe", "silly", "slim", "soft", "tall", "true", "warm", "wise", "young",
    "ancient", "bright", "clever", "cosmic", "danger", "dragon", "flying", "frozen", "golden", "hidden",
    "hollow", "jungle", "little", "modern", "mystic", "narrow", "native", "nimble", "orange", "purple",
    "radiant", "robust", "rugged", "secret", "shadow", "silent", "silver", "simple", "smooth", "spark",
    "speedy", "spring", "stellar", "strong", "summer", "timber", "unique", "velvet", "winter", "yellow"
];

pub const DEFAULT_NOUNS: &[&str] = &[
    "craft", "mine", "block", "world", "land", "zone", "realm", "town", "city", "hub",
    "play", "game", "cube", "host", "node", "core", "star", "moon", "sun", "sky",
    "lion", "wolf", "bear", "fox", "deer", "hawk", "crow", "cat", "dog", "duck",
    "fire", "ice", "rock", "sand", "wood", "tree", "leaf", "root", "wind", "wave",
    "boat", "ship", "cart", "fort", "base", "gate", "door", "path", "road", "park",
    "hill", "peak", "cave", "lake", "pond", "pool", "bay", "cove", "isle", "camp",
    "king", "lord", "hero", "boss", "crew", "team", "clan", "pack", "band", "guild",
    "sword", "bow", "axe", "pick", "helm", "ring", "coin", "gem", "ruby", "iron",
    "apple", "bread", "cake", "fish", "meat", "soup", "rose", "pine", "palm", "oak",
    "bell", "book", "drum", "flag", "horn", "lamp", "mask", "pipe", "rope", "tent"
];

// Fallback static IPs for e4mc relays to guarantee zero DNS lookups during scan
pub const RELAY_IPS: &[(&str, &str)] = &[
    ("eu.e4mc.link", "168.119.164.233"),
    ("us.e4mc.link", "5.78.111.101"),
    ("broker.e4mc.link", "168.119.164.233"),
];

pub async fn get_best_relay() -> (String, String) {
    let client = reqwest::Client::new();
    if let Ok(Ok(resp)) = tokio::time::timeout(Duration::from_secs(3), client.get("https://broker.e4mc.link/getBestRelay").send()).await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(host) = json.get("host").and_then(|h| h.as_str()) {
                let clean = host.trim();
                if !clean.is_empty() {
                    // Try to resolve relay IP
                    if let Ok(mut ips) = tokio::net::lookup_host(format!("{}:25565", clean)).await {
                        if let Some(ip) = ips.next() {
                            return (clean.to_string(), ip.ip().to_string());
                        }
                    }
                }
            }
        }
    }
    // Default fallback
    ("eu.e4mc.link".to_string(), "168.119.164.233".to_string())
}

pub fn get_random_target(adjectives: &[String], nouns: &[String], relay_host: &str, relay_ip: &str) -> (String, u16, String) {
    let mut rng = rand::thread_rng();
    let adj = adjectives.choose(&mut rng).map(|s| s.as_str()).unwrap_or("fast");
    let noun = nouns.choose(&mut rng).map(|s| s.as_str()).unwrap_or("craft");
    
    let (host, ip) = if !relay_host.is_empty() && !relay_ip.is_empty() {
        (relay_host, relay_ip)
    } else {
        let (h, i) = RELAY_IPS.choose(&mut rng).unwrap();
        (*h, *i)
    };

    let domain = format!("{}-{}.{}", adj, noun, host);
    (ip.to_string(), 25565, domain)
}

pub async fn load_or_fetch_words(url: &str, filename: &str, default_words: &[&str]) -> Vec<String> {
    if let Ok(file) = File::open(filename) {
        let reader = BufReader::new(file);
        let words: Vec<String> = reader.lines().flatten().filter(|w| !w.trim().is_empty()).collect();
        if words.len() >= 20 {
            return words;
        }
    }

    let mut words = Vec::new();
    let client = reqwest::Client::new();
    if let Ok(Ok(resp)) = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await {
        if resp.content_length().unwrap_or(0) > 5_000_000 { return default_words.iter().map(|s| s.to_string()).collect(); }
        if let Ok(text) = resp.text().await {
            for line in text.lines() {
                let word = line.trim().to_lowercase();
                if !word.is_empty() && word.len() <= 12 {
                    words.push(word);
                }
            }
            if words.len() >= 20 {
                if let Ok(mut file) = File::create(filename) {
                    let _ = file.write_all(text.as_bytes());
                }
                return words;
            }
        }
    }

    // Fallback to built-in dictionary
    default_words.iter().map(|s| s.to_string()).collect()
}
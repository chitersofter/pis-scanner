use tokio::io::AsyncReadExt;
use crate::types::TargetedDbType;
use std::sync::{Mutex, OnceLock};
use std::collections::{HashSet, HashMap};

pub const TARGET_IPS: &[&str] = &[
    // Hetzner High-Density MC Nodes
    "168.119", "138.201", "135.181", "65.108", "65.109", "65.21", "88.99", "78.46", "78.47", 
    "148.251", "176.9", "144.76", "159.69", "94.130", "188.40", "162.55", "116.202", "116.203",
    // OVH & OVH Game High-Performance Ranges
    "51.89", "51.77", "51.68", "51.38", "51.75", "54.38", "54.37", "141.95", "147.135", 
    "149.202", "146.59", "178.32", "178.33", "137.74", "164.132", "37.187", "91.121", "188.165", 
    "198.27", "198.50", "142.44", "198.244", "192.99", "135.148", "147.189",
    // RU/EU/CIS Game Hosting Clusters (Timeweb, Aeza, Selectel, FirstVDS, FastVPS, Craft-Host, etc.)
    "194.87.123", "194.87.124", "194.87.125", "185.189", "185.221", "185.178", "185.244", "185.250",
    "45.136", "45.142", "45.153", "45.89", "45.9", "45.8", "45.154", "45.155", "45.156", "45.67",
    "91.240", "94.250", "89.108", "89.111", "89.117", "193.124", "193.160", "193.161", "193.162", 
    "193.163", "193.164", "193.168", "37.230", "46.174", "46.175", "62.122", "178.159", "188.225", 
    "188.130", "195.133", "212.22", "213.159", "109.250", "109.173", "109.234",
    // Contabo & Dedicated VPS Clusters
    "161.97", "173.212", "173.249", "194.163", "144.91", "62.171", "207.180", "213.136", "38.242",
    // Scaleway, DigitalOcean, Oracle Cloud, Linode, Vultr
    "51.15", "51.158", "51.159", "163.172", "151.115", "129.151", "140.238", "152.67", "152.70", 
    "138.197", "134.209", "167.99", "165.22", "159.203", "159.65", "157.245", "157.230", "104.248", 
    "64.225", "64.227", "139.144", "172.105", "104.238", "45.76", "45.58"
];

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static KNOWN_SUBNETS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static GEO_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Extracts `/24` subnet: `A.B.C` (e.g. `194.87.123`)
pub fn extract_subnet_24(ip: &str) -> Option<String> {
    let clean_ip = ip.split(':').next().unwrap_or(ip);
    let parts: Vec<&str> = clean_ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Extracts `/16` subnet: `A.B` (e.g. `194.87`)
pub fn extract_subnet_16(ip: &str) -> Option<String> {
    let clean_ip = ip.split(':').next().unwrap_or(ip);
    let parts: Vec<&str> = clean_ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None
    }
}

pub fn extract_subnet(ip: &str) -> Option<String> {
    extract_subnet_24(ip).or_else(|| extract_subnet_16(ip))
}

#[derive(Clone, Copy, Debug)]
pub struct FastSubnet {
    pub start: u32,
    pub end: u32,
}

pub fn parse_subnet_to_fast(s: &str) -> Option<FastSubnet> {
    let s = s.trim();
    if s.is_empty() { return None; }
    
    // Check if CIDR notation: "192.0.2.0/24" or "198.51.0.0/16"
    if let Some((ip_part, mask_part)) = s.split_once('/') {
        if let Ok(ip) = ip_part.parse::<std::net::Ipv4Addr>() {
            if let Ok(mask) = mask_part.parse::<u8>() {
                if mask <= 32 {
                    let ip_u32 = u32::from(ip);
                    let mask_u32 = if mask == 0 { 0 } else { !0u32 << (32 - mask) };
                    let start = (ip_u32 & mask_u32) + 1;
                    let end = (ip_u32 | !mask_u32).saturating_sub(1);
                    if end >= start {
                        return Some(FastSubnet { start, end });
                    }
                }
            }
        }
    }

    let parts: Vec<&str> = s.split('.').collect();
    match parts.len() {
        3 => {
            // "194.87.123" -> /24
            let o1 = parts[0].parse::<u8>().ok()?;
            let o2 = parts[1].parse::<u8>().ok()?;
            let o3 = parts[2].parse::<u8>().ok()?;
            let base = ((o1 as u32) << 24) | ((o2 as u32) << 16) | ((o3 as u32) << 8);
            Some(FastSubnet { start: base + 1, end: base + 254 })
        },
        2 => {
            // "178.62" -> /16
            let o1 = parts[0].parse::<u8>().ok()?;
            let o2 = parts[1].parse::<u8>().ok()?;
            let base = ((o1 as u32) << 24) | ((o2 as u32) << 16);
            Some(FastSubnet { start: base + 1, end: base + 65534 })
        },
        4 => {
            // Single IP
            let o1 = parts[0].parse::<u8>().ok()?;
            let o2 = parts[1].parse::<u8>().ok()?;
            let o3 = parts[2].parse::<u8>().ok()?;
            let o4 = parts[3].parse::<u8>().ok()?;
            let ip = ((o1 as u32) << 24) | ((o2 as u32) << 16) | ((o3 as u32) << 8) | (o4 as u32);
            Some(FastSubnet { start: ip, end: ip })
        },
        _ => None,
    }
}

static RAM_SUBNETS_LIC: OnceLock<std::sync::Arc<Vec<FastSubnet>>> = OnceLock::new();
static RAM_SUBNETS_NOLIC: OnceLock<std::sync::Arc<Vec<FastSubnet>>> = OnceLock::new();
static RAM_SUBNETS_ALL: OnceLock<std::sync::Arc<Vec<FastSubnet>>> = OnceLock::new();

pub fn get_ram_subnets(db_type: TargetedDbType) -> std::sync::Arc<Vec<FastSubnet>> {
    match db_type {
        TargetedDbType::Lic => {
            RAM_SUBNETS_LIC.get_or_init(|| std::sync::Arc::new(get_fast_targeted_subnets(TargetedDbType::Lic))).clone()
        }
        TargetedDbType::NoLic => {
            RAM_SUBNETS_NOLIC.get_or_init(|| std::sync::Arc::new(get_fast_targeted_subnets(TargetedDbType::NoLic))).clone()
        }
        TargetedDbType::All => {
            RAM_SUBNETS_ALL.get_or_init(|| std::sync::Arc::new(get_fast_targeted_subnets(TargetedDbType::All))).clone()
        }
    }
}

pub fn get_fast_targeted_subnets(db_type: TargetedDbType) -> Vec<FastSubnet> {
    let raw_subnets = get_targeted_subnets(db_type);
    let mut list = Vec::with_capacity(raw_subnets.len());
    for s in &raw_subnets {
        if let Some(fs) = parse_subnet_to_fast(s) {
            list.push(fs);
        }
    }
    if list.is_empty() {
        list.push(FastSubnet { start: 0x01010101, end: 0xFEFEFEFE });
    }
    list
}

pub fn add_subnet_to_db(ip: &str, db_type: TargetedDbType) {
    let filename = match db_type {
        TargetedDbType::Lic => "targeted_lic.txt",
        TargetedDbType::NoLic => "targeted_nolic.txt",
        TargetedDbType::All => return,
    };

    let cache = KNOWN_SUBNETS.get_or_init(|| {
        let mut set = HashSet::new();
        for &def in TARGET_IPS {
            set.insert(def.to_string());
        }
        for f in &["targeted_lic.txt", "targeted_nolic.txt"] {
            if let Ok(c) = std::fs::read_to_string(f) {
                for line in c.lines() {
                    let s = line.trim();
                    if !s.is_empty() {
                        set.insert(s.to_string());
                    }
                }
            }
        }
        Mutex::new(set)
    });

    if let Ok(mut subnets) = cache.lock() {
        // Prefer high-density /24 subnets for pinpoint game server hosting targeting
        if let Some(sub24) = extract_subnet_24(ip) {
            if !subnets.contains(&sub24) {
                subnets.insert(sub24.clone());
                tokio::spawn(async move {
                    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(filename) {
                        use std::io::Write;
                        let _ = writeln!(file, "{}", sub24);
                    }
                });
            }
        }

        if let Some(sub16) = extract_subnet_16(ip) {
            if !subnets.contains(&sub16) {
                subnets.insert(sub16.clone());
                tokio::spawn(async move {
                    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(filename) {
                        use std::io::Write;
                        let _ = writeln!(file, "{}", sub16);
                    }
                });
            }
        }
    }
}

pub fn get_targeted_subnets(db_type: TargetedDbType) -> Vec<String> {
    let mut combined: Vec<String> = TARGET_IPS.iter().map(|&s| s.to_string()).collect();
    let filenames = match db_type {
        TargetedDbType::Lic => vec!["targeted_lic.txt"],
        TargetedDbType::NoLic => vec!["targeted_nolic.txt"],
        TargetedDbType::All => vec!["targeted_lic.txt", "targeted_nolic.txt"],
    };
    for filename in filenames {
        if let Ok(content) = std::fs::read_to_string(filename) {
            let subnets: Vec<String> = content.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            combined.extend(subnets);
        }
    }
    combined.sort();
    combined.dedup();
    combined
}

pub fn clean_motd(motd: &str) -> String {
    let mut cleaned = motd.replace('\n', " ");
    for code in &["§0", "§1", "§2", "§3", "§4", "§5", "§6", "§7", "§8", "§9", "§a", "§b", "§c", "§d", "§e", "§f", "§l", "§n", "§o", "§m", "§k", "§r"] {
        cleaned = cleaned.replace(code, "");
    }
    cleaned
}

pub async fn get_geo(ip: &str) -> String {
    if ip.contains(".e4mc.link") { return "e4mc Host".to_string(); }
    if ip.chars().any(|c| c.is_alphabetic()) && !ip.contains("aternos.me") { return "Dedicated Host".to_string(); }

    let cache = GEO_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let subnet = extract_subnet(ip);

    if let Ok(guard) = cache.lock() {
        if let Some(city) = guard.get(ip) {
            return city.clone();
        }
        if let Some(ref s) = subnet {
            if let Some(city) = guard.get(s) {
                return city.clone();
            }
        }
    }

    let client = HTTP_CLIENT.get_or_init(reqwest::Client::new);

    let mut found_city = String::new();

    // 1. Primary HTTPS lookup via ipwho.is
    if let Ok(Ok(resp)) = tokio::time::timeout(std::time::Duration::from_millis(700), client.get(format!("https://ipwho.is/{}", ip)).send()).await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if json["success"].as_bool().unwrap_or(false) {
                found_city = json["city"].as_str().unwrap_or("Unknown").to_string();
            }
        }
    }

    // 2. Fallback to ip-api
    if found_city.is_empty() {
        if let Ok(Ok(resp)) = tokio::time::timeout(std::time::Duration::from_millis(700), client.get(format!("http://ip-api.com/json/{}", ip)).send()).await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json["status"].as_str().unwrap_or("") == "success" {
                    found_city = json["city"].as_str().unwrap_or("Unknown").to_string();
                }
            }
        }
    }

    if !found_city.is_empty() {
        if let Ok(mut guard) = cache.lock() {
            if guard.len() >= 10_000 {
                guard.clear();
            }
            guard.insert(ip.to_string(), found_city.clone());
            if let Some(s) = subnet {
                guard.insert(s, found_city.clone());
            }
        }
    }

    found_city
}

pub async fn read_varint(stream: &mut tokio::net::TcpStream) -> std::io::Result<i32> {
    let mut num_read = 0;
    let mut result = 0;
    loop {
        let mut read = [0u8; 1];
        stream.read_exact(&mut read).await?;
        let value = (read[0] & 0b01111111) as i32;
        if num_read >= 5 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt too big")); }
        result |= value << (7 * num_read);
        num_read += 1;
        if (read[0] & 0b10000000) == 0 { break; }
    }
    Ok(result)
}

pub async fn read_packet(stream: &mut tokio::net::TcpStream) -> std::io::Result<(i32, Vec<u8>)> {
    let len = tokio::time::timeout(std::time::Duration::from_millis(1800), read_varint(stream))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "Read packet header timed out"))??;
    if len <= 0 || len > 262144 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid packet length"));
    }
    let mut data = vec![0u8; len as usize];
    tokio::time::timeout(std::time::Duration::from_millis(1800), stream.read_exact(&mut data))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "Read packet payload timed out"))??;
    
    let mut id = 0;
    let mut num_read = 0;
    let mut i = 0;
    loop {
        if i >= data.len() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "EOF")); }
        let read = data[i];
        i += 1;
        let value = (read & 0b01111111) as i32;
        if num_read >= 5 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt too big")); }
        id |= value << (7 * num_read);
        num_read += 1;
        if (read & 0b10000000) == 0 { break; }
    }
    Ok((id, data[i..].to_vec()))
}

pub fn write_varint(val: i32, buf: &mut Vec<u8>) {
    let mut uval = val as u32;
    loop {
        let mut temp = (uval & 0b01111111) as u8;
        uval >>= 7;
        if uval != 0 { temp |= 0b10000000; }
        buf.push(temp);
        if uval == 0 { break; }
    }
}

pub fn write_string(s: &str, buf: &mut Vec<u8>) {
    write_varint(s.len() as i32, buf);
    buf.extend_from_slice(s.as_bytes());
}

pub fn wrap_packet(packet: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    write_varint(packet.len() as i32, &mut buf);
    buf.extend_from_slice(packet);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_subnet_24() {
        assert_eq!(extract_subnet_24("192.168.1.100"), Some("192.168.1".to_string()));
        assert_eq!(extract_subnet_24("192.0.2.10:25565"), Some("192.0.2".to_string()));
        assert_eq!(extract_subnet_24("invalid_ip"), None);
    }

    #[test]
    fn test_extract_subnet_16() {
        assert_eq!(extract_subnet_16("192.168.1.100"), Some("192.168".to_string()));
        assert_eq!(extract_subnet_16("192.0.2.10:25565"), Some("192.0".to_string()));
        assert_eq!(extract_subnet_16("invalid_ip"), None);
    }

    #[test]
    fn test_clean_motd() {
        assert_eq!(clean_motd("§aHello §lWorld§r!"), "Hello World!");
        assert_eq!(clean_motd("Pure text"), "Pure text");
    }

    #[test]
    fn test_varint_roundtrip() {
        let mut buf = Vec::new();
        write_varint(25565, &mut buf);
        assert_eq!(buf, vec![0xDD, 0xC7, 0x01]);
        
        let wrapped = wrap_packet(&[0x00, 0x01]);
        assert_eq!(wrapped, vec![0x02, 0x00, 0x01]);
    }
}
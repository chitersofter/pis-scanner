use rand::prelude::*;

pub const PLAYIT_IPS: &[&str] = &[
    "147.185.221.31", 
    "147.185.221.16",
    "147.185.221.17",
    "147.185.221.18",
    "147.185.221.19",
    "147.185.221.20",
    "147.185.221.21",
    "147.185.221.22",
    "147.185.221.23",
    "147.185.221.24",
    "147.185.221.",
    "147.185.222.",
    "23.133.216.",
    "147.185.200.",
];

#[allow(dead_code)]
pub fn get_all_combinations() -> Vec<(String, u16)> {
    let mut combinations = Vec::new();
    for ip in PLAYIT_IPS {
        if ip.ends_with('.') {
            for octet in 1..=254u8 {
                let full_ip = format!("{}{}", ip, octet);
                for port in 1024..=65535u16 {
                    combinations.push((full_ip.clone(), port));
                }
            }
        } else {
            for port in 1024..=65535u16 {
                combinations.push((ip.to_string(), port));
            }
        }
    }
    combinations
}

pub fn get_target(_index: u32) -> (String, u16) {
    let mut rng = thread_rng();
    let ip_base = PLAYIT_IPS.choose(&mut rng).unwrap_or(&"147.185.221.31");
    let ip = if ip_base.ends_with('.') {
        format!("{}{}", ip_base, rng.gen_range(1..=254))
    } else {
        ip_base.to_string()
    };
    let port = rng.gen_range(1024..=65535);
    (ip, port)
}
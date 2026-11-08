use rand::prelude::*;

pub const NGROK_REGIONS: &[(&str, &[&str])] = &[
    ("tcp.ngrok.io", &["3.146.103.81", "18.217.92.205", "3.12.57.198", "3.12.245.36"]),
    ("tcp.eu.ngrok.io", &["3.74.27.83", "18.192.31.30", "3.71.225.231", "3.78.28.71"]),
    ("tcp.ap.ngrok.io", &["13.250.177.193", "18.136.14.77", "13.212.80.20"]),
    ("tcp.au.ngrok.io", &["13.236.108.77", "54.79.105.15", "13.238.169.176"]),
    ("tcp.sa.ngrok.io", &["18.231.111.168", "54.232.183.181", "177.71.254.103"]),
    ("tcp.jp.ngrok.io", &["13.114.153.253", "52.193.184.24", "18.176.69.155"]),
    ("tcp.in.ngrok.io", &["13.232.221.73", "13.233.151.101", "15.207.207.192"]),
];

pub fn get_random_target() -> (String, u16, String) {
    let mut rng = thread_rng();
    let (region, ips) = NGROK_REGIONS.choose(&mut rng).unwrap();
    let ip = ips.choose(&mut rng).unwrap();
    let prefix = rng.gen_range(0..=8);
    let port = rng.gen_range(10000..=65535);
    
    let domain = format!("{}.{}", prefix, region);
    (ip.to_string(), port, domain)
}
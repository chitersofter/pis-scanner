use std::time::Duration;
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;

use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
pub struct ParsedProxy {
    pub host: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
}

pub fn parse_proxy(raw: &str) -> Option<ParsedProxy> {
    let clean = raw.trim();
    if clean.is_empty() {
        return None;
    }

    // Format: user:pass@ip:port
    if let Some((auth_part, host_part)) = clean.split_once('@') {
        if let Some((user, pass)) = auth_part.split_once(':') {
            if let Some((ip, port_str)) = host_part.split_once(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    return Some(ParsedProxy {
                        host: ip.to_string(),
                        port,
                        auth: Some((user.to_string(), pass.to_string())),
                    });
                }
            }
        }
    }

    let parts: Vec<&str> = clean.split(':').collect();
    match parts.len() {
        4 => {
            // Format: ip:port:user:pass
            let host = parts[0].to_string();
            let port = parts[1].parse::<u16>().ok()?;
            let user = parts[2].to_string();
            let pass = parts[3].to_string();
            Some(ParsedProxy {
                host,
                port,
                auth: Some((user, pass)),
            })
        }
        2 => {
            // Format: ip:port
            let host = parts[0].to_string();
            let port = parts[1].parse::<u16>().ok()?;
            Some(ParsedProxy {
                host,
                port,
                auth: None,
            })
        }
        _ => None,
    }
}

pub async fn is_proxy_alive(proxy: &str) -> bool {
    if let Some(parsed) = parse_proxy(proxy) {
        let proxy_target = (parsed.host.as_str(), parsed.port);
        let connect_fut = async {
            if let Some((ref user, ref pass)) = parsed.auth {
                Socks5Stream::connect_with_password(proxy_target, "1.1.1.1:80", user.as_str(), pass.as_str()).await
            } else {
                Socks5Stream::connect(proxy_target, "1.1.1.1:80").await
            }
        };

        match tokio::time::timeout(Duration::from_millis(2500), connect_fut).await {
            Ok(Ok(_)) => true,
            _ => false,
        }
    } else {
        false
    }
}

pub async fn validate_proxies(proxies: Vec<String>) -> Vec<String> {
    let semaphore = Arc::new(Semaphore::new(60));
    let mut tasks = Vec::with_capacity(proxies.len());

    for p in proxies {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            if is_proxy_alive(&p).await {
                Some(p)
            } else {
                None
            }
        }));
    }

    let mut working = Vec::new();
    for task in tasks {
        if let Ok(Some(valid_proxy)) = task.await {
            working.push(valid_proxy);
        }
    }
    working
}

pub async fn scrape_proxies() -> Vec<String> {
    let urls = [
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=socks5&timeout=5000&country=all",
        "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt",
        "https://raw.githubusercontent.com/hookzof/socks5_list/master/proxy.txt"
    ];
    let mut raw_proxies = Vec::new();
    let client = reqwest::Client::new();
    
    for url in urls {
        if let Ok(Ok(resp)) = tokio::time::timeout(Duration::from_secs(5), client.get(url).send()).await {
            if resp.content_length().unwrap_or(0) > 5_000_000 { continue; }
            if let Ok(text) = resp.text().await {
                for line in text.lines() {
                    let p = line.trim().to_string();
                    if !p.is_empty() && p.contains(':') {
                        raw_proxies.push(p);
                    }
                }
            }
        }
    }
    raw_proxies.sort();
    raw_proxies.dedup();

    validate_proxies(raw_proxies).await
}

pub async fn connect_stream(target: &str, proxy: Option<&str>) -> std::io::Result<TcpStream> {
    match proxy {
        Some(p) => {
            if let Some(parsed) = parse_proxy(p) {
                let proxy_target = (parsed.host.as_str(), parsed.port);
                let connect_fut = async {
                    if let Some((ref user, ref pass)) = parsed.auth {
                        Socks5Stream::connect_with_password(proxy_target, target, user.as_str(), pass.as_str()).await
                    } else {
                        Socks5Stream::connect(proxy_target, target).await
                    }
                };

                match tokio::time::timeout(Duration::from_secs(3), connect_fut).await {
                    Ok(Ok(stream)) => Ok(stream.into_inner()),
                    Ok(Err(e)) => Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e.to_string())),
                    Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Таймаут прокси")),
                }
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Невалидный формат прокси"))
            }
        }
        None => {
            match tokio::time::timeout(Duration::from_millis(1200), TcpStream::connect(target)).await {
                Ok(Ok(stream)) => Ok(stream),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Таймаут подключения")),
            }
        }
    }
}

pub async fn check_public_ip(proxy: Option<&str>) -> String {
    let client = match proxy {
        Some(p) => {
            let proxy_str = if let Some(parsed) = parse_proxy(p) {
                if let Some((ref user, ref pass)) = parsed.auth {
                    format!("socks5://{}:{}@{}:{}", user, pass, parsed.host, parsed.port)
                } else {
                    format!("socks5://{}:{}", parsed.host, parsed.port)
                }
            } else {
                format!("socks5://{}", p)
            };

            let proxy_cfg = match reqwest::Proxy::all(proxy_str) {
                Ok(cfg) => cfg,
                Err(_) => return "Невалидный формат прокси".to_string(),
            };
            reqwest::Client::builder().proxy(proxy_cfg).build().unwrap_or_default()
        },
        None => reqwest::Client::new(),
    };

    if let Ok(Ok(resp)) = tokio::time::timeout(Duration::from_secs(4), client.get("https://api.ipify.org").send()).await {
        if let Ok(ip) = resp.text().await {
            return ip;
        }
    }
    "Не удалось определить".to_string()
}
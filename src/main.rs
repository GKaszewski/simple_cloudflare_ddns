use std::{collections::HashMap, error::Error, fs};

use reqwest::Client;
use serde::{Deserialize, Serialize};

const LAST_IP_FILE: &str = "last_ips.txt";

#[derive(Deserialize)]
struct Config {
    api_token: String,
    check_interval: u64,
    dns_records: Vec<DnsRecord>,
}

#[derive(Deserialize)]
struct ZoneResponse {
    result: Vec<ZoneInfo>,
}

#[derive(Deserialize)]
struct ZoneInfo {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct DnsRecordInfo {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct DnsRecordResponse {
    result: Vec<DnsRecordInfo>,
}

#[derive(Deserialize, Serialize, Clone)]
struct DnsRecord {
    dns_name: String,
    proxied: bool,
}

#[derive(Deserialize)]
struct IpResponse {
    ip: String,
}

#[derive(Serialize)]
struct DnsUpdateRequest {
    r#type: String,
    name: String,
    content: String,
    ttl: u32,
    proxied: bool,
}

#[derive(Deserialize)]
struct CloudflareResponse {
    success: bool,
    errors: Vec<serde_json::Value>,
}

async fn get_public_ip() -> Result<String, reqwest::Error> {
    let response: IpResponse = reqwest::get("https://api64.ipify.org?format=json")
        .await?
        .json()
        .await?;

    Ok(response.ip)
}

async fn get_zone_id(
    client: &Client,
    api_token: &str,
    domain: &str,
) -> Result<String, Box<dyn Error>> {
    let url = "https://api.cloudflare.com/client/v4/zones";
    let response: ZoneResponse = client
        .get(url)
        .header("Authorization", format!("Bearer {}", api_token))
        .header("Content-Type", "application/json")
        .send()
        .await?
        .json()
        .await?;

    for zone in response.result {
        if zone.name == domain {
            return Ok(zone.id);
        }
    }

    Err(format!("Zone ID not found for domain: {}", domain).into())
}

async fn get_record_id(
    client: &Client,
    api_token: &str,
    zone_id: &str,
    dns_name: &str,
) -> Result<String, Box<dyn Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        zone_id
    );
    let response: DnsRecordResponse = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_token))
        .header("Content-Type", "application/json")
        .send()
        .await?
        .json()
        .await?;

    for record in response.result {
        if record.name == dns_name {
            return Ok(record.id);
        }
    }

    Err(format!("DNS record ID not found for domain: {}", dns_name).into())
}

async fn update_dns_record(
    client: &Client,
    ip: &str,
    config: &Config,
    record: &DnsRecord,
    zone_id: &str,
    record_id: &str,
) -> Result<(), Box<dyn Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
        zone_id, record_id
    );

    let request_data = DnsUpdateRequest {
        r#type: "A".to_string(),
        name: record.dns_name.clone(),
        content: ip.to_string(),
        ttl: 1,
        proxied: record.proxied,
    };

    let response: CloudflareResponse = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", config.api_token))
        .header("Content-Type", "application/json")
        .json(&request_data)
        .send()
        .await?
        .json()
        .await?;

    if response.success {
        println!("✅ Updated DNS record for {} to {}", record.dns_name, ip);
        Ok(())
    } else {
        println!("Failed to update DNS record: {:?}", response.errors);
        Err("Cloudflare API error".into())
    }
}

fn read_last_ips() -> serde_json::Value {
    fs::read_to_string(LAST_IP_FILE)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn save_last_ips(ips: &serde_json::Value) {
    fs::write(LAST_IP_FILE, serde_json::to_string_pretty(ips).unwrap()).ok();
}

fn load_config() -> Result<Config, Box<dyn Error>> {
    let config = fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&config)?;

    Ok(config)
}

#[tokio::main]
async fn main() {
    let config = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {:?}", e);
            return;
        }
    };

    let client = Client::new();
    let mut last_ips = read_last_ips();

    let mut zone_id_map = HashMap::new();
    let mut record_id_map = HashMap::new();

    for record in &config.dns_records {
        let domain_parts: Vec<&str> = record.dns_name.split('.').collect();
        if domain_parts.len() < 2 {
            eprintln!("Invalid domain name: {}", record.dns_name);
            continue;
        }

        let domain = format!(
            "{}.{}",
            domain_parts[domain_parts.len() - 2],
            domain_parts[domain_parts.len() - 1]
        );

        let zone_id = match get_zone_id(&client, &config.api_token, &domain).await {
            Ok(id) => id,
            Err(e) => {
                eprintln!("Failed to get zone ID for {}: {:?}", domain, e);
                continue;
            }
        };

        zone_id_map.insert(record.dns_name.clone(), zone_id.clone());

        let record_id =
            match get_record_id(&client, &config.api_token, &zone_id, &record.dns_name).await {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("Failed to get record ID for {}: {:?}", record.dns_name, e);
                    continue;
                }
            };

        record_id_map.insert(record.dns_name.clone(), record_id.clone());
    }

    loop {
        match get_public_ip().await {
            Ok(current_ip) => {
                for record in &config.dns_records {
                    let last_ip = last_ips.get(&record.dns_name).and_then(|v| v.as_str());

                    if last_ip != Some(&current_ip) {
                        println!(
                            "IP has changed to {}, updating dns for {}...",
                            current_ip, record.dns_name
                        );

                        if update_dns_record(
                            &client,
                            &current_ip,
                            &config,
                            record,
                            &zone_id_map[&record.dns_name],
                            &record_id_map[&record.dns_name],
                        )
                        .await
                        .is_ok()
                        {
                            last_ips[&record.dns_name] = serde_json::json!(current_ip);
                        }
                    } else {
                        println!(
                            "IP has not changed for {}, skipping update",
                            record.dns_name
                        );
                    }
                }

                save_last_ips(&last_ips);
            }
            Err(e) => println!("Failed to get public IP: {:?}", e),
        }

        tokio::time::sleep(std::time::Duration::from_secs(config.check_interval)).await;
    }
}

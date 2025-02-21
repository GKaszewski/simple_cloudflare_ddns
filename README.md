# Simple Cloudflare DDNS

This utility tool updates DNS records in Cloudflare when ip changes.

Sample `config.toml` file:

```
# Cloudflare API settings
api_token = "your_cloudflare_api_token"
check_interval = 300  # Check interval in seconds (default 5 minutes)

# DNS records (multiple)
[[dns_records]]
dns_name = "your.domain1.com"
proxied = false

[[dns_records]]
dns_name = "your.domain2.com"
proxied = true
```

## Installation
Clone repo.

```
cargo build --release
sudo mv target/release/ddns-updater /usr/local/bin/
sudo mv ddns-updater.service /etc/systemd/system/ddns-updater.service
```

Run service
```
sudo systemctl daemon-reload
sudo systemctl enable ddns-updater
sudo systemctl start ddns-updater
```


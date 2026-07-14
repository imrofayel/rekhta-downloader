use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

pub fn create_client() -> Result<Client> {
    let client = Client::builder()
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Some(Duration::from_secs(10)))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    Ok(client)
}

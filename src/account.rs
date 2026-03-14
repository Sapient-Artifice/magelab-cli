use crate::client::remote::RemoteClient;
use anyhow::Result;

pub async fn list_models(client: &RemoteClient) -> Result<()> {
    let resp = client.list_models().await?;
    if let Some(data) = resp["data"].as_array() {
        println!("{:<40} PROVIDER", "MODEL");
        println!("{}", "─".repeat(55));
        for model in data {
            let id = model["id"].as_str().unwrap_or("?");
            let owner = model["owned_by"].as_str().unwrap_or("?");
            println!("{:<40} {}", id, owner);
        }
    }
    Ok(())
}

pub async fn show_usage(client: &RemoteClient) -> Result<()> {
    let resp = client.usage_summary().await?;
    println!("Usage Summary");
    println!("─────────────");
    if let Some(total) = resp["total_requests"].as_i64() {
        println!("  Requests:    {}", total);
    }
    if let Some(tokens) = resp["total_tokens_used"].as_i64() {
        println!("  Tokens:      {}", tokens);
    }
    if let Some(cost) = resp["total_cost"].as_f64() {
        println!("  Cost:        ${:.4}", cost);
    }
    Ok(())
}

pub async fn show_balance(client: &RemoteClient) -> Result<()> {
    let resp = client.balance().await?;
    println!("Account Balance");
    println!("───────────────");
    if let Some(credit) = resp["available_credit"].as_f64() {
        println!("  Available:   ${:.4}", credit);
    }
    if let Some(total) = resp["total_credit"].as_f64() {
        println!("  Total:       ${:.4}", total);
    }
    if let Some(used) = resp["total_cost"].as_f64() {
        println!("  Used:        ${:.4}", used);
    }
    Ok(())
}

pub async fn list_keys(client: &RemoteClient) -> Result<()> {
    let resp = client.list_keys().await?;
    if let Some(keys) = resp["api_keys"].as_array() {
        println!("{:<8} {:<30} {:<10} CREATED", "ID", "KEY", "STATUS");
        println!("{}", "─".repeat(65));
        for key in keys {
            let id = key["id"]
                .as_str()
                .or(key["id"].as_i64().map(|_| ""))
                .unwrap_or("?");
            let val = key["key_preview"].as_str().unwrap_or("***");
            let revoked = key["is_revoked"].as_bool().unwrap_or(false);
            let status = if revoked { "revoked" } else { "active" };
            let created = key["created_at"].as_str().unwrap_or("?");
            println!("{:<8} {:<30} {:<10} {}", id, val, status, created);
        }
    }
    Ok(())
}

pub async fn create_key(client: &RemoteClient) -> Result<()> {
    let resp = client.generate_key().await?;
    if let Some(key) = resp["api_key"].as_str() {
        println!("New API key: {}", key);
        println!("Save this — it won't be shown again.");
    }
    Ok(())
}

pub async fn revoke_key(client: &RemoteClient, key_id: &str) -> Result<()> {
    client.revoke_key(key_id).await?;
    println!("Key {} revoked.", key_id);
    Ok(())
}

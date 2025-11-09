//! Test script to inspect message reactions in raw API responses
//!
//! This script fetches messages from a test room and logs the raw JSON
//! to investigate undocumented reaction fields.

use std::env;
use webex::Webex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    let token = env::var("WEBEX_TOKEN").expect("WEBEX_TOKEN environment variable not set");
    let webex = Webex::new(&token);

    println!("=== Fetching rooms ===");
    let rooms = webex.list_rooms(None).await?;

    // Find the test room
    let test_room = rooms
        .iter()
        .find(|r| r.title.as_deref() == Some("*** webex-tui test ***"))
        .expect("Could not find test room '*** webex-tui test ***'");

    println!("Found test room: {:?}", test_room.title);
    println!("Room ID: {}", test_room.id);

    // Fetch messages from the test room
    println!("\n=== Fetching messages from test room ===");
    let params = webex::types::MessageListParams {
        room_id: &test_room.id,
        parent_id: None,
        mentioned_people: &[],
        before: None,
        before_message: None,
        max: Some(10), // Get last 10 messages
    };

    let messages = webex.list_messages(&params).await?;
    println!("Found {} messages", messages.len());

    // Now let's make a raw HTTP request to see the full JSON response
    println!("\n=== Making raw HTTP request to inspect full JSON ===");

    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "https://webexapis.com/v1/messages?roomId={}&max=10",
            test_room.id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    let status = response.status();
    let raw_json = response.text().await?;

    println!("Status: {}", status);
    println!("\n=== RAW JSON RESPONSE ===");
    println!("{}", raw_json);

    // Pretty print the JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw_json) {
        println!("\n=== PRETTY PRINTED JSON ===");
        println!("{}", serde_json::to_string_pretty(&parsed)?);
    }

    Ok(())
}

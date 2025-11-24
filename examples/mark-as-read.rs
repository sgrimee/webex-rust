use std::env;

const BOT_ACCESS_TOKEN: &str = "BOT_ACCESS_TOKEN";
const BOT_EMAIL: &str = "BOT_EMAIL";

///
/// # Mark Messages as Read/Unread
///
/// This example demonstrates how to mark messages as read or unread on the server.
/// When a message is marked as read/unread, other Webex clients will also see it as read/unread.
///
/// The bot will:
/// 1. Listen for incoming messages
/// 2. If the message contains "read", mark it as read
/// 3. If the message contains "unread", mark it as unread
/// 4. Reply with the action taken
///
/// # Usage
///
/// BOT_ACCESS_TOKEN="<token>" BOT_EMAIL="botname@webex.bot" cargo run --example mark-as-read
///
/// You can obtain a bot token by logging into the [Cisco Webex developer site](https://developer.webex.com/), then
///
/// * Select "My Webex Apps" from your profile menu (available by clicking on your avatar on the top right)
/// * Select "Create New App"
/// * Select "Create a Bot"
/// * Choose something unique to yourself for testing, e.g., "username-mark-read-bot"
/// * **Save** the "Bot's Access Token" you see on the next page.  If you fail to do so, you can
///   regenerate it later, but this will invalidate the old token.
///

#[tokio::main]
async fn main() {
    env_logger::init();

    let token = env::var(BOT_ACCESS_TOKEN)
        .unwrap_or_else(|_| panic!("{} not specified in environment", BOT_ACCESS_TOKEN));
    let bot_email = env::var(BOT_EMAIL)
        .unwrap_or_else(|_| panic!("{} not specified in environment", BOT_EMAIL));

    let webex = webex::Webex::new(token.as_str()).await;
    let mut event_stream = webex.event_stream().await.expect("event stream");

    println!("Bot started. Listening for messages...");

    while let Ok(event) = event_stream.next().await {
        // Process new messages
        if event.activity_type() == webex::ActivityType::Message(webex::MessageActivity::Posted) {
            // The event stream doesn't contain the message -- you have to go fetch it
            if let Ok(msg) = webex
                .get::<webex::Message>(&event.try_global_id().unwrap())
                .await
            {
                match &msg.person_email {
                    // Reply as long as it doesn't appear to be our own message
                    Some(sender) if sender != bot_email.as_str() => {
                        let message_text = msg.text.as_ref().map(|s| s.to_lowercase());
                        println!("Received message from {}: {:?}", sender, msg.text);

                        let message_id = msg.id.as_ref().unwrap();
                        let room_id = msg.room_id.as_ref().unwrap();

                        // Determine action based on message content
                        let action = if let Some(text) = &message_text {
                            if text.contains("unread") {
                                Some("unread")
                            } else if text.contains("read") {
                                Some("read")
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        match action {
                            Some("read") => {
                                // Mark the original message as read
                                match webex.mark_message_as_read(message_id, room_id).await {
                                    Ok(()) => {
                                        println!("Successfully marked message as read: {}", message_id);
                                        let mut reply = webex::types::MessageOut::from(&msg);
                                        reply.text = Some(format!(
                                            "✓ Message marked as **read** on the server. Other clients will see it as read."
                                        ));
                                        webex.send_message(&reply).await.unwrap();
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to mark message as read: {:?}", e);
                                        let mut reply = webex::types::MessageOut::from(&msg);
                                        reply.text = Some(format!(
                                            "❌ Failed to mark message as read: {:?}", e
                                        ));
                                        webex.send_message(&reply).await.unwrap();
                                    }
                                }
                            }
                            Some("unread") => {
                                // Mark the original message as unread
                                match webex.mark_message_as_unread(message_id, room_id).await {
                                    Ok(()) => {
                                        println!("Successfully marked message as unread: {}", message_id);
                                        let mut reply = webex::types::MessageOut::from(&msg);
                                        reply.text = Some(format!(
                                            "✓ Message marked as **unread** on the server. Other clients will see it as unread."
                                        ));
                                        webex.send_message(&reply).await.unwrap();
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to mark message as unread: {:?}", e);
                                        let mut reply = webex::types::MessageOut::from(&msg);
                                        reply.text = Some(format!(
                                            "❌ Failed to mark message as unread: {:?}", e
                                        ));
                                        webex.send_message(&reply).await.unwrap();
                                    }
                                }
                            }
                            Some(_) => {
                                // Send help message for unrecognized commands
                                let mut reply = webex::types::MessageOut::from(&msg);
                                reply.text = Some(format!(
                                    "Hi! Send a message containing 'read' to mark it as read, or 'unread' to mark it as unread.\n\nExamples:\n- 'mark this as read'\n- 'set to unread'"
                                ));
                                webex.send_message(&reply).await.unwrap();
                            }
                            None => {
                                // Send help message
                                let mut reply = webex::types::MessageOut::from(&msg);
                                reply.text = Some(format!(
                                    "Hi! Send a message containing 'read' to mark it as read, or 'unread' to mark it as unread.\n\nExamples:\n- 'mark this as read'\n- 'set to unread'"
                                ));
                                webex.send_message(&reply).await.unwrap();
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
    }
}

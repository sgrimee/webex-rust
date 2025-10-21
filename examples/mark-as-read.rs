use std::env;

const BOT_ACCESS_TOKEN: &str = "BOT_ACCESS_TOKEN";
const BOT_EMAIL: &str = "BOT_EMAIL";

///
/// # Mark Messages as Read
///
/// This example demonstrates how to mark messages as read on the server.
/// When a message is marked as read, other Webex clients will also see it as read.
///
/// The bot will:
/// 1. Listen for incoming messages
/// 2. Reply to the message
/// 3. Mark the message as read on the server
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
                        println!("Received message from {}: {:?}", sender, msg.text);

                        // Send a reply
                        let mut reply = webex::types::MessageOut::from(&msg);
                        reply.text = Some(format!(
                            "Message received! I'm marking it as read on the server."
                        ));
                        webex.send_message(&reply).await.unwrap();

                        // Mark the original message as read
                        let message_id = msg.id.as_ref().unwrap();
                        let room_id = msg.room_id.as_ref().unwrap();

                        match webex.mark_message_as_read(message_id, room_id).await {
                            Ok(()) => {
                                println!("Successfully marked message as read: {}", message_id);
                            }
                            Err(e) => {
                                eprintln!("Failed to mark message as read: {:?}", e);
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
    }
}

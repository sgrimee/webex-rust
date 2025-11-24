use std::env;

const BOT_ACCESS_TOKEN: &str = "BOT_ACCESS_TOKEN";

/// # Read Status Example
///
/// This example demonstrates how to track message read status across devices.
/// It shows:
/// 1. How to list rooms with read status information
/// 2. How to detect which rooms have unread messages
/// 3. How to listen for membership:seen events (read receipts)
/// 4. How to get membership information for rooms
///
/// # Usage
///
/// BOT_ACCESS_TOKEN="<token>" cargo run --example read-status
///
/// You can obtain a bot token by logging into the [Cisco Webex developer site](https://developer.webex.com/), then
///
/// * Select "My Webex Apps" from your profile menu (available by clicking on your avatar on the top right)
/// * Select "Create New App"
/// * Select "Create a Bot"
/// * Choose something unique to yourself for testing, e.g., "username-read-status"
/// * **Save** the "Bot's Access Token" you see on the next page.  If you fail to do so, you can
///   regenerate it later, but this will invalidate the old token.
///
/// # Notes
///
/// The Webex REST API doesn't directly expose lastSeenId or lastSeenDate fields.
/// These fields are available in the Webex JavaScript SDK through the memberships:seen event.
/// This example demonstrates the API structure for future WebSocket support when those
/// events become available.
///

#[tokio::main]
async fn main() {
    env_logger::init();

    let token = env::var(BOT_ACCESS_TOKEN)
        .unwrap_or_else(|_| panic!("{} not specified in environment", BOT_ACCESS_TOKEN));

    let webex = webex::Webex::new(token.as_str()).await;

    println!("=== Listing Rooms with Read Status ===\n");

    // List all rooms with read status information
    match webex.list_rooms_with_read_status().await {
        Ok(rooms_with_status) => {
            println!("Found {} rooms:", rooms_with_status.len());

            for room_with_status in &rooms_with_status {
                let room = &room_with_status.room;
                let status = &room_with_status.read_status;

                println!("\n  Room: {}", room.title.as_deref().unwrap_or("<no title>"));
                println!("    ID: {}", room.id);
                println!("    Type: {}", room.room_type);
                println!("    Last Activity: {}", room.last_activity);

                if let Some(last_seen_id) = &status.last_seen_id {
                    println!("    Last Seen Message ID: {}", last_seen_id);
                }
                if let Some(last_seen_date) = &status.last_seen_date {
                    println!("    Last Seen Date: {}", last_seen_date);
                }

                println!("    Has Unread: {}", status.has_unread);
            }

            // Get memberships for the first room (if any)
            if let Some(first_room) = rooms_with_status.first() {
                println!("\n=== Memberships for first room ===\n");

                match webex.get_room_memberships(&first_room.room.id).await {
                    Ok(memberships) => {
                        println!("Found {} members:", memberships.len());
                        for membership in &memberships {
                            println!("\n  Member: {}", membership.person_display_name.as_deref().unwrap_or("Unknown"));
                            println!("    Email: {}", membership.person_email.as_deref().unwrap_or("Unknown"));
                            println!("    Moderator: {}", membership.is_moderator);
                            println!("    Created: {}", membership.created);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error getting memberships: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error listing rooms: {}", e);
        }
    }

    println!("\n=== Listening for Events (including membership:seen) ===\n");
    println!("Press Ctrl+C to exit\n");

    // Listen for events including membership:seen events
    match webex.event_stream().await {
        Ok(mut event_stream) => {
            loop {
                match event_stream.next().await {
                    Ok(event) => {
                        use webex::ActivityType;
                        use webex::MembershipActivity;

                        match event.activity_type() {
                            ActivityType::Membership(MembershipActivity::Seen) => {
                                println!("📖 Read Receipt Event Received!");
                                if let Some(activity) = &event.data.activity {
                                    println!("  Activity ID: {}", activity.id);
                                    println!("  Published: {}", activity.published);
                                    if let Some(actor) = event.data.actor.as_ref() {
                                        println!("  User: {}", actor.display_name.as_deref().unwrap_or("Unknown"));
                                    }
                                    // In a real implementation, you would extract lastSeenId
                                    // from the activity object and update your local read status
                                }
                            }
                            ActivityType::Message(msg_activity) => {
                                println!("💬 Message Event: {:?}", msg_activity);
                            }
                            ActivityType::Space(space_activity) => {
                                println!("🏠 Space Event: {:?}", space_activity);
                            }
                            ActivityType::Membership(membership_activity) => {
                                println!("👥 Membership Event: {:?}", membership_activity);
                            }
                            _ => {
                                // Ignore other event types for this example
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error receiving event: {}", e);
                        if !event_stream.is_open {
                            eprintln!("Event stream closed. Exiting.");
                            break;
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error creating event stream: {}", e);
        }
    }
}

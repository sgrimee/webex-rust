# Message Read Status Feature

This document describes the unified message read status feature added to webex-rust.

## Overview

This feature allows you to track and control message read status across all your Webex clients. It provides:

1. **Membership API Support** - Query and update room memberships
2. **Read Status Tracking** - Track read/unread status for rooms
3. **Mark Messages as Read/Unread** - Update read status on the server
4. **WebSocket Event Support** - Listen for `memberships:seen` events (read receipts)

## Key Components

### 1. Membership Struct

Represents a person's membership in a Webex room.

```rust
pub struct Membership {
    pub id: String,
    pub room_id: String,
    pub person_id: String,
    pub person_email: String,
    pub person_display_name: String,
    pub person_org_id: String,
    pub is_moderator: bool,
    pub is_room_hidden: Option<bool>,
    pub room_type: Option<String>,
    pub is_monitor: Option<bool>,
    pub last_seen_id: Option<String>,  // For marking messages as read
    pub created: String,
}
```

### 2. ReadStatus Struct

Tracks read status information for a room.

```rust
pub struct ReadStatus {
    pub last_seen_id: Option<String>,
    pub last_seen_date: Option<String>,
    pub last_activity_id: Option<String>,
    pub has_unread: bool,
}
```

**Methods:**
- `from_room(room: &Room) -> ReadStatus` - Create status from room
- `calculate_has_unread(&self, last_activity: &str) -> bool` - Check for unread messages
- `mark_as_read(&mut self, last_message_id: Option<String>)` - Mark room as read (local)

### 3. RoomWithReadStatus Struct

Combines room information with read status.

```rust
pub struct RoomWithReadStatus {
    pub room: Room,
    pub read_status: ReadStatus,
}
```

### 4. MembershipActivity Enum

Represents different membership-related activities.

```rust
pub enum MembershipActivity {
    Seen,      // Read receipt sent (lastSeenId updated)
    Created,   // User added to room
    Updated,   // Membership updated
    Deleted,   // User removed from room
}
```

### 5. MembershipUpdateParams Struct

Parameters for updating a membership.

```rust
pub struct MembershipUpdateParams<'a> {
    pub is_moderator: Option<bool>,
    pub is_room_hidden: Option<bool>,
    pub last_seen_id: Option<&'a str>,  // Set this to mark messages as read
}
```

## API Methods

### Reading Status

```rust
// Get room with read status information
pub async fn get_room_with_read_status(
    &self,
    room_id: &GlobalId,
) -> Result<RoomWithReadStatus, Error>

// List all rooms with read status
pub async fn list_rooms_with_read_status(&self)
    -> Result<Vec<RoomWithReadStatus>, Error>

// Get memberships for a specific room
pub async fn get_room_memberships(&self, room_id: &str)
    -> Result<Vec<Membership>, Error>

// Get memberships for a specific person
pub async fn get_person_memberships(&self, person_id: &str)
    -> Result<Vec<Membership>, Error>
```

### Writing Status

```rust
// Mark a message as read (syncs across all clients)
pub async fn mark_message_as_read(
    &self,
    message_id: &str,
    room_id: &str,
) -> Result<(), Error>

// Mark a message as unread (syncs across all clients)
pub async fn mark_message_as_unread(
    &self,
    message_id: &str,
    room_id: &str,
) -> Result<(), Error>
```

### Generic Methods

The Membership type also supports generic methods:

```rust
// Get a specific membership by ID
let membership: Membership = webex.get(&membership_id).await?;

// List all memberships
let memberships: Vec<Membership> = webex.list().await?;

// List memberships with filters
let params = MembershipListParams {
    room_id: Some("room-id"),
    person_id: None,
    person_email: None,
    max: Some(100),
};
let memberships: Vec<Membership> = webex.list_with_params(params).await?;
```

## How It Works

### Marking Messages as Read

When you call `mark_message_as_read(message_id, room_id)`:

1. The library retrieves your membership in the specified room
2. It updates the membership's `lastSeenId` field to the target message ID
3. The change syncs across all your Webex clients via PUT `/v1/memberships/{id}`

### Marking Messages as Unread

When you call `mark_message_as_unread(message_id, room_id)`:

1. The library finds the message that comes before the target message
2. It sets `lastSeenId` to that previous message ID
3. This causes the target message (and all subsequent messages) to appear as unread
4. The change syncs across all your Webex clients

### WebSocket Events

The library supports `memberships:seen` events through the WebSocket connection:

```rust
let mut event_stream = webex.event_stream().await?;

loop {
    match event_stream.next().await {
        Ok(event) => {
            if let ActivityType::Membership(MembershipActivity::Seen) = event.activity_type() {
                // Handle read receipt event
                // Extract lastSeenId from event.data.activity
                println!("User read messages!");
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## Important Notes

### REST API Capabilities

The Webex REST API provides:

- ✅ **PUT `/v1/memberships/{id}`** - Update `lastSeenId` to mark messages as read/unread
- ⚠️ **GET `/v1/memberships`** - Does NOT return `lastSeenId` in responses
- ✅ **WebSocket Events** - The `memberships:seen` event includes `lastSeenId`

### Current Implementation

The current implementation:
- ✅ Provides data structures for read status tracking
- ✅ Supports membership API queries
- ✅ Listens for `memberships:seen` WebSocket events
- ✅ Can mark messages as read via PUT `/v1/memberships/{id}`
- ✅ Can mark messages as unread via PUT `/v1/memberships/{id}`
- ✅ Syncs read status across all Webex clients
- ⚠️ Returns conservative defaults (no unread) when `lastSeenDate` is unavailable from REST API

### Limitations

- The REST API does not return `lastSeenId` in GET requests
- Read status information is primarily available through WebSocket events
- The `ReadStatus` struct is useful for tracking status locally or via WebSocket events

## Examples

### Example 1: Mark Messages as Read/Unread

See `examples/mark-as-read.rs` for a complete working example.

```rust
use webex::Webex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let webex = Webex::new("your-token").await;

    let message_id = "message-id-here";
    let room_id = "room-id-here";

    // Mark a message as read
    webex.mark_message_as_read(message_id, room_id).await?;
    println!("Message marked as read across all clients");

    // Mark a message as unread (for later review)
    webex.mark_message_as_unread(message_id, room_id).await?;
    println!("Message marked as unread across all clients");

    Ok(())
}
```

### Example 2: Track Read Status

See `examples/read-status.rs` for a complete working example.

```rust
use webex::Webex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let webex = Webex::new("your-token").await;

    // List rooms with read status
    let rooms = webex.list_rooms_with_read_status().await?;

    for room_with_status in rooms {
        println!("Room: {}", room_with_status.room.title.unwrap_or_default());
        println!("Has unread: {}", room_with_status.read_status.has_unread);

        if let Some(last_seen) = room_with_status.read_status.last_seen_date {
            println!("Last seen: {}", last_seen);
        }
    }

    // Get room memberships
    let memberships = webex.get_room_memberships("room-id").await?;

    for member in memberships {
        println!("Member: {} ({})",
            member.person_display_name,
            member.person_email);
    }

    Ok(())
}
```

## Testing

To test the features:

```bash
# Build the project
cargo build

# Run the mark-as-read example (interactive)
BOT_ACCESS_TOKEN="your-token" BOT_EMAIL="bot@webex.bot" cargo run --example mark-as-read

# Run the read-status example (lists rooms and listens for events)
BOT_ACCESS_TOKEN="your-token" cargo run --example read-status

# Run tests
cargo test
```

## References

- [Webex Memberships API](https://developer.webex.com/docs/api/v1/memberships)
- [Webex Memberships Update API](https://developer.webex.com/docs/api/v1/memberships/update-a-membership)
- [Webex Rooms API](https://developer.webex.com/docs/api/v1/rooms)
- [Webex JS SDK Read Status](https://webex.github.io/webex-js-sdk/samples/browser-read-status/explanation.html)

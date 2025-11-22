# Add unified message read status tracking and control

## Summary

This PR adds comprehensive message read status tracking and control to webex-rust, unifying features from two separate branches into a cohesive implementation.

## Features Added

### 1. Message Read/Unread Control
- ✅ `mark_message_as_read(message_id, room_id)` - Mark messages as read on server
- ✅ `mark_message_as_unread(message_id, room_id)` - Mark messages as unread for later review
- ✅ Changes sync across all Webex clients in real-time

### 2. Read Status Tracking
- ✅ `get_room_with_read_status(room_id)` - Get room with read status info
- ✅ `list_rooms_with_read_status()` - List all rooms with read status
- ✅ `ReadStatus` struct for tracking last seen messages and unread state

### 3. Membership API Support
- ✅ `Membership` type now implements `Gettable` trait
- ✅ `get_room_memberships(room_id)` - Get all members in a room
- ✅ `get_person_memberships(person_id)` - Get all rooms for a person
- ✅ Works with generic API: `list<Membership>()`, `get<Membership>(id)`

### 4. WebSocket Event Support
- ✅ `MembershipActivity` enum for membership events
- ✅ Support for `memberships:seen` events (read receipts)
- ✅ Event types: `Seen`, `Created`, `Updated`, `Deleted`

## API Integration

The new API **complements the existing API perfectly** with zero duplication:

| Integration Aspect | Status |
|-------------------|--------|
| Duplicated methods | ✅ 0 |
| Conflicting methods | ✅ 0 |
| Breaking changes | ✅ 0 |
| Follows existing patterns | ✅ Yes |
| Extends generic API naturally | ✅ Yes |

### How It Integrates

**Generic API Extension** (follows existing pattern):
```rust
// Already worked for Message, Room, Person, etc.
let messages: Vec<Message> = webex.list().await?;

// Now works for Membership too (same pattern):
let memberships: Vec<Membership> = webex.list().await?;
```

**Convenience Methods** (mirrors existing pattern):
```rust
// Existing convenience method:
let all_rooms = webex.get_all_rooms().await?;

// New convenience methods (same pattern):
let members = webex.get_room_memberships(room_id).await?;
let my_rooms = webex.get_person_memberships(person_id).await?;
```

**Enhancement Pattern** (doesn't replace existing):
```rust
// Original room API still works:
let room: Room = webex.get(&room_id).await?;

// Enhanced version adds read status:
let room_with_status = webex.get_room_with_read_status(&room_id).await?;
```

**New Capabilities** (orthogonal functionality):
```rust
// Completely new - no overlap with existing CRUD operations:
webex.mark_message_as_read(message_id, room_id).await?;
webex.mark_message_as_unread(message_id, room_id).await?;
```

## Implementation Details

### Data Structures

**Unified Membership struct:**
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
    pub last_seen_id: Option<String>,  // For read status
    pub created: String,
}
```

**ReadStatus tracking:**
```rust
pub struct ReadStatus {
    pub last_seen_id: Option<String>,
    pub last_seen_date: Option<String>,
    pub last_activity_id: Option<String>,
    pub has_unread: bool,
}
```

**MembershipActivity events:**
```rust
pub enum MembershipActivity {
    Seen,      // Read receipt (memberships:seen event)
    Created,   // User added to room
    Updated,   // Membership updated
    Deleted,   // User removed from room
}
```

### How Mark as Read/Unread Works

**Mark as Read:**
1. Gets user's membership in the room
2. Updates `lastSeenId` field to the target message ID
3. Syncs via PUT `/v1/memberships/{membershipId}`

**Mark as Unread:**
1. Finds the message before the target message
2. Sets `lastSeenId` to that previous message ID
3. Target message and all after it appear as unread
4. Syncs across all Webex clients

## Examples

### Example 1: Interactive Read/Unread Bot
`examples/mark-as-read.rs` - Bot that responds to commands:
- Send "mark as read" → marks message as read
- Send "set to unread" → marks message as unread
- Other text → shows help

### Example 2: Read Status Tracking
`examples/read-status.rs` - Demonstrates:
- Listing rooms with read status
- Getting membership information
- Listening for membership:seen WebSocket events

## Documentation

- ✅ `MESSAGE_READ_STATUS.md` - Comprehensive feature documentation
- ✅ Updated `README.md` with new capabilities
- ✅ Inline code documentation for all public APIs
- ✅ Usage examples in doc comments

## Testing

To test:
```bash
# Interactive mark as read/unread
BOT_ACCESS_TOKEN="token" BOT_EMAIL="bot@webex.bot" cargo run --example mark-as-read

# Read status tracking and events
BOT_ACCESS_TOKEN="token" cargo run --example read-status
```

## Fulfills README Promise

The original README claimed:
> Current functionality includes:
> - Getting room memberships

But this was **never implemented**. This PR delivers on that promise and adds bonus read/unread functionality.

## Breaking Changes

**None.** All existing APIs continue to work unchanged.

## Commits

1. **362652d** - Add feature to mark messages as read on the server
2. **8031fc7** - Add capability to mark messages as unread
3. **5daa5a5** - Unify message read status features from both branches

## Related Work

This PR unifies and supersedes:
- Branch `claude/webex-message-status-011CULyyyEDqD3obksGqKZ4s` (read status tracking)
- Current branch features (mark as read/unread)

## Files Changed

- `src/types.rs` - Added Membership, ReadStatus, RoomWithReadStatus structs; MembershipActivity enum
- `src/lib.rs` - Added 6 new public methods for memberships and read status
- `README.md` - Updated feature list
- `MESSAGE_READ_STATUS.md` - Comprehensive documentation
- `examples/mark-as-read.rs` - Interactive mark as read/unread example
- `examples/read-status.rs` - Read status tracking example

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>

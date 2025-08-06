# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is `webex-rust`, an asynchronous Rust library providing a minimal interface to Webex Teams APIs. It's designed primarily for building bots but supports general API interactions.

## Commands

### Build and Test
- `cargo build` - Build the library
- `cargo test` - Run unit tests  
- `cargo clippy` - Run linter (note: very strict clippy rules enabled)
- `cargo fmt` - Format code
- `cargo doc` - Generate documentation

### Examples
- `cargo run --example hello-world` - Basic message sending example
- `cargo run --example auto-reply` - Bot that automatically replies to messages
- `cargo run --example adaptivecard` - Demonstrates AdaptiveCard usage
- `cargo run --example device-authentication` - Shows device authentication flow

### Development
- `cargo test --lib` - Run library tests only
- `cargo clippy --all-targets --all-features` - Full clippy check
- `cargo build --all-targets` - Build everything including examples

## Architecture

### Core Components

- **`Webex` struct** (`src/lib.rs:92-100`) - Main API client with token-based authentication
- **`WebexEventStream`** (`src/lib.rs:102-108`) - WebSocket event stream handler for real-time events
- **`RestClient`** (`src/lib.rs:247-251`) - Low-level HTTP client wrapper
- **Types module** (`src/types.rs`) - All API data structures and serialization
- **AdaptiveCard module** (`src/adaptive_card.rs`) - Support for interactive cards
- **Auth module** (`src/auth.rs`) - Device authentication flows
- **Error module** (`src/error.rs`) - Comprehensive error handling

### Key Patterns

- **Generic API methods**: `get<T>()`, `list<T>()`, `delete<T>()` work with any `Gettable` type
- **Device registration**: Automatic device setup and caching for WebSocket connections
- **Message handling**: Supports both direct messages and room messages with threading
- **Event streaming**: WebSocket-based real-time event processing with automatic reconnection

### Authentication Flow

1. Token-based authentication for REST API calls
2. Device registration with Webex for WebSocket connections  
3. Mercury URL discovery for optimal WebSocket endpoint
4. Automatic device cleanup and recreation as needed

## Important Notes

- Uses Rust 1.76 toolchain (see `rust-toolchain.toml`)
- Very strict clippy configuration with pedantic and nursery lints enabled
- All public APIs must have documentation (`#![deny(missing_docs)]`)
- WebSocket connections require device registration and token authentication
- Mercury URL caching reduces API calls for device discovery
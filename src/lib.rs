#![deny(missing_docs)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
// clippy::use_self fixed in https://github.com/rust-lang/rust-clippy/pull/9454
// TODO: remove this when clippy bug fixed in stable
#![allow(clippy::use_self)]
// should support this in the future - would be nice if all futures were send
#![allow(clippy::future_not_send)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::option_if_let_else)]
#![cfg_attr(test, deny(warnings))]
#![doc(html_root_url = "https://docs.rs/webex/latest/webex/")]

//! # webex-rust
//!
//! A minimal asynchronous interface to Webex Teams, intended for (but not
//! limited to) implementing bots.
//!
//! Current functionality includes:
//!
//! - Registration with Webex APIs
//! - Monitoring an event stream
//! - Sending direct or group messages
//! - Getting room memberships
//! - Building `AdaptiveCards` and retrieving responses
//!
//! Not all features are fully-fleshed out, particularly the `AdaptiveCard`
//! support (only a few serializations exist, enough to create a form with a
//! few choices, a text box, and a submit button).
//!
//! # DISCLAIMER
//!
//! This crate is not maintained by Cisco, and not an official SDK.  The
//! author is a current developer at Cisco, but has no direct affiliation
//! with the Webex development team.

extern crate lazy_static;

pub mod adaptive_card;
#[allow(missing_docs)]
pub mod error;
pub mod types;
pub use types::*;
pub mod auth;

use error::Error;

use crate::adaptive_card::AdaptiveCard;
use futures::{future::try_join_all, try_join};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, trace, warn};
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{self, Hasher},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as TErr, Message as TMessage},
    MaybeTlsStream, WebSocketStream,
};

/*
 * URLs:
 *
 * https://help.webex.com/en-us/xbcr37/External-Connections-Made-by-the-Serviceability-Connector
 *
 * These apply to the central Webex Teams (Wxt) servers.  WxT also supports enterprise servers;
 * these are not supported.
 */

// Main API URL - default for any request.
const REST_HOST_PREFIX: &str = "https://api.ciscospark.com/v1";
// U2C - service discovery, used to discover other URLs (for example, the mercury URL).
const U2C_HOST_PREFIX: &str = "https://u2c.wbx2.com/u2c/api/v1";
// Default mercury URL, used when the token doesn't have permissions to list organizations.
const DEFAULT_REGISTRATION_HOST_PREFIX: &str = "https://wdm-a.wbx2.com/wdm/api/v1";

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

// Qualify webex devices created by this crate
const DEFAULT_DEVICE_NAME: &str = "rust-client";
const DEVICE_SYSTEM_NAME: &str = "rust-spark-client";

/// Web Socket Stream type
pub type WStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Webex API Client
#[derive(Clone)]
#[must_use]
pub struct Webex {
    id: u64,
    client: RestClient,
    token: String,
    /// Webex Device Information used for device registration
    pub device: DeviceData,
    /// Cached user ID to avoid repeated /people/me calls
    user_id: Arc<Mutex<Option<String>>>,
}

/// Webex Event Stream handler
pub struct WebexEventStream {
    ws_stream: WStream,
    timeout: Duration,
    /// Signifies if `WebStream` is Open
    pub is_open: bool,
}

impl WebexEventStream {
    /// Get the next event from an event stream
    ///
    /// Returns an event or an error
    ///
    /// # Errors
    /// Returns an error when the underlying stream has a problem, but will
    /// continue to work on subsequent calls to `next()` - the errors can safely
    /// be ignored.
    pub async fn next(&mut self) -> Result<Event, Error> {
        loop {
            let next = self.ws_stream.next();

            match tokio::time::timeout(self.timeout, next).await {
                // Timed out
                Err(_) => {
                    // This does not seem to be recoverable, or at least there are conditions under
                    // which it does not recover. Indicate that the connection is closed and a new
                    // one will have to be opened.
                    self.is_open = false;
                    return Err(format!("no activity for at least {:?}", self.timeout).into());
                }
                // Didn't time out
                Ok(next_result) => match next_result {
                    None => {}
                    Some(msg) => match msg {
                        Ok(msg) => {
                            if let Some(h_msg) = self.handle_message(msg)? {
                                return Ok(h_msg);
                            }
                            // `None` messages still reset the timeout (e.g. Ping to keep alive)
                        }
                        Err(TErr::Protocol(_) | TErr::Io(_)) => {
                            // Protocol error probably requires a connection reset
                            // IO error is (apart from WouldBlock) generally an error with the
                            // underlying connection and also fatal
                            self.is_open = false;
                            return Err(msg.unwrap_err().to_string().into());
                        }
                        Err(e) => {
                            return Err(Error::Tungstenite(e, "Error getting next_result".into()))
                        }
                    },
                },
            }
        }
    }

    fn handle_message(&mut self, msg: TMessage) -> Result<Option<Event>, Error> {
        match msg {
            TMessage::Binary(bytes) => {
                let json = std::str::from_utf8(&bytes)?;
                match serde_json::from_str(json) {
                    Ok(ev) => Ok(Some(ev)),
                    Err(e) => {
                        warn!("Couldn't deserialize: {:?}.  Original JSON:\n{}", e, &json);
                        Err(e.into())
                    }
                }
            }
            TMessage::Text(t) => {
                debug!("text: {}", t);
                Ok(None)
            }
            TMessage::Ping(_) => {
                trace!("Ping!");
                Ok(None)
            }
            TMessage::Close(t) => {
                debug!("close: {:?}", t);
                self.is_open = false;
                Err(Error::Closed("Web Socket Closed".to_string()))
            }
            TMessage::Pong(_) => {
                debug!("Pong!");
                Ok(None)
            }
            TMessage::Frame(_) => {
                debug!("Frame");
                Ok(None)
            }
        }
    }

    pub(crate) async fn auth(ws_stream: &mut WStream, token: &str) -> Result<(), Error> {
        /*
         * Authenticate to the stream
         */
        let auth = types::Authorization::new(token);
        debug!("Authenticating to stream");
        match ws_stream
            .send(TMessage::Text(serde_json::to_string(&auth).unwrap()))
            .await
        {
            Ok(()) => {
                /*
                 * The next thing back should be a pong
                 */
                match ws_stream.next().await {
                    Some(msg) => match msg {
                        Ok(msg) => match msg {
                            TMessage::Ping(_) | TMessage::Pong(_) => {
                                debug!("Authentication succeeded");
                                Ok(())
                            }
                            _ => Err(format!("Received {msg:?} in reply to auth message").into()),
                        },
                        Err(e) => Err(format!("Received error from websocket: {e}").into()),
                    },
                    None => Err("Websocket closed".to_string().into()),
                }
            }
            Err(e) => Err(Error::Tungstenite(
                e,
                "failed to send authentication".to_string(),
            )),
        }
    }
}

enum AuthorizationType<'a> {
    None,
    Bearer(&'a str),
    Basic {
        username: &'a str,
        password: &'a str,
    },
}

enum Body<T: Serialize> {
    Json(T),
    UrlEncoded(T),
}

const BODY_NONE: Option<Body<()>> = None;

/// Implements low level REST requests to be used internally by the library
#[derive(Clone)]
struct RestClient {
    host_prefix: HashMap<String, String>,
    web_client: reqwest::Client,
}

impl RestClient {
    /// Creates a new `RestClient`
    pub fn new() -> Self {
        Self {
            host_prefix: HashMap::new(),
            web_client: reqwest::Client::new(),
        }
    }

    /******************************************************************
     * Low-level API.  These calls are chained to build various
     * high-level calls like "get_message"
     ******************************************************************/

    async fn api_get<'a, T: DeserializeOwned>(
        &self,
        rest_method: &str,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'a>,
    ) -> Result<T, Error> {
        self.rest_api(reqwest::Method::GET, rest_method, auth, params, BODY_NONE)
            .await
    }

    async fn api_delete<'a>(
        &self,
        rest_method: &str,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'a>,
    ) -> Result<(), Error> {
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(REST_HOST_PREFIX, String::as_str);
        let url = format!("{prefix}/{rest_method}");
        let mut request_builder = self.web_client.request(reqwest::Method::DELETE, url);
        if let Some(params) = params {
            request_builder = request_builder.query(&params);
        }
        match auth {
            AuthorizationType::None => {}
            AuthorizationType::Bearer(token) => {
                request_builder = request_builder.bearer_auth(token);
            }
            AuthorizationType::Basic { username, password } => {
                request_builder = request_builder.basic_auth(username, Some(password));
            }
        }
        let res = request_builder.send().await?;
        
        // Check for success status codes (200-299) - DELETE often returns 204 No Content
        if res.status().is_success() {
            Ok(())
        } else {
            // Convert non-success responses to errors
            Err(Error::from(res.error_for_status().unwrap_err()))
        }
    }

    async fn api_post<'a, T: DeserializeOwned>(
        &self,
        rest_method: &str,
        body: impl Serialize,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'a>,
    ) -> Result<T, Error>
where {
        self.rest_api(
            reqwest::Method::POST,
            rest_method,
            auth,
            params,
            Some(Body::Json(body)),
        )
        .await
    }

    async fn api_post_form_urlencoded<'a, T: DeserializeOwned>(
        &self,
        rest_method: &str,
        body: impl Serialize,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'a>,
    ) -> Result<T, Error> {
        self.rest_api(
            reqwest::Method::POST,
            rest_method,
            auth,
            params,
            Some(Body::UrlEncoded(body)),
        )
        .await
    }

    async fn api_put<'a, T: DeserializeOwned>(
        &self,
        rest_method: &str,
        body: impl Serialize,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'a>,
    ) -> Result<T, Error> {
        self.rest_api(
            reqwest::Method::PUT,
            rest_method,
            auth,
            params,
            Some(Body::Json(body)),
        )
        .await
    }

    async fn rest_api<T: DeserializeOwned>(
        &self,
        http_method: reqwest::Method,
        url: &str,
        auth: AuthorizationType<'_>,
        params: Option<impl Serialize>,
        body: Option<Body<impl Serialize>>,
    ) -> Result<T, Error> {
        let url_trimmed = url.split('?').next().unwrap_or(url);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{url}");
        let mut request_builder = self.web_client.request(http_method, &full_url);
        if let Some(params) = params {
            request_builder = request_builder.query(&params);
        }
        match body {
            Some(Body::Json(body)) => {
                request_builder = request_builder.json(&body);
            }
            Some(Body::UrlEncoded(body)) => {
                request_builder = request_builder.form(&body);
            }
            None => {}
        }
        match auth {
            AuthorizationType::None => {}
            AuthorizationType::Bearer(token) => {
                request_builder = request_builder.bearer_auth(token);
            }
            AuthorizationType::Basic { username, password } => {
                request_builder = request_builder.basic_auth(username, Some(password));
            }
        }
        let res = request_builder.send().await?;
        
        // Check HTTP status first
        let status = res.status();
        if !status.is_success() {
            let error_text = res.text().await?;
            
            // Try to parse as JSON error response first
            if let Ok(json_error) = serde_json::from_str::<serde_json::Value>(&error_text) {
                if let Some(message) = json_error.get("message").and_then(|m| m.as_str()) {
                    // Team 404 errors are expected when user doesn't have access - log as debug
                    if status == StatusCode::NOT_FOUND && 
                       full_url.contains("/teams/") && 
                       message.contains("Could not find teams") {
                        debug!("HTTP {} error for {}: {} (expected when not a team member)", status.as_u16(), full_url, message);
                    } else {
                        warn!("HTTP {} error for {}: {}", status.as_u16(), full_url, message);
                    }
                    return Err(Error::StatusText(status, message.to_string()));
                }
            }
            
            // Handle HTML error pages (like 403 from device endpoints)
            if error_text.starts_with("<!doctype html") || error_text.starts_with("<html") {
                let clean_error = if error_text.contains("<title>") && error_text.contains("</title>") {
                    // Extract title from HTML
                    let start = error_text.find("<title>").unwrap() + 7;
                    let end = error_text.find("</title>").unwrap();
                    error_text[start..end].to_string()
                } else {
                    format!("HTTP {} - HTML error page returned", status.as_u16())
                };
                debug!("HTTP {} error for {}: {}", status.as_u16(), full_url, clean_error);
                return Err(Error::StatusText(status, clean_error));
            }
            
            // Fallback to generic HTTP error
            debug!("HTTP {} error for {}: {}", status.as_u16(), full_url, error_text);
            return Err(Error::StatusText(status, error_text));
        }
        
        // Get response text for successful responses
        let response_text = res.text().await?;
        debug!("API Response for {}: {}", full_url, response_text);
        
        // Parse the response
        match serde_json::from_str(&response_text) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                error!("Failed to parse API response for {}: {}", full_url, e);
                error!("Raw response: {}", response_text);
                Err(e.into())
            }
        }
    }
}

impl Webex {
    /// Constructs a new Webex Teams context from a token
    /// Tokens can be obtained when creating a bot, see <https://developer.webex.com/my-apps> for
    /// more information and to create your own Webex bots.
    pub async fn new(token: &str) -> Self {
        Self::new_with_device_name(DEFAULT_DEVICE_NAME, token).await
    }

    /// Constructs a new Webex Teams context from a token and a chosen name
    /// The name is used to identify the device/client with Webex api
    pub async fn new_with_device_name(device_name: &str, token: &str) -> Self {
        let mut client: RestClient = RestClient {
            host_prefix: HashMap::new(),
            web_client: reqwest::Client::new(),
        };

        let mut hasher = DefaultHasher::new();
        hash::Hash::hash_slice(token.as_bytes(), &mut hasher);
        let id = hasher.finish();

        // Have to insert this before calling get_mercury_url() since it uses U2C for the catalog
        // request.
        client
            .host_prefix
            .insert("limited/catalog".to_string(), U2C_HOST_PREFIX.to_string());

        let mut webex = Self {
            id,
            client,
            token: token.to_string(),
            device: DeviceData {
                device_name: Some(DEFAULT_DEVICE_NAME.to_string()),
                device_type: Some("DESKTOP".to_string()),
                localized_model: Some("rust".to_string()),
                model: Some(format!("rust-v{CRATE_VERSION}")),
                name: Some(device_name.to_owned()),
                system_name: Some(DEVICE_SYSTEM_NAME.to_string()),
                system_version: Some(CRATE_VERSION.to_string()),
                ..DeviceData::default()
            },
            user_id: Arc::new(Mutex::new(None)),
        };

        let devices_url = match webex.get_mercury_url().await {
            Ok(url) => {
                trace!("Fetched mercury url {}", url);
                url
            }
            Err(e) => {
                debug!("Failed to fetch devices url, falling back to default");
                debug!("Error: {:?}", e);
                DEFAULT_REGISTRATION_HOST_PREFIX.to_string()
            }
        };
        webex
            .client
            .host_prefix
            .insert("devices".to_string(), devices_url);

        webex
    }

    /// Get an event stream handle
    pub async fn event_stream(&self) -> Result<WebexEventStream, Error> {
        // Helper function to connect to a device
        // refactored out to make it easier to loop through all devices and also lazily create a
        // new one if needed
        async fn connect_device(s: &Webex, device: DeviceData) -> Result<WebexEventStream, Error> {
            trace!("Attempting connection with device named {:?}", device.name);
            let Some(ws_url) = device.ws_url else {
                return Err("Device has no ws_url".into());
            };
            let url = url::Url::parse(ws_url.as_str())
                .map_err(|_| Error::from("Failed to parse ws_url"))?;
            debug!("Connecting to {:?}", url);
            match connect_async(url.as_str()).await {
                Ok((mut ws_stream, _response)) => {
                    debug!("Connected to {}", url);
                    WebexEventStream::auth(&mut ws_stream, &s.token).await?;
                    debug!("Authenticated");
                    let timeout = Duration::from_secs(20);
                    Ok(WebexEventStream {
                        ws_stream,
                        timeout,
                        is_open: true,
                    })
                }
                Err(e) => {
                    warn!("Failed to connect to {:?}: {:?}", url, e);
                    Err(Error::Tungstenite(
                        e,
                        "Failed to connect to ws_url".to_string(),
                    ))
                }
            }
        }

        // get_devices automatically tries to set up devices if the get fails.
        // Keep only devices named DEVICE_NAME to avoid conflicts with other clients
        let mut devices: Vec<DeviceData> = self
            .get_devices()
            .await?
            .iter()
            .filter(|d| d.name == self.device.name)
            .inspect(|d| trace!("Kept device: {}", d))
            .cloned()
            .collect();

        // Sort devices in descending order by modification time, meaning latest created device
        // first.
        devices.sort_by(|a: &DeviceData, b: &DeviceData| {
            b.modification_time
                .unwrap_or_else(chrono::Utc::now)
                .cmp(&a.modification_time.unwrap_or_else(chrono::Utc::now))
        });

        for device in devices {
            if let Ok(event_stream) = connect_device(self, device).await {
                trace!("Successfully connected to device.");
                return Ok(event_stream);
            }
        }

        // Failed to connect to any existing devices, creating new one
        match self.setup_devices().await {
            Ok(device) => connect_device(self, device).await,
            Err(e) => match &e {
                Error::StatusText(status, _) if *status == StatusCode::FORBIDDEN => {
                    debug!("Device creation returned 403 - may need spark:devices_write scope in integration");
                    Err(e)
                }
                _ => {
                    error!("Failed to setup devices: {e}");
                    Err(e)
                }
            }
        }
    }

    async fn get_mercury_url(&self) -> Result<String, Option<error::Error>> {
        // Bit of a hacky workaround, error::Error does not implement clone
        // TODO: this can be fixed by returning a Result<String, &error::Error>
        lazy_static::lazy_static! {
            static ref MERCURY_CACHE: Mutex<HashMap<u64, Result<String, ()>>> = Mutex::new(HashMap::new());
        }
        if let Ok(Some(result)) = MERCURY_CACHE
            .lock()
            .map(|cache| cache.get(&self.id).cloned())
        {
            trace!("Found mercury URL in cache!");
            return result.map_err(|()| None);
        }

        let mercury_url = self.get_mercury_url_uncached().await;

        if let Ok(mut cache) = MERCURY_CACHE.lock() {
            let result = mercury_url.as_ref().map_or(Err(()), |url| Ok(url.clone()));
            trace!("Saving mercury url to cache: {}=>{:?}", self.id, &result);
            cache.insert(self.id, result);
        }

        mercury_url.map_err(Some)
    }

    async fn get_mercury_url_uncached(&self) -> Result<String, error::Error> {
        // Steps:
        // 1. Get org id by GET /v1/organizations
        // 2. Get urls json from https://u2c.wbx2.com/u2c/api/v1/limited/catalog?orgId=[org id]
        // 3. mercury url is urls["serviceLinks"]["wdm"]
        //
        // 4. Add caching because this doesn't change, and it can be slow

        let orgs = match self.list::<Organization>().await {
            Ok(orgs) => orgs,
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("missing required scopes") || error_msg.contains("missing required roles") {
                    debug!("Insufficient permissions to list organizations, falling back to default mercury URL");
                    return Err("Can't get mercury URL with insufficient organization permissions".into());
                } else {
                    return Err(e);
                }
            }
        };
        if orgs.is_empty() {
            return Err("Can't get mercury URL with no orgs".into());
        }
        let org_id = &orgs[0].id;
        let api_url = "limited/catalog";
        let params = [("format", "hostmap"), ("orgId", org_id.as_str())];
        let catalogs = self
            .client
            .api_get::<CatalogReply>(
                api_url,
                Some(params),
                AuthorizationType::Bearer(&self.token),
            )
            .await?;
        let mercury_url = catalogs.service_links.wdm;

        Ok(mercury_url)
    }

    /// Get list of organizations
    #[deprecated(
        since = "0.6.3",
        note = "Please use `webex::list::<Organization>()` instead"
    )]
    pub async fn get_orgs(&self) -> Result<Vec<Organization>, Error> {
        self.list().await
    }
    /// Get attachment action
    /// Retrieves the attachment for the given ID.  This can be used to
    /// retrieve data from an `AdaptiveCard` submission
    #[deprecated(
        since = "0.6.3",
        note = "Please use `webex::get::<AttachmentAction>(id)` instead"
    )]
    pub async fn get_attachment_action(&self, id: &GlobalId) -> Result<AttachmentAction, Error> {
        self.get(id).await
    }

    /// Get a message by ID
    #[deprecated(
        since = "0.6.3",
        note = "Please use `webex::get::<Message>(id)` instead"
    )]
    pub async fn get_message(&self, id: &GlobalId) -> Result<Message, Error> {
        self.get(id).await
    }

    /// Delete a message by ID
    #[deprecated(
        since = "0.6.3",
        note = "Please use `webex::delete::<Message>(id)` instead"
    )]
    pub async fn delete_message(&self, id: &GlobalId) -> Result<(), Error> {
        self.delete::<Message>(id).await
    }

    /// Get available rooms
    #[deprecated(since = "0.6.3", note = "Please use `webex::list::<Room>()` instead")]
    pub async fn get_rooms(&self) -> Result<Vec<Room>, Error> {
        self.list().await
    }

    /// Get all rooms from all organizations that the client belongs to.
    /// Will be slow as does multiple API calls (one to get teamless rooms, one to get teams, then
    /// one per team).
    pub async fn get_all_rooms(&self) -> Result<Vec<Room>, Error> {
        let (mut all_rooms, teams) = try_join!(self.list(), self.list::<Team>())?;
        let futures: Vec<_> = teams
            .into_iter()
            .map(|team| {
                let params = [("teamId", team.id)];
                self.client.api_get::<ListResult<Room>>(
                    Room::API_ENDPOINT,
                    Some(params),
                    AuthorizationType::Bearer(&self.token),
                )
            })
            .collect();
        let teams_rooms = try_join_all(futures).await?;
        for room in teams_rooms {
            all_rooms.extend(
                room.items
                    .or(room.devices)
                    .unwrap_or_else(Vec::new)
            );
        }
        Ok(all_rooms)
    }

    /// Get available room
    #[deprecated(since = "0.6.3", note = "Please use `webex::get::<Room>(id)` instead")]
    pub async fn get_room(&self, id: &GlobalId) -> Result<Room, Error> {
        self.get(id).await
    }

    /// Get information about person
    #[deprecated(
        since = "0.6.3",
        note = "Please use `webex::get::<Person>(id)` instead"
    )]
    pub async fn get_person(&self, id: &GlobalId) -> Result<Person, Error> {
        self.get(id).await
    }

    /// Send a message to a user or room
    ///
    /// # Arguments
    /// * `message`: [`MessageOut`] - the message to send, including one of `room_id`,
    ///   `to_person_id` or `to_person_email`.
    ///
    /// # Errors
    /// Types of errors returned:
    /// * [`Error::Limited`] - returned on HTTP 423/429 with an optional Retry-After.
    /// * [`Error::Status`] | [`Error::StatusText`] - returned when the request results in a non-200 code.
    /// * [`Error::Json`] - returned when your input object cannot be serialized, or the return
    ///   value cannot be deserialised. (If this happens, this is a library bug and should be
    ///   reported.)
    /// * [`Error::UTF8`] - returned when the request returns non-UTF8 code.
    pub async fn send_message(&self, message: &MessageOut) -> Result<Message, Error> {
        self.client
            .api_post(
                "messages",
                message,
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
    }

    /// Edit an existing message
    ///
    /// # Arguments
    /// * `params`: [`MessageEditParams`] - the message to edit, including the message ID and the room ID,
    ///   as well as the new message text.
    ///
    /// # Errors
    /// Types of errors returned:
    /// * [`Error::Limited`] - returned on HTTP 423/429 with an optional Retry-After.
    /// * [`Error::Status`] | [`Error::StatusText`] - returned when the request results in a non-200 code.
    /// * [`Error::Json`] - returned when your input object cannot be serialized, or the return
    ///   value cannot be deserialised. (If this happens, this is a library bug and should be reported).
    pub async fn edit_message(
        &self,
        message_id: &GlobalId,
        params: &MessageEditParams<'_>,
    ) -> Result<Message, Error> {
        let rest_method = format!("messages/{}", message_id.id());
        self.client
            .api_put(
                &rest_method,
                params,
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
    }

    /// Get a resource from an ID
    /// # Errors
    /// * [`Error::Limited`] - returned on HTTP 423/429 with an optional Retry-After.
    /// * [`Error::Status`] | [`Error::StatusText`] - returned when the request results in a non-200 code.
    /// * [`Error::Json`] - returned when your input object cannot be serialized, or the return
    ///   value cannot be deserialised. (If this happens, this is a library bug and should be
    ///   reported.)
    /// * [`Error::UTF8`] - returned when the request returns non-UTF8 code.
    pub async fn get<T: Gettable + DeserializeOwned>(&self, id: &GlobalId) -> Result<T, Error> {
        let rest_method = format!("{}/{}", T::API_ENDPOINT, id.id());
        self.client
            .api_get::<T>(
                rest_method.as_str(),
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
    }

    /// Delete a resource from an ID
    pub async fn delete<T: Gettable + DeserializeOwned>(&self, id: &GlobalId) -> Result<(), Error> {
        let rest_method = format!("{}/{}", T::API_ENDPOINT, id.id());
        self.client
            .api_delete(
                rest_method.as_str(),
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
    }

    /// List resources of a type
    pub async fn list<T: Gettable + DeserializeOwned>(&self) -> Result<Vec<T>, Error> {
        self.client
            .api_get::<ListResult<T>>(
                T::API_ENDPOINT,
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
            .map(|result| {
                result.items
                    .or(result.devices)
                    .unwrap_or_else(Vec::new)
            })
    }

    /// List resources of a type, with parameters
    pub async fn list_with_params<T: Gettable + DeserializeOwned>(
        &self,
        list_params: T::ListParams<'_>,
    ) -> Result<Vec<T>, Error> {
        self.client
            .api_get::<ListResult<T>>(
                T::API_ENDPOINT,
                Some(list_params),
                AuthorizationType::Bearer(&self.token),
            )
            .await
            .map(|result| {
                result.items
                    .or(result.devices)
                    .unwrap_or_else(Vec::new)
            })
    }

    /// Get the current user's ID, caching it for future calls
    /// 
    /// # Errors
    /// * [`Error::Limited`] - returned on HTTP 423/429 with an optional Retry-After.
    /// * [`Error::Status`] | [`Error::StatusText`] - returned when the request results in a non-200 code.
    /// * [`Error::Json`] - returned when input/output cannot be serialized/deserialized.
    /// * [`Error::UTF8`] - returned when the request returns non-UTF8 code.
    async fn get_user_id(&self) -> Result<String, Error> {
        // Check if we already have the user ID cached
        if let Ok(guard) = self.user_id.lock() {
            if let Some(cached_id) = guard.as_ref() {
                return Ok(cached_id.clone());
            }
        }

        // Fetch the user ID from the API
        let me_global_id = types::GlobalId::new_with_cluster_unchecked(types::GlobalIdType::Person, "me".to_string(), None);
        let me = self.get::<types::Person>(&me_global_id).await?;
        
        // Cache it for future use
        if let Ok(mut guard) = self.user_id.lock() {
            *guard = Some(me.id.clone());
        }
        
        debug!("Cached user ID: {}", me.id);
        Ok(me.id)
    }

    /// Leave a room by deleting the current user's membership
    /// 
    /// # Arguments
    /// * `room_id`: The ID of the room to leave
    /// 
    /// # Errors
    /// * [`Error::UserError`] - returned when attempting to leave a 1:1 direct room (not supported by Webex API)
    /// * [`Error::Limited`] - returned on HTTP 423/429 with an optional Retry-After.
    /// * [`Error::Status`] | [`Error::StatusText`] - returned when the request results in a non-200 code.
    /// * [`Error::Json`] - returned when input/output cannot be serialized/deserialized.
    /// * [`Error::UTF8`] - returned when the request returns non-UTF8 code.
    /// 
    /// # Note
    /// The Webex API does not support leaving or deleting 1:1 direct message rooms.
    /// This function will return an error for direct rooms. Only group rooms can be left.
    pub async fn leave_room(&self, room_id: &types::GlobalId) -> Result<(), Error> {
        debug!("Leaving room: {}", room_id.id());

        // First, get the room details to check if it's a direct room
        let room = self.get::<types::Room>(room_id).await?;
        
        // Check if this is a 1:1 direct room - these cannot be left via API
        if room.room_type == "direct" {
            return Err(error::Error::UserError(
                "Cannot leave a 1:1 direct message room. The Webex API does not support leaving or hiding direct rooms. Only group rooms can be left.".to_string()
            ));
        }

        // Get the current user ID (cached after first call)
        let my_user_id = self.get_user_id().await?;
        debug!("Current user ID: {}", my_user_id);

        // Get all memberships in this room
        let membership_params = types::MembershipListParams {
            room_id: Some(room_id.id()),
            ..Default::default()
        };

        let memberships = self.list_with_params::<types::Membership>(membership_params).await?;

        let membership = memberships.into_iter().find(|m| m.person_id == my_user_id)
            .ok_or_else(|| error::Error::UserError("User is not a member of this room".to_string()))?;

        debug!("Found membership with ID: {}", membership.id);
        let membership_id = types::GlobalId::new(types::GlobalIdType::Membership, membership.id.clone())?;
        let rest_method = format!("memberships/{}", membership_id.id());

        self.client.api_delete(&rest_method, None::<()>, AuthorizationType::Bearer(&self.token)).await?;
        debug!("Successfully left room: {}", room_id.id());

        Ok(())
    }

    async fn get_devices(&self) -> Result<Vec<DeviceData>, Error> {
        match self
            .client
            .api_get::<DevicesReply>(
                "devices",
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
        {
            #[rustfmt::skip]
            Ok(DevicesReply { devices: Some(devices), .. }) => Ok(devices),
            Ok(DevicesReply { devices: None, .. }) => {
                debug!("Chaining one-time device setup from devices query");
                self.setup_devices().await.map(|device| vec![device])
            }
            Err(e) => match e {
                Error::Status(s) | Error::StatusText(s, _) => {
                    if s == StatusCode::NOT_FOUND {
                        debug!("No devices found, creating new one");
                        self.setup_devices().await.map(|device| vec![device])
                    } else if s == StatusCode::FORBIDDEN {
                        debug!("Device endpoint returned 403 Forbidden - expected for third-party integrations, creating new device");
                        match self.setup_devices().await {
                            Ok(device) => Ok(vec![device]),
                            Err(setup_err) => {
                                debug!("Setup devices also failed (expected): {setup_err}");
                                // Return empty vec so event_stream can create a device via the fallback path
                                Ok(vec![])
                            }
                        }
                    } else {
                        Err(e)
                    }
                }
                Error::Limited(_, _) => Err(e),
                _ => {
                    error!("Can't decode devices reply: {e}");
                    Err(format!("Can't decode devices reply: {e}").into())
                }
            },
        }
    }

    async fn setup_devices(&self) -> Result<DeviceData, Error> {
        trace!("Setting up new device: {}", &self.device);
        self.client
            .api_post(
                "devices",
                &self.device,
                None::<()>,
                AuthorizationType::Bearer(&self.token),
            )
            .await
    }
}

impl From<&AttachmentAction> for MessageOut {
    fn from(action: &AttachmentAction) -> Self {
        Self {
            room_id: action.room_id.clone(),
            ..Self::default()
        }
    }
}

impl From<&Message> for MessageOut {
    fn from(msg: &Message) -> Self {
        let mut new_msg = Self::default();

        if msg.room_type == Some(RoomType::Group) {
            new_msg.room_id.clone_from(&msg.room_id);
        } else if let Some(_person_id) = &msg.person_id {
            new_msg.to_person_id.clone_from(&msg.person_id);
        } else {
            new_msg.to_person_email.clone_from(&msg.person_email);
        }

        new_msg
    }
}

impl Message {
    /// Reply to a message.
    /// Posts the reply in the same chain as the replied-to message.
    /// Contrast with [`MessageOut::from()`] which only replies in the same room.
    #[must_use]
    pub fn reply(&self) -> MessageOut {
        MessageOut {
            room_id: self.room_id.clone(),
            parent_id: self
                .parent_id
                .as_deref()
                .or(self.id.as_deref())
                .map(ToOwned::to_owned),
            ..Default::default()
        }
    }
}

impl MessageOut {
    /// Generates a new outgoing message from an existing message
    ///
    /// # Arguments
    ///
    /// * `msg` - the template message
    ///
    /// Use `from_msg` to create a reply from a received message.
    #[deprecated(since = "0.2.0", note = "Please use the from instead")]
    #[must_use]
    pub fn from_msg(msg: &Message) -> Self {
        Self::from(msg)
    }

    /// Add attachment to an existing message
    ///
    /// # Arguments
    ///
    /// * `card` - Adaptive Card to attach
    pub fn add_attachment(&mut self, card: AdaptiveCard) -> &Self {
        self.attachments = Some(vec![Attachment {
            content_type: "application/vnd.microsoft.card.adaptive".to_string(),
            content: card,
        }]);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::ServerGuard;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Helper function to create a test Webex client with mocked RestClient
    async fn create_test_webex_client(server: &ServerGuard) -> Webex {
        let mut host_prefix = HashMap::new();
        host_prefix.insert("people/me".to_string(), server.url());
        host_prefix.insert("rooms/Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy".to_string(), server.url());
        host_prefix.insert("memberships".to_string(), server.url());
        host_prefix.insert("memberships/Y2lzY29zcGFyazovL3VzL01FTUJFUlNISVAvODc2NTQzMjEtNDMyMS00MzIxLTQzMjEtMjEwOTg3NjU0MzIx".to_string(), server.url());
        
        let rest_client = RestClient {
            host_prefix,
            web_client: reqwest::Client::new(),
        };
        
        let device = DeviceData {
            url: Some("test_url".to_string()),
            ws_url: Some("ws://test".to_string()),
            device_name: Some("test_device".to_string()),
            device_type: Some("DESKTOP".to_string()),
            localized_model: Some("rust-sdk-test".to_string()),
            modification_time: Some(chrono::Utc::now()),
            model: Some("rust-sdk-test".to_string()),
            name: Some(format!("rust-sdk-test-{}", COUNTER.fetch_add(1, Ordering::SeqCst))),
            system_name: Some("rust-sdk-test".to_string()),
            system_version: Some("0.1.0".to_string()),
        };

        Webex {
            id: 1,
            client: rest_client,
            token: "test_token".to_string(),
            device,
            user_id: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn test_leave_room_success() {
        let mut server = mockito::Server::new_async().await;

        // Mock the GET /rooms/{id} API call to check room type
        let room_mock = server
            .mock("GET", "/rooms/Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy",
                "title": "Test Room",
                "type": "group",
                "isLocked": false,
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "creatorId": "test_person_id",
                "created": "2024-01-01T00:00:00.000Z"
            }).to_string())
            .create_async()
            .await;

        // Mock the people/me API call
        let people_mock = server
            .mock("GET", "/people/me")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "test_person_id",
                "emails": ["test@example.com"],
                "displayName": "Test User",
                "orgId": "test_org_id",
                "created": "2024-01-01T00:00:00.000Z",
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "status": "active",
                "type": "person"
            }).to_string())
            .create_async()
            .await;

        // Mock the membership list API call
        let membership_mock = server
            .mock("GET", "/memberships")
            .match_header("authorization", "Bearer test_token")
            .match_query(mockito::Matcher::UrlEncoded("roomId".into(), "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "items": [{
                    "id": "87654321-4321-4321-4321-210987654321",
                    "roomId": "test_room_id",
                    "personId": "test_person_id",
                    "personEmail": "test@example.com",
                    "personDisplayName": "Test User",
                    "personOrgId": "test_org_id",
                    "isModerator": false,
                    "isMonitor": false,
                    "created": "2024-01-01T00:00:00.000Z"
                }]
            }"#)
            .create_async()
            .await;
        
        // Mock the membership deletion API call
        let delete_mock = server
            .mock("DELETE", "/memberships/Y2lzY29zcGFyazovL3VzL01FTUJFUlNISVAvODc2NTQzMjEtNDMyMS00MzIxLTQzMjEtMjEwOTg3NjU0MzIx")
            .match_header("authorization", "Bearer test_token")
            .with_status(204)
            .with_body("")
            .create_async()
            .await;

        let webex_client = create_test_webex_client(&server).await;
        let room_id = types::GlobalId::new(types::GlobalIdType::Room, "12345678-1234-1234-1234-123456789012".to_string()).unwrap();
        
        let result = webex_client.leave_room(&room_id).await;

        if let Err(e) = &result {
            eprintln!("Error: {}", e);
        }
        assert!(result.is_ok());
        room_mock.assert_async().await;
        people_mock.assert_async().await;
        membership_mock.assert_async().await;
        delete_mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_leave_room_user_not_member() {
        let mut server = mockito::Server::new_async().await;

        // Mock the GET /rooms/{id} API call to check room type
        let room_mock = server
            .mock("GET", "/rooms/Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy",
                "title": "Test Room",
                "type": "group",
                "isLocked": false,
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "creatorId": "test_person_id",
                "created": "2024-01-01T00:00:00.000Z"
            }).to_string())
            .create_async()
            .await;

        // Mock the people/me API call
        let people_mock = server
            .mock("GET", "/people/me")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "test_person_id",
                "emails": ["test@example.com"],
                "displayName": "Test User",
                "orgId": "test_org_id",
                "created": "2024-01-01T00:00:00.000Z",
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "status": "active",
                "type": "person"
            }).to_string())
            .create_async()
            .await;

        // Mock the membership list API call returning empty list
        let membership_mock = server
            .mock("GET", "/memberships")
            .match_query(mockito::Matcher::UrlEncoded("roomId".into(), "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy".into()))
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "items": []
            }).to_string())
            .create_async()
            .await;

        let webex_client = create_test_webex_client(&server).await;
        let room_id = types::GlobalId::new(types::GlobalIdType::Room, "12345678-1234-1234-1234-123456789012".to_string()).unwrap();
        
        let result = webex_client.leave_room(&room_id).await;

        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.to_string(), "User is not a member of this room");
        }
        room_mock.assert_async().await;
        people_mock.assert_async().await;
        membership_mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_leave_room_api_error() {
        let mut server = mockito::Server::new_async().await;

        // Mock the GET /rooms/{id} API call to check room type
        let room_mock = server
            .mock("GET", "/rooms/Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy",
                "title": "Test Room",
                "type": "group",
                "isLocked": false,
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "creatorId": "test_person_id",
                "created": "2024-01-01T00:00:00.000Z"
            }).to_string())
            .create_async()
            .await;

        // Mock the people/me API call
        let people_mock = server
            .mock("GET", "/people/me")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "test_person_id",
                "emails": ["test@example.com"],
                "displayName": "Test User",
                "orgId": "test_org_id",
                "created": "2024-01-01T00:00:00.000Z",
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "status": "active",
                "type": "person"
            }).to_string())
            .create_async()
            .await;

        // Mock the membership list API call returning error
        let membership_mock = server
            .mock("GET", "/memberships")
            .match_query(mockito::Matcher::UrlEncoded("roomId".into(), "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy".into()))
            .match_header("authorization", "Bearer test_token")
            .with_status(403)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "message": "Access denied",
                "errors": []
            }).to_string())
            .create_async()
            .await;

        let webex_client = create_test_webex_client(&server).await;
        let room_id = types::GlobalId::new(types::GlobalIdType::Room, "12345678-1234-1234-1234-123456789012".to_string()).unwrap();

        let result = webex_client.leave_room(&room_id).await;

        assert!(result.is_err());
        room_mock.assert_async().await;
        people_mock.assert_async().await;
        membership_mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_leave_room_direct_room_error() {
        let mut server = mockito::Server::new_async().await;

        // Mock the GET /rooms/{id} API call - return a direct room
        let room_mock = server
            .mock("GET", "/rooms/Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": "Y2lzY29zcGFyazovL3VzL1JPT00vMTIzNDU2NzgtMTIzNC0xMjM0LTEyMzQtMTIzNDU2Nzg5MDEy",
                "title": "Direct Chat",
                "type": "direct",
                "isLocked": false,
                "lastActivity": "2024-01-01T00:00:00.000Z",
                "creatorId": "test_person_id",
                "created": "2024-01-01T00:00:00.000Z"
            }).to_string())
            .create_async()
            .await;

        let webex_client = create_test_webex_client(&server).await;
        let room_id = types::GlobalId::new(types::GlobalIdType::Room, "12345678-1234-1234-1234-123456789012".to_string()).unwrap();
        
        let result = webex_client.leave_room(&room_id).await;

        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.to_string().contains("Cannot leave a 1:1 direct message room"));
        }
        room_mock.assert_async().await;
    }
}

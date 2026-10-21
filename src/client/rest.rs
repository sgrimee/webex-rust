//! Low-level REST client for Webex API requests.

use crate::error::Error;
use crate::types::{EmptyReply, Gettable, ListResult};
use log::{error, trace};
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Serialize};
use serde_json;
use std::collections::HashMap;

/// Authorization type for REST requests.
#[derive(Clone, Copy)]
pub enum AuthorizationType<'a> {
    /// No authorization
    None,
    /// Bearer token authorization
    Bearer(&'a str),
    /// Basic authentication
    Basic {
        /// Username
        username: &'a str,
        /// Password
        password: &'a str,
    },
}

/// Body type for REST requests.
enum Body<T: Serialize> {
    Json(T),
    UrlEncoded(T),
}

const BODY_NONE: Option<Body<()>> = None;

/// Implements low level REST requests to be used internally by the library.
#[derive(Clone)]
pub struct RestClient {
    /// Host prefix mapping for different API endpoints
    pub host_prefix: HashMap<String, String>,
    /// Underlying HTTP client
    pub web_client: reqwest::Client,
}

impl RestClient {
    /// Creates a new `RestClient`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            host_prefix: HashMap::new(),
            web_client: reqwest::Client::new(),
        }
    }

    /// Creates a `RestClient` with existing `host_prefix` and `web_client`.
    #[must_use]
    pub const fn new_with(
        host_prefix: HashMap<String, String>,
        web_client: reqwest::Client,
    ) -> Self {
        Self {
            host_prefix,
            web_client,
        }
    }

    /// Performs a GET request and returns the resource as JSON.
    ///
    /// # Arguments
    ///
    /// * `token` - Authorization token
    /// * `full_url` - Full URL to GET (e.g. `<https://api.ciscospark.com/v1/messages/abc123`>)
    pub async fn get_full_url<R: DeserializeOwned>(
        &self,
        token: &str,
        full_url: &str,
    ) -> Result<R, Error> {
        self.get_with_url(token, full_url).await
    }

    /// Performs a GET request with custom headers and returns the resource as JSON.
    ///
    /// # Arguments
    ///
    /// * `token` - Authorization token
    /// * `url` - Resource path (e.g. "rooms")
    /// * `params` - Query parameters
    /// * `host` - Optional custom host prefix
    pub async fn get_with_params<R: DeserializeOwned, P: Serialize>(
        &self,
        token: &str,
        url: &str,
        params: &P,
        host: Option<&str>,
    ) -> Result<R, Error> {
        let host_prefix = self.get_host_prefix(host, "rest");
        let full_url = format!("{host_prefix}/{url}");
        self.request_with_query(
            "GET",
            AuthorizationType::Bearer(token),
            &full_url,
            Some(params),
            BODY_NONE,
        )
        .await
    }

    /// Performs a GET request.
    async fn get_with_url<R: DeserializeOwned>(
        &self,
        token: &str,
        full_url: &str,
    ) -> Result<R, Error> {
        self.request("GET", AuthorizationType::Bearer(token), full_url, BODY_NONE)
            .await
    }

    /// Performs a POST request with JSON body.
    pub async fn post<R: DeserializeOwned, D: Serialize>(
        &self,
        token: &str,
        url: &str,
        data: &D,
        host: Option<&str>,
    ) -> Result<R, Error> {
        let host_prefix = self.get_host_prefix(host, "rest");
        let full_url = format!("{host_prefix}/{url}");
        self.request(
            "POST",
            AuthorizationType::Bearer(token),
            &full_url,
            Some(Body::Json(data)),
        )
        .await
    }

    /// Performs a PUT request with JSON body.
    pub async fn put<R: DeserializeOwned, D: Serialize>(
        &self,
        token: &str,
        url: &str,
        data: &D,
        host: Option<&str>,
    ) -> Result<R, Error> {
        let host_prefix = self.get_host_prefix(host, "rest");
        let full_url = format!("{host_prefix}/{url}");
        self.request(
            "PUT",
            AuthorizationType::Bearer(token),
            &full_url,
            Some(Body::Json(data)),
        )
        .await
    }

    /// Performs a DELETE request.
    pub async fn delete<T: Gettable>(
        &self,
        token: &str,
        id: &str,
        host: Option<&str>,
    ) -> Result<(), Error> {
        let host_prefix = self.get_host_prefix(host, "rest");
        let full_url = format!("{host_prefix}/{}/{id}", T::API_ENDPOINT);
        let _: EmptyReply = self
            .request(
                "DELETE",
                AuthorizationType::Bearer(token),
                &full_url,
                BODY_NONE,
            )
            .await?;
        Ok(())
    }

    /// Performs a POST request with URL-encoded form body.
    /// Used primarily for OAuth authentication flows.
    pub async fn api_post_form_urlencoded<T: DeserializeOwned, B: Serialize>(
        &self,
        rest_method: &str,
        body: B,
        _params: Option<impl Serialize>,
        auth: AuthorizationType<'_>,
    ) -> Result<T, Error> {
        // Get the host prefix for the URL
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(super::REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{rest_method}");

        // params are not currently used but kept for API compatibility
        self.request("POST", auth, &full_url, Some(Body::UrlEncoded(body)))
            .await
    }

    /// Gets the host prefix for a given host key.
    fn get_host_prefix(&self, host: Option<&str>, default_key: &str) -> String {
        host.map_or_else(
            || {
                self.host_prefix
                    .get(default_key)
                    .map_or_else(|| super::REST_HOST_PREFIX.to_string(), Clone::clone)
            },
            ToString::to_string,
        )
    }

    /// Performs an HTTP request with query parameters and optional body.
    async fn request_with_query<R: DeserializeOwned, Q: Serialize, D: Serialize>(
        &self,
        method: &str,
        auth: AuthorizationType<'_>,
        url: &str,
        query: Option<&Q>,
        body: Option<Body<D>>,
    ) -> Result<R, Error> {
        trace!("Webex HTTP {method} request");
        let mut req = self
            .web_client
            .request(method.parse().unwrap(), url)
            .header("User-Agent", format!("webex-rust/{}", super::CRATE_VERSION));

        // Add query parameters if provided
        if let Some(params) = query {
            req = req.query(params);
        }

        // Apply authorization
        req = match auth {
            AuthorizationType::None => req,
            AuthorizationType::Bearer(token) => req.bearer_auth(token),
            AuthorizationType::Basic { username, password } => {
                req.basic_auth(username, Some(password))
            }
        };

        req = match body {
            Some(Body::Json(data)) => req.json(&data),
            Some(Body::UrlEncoded(data)) => req.form(&data),
            None => req,
        };

        let response = req.send().await?;
        let status = response.status();
        let response_text = response.text().await?;

        if status.is_success() {
            trace!(
                "Webex HTTP {status} response ({} bytes)",
                response_text.len()
            );

            // Handle empty responses (like 204 No Content)
            if response_text.is_empty() {
                Ok(serde_json::from_str("{}")?)
            } else {
                Ok(serde_json::from_str(&response_text)?)
            }
        } else {
            // Log errors with appropriate level based on context
            Self::log_error(status, url, &response_text);
            Err(Self::handle_error_response(status, response_text))
        }
    }

    /// Performs an HTTP request with the given method, URL, and optional body.
    async fn request<R: DeserializeOwned, D: Serialize>(
        &self,
        method: &str,
        auth: AuthorizationType<'_>,
        url: &str,
        body: Option<Body<D>>,
    ) -> Result<R, Error> {
        trace!("Webex HTTP {method} request");
        let mut req = self
            .web_client
            .request(method.parse().unwrap(), url)
            .header("User-Agent", format!("webex-rust/{}", super::CRATE_VERSION));

        // Apply authorization
        req = match auth {
            AuthorizationType::None => req,
            AuthorizationType::Bearer(token) => req.bearer_auth(token),
            AuthorizationType::Basic { username, password } => {
                req.basic_auth(username, Some(password))
            }
        };

        req = match body {
            Some(Body::Json(data)) => req.json(&data),
            Some(Body::UrlEncoded(data)) => req.form(&data),
            None => req,
        };

        let response = req.send().await?;
        let status = response.status();
        let response_text = response.text().await?;

        if status.is_success() {
            trace!(
                "Webex HTTP {status} response ({} bytes)",
                response_text.len()
            );

            // Handle empty responses (like 204 No Content)
            if response_text.is_empty() {
                Ok(serde_json::from_str("{}")?)
            } else {
                Ok(serde_json::from_str(&response_text)?)
            }
        } else {
            // Log errors with appropriate level based on context
            Self::log_error(status, url, &response_text);
            Err(Self::handle_error_response(status, response_text))
        }
    }

    /// Logs HTTP errors with appropriate log level based on context.
    fn log_error(status: StatusCode, url: &str, response_text: &str) {
        // Try to parse as JSON to get structured error message
        if let Ok(json_error) = serde_json::from_str::<serde_json::Value>(response_text) {
            if let Some(message) = json_error.get("message").and_then(|m| m.as_str()) {
                // Team 404 errors are expected when user doesn't have team access - log as debug
                if status == StatusCode::NOT_FOUND
                    && url.contains("/teams")
                    && message.contains("Could not find teams")
                {
                    trace!("HTTP {status} (expected when not a team member)");
                    return;
                }
            }
        }

        // Log all other errors at error level
        error!("HTTP {status} ({} response bytes)", response_text.len());
    }

    /// Handles error responses from the API.
    fn handle_error_response(status: StatusCode, response_text: String) -> Error {
        if response_text.starts_with("<!DOCTYPE html>") || response_text.starts_with("<html") {
            let title = extract_html_title(&response_text, status);
            Error::StatusText(status, title)
        } else {
            match serde_json::from_str::<serde_json::Value>(&response_text) {
                Ok(json) => {
                    if let Some(message) = json.get("message").and_then(|v| v.as_str()) {
                        Error::StatusText(status, message.to_string())
                    } else {
                        Error::StatusText(status, response_text)
                    }
                }
                Err(_) => Error::StatusText(status, response_text),
            }
        }
    }

    /// Generic GET request for any `Gettable` type.
    pub async fn get<T: Gettable + DeserializeOwned>(
        &self,
        token: &str,
        id: &str,
        host: Option<&str>,
    ) -> Result<T, Error> {
        let host_prefix = self.get_host_prefix(host, "rest");
        let full_url = format!("{host_prefix}/{}/{id}", T::API_ENDPOINT);
        self.get_with_url(token, &full_url).await
    }

    /// Generic single-page LIST request for any `Gettable` type.
    ///
    /// Does not follow `Link` headers.
    pub async fn list<T: Gettable + DeserializeOwned>(
        &self,
        token: &str,
        params: &T::ListParams<'_>,
        host: Option<&str>,
    ) -> Result<Vec<T>, Error> {
        let list_result: ListResult<T> = self
            .get_with_params(token, T::API_ENDPOINT, params, host)
            .await?;

        Ok(list_result
            .items
            .or(list_result.devices)
            .unwrap_or_default())
    }

    // Legacy API methods for compatibility with client/mod.rs

    /// Performs a GET request (legacy API name).
    pub async fn api_get<R: DeserializeOwned>(
        &self,
        rest_method: &str,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'_>,
    ) -> Result<R, Error> {
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(super::REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{rest_method}");

        if let Some(params) = params {
            self.request_with_query("GET", auth, &full_url, Some(&params), BODY_NONE)
                .await
        } else {
            self.request("GET", auth, &full_url, BODY_NONE).await
        }
    }

    /// Performs a POST request with JSON body (legacy API name).
    pub async fn api_post<R: DeserializeOwned>(
        &self,
        rest_method: &str,
        body: impl Serialize,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'_>,
    ) -> Result<R, Error> {
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(super::REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{rest_method}");

        // Params are sent as the query string, like `api_get`.
        self.request_with_query(
            "POST",
            auth,
            &full_url,
            params.as_ref(),
            Some(Body::Json(body)),
        )
        .await
    }

    /// Performs a PUT request with JSON body (legacy API name).
    pub async fn api_put<R: DeserializeOwned>(
        &self,
        rest_method: &str,
        body: impl Serialize,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'_>,
    ) -> Result<R, Error> {
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(super::REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{rest_method}");

        self.request_with_query(
            "PUT",
            auth,
            &full_url,
            params.as_ref(),
            Some(Body::Json(body)),
        )
        .await
    }

    /// Performs a DELETE request (legacy API name).
    pub async fn api_delete(
        &self,
        rest_method: &str,
        params: Option<impl Serialize>,
        auth: AuthorizationType<'_>,
    ) -> Result<(), Error> {
        let url_trimmed = rest_method.split('?').next().unwrap_or(rest_method);
        let prefix = self
            .host_prefix
            .get(url_trimmed)
            .map_or(super::REST_HOST_PREFIX, String::as_str);
        let full_url = format!("{prefix}/{rest_method}");

        let _: EmptyReply = self
            .request_with_query("DELETE", auth, &full_url, params.as_ref(), BODY_NONE)
            .await?;
        Ok(())
    }
}

impl Default for RestClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract title from HTML error page.
fn extract_html_title(html: &str, status: StatusCode) -> String {
    if let (Some(start_pos), Some(end_pos)) = (html.find("<title>"), html.find("</title>")) {
        let start = start_pos + 7;
        if start < end_pos && end_pos <= html.len() {
            return html[start..end_pos].to_string();
        }
    }
    format!("HTTP {} - HTML error page returned", status.as_u16())
}

#[cfg(test)]
#[allow(clippy::significant_drop_tightening)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct PageItem {
        id: String,
    }

    impl Gettable for PageItem {
        const API_ENDPOINT: &'static str = "page-items";
        type ListParams<'a> = [(&'a str, &'a str); 1];
    }

    #[tokio::test]
    async fn list_returns_only_first_page_with_query_params() {
        let mut server = mockito::Server::new_async().await;
        let next_url = format!("{}/page-items?cursor=second", server.url());

        let first = server
            .mock("GET", "/page-items")
            .match_query(mockito::Matcher::UrlEncoded("max".into(), "1".into()))
            .match_header("authorization", "Bearer test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", &format!("<{next_url}>; rel=\"next\""))
            .with_body(r#"{"items":[{"id":"first"}]}"#)
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("GET", "/page-items")
            .match_query(mockito::Matcher::UrlEncoded(
                "cursor".into(),
                "second".into(),
            ))
            .with_status(500)
            .expect(0)
            .create_async()
            .await;

        let client = RestClient::new();
        let items = client
            .list::<PageItem>("test-token", &[("max", "1")], Some(&server.url()))
            .await
            .expect("first page succeeds independently of later pages");

        assert_eq!(items, vec![PageItem { id: "first".into() }]);
        first.assert_async().await;
        second.assert_async().await;
    }

    #[tokio::test]
    async fn post_put_and_delete_send_their_query_params() {
        #[derive(Serialize)]
        struct Params {
            alpha: &'static str,
        }
        let mut server = mockito::Server::new_async().await;
        let query = || mockito::Matcher::UrlEncoded("alpha".into(), "one".into());
        let post = server
            .mock("POST", "/echo")
            .match_query(query())
            .match_body(mockito::Matcher::Json(serde_json::json!({"x": 1})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"posted"}"#)
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/echo")
            .match_query(query())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"put"}"#)
            .create_async()
            .await;
        let delete = server
            .mock("DELETE", "/echo")
            .match_query(query())
            .with_status(204)
            .create_async()
            .await;
        let mut prefixes = HashMap::new();
        prefixes.insert("echo".to_string(), server.url());
        let client = RestClient {
            host_prefix: prefixes,
            ..RestClient::new()
        };
        let auth = || AuthorizationType::Bearer("test-token");
        let params = || Some(Params { alpha: "one" });
        let posted: PageItem = client
            .api_post("echo", serde_json::json!({"x": 1}), params(), auth())
            .await
            .expect("POST with params");
        assert_eq!(posted.id, "posted");
        let updated: PageItem = client
            .api_put("echo", serde_json::json!({"x": 2}), params(), auth())
            .await
            .expect("PUT with params");
        assert_eq!(updated.id, "put");
        client
            .api_delete("echo", params(), auth())
            .await
            .expect("DELETE with params");
        post.assert_async().await;
        put.assert_async().await;
        delete.assert_async().await;
    }
}

use std::env;
use webex::{auth::DeviceAuthenticator, Webex};

const INTEGRATION_CLIENT_ID: &str = "INTEGRATION_CLIENT_ID";
const INTEGRATION_CLIENT_SECRET: &str = "INTEGRATION_CLIENT_SECRET";

#[tokio::main]
async fn main() {
    let client_id = env::var(INTEGRATION_CLIENT_ID)
        .unwrap_or_else(|_| panic!("{INTEGRATION_CLIENT_ID} not specified in environment"));
    let client_secret = env::var(INTEGRATION_CLIENT_SECRET)
        .unwrap_or_else(|_| panic!("{INTEGRATION_CLIENT_SECRET} not specified in environment"));

    let authenticator = DeviceAuthenticator::new(&client_id, &client_secret);

    let verification_token = authenticator
        .verify()
        .await
        .expect("obtaining verification token");

    println!("{}", verification_token.verification_uri_complete);

    let token = authenticator
        .wait_for_authentication(&verification_token)
        .await
        .expect("waiting for authentication");

    let w = Webex::new(&token).await;

    let rooms = w.get_all_rooms().await.expect("obtaning rooms");

    println!("{rooms:#?}");
}

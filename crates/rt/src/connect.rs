//! Finding the API.

use rt_client::{Client, discover};

/// A client for the configured instance.
pub fn connect() -> anyhow::Result<Client> {
    let endpoint = discover()?;
    Ok(Client::new(&endpoint.url, endpoint.token))
}

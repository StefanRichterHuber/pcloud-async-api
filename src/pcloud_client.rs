use crate::pcloud_model::{self, PCloudResult, UserInfo, WithPCloudResult};
use log::{debug, warn};
use reqwest::{Client, RequestBuilder};

#[derive(Clone)]
pub struct PCloudClient {
    pub(crate) api_host: String,
    pub(crate) client: reqwest::Client,
    /// Session auth token (not the OAuth2 token, which is set as default header). Common for all copies of this PCloudClient
    session_token: std::sync::Arc<Option<PCloudClientSession>>,
}

/// Contains the client session opened on login (not necessary for oauth2 sessions)
/// Due to drop implementation, logout automatically happens once the sessions drops
#[derive(Clone, Debug)]
struct PCloudClientSession {
    /// Auth token (not the OAuth2 token, which is set as default header)
    token: String,
    /// Host to connect to pCloud API
    api_host: String,
    /// Client to connect
    client: reqwest::Client,
}

impl PCloudClientSession {
    /// Adds the session token to the query build
    fn add_token(&self, r: RequestBuilder) -> RequestBuilder {
        let token = self.token.clone();
        let result = r.query(&[("auth", token)]);
        return result;
    }
}

impl Drop for PCloudClientSession {
    /// Drop the aquired session token
    fn drop(&mut self) {
        let client = self.client.clone();
        let api_host = self.api_host.clone();
        let token = self.token.clone();

        let op = tokio::spawn(async move {
            let result = PCloudClient::logout(&client, &api_host, &token).await;

            match result {
                Ok(v) => {
                    if v {
                        debug!("Successful logout");
                    } else {
                        warn!("Failed to logout");
                    }
                    return v;
                }
                Err(_) => {
                    warn!("Error on logout");
                    return false;
                }
            }
        });
        // Wait until the lockout thread finished
        futures::executor::block_on(op).unwrap();
    }
}

#[allow(dead_code)]
impl PCloudClient {
    /// Creates a new PCloudClient instance with an already present OAuth 2.0 authentication token. Automatically determines nearest API server for best performance
    pub async fn with_oauth(
        host: &str,
        oauth2: &str,
    ) -> Result<PCloudClient, Box<dyn std::error::Error>> {
        let builder = reqwest::ClientBuilder::new();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            reqwest::header::HeaderValue::from_str(format!("Bearer {}", oauth2).as_str()).unwrap(),
        );

        let client = builder.default_headers(headers).build().unwrap();

        let best_host = PCloudClient::get_best_api_server(&client, host, None).await?;

        Ok(PCloudClient {
            api_host: best_host,
            client: client,
            session_token: std::sync::Arc::new(None),
        })
    }

    /// Creates a new PCloudClient instance using username and password to obtain a temporary auth token. Token is shared between all clones of this instance and revoked when the last instance is dropped. Automatically determines nearest API server for best performance.
    pub async fn with_username_and_password(
        host: &str,
        username: &str,
        password: &str,
    ) -> Result<PCloudClient, Box<dyn std::error::Error>> {
        let token = PCloudClient::login(host, username, password).await?;

        let builder = reqwest::ClientBuilder::new();

        let client = builder.build().unwrap();

        let best_host =
            PCloudClient::get_best_api_server(&client, host, Some(token.clone())).await?;

        let session = PCloudClientSession {
            api_host: best_host.clone(),
            client: client.clone(),
            token: token,
        };

        Ok(PCloudClient {
            api_host: best_host,
            client: client,
            session_token: std::sync::Arc::new(Some(session)),
        })
    }

    /// Performs the login to pCloud using username and password.
    async fn login(
        host: &str,
        username: &str,
        password: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/userinfo?getauth=1", host);

        let client = reqwest::ClientBuilder::new().build()?;

        let mut r = client.get(url);

        r = r.query(&[("username", username)]);
        r = r.query(&[("password", password)]);

        let user_info = r.send().await?.json::<pcloud_model::UserInfo>().await?;

        if user_info.result == PCloudResult::Ok && user_info.auth.is_some() {
            debug!("Successful login for user {}", username);
            Ok(user_info.auth.unwrap())
        } else {
            Err(PCloudResult::AccessDenied)?
        }
    }

    /// Performs the logout for the token aquired with login
    async fn logout(
        client: &Client,
        api_host: &str,
        token: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut r = client.get(format!("{}/logout", api_host));

        r = r.query(&[("auth", token)]);

        let response = r
            .send()
            .await?
            .json::<pcloud_model::LogoutResponse>()
            .await?;

        Ok(response.result == PCloudResult::Ok
            && response.auth_deleted.is_some()
            && response.auth_deleted.unwrap())
    }

    /// If theres is a session token present, add it to the given request.
    pub(crate) fn add_token(&self, r: RequestBuilder) -> RequestBuilder {
        let arc = self.session_token.clone();

        if let Some(ref session) = *arc {
            return session.add_token(r);
        }

        return r;
    }

    // Determine fastest api server for the given default api server (either api.pcloud.com or eapi.pcloud.com)
    async fn get_best_api_server(
        client: &reqwest::Client,
        host: &str,
        session_token: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/getapiserver", host);

        let mut r = client.get(url);

        if let Some(v) = session_token {
            r = r.query(&[("auth", v)]);
        }

        let api_servers = r.send().await?.json::<pcloud_model::ApiServers>().await?;

        let best_host = match api_servers.result {
            pcloud_model::PCloudResult::Ok => {
                let best_host_url = api_servers.api.get(0).unwrap();
                debug!(
                    "Found nearest pCloud API endpoint https://{} for default endpoint {}",
                    best_host_url, host
                );
                format!("https://{}", best_host_url)
            }
            _ => host.to_string(),
        };

        Ok(best_host)
    }

    /// Get user info
    pub async fn get_user_info(&self) -> Result<UserInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/userinfo", self.api_host);
        let mut r = self.client.get(url);

        r = self.add_token(r);

        debug!("Requesting user info");
        let user_info = r.send().await?.json::<UserInfo>().await?.assert_ok()?;

        Ok(user_info)
    }
}

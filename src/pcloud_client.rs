use crate::events::DiffRequestBuilder;
use crate::file_ops::ChecksumFileRequestBuilder;
use crate::file_ops::CopyFileRequestBuilder;
use crate::file_ops::FileDeleteRequestBuilder;
use crate::file_ops::FileDownloadRequestBuilder;
use crate::file_ops::FileStatRequestBuilder;
use crate::file_ops::InitiateSavezipRequestBuilder;
use crate::file_ops::ListRevisionsRequestBuilder;
use crate::file_ops::MoveFileRequestBuilder;
use crate::file_ops::PCloudFile;
use crate::file_ops::PublicFileDownloadRequestBuilder;
use crate::file_ops::PublicFileLinkRequestBuilder;
use crate::file_ops::Tree;
use crate::file_ops::UploadRequestBuilder;
use crate::folder_ops::CopyFolderRequestBuilder;
use crate::folder_ops::CreateFolderRequestBuilder;
use crate::folder_ops::DeleteFolderRequestBuilder;
use crate::folder_ops::ListFolderRequestBuilder;
use crate::folder_ops::MoveFolderRequestBuilder;
use crate::folder_ops::PCloudFolder;
use crate::pcloud_model::RevisionList;
use crate::pcloud_model::{self, FileOrFolderStat, PCloudResult, UserInfo, WithPCloudResult};
use log::{debug, warn};
use reqwest::{Client, RequestBuilder, Response};

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

    /// List events on the users pCloud account.
    /// see https://docs.pcloud.com/methods/general/diff.html for details
    pub fn diff(&self) -> DiffRequestBuilder {
        DiffRequestBuilder::create(self)
    }

    /// Lists the content of a folder. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn list_folder<'a, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: T,
    ) -> Result<ListFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        ListFolderRequestBuilder::for_folder(self, folder_like)
    }

    /// Uploads files into a folder. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn upload_file_into_folder<'a, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: T,
    ) -> Result<UploadRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        UploadRequestBuilder::into_folder(self, folder_like)
    }

    /// Creates a new folder in a parent folder. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn create_folder<'a, T: TryInto<PCloudFolder>>(
        &self,
        parent_folder_like: T,
        name: &str,
    ) -> Result<CreateFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        CreateFolderRequestBuilder::for_folder(self, parent_folder_like, name)
    }

    /// Deletes a folder. Either only if empty or recursively. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn delete_folder<'a, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: T,
    ) -> Result<DeleteFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        DeleteFolderRequestBuilder::for_folder(self, folder_like)
    }

    /// Copies a folder identified by folderid or path to either topath or tofolderid.
    pub fn copy_folder<'a, S: TryInto<PCloudFolder>, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<CopyFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        S::Error: 'a + std::error::Error,
        T::Error: 'a + std::error::Error,
    {
        CopyFolderRequestBuilder::copy_folder(self, folder_like, target_folder_like)
    }

    /// Renames (and/or moves) a folder identified by folderid or path to either topath (if topath is a existing folder to place source folder without new name for the folder it MUST end with slash - /newpath/) or tofolderid/toname (one or both can be provided).
    pub fn move_folder<'a, S: TryInto<PCloudFolder>, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<MoveFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        S::Error: 'a + std::error::Error,
        T::Error: 'a + std::error::Error,
    {
        MoveFolderRequestBuilder::move_folder(self, folder_like, target_folder_like)
    }

    /// Copies the given file to the given folder. Either set a target folder id and then the target with with_new_name or give a full new file path as target path
    pub fn copy_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
        &self,
        file_like: S,
        target_folder_like: T,
    ) -> Result<CopyFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        S::Error: 'a + std::error::Error,
        T::Error: 'a + std::error::Error,
    {
        CopyFileRequestBuilder::copy_file(self, file_like, target_folder_like)
    }

    /// Moves the given file to the given folder. Either set a target folder id and then the target with with_new_name or give a full new file path as target path
    pub fn move_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
        &self,
        file_like: S,
        target_folder_like: T,
    ) -> Result<MoveFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        S::Error: 'a + std::error::Error,
        T::Error: 'a + std::error::Error,
    {
        MoveFileRequestBuilder::move_file(self, file_like, target_folder_like)
    }

    /// Lists revisions for a given fileid / path
    pub async fn list_file_revisions<'a, S: TryInto<PCloudFile>>(
        &self,
        file_like: S,
    ) -> Result<RevisionList, Box<dyn 'a + std::error::Error>>
    where
        S::Error: 'a + std::error::Error,
    {
        ListRevisionsRequestBuilder::for_file(self, file_like)?
            .get()
            .await
    }

    /// Returns the metadata of a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub async fn get_file_metadata<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileOrFolderStat, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileStatRequestBuilder::for_file(self, file_like)?
            .get()
            .await
    }

    /// Requests deleting a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub async fn delete_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileOrFolderStat, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileDeleteRequestBuilder::for_file(self, file_like)?
            .execute()
            .await
    }

    /// Requests the checksums of a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn checksum_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<ChecksumFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        ChecksumFileRequestBuilder::for_file(self, file_like)
    }

    /// Returns the public link for a pCloud file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn get_public_link_for_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<PublicFileLinkRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        PublicFileLinkRequestBuilder::for_file(&self, file_like)
    }

    /// Returns the public download link for a public file link
    pub async fn get_public_download_link_for_file(
        &self,
        link: &pcloud_model::PublicFileLink,
    ) -> Result<pcloud_model::DownloadLink, Box<dyn std::error::Error>> {
        PublicFileDownloadRequestBuilder::for_public_file(self, link.code.clone().unwrap().as_str())
            .get()
            .await
    }

    /// Returns the download link for a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn get_download_link_for_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileDownloadRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileDownloadRequestBuilder::for_file(self, file_like)
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

    /// Downloads a DownloadLink
    pub async fn download_link(
        &self,
        link: &pcloud_model::DownloadLink,
    ) -> Result<Response, Box<dyn std::error::Error>> {
        if let Some(url) = link.into_url() {
            debug!("Downloading file link {}", url);

            // No authentication necessary!
            // r = self.add_token(r);
            let resp = self.client.get(url).send().await?;

            Ok(resp)
        } else {
            Err(PCloudResult::ProvideURL)?
        }
    }

    /// Fetches the download link for the latest file revision and directly downloads the file.  Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub async fn download_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<Response, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let link = self.get_download_link_for_file(file_like)?.get().await?;
        self.download_link(&link).await
    }

    /// Creates a zip file on the remote file system with the content specified by the given Tree
    pub fn create_zip(&self, tree: Tree) -> InitiateSavezipRequestBuilder {
        InitiateSavezipRequestBuilder::zip(self, tree)
    }
}

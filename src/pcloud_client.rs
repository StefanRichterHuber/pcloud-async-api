use crate::pcloud_model::{self, Diff, FileOrFolderStat, Metadata, PCloudResult, PublicFileLink};
use chrono::{DateTime, Utc};
use reqwest::{Error, Response};

/// Generic description of a PCloud File. Either by its file id (preferred) or by its path
pub struct PCloudFile {
    /// ID of the target file
    file_id: Option<u64>,
    /// Path of the target file
    path: Option<String>,
}

/// Convert Strings into pCloud file paths
impl Into<PCloudFile> for &str {
    fn into(self) -> PCloudFile {
        PCloudFile {
            file_id: None,
            path: Some(self.to_string()),
        }
    }
}

/// Convert u64 into pCloud file ids
impl Into<PCloudFile> for u64 {
    fn into(self) -> PCloudFile {
        PCloudFile {
            file_id: Some(self),
            path: None,
        }
    }
}

/// Extract file id from pCloud file metadata
impl TryInto<PCloudFile> for &Metadata {
    type Error = PCloudResult;

    fn try_into(self) -> Result<PCloudFile, PCloudResult> {
        if self.isfolder {
            Err(PCloudResult::InvalidFileOrFolderName)?
        } else {
            Ok(PCloudFile {
                file_id: self.fileid,
                path: None,
            })
        }
    }
}

impl TryInto<PCloudFile> for &FileOrFolderStat {
    type Error = PCloudResult;
    fn try_into(self) -> Result<PCloudFile, PCloudResult> {
        if self.result == PCloudResult::Ok && self.metadata.is_some() {
            let metadata = self.metadata.as_ref().unwrap();
            metadata.try_into()
        } else {
            Err(PCloudResult::InvalidFileOrFolderName)?
        }
    }
}

/// Generic description of a PCloud folder. Either by its file id (preferred) or by its path
pub struct PCloudFolder {
    /// ID of the target folder
    pub folder_id: Option<u64>,
    /// Path of the target folder
    pub path: Option<String>,
}

/// Convert Strings into pCloud folder paths
impl TryInto<PCloudFolder> for &str {
    type Error = PCloudResult;

    fn try_into(self) -> Result<PCloudFolder, PCloudResult> {
        if self.starts_with("/") {
            // File paths must always be absolute paths
            Ok(PCloudFolder {
                folder_id: None,
                path: Some(self.to_string()),
            })
        } else {
            Err(PCloudResult::InvalidPath)?
        }
    }
}

/// Convert u64 into pCloud folder ids
impl Into<PCloudFolder> for u64 {
    fn into(self) -> PCloudFolder {
        PCloudFolder {
            folder_id: Some(self),
            path: None,
        }
    }
}

/// Extract file id from pCloud folder metadata
impl TryInto<PCloudFolder> for &Metadata {
    type Error = PCloudResult;

    fn try_into(self) -> Result<PCloudFolder, PCloudResult> {
        if !self.isfolder {
            Err(PCloudResult::InvalidFileOrFolderName)?
        } else {
            Ok(PCloudFolder {
                folder_id: self.folderid,
                path: None,
            })
        }
    }
}

impl TryInto<PCloudFolder> for &FileOrFolderStat {
    type Error = PCloudResult;

    fn try_into(self) -> Result<PCloudFolder, PCloudResult> {
        if self.result == PCloudResult::Ok && self.metadata.is_some() {
            let metadata = self.metadata.as_ref().unwrap();
            metadata.try_into()
        } else {
            Err(PCloudResult::InvalidPath)?
        }
    }
}

pub struct ListFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///
    path: Option<String>,
    ///  id of the folder
    folder_id: Option<u64>,
    /// If is set full directory tree will be returned, which means that all directories will have contents filed.
    recursive: bool,
    ///  If is set, deleted files and folders that can be undeleted will be displayed.
    showdeleted: bool,
    ///  If is set, only the folder (sub)structure will be returned.
    nofiles: bool,
    /// If is set, only user's own folders and files will be displayed.
    noshares: bool,
}

#[allow(dead_code)]
impl ListFolderRequestBuilder {
    fn for_folder<'a, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<ListFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = folder_like.try_into()?;

        if f.folder_id.is_some() {
            Ok(ListFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                recursive: false,
                showdeleted: false,
                nofiles: false,
                noshares: false,
            })
        } else if f.path.is_some() {
            Ok(ListFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                recursive: false,
                showdeleted: false,
                nofiles: false,
                noshares: false,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// If is set full directory tree will be returned, which means that all directories will have contents filed.
    pub fn recursive(mut self, value: bool) -> ListFolderRequestBuilder {
        self.recursive = value;
        self
    }

    ///  If is set, deleted files and folders that can be undeleted will be displayed.
    pub fn showdeleted(mut self, value: bool) -> ListFolderRequestBuilder {
        self.showdeleted = value;
        self
    }

    ///  If is set, only the folder (sub)structure will be returned.
    pub fn nofiles(mut self, value: bool) -> ListFolderRequestBuilder {
        self.nofiles = value;
        self
    }

    /// If is set, only user's own folders and files will be displayed.
    pub fn noshares(mut self, value: bool) -> ListFolderRequestBuilder {
        self.noshares = value;
        self
    }

    pub async fn get(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/listfolder", self.client.api_host));

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.folder_id.is_some() {
            r = r.query(&[("folderid", self.folder_id.unwrap())]);
        }

        if self.recursive {
            r = r.query(&[("recursive", "1")]);
        }

        if self.showdeleted {
            r = r.query(&[("showdeleted", "1")]);
        }

        if self.nofiles {
            r = r.query(&[("nofiles", "1")]);
        }

        if self.noshares {
            r = r.query(&[("noshares", "1")]);
        }

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(stat)
    }
}

pub struct DiffRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// receive only changes since that diffid.
    diff_id: Option<u64>,
    /// datetime receive only events generated after that time
    after: Option<DateTime<Utc>>,
    /// return last number of events with highest diffids (that is the last events)
    last: Option<u64>,
    /// if set, the connection will block until an event arrives. Works only with diffid
    block: bool,
    /// if provided, no more than limit entries will be returned
    limit: Option<u64>,
}

#[allow(dead_code)]
impl DiffRequestBuilder {
    fn create(client: &PCloudClient) -> DiffRequestBuilder {
        DiffRequestBuilder {
            diff_id: None,
            after: None,
            last: None,
            block: false,
            limit: None,
            client: client.clone(),
        }
    }

    /// receive only changes since that diffid.
    pub fn after_diff_id(mut self, value: u64) -> DiffRequestBuilder {
        self.diff_id = Some(value);
        self
    }

    /// datetime receive only events generated after that time
    pub fn after(mut self, value: DateTime<Utc>) -> DiffRequestBuilder {
        self.after = Some(value);
        self
    }

    ///  return last number of events with highest diffids (that is the last events)
    pub fn only_last(mut self, value: u64) -> DiffRequestBuilder {
        self.last = Some(value);
        self
    }

    /// if set, the connection will block until an event arrives. Works only with diffid
    pub fn block(mut self, value: bool) -> DiffRequestBuilder {
        self.block = value;
        self
    }
    /// if provided, no more than limit entries will be returned
    pub fn limit(mut self, value: u64) -> DiffRequestBuilder {
        self.limit = Some(value);
        self
    }

    pub async fn get(self) -> Result<Diff, Error> {
        let url = format!("{}/diff", self.client.api_host);
        let mut r = self.client.client.get(url);

        if self.diff_id.is_some() {
            r = r.query(&[("diffid", self.diff_id.unwrap())]);
        }

        if self.last.is_some() {
            r = r.query(&[("last", self.last.unwrap())]);
        }

        if self.limit.is_some() {
            r = r.query(&[("limit", self.limit.unwrap())]);
        }

        if self.block {
            r = r.query(&[("block", "1")]);
        }

        if self.after.is_some() {
            r = r.query(&[(
                "after",
                pcloud_model::format_date_time_for_pcloud(self.after.unwrap()),
            )]);
        }
        let diff = r.send().await?.json::<pcloud_model::Diff>().await?;
        Ok(diff)
    }
}

pub struct PublicFileLinkRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// file id of the file for public link
    fileid: Option<u64>,
    /// path to the file for public link
    path: Option<String>,
    /// Datetime when the link will stop working
    expire: Option<DateTime<Utc>>,
    maxdownloads: Option<u64>,
    maxtraffic: Option<u64>,
    shortlink: bool,
    linkpassword: Option<String>,
}

#[allow(dead_code)]
impl PublicFileLinkRequestBuilder {
    fn for_file_id(client: &PCloudClient, file_id: u64) -> PublicFileLinkRequestBuilder {
        PublicFileLinkRequestBuilder {
            fileid: Some(file_id),
            path: None,
            expire: None,
            maxdownloads: None,
            maxtraffic: None,
            shortlink: false,
            linkpassword: None,
            client: client.clone(),
        }
    }

    fn for_file_path(client: &PCloudClient, path: &str) -> PublicFileLinkRequestBuilder {
        PublicFileLinkRequestBuilder {
            fileid: None,
            path: Some(path.to_string()),
            expire: None,
            maxdownloads: None,
            maxtraffic: None,
            shortlink: false,
            linkpassword: None,
            client: client.clone(),
        }
    }
    //  Datetime when the link will stop working
    pub fn expire_link_after(mut self, value: DateTime<Utc>) -> PublicFileLinkRequestBuilder {
        self.expire = Some(value);
        self
    }

    /// Maximum number of downloads for this file
    pub fn with_max_downloads(mut self, value: u64) -> PublicFileLinkRequestBuilder {
        self.maxdownloads = Some(value);
        self
    }

    /// Maximum traffic that this link will consume (in bytes, started downloads will not be cut to fit in this limit)
    pub fn with_max_traffic(mut self, value: u64) -> PublicFileLinkRequestBuilder {
        self.maxtraffic = Some(value);
        self
    }

    ///  If set, a short link will also be generated
    pub fn with_shortlink(mut self, value: bool) -> PublicFileLinkRequestBuilder {
        self.shortlink = value;
        self
    }

    ///  Sets password for the link.
    pub fn with_password(mut self, value: &str) -> PublicFileLinkRequestBuilder {
        self.linkpassword = Some(value.to_string());
        self
    }

    pub async fn get(self) -> Result<PublicFileLink, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getfilepublink", self.client.api_host));

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.fileid.is_some() {
            r = r.query(&[("fileid", self.fileid.unwrap())]);
        }

        if self.maxdownloads.is_some() {
            r = r.query(&[("maxdownloads", self.maxdownloads.unwrap())]);
        }

        if self.linkpassword.is_some() {
            r = r.query(&[("linkpassword", self.linkpassword.unwrap())]);
        }

        if self.maxtraffic.is_some() {
            r = r.query(&[("maxtraffic", self.maxtraffic.unwrap())]);
        }

        if self.shortlink {
            r = r.query(&[("shortlink", "1")]);
        }

        if self.expire.is_some() {
            r = r.query(&[(
                "expire",
                pcloud_model::format_date_time_for_pcloud(self.expire.unwrap()),
            )]);
        }

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::PublicFileLink>()
            .await?;
        Ok(diff)
    }
}

pub struct PublicFileDownloadRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// either 'code' or 'shortcode'
    code: String,
    ///  File id, if the link is to a folder
    fileid: Option<u64>,
}

#[allow(dead_code)]
impl PublicFileDownloadRequestBuilder {
    fn for_public_file(client: &PCloudClient, code: &str) -> PublicFileDownloadRequestBuilder {
        PublicFileDownloadRequestBuilder {
            code: code.to_string(),
            fileid: None,
            client: client.clone(),
        }
    }

    fn for_file_in_public_folder(
        client: &PCloudClient,
        code: &str,
        file_id: u64,
    ) -> PublicFileDownloadRequestBuilder {
        PublicFileDownloadRequestBuilder {
            code: code.to_string(),
            fileid: Some(file_id),
            client: client.clone(),
        }
    }

    pub async fn get(self) -> Result<pcloud_model::DownloadLink, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getpublinkdownload", self.client.api_host));

        r = r.query(&[("code", self.code)]);

        if self.fileid.is_some() {
            r = r.query(&[("fileid", self.fileid.unwrap())]);
        }

        let diff = r.send().await?.json::<pcloud_model::DownloadLink>().await?;
        Ok(diff)
    }
}

struct FileDownloadRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
}

#[allow(dead_code)]
impl FileDownloadRequestBuilder {
    fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<FileDownloadRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(FileDownloadRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    pub async fn get(self) -> Result<pcloud_model::DownloadLink, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getfilelink", self.client.api_host));

        if self.file_id.is_some() {
            r = r.query(&[("fileid", self.file_id.unwrap())]);
        }
        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        let diff = r.send().await?.json::<pcloud_model::DownloadLink>().await?;
        Ok(diff)
    }
}

pub struct FileStatRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
}

#[allow(dead_code)]
impl FileStatRequestBuilder {
    fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<FileStatRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() {
            Ok(FileStatRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else if f.path.is_some() {
            Ok(FileStatRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    pub async fn get(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/stat", self.client.api_host));

        if self.file_id.is_some() {
            r = r.query(&[("fileid", self.file_id.unwrap())]);
        }
        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(diff)
    }
}

#[derive(Clone)]
pub struct PCloudClient {
    api_host: String,
    client: reqwest::Client,
}

#[allow(dead_code)]
impl PCloudClient {
    /// Creates a new PCloudClient instance with OAuth 2.0 authentication. Automatically determines nearest API server for best performance
    pub async fn with_oauth(host: &str, oauth2: &str) -> Result<PCloudClient, Error> {
        let builder = reqwest::ClientBuilder::new();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            reqwest::header::HeaderValue::from_str(format!("Bearer {}", oauth2).as_str()).unwrap(),
        );

        let client = builder.default_headers(headers).build().unwrap();

        let best_host = PCloudClient::get_best_api_server(&client, host).await?;

        Ok(PCloudClient {
            api_host: best_host,
            client: client,
        })
    }

    // Determine fastest api server for the given default api server (either api.pcloud.com or eapi.pcloud.com)
    async fn get_best_api_server(client: &reqwest::Client, host: &str) -> Result<String, Error> {
        let api_servers = client
            .get(format!("{}/getapiserver", host))
            .send()
            .await?
            .json::<pcloud_model::ApiServers>()
            .await?;

        let best_host = match api_servers.result {
            pcloud_model::PCloudResult::Ok => {
                format!("https://{}", api_servers.api.get(0).unwrap())
            }
            _ => host.to_string(),
        };

        Ok(best_host)
    }

    /// List updates of the user's folders/files.
    pub fn diff(&self) -> DiffRequestBuilder {
        DiffRequestBuilder::create(self)
    }

    /// Lists the content of a folder
    pub fn list_folder<'a, T: TryInto<PCloudFolder>>(
        &self,
        folder_like: T,
    ) -> Result<ListFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        ListFolderRequestBuilder::for_folder(self, folder_like)
    }

    /// Returns the metadata of a file
    pub fn get_file_metadata<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileStatRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileStatRequestBuilder::for_file(self, file_like)
    }

    /// Returns the public link for a pCloud file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn get_public_link_for_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<PublicFileLinkRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() {
            Ok(PublicFileLinkRequestBuilder::for_file_id(
                self,
                f.file_id.unwrap(),
            ))
        } else if f.path.is_some() {
            Ok(PublicFileLinkRequestBuilder::for_file_path(
                self,
                f.path.unwrap().as_str(),
            ))
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Returns the public download link for a public file link
    pub async fn get_public_download_link_for_file(
        &self,
        link: &pcloud_model::PublicFileLink,
    ) -> Result<pcloud_model::DownloadLink, Box<dyn std::error::Error>> {
        let result = PublicFileDownloadRequestBuilder::for_public_file(
            self,
            link.code.clone().unwrap().as_str(),
        )
        .get()
        .await?;

        Ok(result)
    }

    /// Returns the download link for a file
    pub async fn get_download_link_for_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<pcloud_model::DownloadLink, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let result = FileDownloadRequestBuilder::for_file(self, file_like)?
            .get()
            .await?;

        Ok(result)
    }

    /// Downloads a DownloadLink
    pub async fn download_link(
        &self,
        link: &pcloud_model::DownloadLink,
    ) -> Result<Response, Box<dyn std::error::Error>> {
        if link.hosts.len() > 0 && link.path.is_some() {
            let url = format!(
                "https://{}{}",
                link.hosts.get(0).unwrap(),
                link.path.as_ref().unwrap()
            );

            let resp = self.client.get(url).send().await?;

            Ok(resp)
        } else {
            Err(PCloudResult::ProvideURL)?
        }
    }

    /// Fetches the download link and directly downloads the file
    pub async fn download_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<Response, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let link = self.get_download_link_for_file(file_like).await?;
        let file = self.download_link(&link).await?;

        Ok(file)
    }
}

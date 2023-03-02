use crate::pcloud_model::{self, Diff, PublicFileLink};
use chrono::{DateTime, Utc};
use reqwest::Error;
use serde::de::IntoDeserializer;

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
        let mut r = self
            .client
            .client
            .get(format!("{}/diff", self.client.api_host));

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

    pub async fn get(self) -> Result<pcloud_model::PublicLinkDownload, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getpublinkdownload", self.client.api_host));

        r = r.query(&[("code", self.code)]);

        if self.fileid.is_some() {
            r = r.query(&[("fileid", self.fileid.unwrap())]);
        }

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::PublicLinkDownload>()
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
    pub fn with_oauth(host: &str, oauth2: &str) -> PCloudClient {
        let builder = reqwest::ClientBuilder::new();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            reqwest::header::HeaderValue::from_str(format!("Bearer {}", oauth2).as_str()).unwrap(),
        );

        let client = builder.default_headers(headers).build().unwrap();
        PCloudClient {
            api_host: host.to_string(),
            client: client,
        }
    }

    /// List updates of the user's folders/files.
    pub fn diff(&self) -> DiffRequestBuilder {
        DiffRequestBuilder::create(self)
    }

    /// Returns the public link for a pCloud file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn get_public_link_for_file<T: TryInto<pcloud_model::PCloudFile>>(
        &self,
        desc: T,
    ) -> Result<PublicFileLinkRequestBuilder, pcloud_model::PCloudResult> {
        let file = desc.try_into();

        match file {
            Ok(f) => {
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
                    Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)
                }
            }
            Err(_) => Err(pcloud_model::PCloudResult::InvalidFileOrFolderName),
        }
    }

    pub async fn get_public_download(
        &self,
        link: &pcloud_model::PublicFileLink,
    ) -> Result<pcloud_model::PublicLinkDownload, Error> {
        PublicFileDownloadRequestBuilder::for_public_file(self, link.code.clone().unwrap().as_str())
            .get()
            .await
    }

    pub async fn get_public_download_for_file(
        &self,
        code: &str,
    ) -> Result<pcloud_model::PublicLinkDownload, Error> {
        PublicFileDownloadRequestBuilder::for_public_file(self, code)
            .get()
            .await
    }

    pub async fn get_public_download_for_file_in_folder(
        &self,
        code: &str,
        file_id: u64,
    ) -> Result<pcloud_model::PublicLinkDownload, Error> {
        PublicFileDownloadRequestBuilder::for_file_in_public_folder(self, code, file_id)
            .get()
            .await
    }
}

use std::fmt::Display;

use crate::pcloud_model::{
    self, Diff, FileOrFolderStat, Metadata, PCloudResult, PublicFileLink, UserInfo,
};
use chrono::{DateTime, TimeZone};
use log::{debug, error, info, log_enabled, warn, Level};
use reqwest::{Body, Client, Error, RequestBuilder, Response};

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
        if self == "/" {
            // Root folder has always id 0
            Ok(PCloudFolder {
                folder_id: Some(0),
                path: None,
            })
        } else if self.starts_with("/") {
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

pub struct DeleteFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Path of the folder
    path: Option<String>,
    ///  id of the folder
    folder_id: Option<u64>,
}

#[allow(dead_code)]
impl DeleteFolderRequestBuilder {
    fn for_folder<'a, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<DeleteFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = folder_like.try_into()?;

        if f.folder_id.is_some() {
            Ok(DeleteFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
            })
        } else if f.path.is_some() {
            Ok(DeleteFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Deletes the folder and all its content recursively
    pub async fn delete_recursive(self) -> Result<pcloud_model::FolderRecursivlyDeleted, Error> {
        let url = format!("{}/deletefolderrecursive", self.client.api_host);

        let mut r = self.client.client.get(url);

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.folder_id.is_some() {
            r = r.query(&[("folderid", self.folder_id.unwrap())]);
        }

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FolderRecursivlyDeleted>()
            .await?;
        Ok(stat)
    }

    /// Deletes the folder, only if  it is empty
    pub async fn delete_folder_only(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let url = format!("{}/deletefolder", self.client.api_host);

        let mut r = self.client.client.get(url);

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.folder_id.is_some() {
            r = r.query(&[("folderid", self.folder_id.unwrap())]);
        }

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(stat)
    }
}

pub struct CreateFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Path of the parent folder
    path: Option<String>,
    ///  id of the parent folder
    folder_id: Option<u64>,
    /// Name of the folder to create
    name: String,
    /// Creates a folder if the folder doesn't exist or returns the existing folder's metadata.
    if_not_exists: bool,
}

#[allow(dead_code)]
impl CreateFolderRequestBuilder {
    fn for_folder<'a, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        folder_like_parent: T,
        name: &str,
    ) -> Result<CreateFolderRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = folder_like_parent.try_into()?;

        if f.folder_id.is_some() {
            Ok(CreateFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                name: name.to_string(),
                if_not_exists: true,
            })
        } else if f.path.is_some() {
            Ok(CreateFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                name: name.to_string(),
                if_not_exists: true,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// If true (default), creates a folder if the folder doesn't exist or returns the existing folder's metadata. If false, creating of the folder fails
    pub fn if_not_exists(mut self, value: bool) -> CreateFolderRequestBuilder {
        self.if_not_exists = value;
        self
    }

    /// Creates the folder
    pub async fn execute(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let url = if self.if_not_exists {
            format!("{}/createfolderifnotexists", self.client.api_host)
        } else {
            format!("{}/createfolder", self.client.api_host)
        };

        let mut r = self.client.client.get(url);

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.folder_id.is_some() {
            r = r.query(&[("folderid", self.folder_id.unwrap())]);
        }

        r = r.query(&[("name", self.name)]);

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(stat)
    }
}

pub struct CopyFileRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// source file path
    from_path: Option<String>,
    /// source file id
    from_file_id: Option<u64>,
    /// destination folder path
    to_path: Option<String>,
    /// destination folder id
    to_folder_id: Option<u64>,
    /// New file name
    to_name: Option<String>,
    /// Overwrite file
    overwrite: bool,
    /// if set, file modified time is set. Have to be unix time seconds.
    mtime: Option<i64>,
    /// if set, file created time is set. It's required to provide mtime to set ctime. Have to be unix time seconds.
    ctime: Option<i64>,
}

#[allow(dead_code)]
impl CopyFileRequestBuilder {
    fn copy_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        file_like: S,
        target_folder_like: T,
    ) -> Result<CopyFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
        S::Error: 'a + std::error::Error,
    {
        let source: PCloudFile = file_like.try_into()?;
        let target: PCloudFolder = target_folder_like.try_into()?;

        if (source.file_id.is_some() || source.path.is_some())
            && (target.folder_id.is_some() || target.path.is_some())
        {
            Ok(CopyFileRequestBuilder {
                from_path: source.path,
                from_file_id: source.file_id,
                to_path: target.path,
                to_folder_id: target.folder_id,
                client: client.clone(),
                to_name: None,
                overwrite: true,
                mtime: None,
                ctime: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }
    // If it is set (default true) and file with the specified name already exists, it will be overwritten
    pub fn overwrite(mut self, value: bool) -> CopyFileRequestBuilder {
        self.overwrite = value;
        self
    }

    /// if set, file modified time is set. Have to be unix time seconds.
    pub fn mtime<Tz>(mut self, value: &DateTime<Tz>) -> CopyFileRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.mtime = Some(value.timestamp());
        self
    }

    ///  if set, file created time is set. It's required to provide mtime to set ctime. Have to be unix time seconds.
    pub fn ctime<Tz>(mut self, value: &DateTime<Tz>) -> CopyFileRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.ctime = Some(value.timestamp());
        self
    }

    /// name of the destination file. If omitted, then the original filename is used
    pub fn with_new_name(mut self, value: &str) -> CopyFileRequestBuilder {
        self.to_name = Some(value.to_string());
        self
    }

    // Finally uploads the file with the given name and the given content
    pub async fn execute(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let mut r = self
            .client
            .client
            .post(format!("{}/copyfile", self.client.api_host));

        if self.from_path.is_some() {
            r = r.query(&[("path", self.from_path.unwrap())]);
        }

        if self.from_file_id.is_some() {
            r = r.query(&[("fileid", self.from_file_id.unwrap())]);
        }

        if self.to_path.is_some() {
            r = r.query(&[("topath", self.to_path.unwrap())]);
        }

        if self.to_folder_id.is_some() {
            r = r.query(&[("tofolderid", self.to_folder_id.unwrap())]);
        }

        if self.mtime.is_some() {
            r = r.query(&[("mtime", self.mtime.unwrap())]);
        }

        if self.ctime.is_some() {
            r = r.query(&[("ctime", self.ctime.unwrap())]);
        }

        if self.to_name.is_some() {
            r = r.query(&[("toname", self.to_name.unwrap())]);
        }

        if !self.overwrite {
            r = r.query(&[("noover", "1")]);
        }

        r = self.client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(result)
    }
}

pub struct MoveFileRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// source file path
    from_path: Option<String>,
    /// source file id
    from_file_id: Option<u64>,
    /// destination folder path
    to_path: Option<String>,
    /// destination folder id
    to_folder_id: Option<u64>,
    /// New file name
    to_name: Option<String>,
}

#[allow(dead_code)]
impl MoveFileRequestBuilder {
    fn move_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        file_like: S,
        target_folder_like: T,
    ) -> Result<MoveFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
        S::Error: 'a + std::error::Error,
    {
        let source: PCloudFile = file_like.try_into()?;
        let target: PCloudFolder = target_folder_like.try_into()?;

        if (source.file_id.is_some() || source.path.is_some())
            && (target.folder_id.is_some() || target.path.is_some())
        {
            Ok(MoveFileRequestBuilder {
                from_path: source.path,
                from_file_id: source.file_id,
                to_path: target.path,
                to_folder_id: target.folder_id,
                client: client.clone(),
                to_name: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// name of the destination file. If omitted, then the original filename is used
    pub fn with_new_name(mut self, value: &str) -> MoveFileRequestBuilder {
        self.to_name = Some(value.to_string());
        self
    }

    // Finally uploads the file with the given name and the given content
    pub async fn execute(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let mut r = self
            .client
            .client
            .post(format!("{}/renamefile", self.client.api_host));

        if self.from_path.is_some() {
            r = r.query(&[("path", self.from_path.unwrap())]);
        }

        if self.from_file_id.is_some() {
            r = r.query(&[("fileid", self.from_file_id.unwrap())]);
        }

        if self.to_path.is_some() {
            r = r.query(&[("topath", self.to_path.unwrap())]);
        }

        if self.to_folder_id.is_some() {
            r = r.query(&[("tofolderid", self.to_folder_id.unwrap())]);
        }

        if self.to_name.is_some() {
            r = r.query(&[("toname", self.to_name.unwrap())]);
        }

        r = self.client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
        Ok(result)
    }
}

pub struct UploadRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Path of the target folder
    path: Option<String>,
    ///  id of the target folder
    folder_id: Option<u64>,
    /// If is set, partially uploaded files will not be saved
    no_partial: bool,
    /// if set, the uploaded file will be renamed, if file with the requested name exists in the folder.
    rename_if_exists: bool,
    /// if set, file modified time is set. Have to be unix time seconds.
    mtime: Option<i64>,
    /// if set, file created time is set. It's required to provide mtime to set ctime. Have to be unix time seconds.
    ctime: Option<i64>,
    /// files to upload
    files: Vec<reqwest::multipart::Part>,
}

#[allow(dead_code)]
impl UploadRequestBuilder {
    fn into_folder<'a, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<UploadRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = folder_like.try_into()?;

        if f.folder_id.is_some() {
            Ok(UploadRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                no_partial: true,
                rename_if_exists: false,
                mtime: None,
                ctime: None,
                files: Vec::new(),
            })
        } else if f.path.is_some() {
            Ok(UploadRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                no_partial: true,
                rename_if_exists: false,
                mtime: None,
                ctime: None,
                files: Vec::new(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    ///  If is set, partially uploaded files will not be saved (defaults to true)
    pub fn no_partial(mut self, value: bool) -> UploadRequestBuilder {
        self.no_partial = value;
        self
    }

    ///  if set, the uploaded file will be renamed, if file with the requested name exists in the folder.
    pub fn rename_if_exists(mut self, value: bool) -> UploadRequestBuilder {
        self.rename_if_exists = value;
        self
    }

    /// if set, file modified time is set. Have to be unix time seconds.
    pub fn mtime<Tz>(mut self, value: &DateTime<Tz>) -> UploadRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.mtime = Some(value.timestamp());
        self
    }

    ///  if set, file created time is set. It's required to provide mtime to set ctime. Have to be unix time seconds.
    pub fn ctime<Tz>(mut self, value: &DateTime<Tz>) -> UploadRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.ctime = Some(value.timestamp());
        self
    }

    /// Adds a file to the upload request
    pub fn with_file<T: Into<Body>>(mut self, file_name: &str, body: T) -> UploadRequestBuilder {
        let file_part = reqwest::multipart::Part::stream(body).file_name(file_name.to_string());
        self.files.push(file_part);
        self
    }

    // Finally uploads the file with the given name and the given content
    pub async fn upload(self) -> Result<pcloud_model::UploadedFile, Error> {
        let mut r = self
            .client
            .client
            .post(format!("{}/uploadfile", self.client.api_host));

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        if self.folder_id.is_some() {
            r = r.query(&[("folderid", self.folder_id.unwrap())]);
        }

        if self.no_partial {
            r = r.query(&[("nopartial", "1")]);
        }

        if self.rename_if_exists {
            r = r.query(&[("renameifexists", "1")]);
        }

        if self.mtime.is_some() {
            r = r.query(&[("mtime", self.mtime.unwrap())]);
        }

        if self.ctime.is_some() {
            r = r.query(&[("ctime", self.ctime.unwrap())]);
        }

        r = self.client.add_token(r);

        let mut form = reqwest::multipart::Form::new();
        for part in self.files {
            form = form.part("part", part);
        }

        r = r.multipart(form);

        let result = r.send().await?.json::<pcloud_model::UploadedFile>().await?;
        Ok(result)
    }
}

pub struct ListFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Path of the folder
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

        r = self.client.add_token(r);

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
    after: Option<String>,
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
    pub fn after<Tz>(mut self, value: &DateTime<Tz>) -> DiffRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.after = Some(pcloud_model::format_date_time_for_pcloud(value));
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

        r = self.client.add_token(r);

        if self.after.is_some() {
            r = r.query(&[("after", self.after.unwrap())]);
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
    expire: Option<String>,
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
    pub fn expire_link_after<Tz>(mut self, value: &DateTime<Tz>) -> PublicFileLinkRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.expire = Some(pcloud_model::format_date_time_for_pcloud(value));
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
            r = r.query(&[("expire", self.expire.unwrap())]);
        }

        r = self.client.add_token(r);

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

        r = self.client.add_token(r);

        let diff = r.send().await?.json::<pcloud_model::DownloadLink>().await?;
        Ok(diff)
    }
}

pub struct ChecksumFileRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
}

#[allow(dead_code)]
impl ChecksumFileRequestBuilder {
    fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<ChecksumFileRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(ChecksumFileRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    pub async fn get(self) -> Result<pcloud_model::FileChecksums, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/checksumfile", self.client.api_host));

        if self.file_id.is_some() {
            r = r.query(&[("fileid", self.file_id.unwrap())]);
        }

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileChecksums>()
            .await?;
        Ok(diff)
    }
}

pub struct FileDeleteRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
}

#[allow(dead_code)]
impl FileDeleteRequestBuilder {
    fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<FileDeleteRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(FileDeleteRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    pub async fn execute(self) -> Result<pcloud_model::FileOrFolderStat, Error> {
        let mut r = self
            .client
            .client
            .get(format!("{}/deletefile", self.client.api_host));

        if self.file_id.is_some() {
            r = r.query(&[("fileid", self.file_id.unwrap())]);
        }

        if self.path.is_some() {
            r = r.query(&[("path", self.path.unwrap())]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?;
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

        r = self.client.add_token(r);

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

        r = self.client.add_token(r);

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
    pub async fn with_oauth(host: &str, oauth2: &str) -> Result<PCloudClient, Error> {
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

    /// Creates a new PCloudClient instance using username and password to obtain a temporary auth token. Token is revoked on drop of this instance.
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

        let userinfo = r.send().await?.json::<pcloud_model::UserInfo>().await?;

        if userinfo.result == PCloudResult::Ok && userinfo.auth.is_some() {
            Ok(userinfo.auth.unwrap())
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
    fn add_token(&self, r: RequestBuilder) -> RequestBuilder {
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
    ) -> Result<String, Error> {
        let url = format!("{}/getapiserver", host);

        let mut r = client.get(url);

        r = r.query(&[("auth", session_token)]);

        let api_servers = r.send().await?.json::<pcloud_model::ApiServers>().await?;

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

    /// Returns the metadata of a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn get_file_metadata<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileStatRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileStatRequestBuilder::for_file(self, file_like)
    }

    /// Requests deleting a file. Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
    pub fn delete_file<'a, T: TryInto<PCloudFile>>(
        &self,
        file_like: T,
    ) -> Result<FileDeleteRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        FileDeleteRequestBuilder::for_file(self, file_like)
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

    /// Returns the download link for a file.  Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
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

    /// Get user info
    pub async fn get_user_info(&self) -> Result<UserInfo, Box<dyn std::error::Error>> {
        let url = format!("{}/userinfo", self.api_host);
        let mut r = self.client.get(url);

        r = self.add_token(r);

        let userinfo = r.send().await?.json::<UserInfo>().await?;

        Ok(userinfo)
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

            let mut r = self.client.get(url);

            r = self.add_token(r);

            let resp = r.send().await?;

            Ok(resp)
        } else {
            Err(PCloudResult::ProvideURL)?
        }
    }

    /// Fetches the download link and directly downloads the file.  Accepts either a file id (u64), a file path (String) or any other pCloud object describing a file (like Metadata)
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

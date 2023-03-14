use std::{fmt::Display, time::Duration};

use crate::{
    folder_ops::PCloudFolder,
    pcloud_client::PCloudClient,
    pcloud_model::{
        self, FileOrFolderStat, Metadata, PCloudResult, PublicFileLink, RevisionList,
        SaveZipProgressResponse, UploadedFile, WithPCloudResult,
    },
};
use chrono::{DateTime, TimeZone};
use log::{debug, warn};
use reqwest::{Body, RequestBuilder};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    time::sleep,
};
use uuid::Uuid;

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

/// Convert Strings into pCloud file paths
impl Into<PCloudFile> for String {
    fn into(self) -> PCloudFile {
        PCloudFile {
            file_id: None,
            path: Some(self),
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

/// Convert u64 into pCloud file ids
impl Into<PCloudFile> for &u64 {
    fn into(self) -> PCloudFile {
        PCloudFile {
            file_id: Some(self.clone()),
            path: None,
        }
    }
}

/// Extract file id from pCloud file or folder metadata response
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

/// Extract file id from pCloud file or folder metadata response
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

/// Some methods can work with trees - that is set of files and folders, where folders can have files and subfolders inside them and so on.
/// see https://docs.pcloud.com/structures/tree.html
#[derive(Debug)]
pub struct Tree {
    /// If set, contents of the folder with the given id will appear as root elements of the three. The folder itself does not appear as a part of the structure.
    folder_id: Option<u64>,
    /// If set, defines one or more folders that will appear as folders in the root folder. If multiple folderids are given, they MUST be separated by coma ,.
    folder_ids: Vec<u64>,
    /// If set, files with corresponding ids will appear in the root folder of the tree structure. If more than one fileid is provided, they MUST be separated by coma ,.
    file_ids: Vec<u64>,
    /// If set, files with corresponding ids will appear in the root folder of the tree structure. If more than one fileid is provided, they MUST be separated by coma ,.
    exclude_folder_ids: Vec<u64>,
    /// If set, defines fileids that are not to be included in the tree structure.
    exclude_file_ids: Vec<u64>,
}

/// Some methods can work with trees - that is set of files and folders, where folders can have files and subfolders inside them and so on.
#[allow(dead_code)]
impl Tree {
    pub fn create() -> Tree {
        Tree {
            folder_id: None,
            folder_ids: Vec::default(),
            file_ids: Vec::default(),
            exclude_folder_ids: Vec::default(),
            exclude_file_ids: Vec::default(),
        }
    }

    /// Adds this tree to a request
    pub(crate) fn add_to_request(&self, mut r: RequestBuilder) -> RequestBuilder {
        if let Some(v) = self.folder_id {
            r = r.query(&[("folderid", v)]);
        }

        if !self.folder_ids.is_empty() {
            let v = self
                .folder_ids
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(",");
            r = r.query(&[("folderids", v)]);
        }

        if !self.file_ids.is_empty() {
            let v = self
                .file_ids
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(",");
            r = r.query(&[("fileids", v)]);
        }

        if !self.exclude_folder_ids.is_empty() {
            let v = self
                .exclude_folder_ids
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(",");
            r = r.query(&[("excludefolderids", v)]);
        }

        if !self.exclude_file_ids.is_empty() {
            let v = self
                .exclude_file_ids
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(",");
            r = r.query(&[("excludefileids", v)]);
        }

        return r;
    }

    /// Adds a file or folder from a metadata object
    pub fn with(self, source: &Metadata) -> Result<Self, Box<dyn std::error::Error>> {
        if source.isfolder {
            self.with_folder(source)
        } else {
            self.with_file(source)
        }
    }

    /// Excludes a file or folder
    pub fn without(self, source: &Metadata) -> Result<Self, Box<dyn std::error::Error>> {
        if source.isfolder {
            self.without_folder(source)
        } else {
            self.without_file(source)
        }
    }

    /// If set, files with corresponding ids will appear in the root folder of the tree structure.
    pub fn with_file<'a, T: TryInto<PCloudFile>>(
        mut self,
        file_like: T,
    ) -> Result<Self, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let file: PCloudFile = file_like.try_into()?;

        if let Some(file_id) = file.file_id {
            self.file_ids.push(file_id);
            Ok(self)
        } else {
            Err(PCloudResult::InvalidFileId)?
        }
    }

    /// If set, defines fileids that are not to be included in the tree structure.
    pub fn without_file<'a, T: TryInto<PCloudFile>>(
        mut self,
        file_like: T,
    ) -> Result<Self, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let file: PCloudFile = file_like.try_into()?;

        if let Some(file_id) = file.file_id {
            self.exclude_file_ids.push(file_id);
            Ok(self)
        } else {
            Err(PCloudResult::InvalidFileId)?
        }
    }

    /// If set, defines one or more folders that will appear as folders in the root folder.
    pub fn with_folder<'a, T: TryInto<PCloudFolder>>(
        mut self,
        folder_like: T,
    ) -> Result<Self, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let folder: PCloudFolder = folder_like.try_into()?;

        if let Some(folder_id) = folder.folder_id {
            self.folder_ids.push(folder_id);
            Ok(self)
        } else {
            Err(PCloudResult::InvalidFolderId)?
        }
    }

    /// If set, folders with the given id will be removed from the tree structure. This is useful when you want to include a folder in the tree structure with some of it's subfolders excluded.
    pub fn without_folder<'a, T: TryInto<PCloudFolder>>(
        mut self,
        folder_like: T,
    ) -> Result<Self, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let folder: PCloudFolder = folder_like.try_into()?;

        if let Some(folder_id) = folder.folder_id {
            self.exclude_folder_ids.push(folder_id);
            Ok(self)
        } else {
            Err(PCloudResult::InvalidFolderId)?
        }
    }

    /// If set, contents of the folder with the given id will appear as root elements of the three. The folder itself does not appear as a part of the structure.
    pub fn with_content_of_folder<'a, T: TryInto<PCloudFolder>>(
        mut self,
        folder_like: T,
    ) -> Result<Self, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let folder: PCloudFolder = folder_like.try_into()?;

        if let Some(folder_id) = folder.folder_id {
            self.folder_id = Some(folder_id);
            Ok(self)
        } else {
            Err(PCloudResult::InvalidFolderId)?
        }
    }
}

pub struct SaveZipRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Tree containing the files / folders to pack
    tree: Tree,
    ///  path where to save the zip archive
    to_path: Option<String>,
    ///  folder id of the folder, where to save the zip archive
    to_folder_id: Option<u64>,
    /// filename of the desired zip archive
    to_name: Option<String>,
    /// key to retrieve the progress for the zipping process
    progress_hash: Option<String>,
}

pub struct InitiateSavezipRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Tree containing the files / folders to pack
    tree: Tree,
}

#[allow(dead_code)]
impl InitiateSavezipRequestBuilder {
    /// Initiates the request
    pub(crate) fn zip(client: &PCloudClient, tree: Tree) -> InitiateSavezipRequestBuilder {
        InitiateSavezipRequestBuilder {
            client: client.clone(),
            tree: tree,
        }
    }

    /// Full path of the zip file to create
    pub fn to_path(self, path: &str) -> SaveZipRequestBuilder {
        SaveZipRequestBuilder {
            client: self.client,
            tree: self.tree,
            to_path: Some(path.to_string()),
            to_folder_id: None,
            to_name: None,
            progress_hash: None,
        }
    }

    /// Target folder and file name of the target zip file
    pub fn to_folder<'a, T: TryInto<PCloudFolder>>(
        self,
        folder_like: T,
        file_name: &str,
    ) -> Result<SaveZipRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f: PCloudFolder = folder_like.try_into()?;

        Ok(SaveZipRequestBuilder {
            client: self.client,
            tree: self.tree,
            to_path: f.path,
            to_folder_id: f.folder_id,
            to_name: Some(file_name.to_string()),
            progress_hash: None,
        })
    }
}

impl SaveZipRequestBuilder {
    /// Get the progress in process of zipping file in the user's filesystem.
    async fn fetch_progress(
        client: &PCloudClient,
        progress_hash: &str,
    ) -> Result<SaveZipProgressResponse, Box<dyn std::error::Error>> {
        let mut r = client
            .client
            .get(format!("{}/savezipprogress", client.api_host));

        r = r.query(&[("progresshash", progress_hash)]);

        r = client.add_token(r);

        let result = r.send().await?.json::<SaveZipProgressResponse>().await?;
        Ok(result)
    }

    /// Get the progress in process of zipping file in the user's filesystem and sends it to the given channel
    async fn fetch_progress_and_send_event(
        client: &PCloudClient,
        progress_hash: &str,
        tx: &Sender<SaveZipProgressResponse>,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let progress = SaveZipRequestBuilder::fetch_progress(client, progress_hash).await?;
        let remaining = progress.totalfiles - progress.files;
        tx.send(progress).await?;

        Ok(remaining)
    }

    ///  Starts creating a zip file in the user's filesystem and notifies the user of the progress
    pub async fn execute_with_progress_notification(
        self,
    ) -> Result<
        (
            pcloud_model::FileOrFolderStat,
            Receiver<SaveZipProgressResponse>,
        ),
        Box<dyn std::error::Error>,
    > {
        let progress_hash = Uuid::new_v4().to_string();
        let progress_client = self.client.clone();

        let req = SaveZipRequestBuilder {
            client: self.client,
            tree: self.tree,
            to_path: self.to_path,
            to_folder_id: self.to_folder_id,
            to_name: self.to_name,
            progress_hash: Some(progress_hash.clone()),
        };
        let result = req.execute().await?;

        let (tx, rx) = mpsc::channel::<SaveZipProgressResponse>(32);

        tokio::spawn(async move {
            loop {
                match SaveZipRequestBuilder::fetch_progress_and_send_event(
                    &progress_client,
                    &progress_hash,
                    &tx,
                )
                .await
                {
                    Ok(remaining) => {
                        if remaining == 0 {
                            break;
                        }
                    }
                    Err(err) => {
                        warn!("Errors during receiving savezipprogress: {}", err);
                    }
                };
                sleep(Duration::from_millis(1000)).await;
            }
        });

        Ok((result, rx))
    }

    /// Starts creating a zip file in the user's filesystem.
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/savezip", self.client.api_host));

        if let Some(v) = self.to_path {
            r = r.query(&[("topath", v)]);
        }

        if let Some(v) = self.to_folder_id {
            r = r.query(&[("tofolderid", v)]);
        }

        if let Some(v) = self.to_name {
            r = r.query(&[("toname", v)]);
        }

        if let Some(v) = self.progress_hash {
            r = r.query(&[("progresshash", v)]);
        }

        r = self.tree.add_to_request(r);

        r = self.client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(result)
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
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl CopyFileRequestBuilder {
    pub(crate) fn copy_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
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
                revision_id: None,
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

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> CopyFileRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    // Execute the copy operation
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .post(format!("{}/copyfile", self.client.api_host));

        if let Some(v) = self.from_path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.from_file_id {
            r = r.query(&[("fileid", v)]);
        }

        if let Some(v) = self.to_path {
            r = r.query(&[("topath", v)]);
        }

        if let Some(v) = self.to_folder_id {
            r = r.query(&[("tofolderid", v)]);
        }

        if let Some(v) = self.mtime {
            r = r.query(&[("mtime", v)]);
        }

        if let Some(v) = self.ctime {
            r = r.query(&[("ctime", v)]);
        }

        if let Some(v) = self.to_name {
            r = r.query(&[("toname", v)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        if !self.overwrite {
            r = r.query(&[("noover", "1")]);
        }

        r = self.client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
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
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl MoveFileRequestBuilder {
    pub(crate) fn move_file<'a, S: TryInto<PCloudFile>, T: TryInto<PCloudFolder>>(
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
                revision_id: None,
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

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> MoveFileRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    // Execute the move operation
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .post(format!("{}/renamefile", self.client.api_host));

        if let Some(v) = self.from_path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.from_file_id {
            r = r.query(&[("fileid", v)]);
        }

        if let Some(v) = self.to_path {
            r = r.query(&[("topath", v)]);
        }

        if let Some(v) = self.to_folder_id {
            r = r.query(&[("tofolderid", v)]);
        }

        if let Some(v) = self.to_name {
            r = r.query(&[("toname", v)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        r = self.client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
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
    pub(crate) fn into_folder<'a, T: TryInto<PCloudFolder>>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<UploadRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = folder_like.try_into()?;

        if f.folder_id.is_some() || f.path.is_some() {
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

    /// Adds a file to the upload request. Multiple files can be added!
    pub fn with_file<T: Into<Body>>(mut self, file_name: &str, body: T) -> UploadRequestBuilder {
        let file_part = reqwest::multipart::Part::stream(body).file_name(file_name.to_string());
        self.files.push(file_part);
        self
    }

    // Finally uploads the files
    pub async fn upload(self) -> Result<UploadedFile, Box<dyn std::error::Error>> {
        if self.files.is_empty() {
            // Short cut operation if no files are configured to upload
            debug!("Requested file upload, but no files are added to the request.");
            let result = UploadedFile {
                result: PCloudResult::Ok,
                fileids: Vec::default(),
                metadata: Vec::default(),
            };
            return Ok(result);
        }

        let mut r = self
            .client
            .client
            .post(format!("{}/uploadfile", self.client.api_host));

        if let Some(v) = self.path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.folder_id {
            r = r.query(&[("folderid", v)]);
        }

        if self.no_partial {
            r = r.query(&[("nopartial", "1")]);
        }

        if self.rename_if_exists {
            r = r.query(&[("renameifexists", "1")]);
        }

        if let Some(v) = self.mtime {
            r = r.query(&[("mtime", v)]);
        }

        if let Some(v) = self.ctime {
            r = r.query(&[("ctime", v)]);
        }

        r = self.client.add_token(r);

        let mut form = reqwest::multipart::Form::new();
        for part in self.files {
            form = form.part("part", part);
        }

        r = r.multipart(form);

        let result = r.send().await?.json::<UploadedFile>().await?.assert_ok()?;
        Ok(result)
    }
}

pub struct PublicFileLinkRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// file id of the file for public link
    file_id: Option<u64>,
    /// path to the file for public link
    path: Option<String>,
    /// Datetime when the link will stop working
    expire: Option<String>,
    max_downloads: Option<u64>,
    max_traffic: Option<u64>,
    short_link: bool,
    link_password: Option<String>,
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl PublicFileLinkRequestBuilder {
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<PublicFileLinkRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f: PCloudFile = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(PublicFileLinkRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
                expire: None,
                max_downloads: None,
                max_traffic: None,
                short_link: false,
                link_password: None,
                revision_id: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
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
        self.max_downloads = Some(value);
        self
    }

    /// Maximum traffic that this link will consume (in bytes, started downloads will not be cut to fit in this limit)
    pub fn with_max_traffic(mut self, value: u64) -> PublicFileLinkRequestBuilder {
        self.max_traffic = Some(value);
        self
    }

    ///  If set, a short link will also be generated
    pub fn with_shortlink(mut self, value: bool) -> PublicFileLinkRequestBuilder {
        self.short_link = value;
        self
    }

    ///  Sets password for the link.
    pub fn with_password(mut self, value: &str) -> PublicFileLinkRequestBuilder {
        self.link_password = Some(value.to_string());
        self
    }

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> PublicFileLinkRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    pub async fn get(self) -> Result<PublicFileLink, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getfilepublink", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting public link for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting public link for file {}", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(v) = self.max_downloads {
            r = r.query(&[("maxdownloads", v)]);
        }

        if let Some(v) = self.link_password {
            r = r.query(&[("linkpassword", v)]);
        }

        if let Some(v) = self.max_traffic {
            r = r.query(&[("maxtraffic", v)]);
        }

        if self.short_link {
            r = r.query(&[("shortlink", "1")]);
        }

        if let Some(v) = self.expire {
            r = r.query(&[("expire", v)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::PublicFileLink>()
            .await?
            .assert_ok()?;
        Ok(diff)
    }
}

pub(crate) struct PublicFileDownloadRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// either 'code' or 'shortcode'
    code: String,
    ///  File id, if the link is to a folder
    file_id: Option<u64>,
}

#[allow(dead_code)]
impl PublicFileDownloadRequestBuilder {
    /// Requests the download of a public file with a given code
    pub(crate) fn for_public_file(
        client: &PCloudClient,
        code: &str,
    ) -> PublicFileDownloadRequestBuilder {
        PublicFileDownloadRequestBuilder {
            code: code.to_string(),
            file_id: None,
            client: client.clone(),
        }
    }

    /// Requests a file from a public folder with a given code
    pub(crate) fn for_file_in_public_folder(
        client: &PCloudClient,
        code: &str,
        file_id: u64,
    ) -> PublicFileDownloadRequestBuilder {
        PublicFileDownloadRequestBuilder {
            code: code.to_string(),
            file_id: Some(file_id),
            client: client.clone(),
        }
    }

    /// Create file download link
    pub async fn get(self) -> Result<pcloud_model::DownloadLink, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getpublinkdownload", self.client.api_host));

        r = r.query(&[("code", self.code)]);

        if let Some(id) = self.file_id {
            r = r.query(&[("fileid", id)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::DownloadLink>()
            .await?
            .assert_ok()?;
        Ok(diff)
    }
}

pub struct ListRevisionsRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
}

impl ListRevisionsRequestBuilder {
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<ListRevisionsRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(ListRevisionsRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Executes the request
    pub async fn get(self) -> Result<RevisionList, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/listrevisions", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting file revisions for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting file revisions for file {}", p);
            r = r.query(&[("path", p)]);
        }

        r = self.client.add_token(r);

        let result = r.send().await?.json::<RevisionList>().await?.assert_ok()?;
        Ok(result)
    }
}

pub struct ChecksumFileRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl ChecksumFileRequestBuilder {
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
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
                revision_id: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> ChecksumFileRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    /// Executes the request
    pub async fn get(self) -> Result<pcloud_model::FileChecksums, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/checksumfile", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting file checksums for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting file checksums for file {}", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileChecksums>()
            .await?
            .assert_ok()?;
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
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
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

    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/deletefile", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting delete for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting delete for file {}", p);
            r = r.query(&[("path", p)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(diff)
    }
}

pub struct FileDownloadRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    ///  ID of the  file
    file_id: Option<u64>,
    /// Path to the  file
    path: Option<String>,
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl FileDownloadRequestBuilder {
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
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
                revision_id: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> FileDownloadRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    /// Fetch the download link for the file
    pub async fn get(self) -> Result<pcloud_model::DownloadLink, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/getfilelink", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting download for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting download for file {}", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::DownloadLink>()
            .await?
            .assert_ok()?;
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
    /// File revision to fetch
    revision_id: Option<u64>,
}

#[allow(dead_code)]
impl FileStatRequestBuilder {
    pub(crate) fn for_file<'a, T: TryInto<PCloudFile>>(
        client: &PCloudClient,
        file_like: T,
    ) -> Result<FileStatRequestBuilder, Box<dyn 'a + std::error::Error>>
    where
        T::Error: 'a + std::error::Error,
    {
        let f = file_like.try_into()?;

        if f.file_id.is_some() || f.path.is_some() {
            Ok(FileStatRequestBuilder {
                file_id: f.file_id,
                path: f.path,
                client: client.clone(),
                revision_id: None,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// Choose the revision of the file. If not set the latest revision is used.
    pub fn with_revision(mut self, value: u64) -> FileStatRequestBuilder {
        self.revision_id = Some(value);
        self
    }

    /// Fetch the file metadata
    pub async fn get(self) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/stat", self.client.api_host));

        if let Some(id) = self.file_id {
            debug!("Requesting file metadata for file {}", id);
            r = r.query(&[("fileid", id)]);
        }

        if let Some(p) = self.path {
            debug!("Requesting file metadata for file {}", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(v) = self.revision_id {
            r = r.query(&[("revisionid", v)]);
        }

        r = self.client.add_token(r);

        let diff = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(diff)
    }
}

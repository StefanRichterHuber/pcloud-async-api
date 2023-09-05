use std::fmt::Display;

use crate::{
    pcloud_client::PCloudClient,
    pcloud_model::{self, FileOrFolderStat, Metadata, PCloudResult, WithPCloudResult},
};
use log::debug;

/// Generic description of a pCloud folder. Either by its file id (preferred) or by its path
pub trait FolderDescriptor {
    /// Convert the descriptor into a PCloudFolder
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult>;
}

impl FolderDescriptor for u64 {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        Ok(self.into())
    }
}

impl FolderDescriptor for &u64 {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        Ok(self.into())
    }
}

impl FolderDescriptor for String {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        self.try_into()
    }
}

impl FolderDescriptor for &str {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        self.try_into()
    }
}

impl FolderDescriptor for &Metadata {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        self.try_into()
    }
}

impl FolderDescriptor for &FileOrFolderStat {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        self.try_into()
    }
}

impl FolderDescriptor for PCloudFolder {
    fn to_folder(self) -> Result<PCloudFolder, PCloudResult> {
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct PCloudFolder {
    /// ID of the target folder
    pub folder_id: Option<u64>,
    /// Path of the target folder
    pub path: Option<String>,
}

impl PCloudFolder {
    pub fn is_empty(&self) -> bool {
        self.folder_id.is_none() && self.path.is_none()
    }
}

impl Display for PCloudFolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(folder_id) = self.folder_id {
            write!(f, "{}", folder_id)
        } else if let Some(path) = &self.path {
            write!(f, "{}", path)
        } else {
            write!(f, "[Empty pCloud folder descriptor!]")
        }
    }
}

/// Convert Strings into pCloud folder paths
impl TryFrom<&str> for PCloudFolder {
    type Error = PCloudResult;

    fn try_from(value: &str) -> Result<PCloudFolder, PCloudResult> {
        if value == "/" {
            // Root folder has always id 0
            Ok(PCloudFolder {
                folder_id: Some(0),
                path: None,
            })
        } else if value.starts_with("/") {
            // File paths must always be absolute paths
            Ok(PCloudFolder {
                folder_id: None,
                path: Some(value.to_string()),
            })
        } else {
            Err(PCloudResult::InvalidPath)?
        }
    }
}

/// Convert Strings into pCloud folder paths
impl TryFrom<String> for PCloudFolder {
    type Error = PCloudResult;

    fn try_from(value: String) -> Result<PCloudFolder, PCloudResult> {
        if value == "/" {
            // Root folder has always id 0
            Ok(PCloudFolder {
                folder_id: Some(0),
                path: None,
            })
        } else if value.starts_with("/") {
            // File paths must always be absolute paths
            Ok(PCloudFolder {
                folder_id: None,
                path: Some(value),
            })
        } else {
            Err(PCloudResult::InvalidPath)?
        }
    }
}

/// Convert u64 into pCloud folder ids
impl From<u64> for PCloudFolder {
    fn from(value: u64) -> PCloudFolder {
        PCloudFolder {
            folder_id: Some(value),
            path: None,
        }
    }
}

/// Convert u64 into pCloud folder ids
impl From<&u64> for PCloudFolder {
    fn from(value: &u64) -> PCloudFolder {
        PCloudFolder {
            folder_id: Some(value.clone()),
            path: None,
        }
    }
}

/// Extract file id from pCloud folder metadata
impl TryFrom<&Metadata> for PCloudFolder {
    type Error = PCloudResult;

    fn try_from(value: &Metadata) -> Result<PCloudFolder, PCloudResult> {
        if !value.isfolder {
            Err(PCloudResult::InvalidFileOrFolderName)?
        } else {
            Ok(PCloudFolder {
                folder_id: value.folderid,
                path: None,
            })
        }
    }
}

/// Extract folder id from pCloud file or folder metadata response
impl TryFrom<&FileOrFolderStat> for PCloudFolder {
    type Error = PCloudResult;

    fn try_from(value: &FileOrFolderStat) -> Result<PCloudFolder, PCloudResult> {
        if value.result == PCloudResult::Ok && value.metadata.is_some() {
            let metadata = value.metadata.as_ref().unwrap();
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
    pub(crate) fn for_folder<'a, T: FolderDescriptor>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<DeleteFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        let f = folder_like.to_folder()?;

        if !f.is_empty() {
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
    pub async fn delete_recursive(
        self,
    ) -> Result<pcloud_model::FolderRecursivlyDeleted, Box<dyn std::error::Error>> {
        let url = format!("{}/deletefolderrecursive", self.client.api_host);

        let mut r = self.client.client.get(url);

        if let Some(p) = self.path {
            debug!("Deleting folder {} recursively", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(id) = self.folder_id {
            debug!("Deleting folder with {} recursively", id);
            r = r.query(&[("folderid", id)]);
        }

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FolderRecursivlyDeleted>()
            .await?
            .assert_ok()?;
        Ok(stat)
    }

    /// Deletes the folder, only if  it is empty
    pub async fn delete_folder_if_empty(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let url = format!("{}/deletefolder", self.client.api_host);

        let mut r = self.client.client.get(url);

        if let Some(p) = self.path {
            debug!("Deleting folder {} if empty", p);
            r = r.query(&[("path", p)]);
        }

        if let Some(id) = self.folder_id {
            debug!("Deleting folder with {} if empty", id);
            r = r.query(&[("folderid", id)]);
        }

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
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
    pub(crate) fn for_folder<'a, T: FolderDescriptor>(
        client: &PCloudClient,
        folder_like_parent: T,
        name: &str,
    ) -> Result<CreateFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        let f = folder_like_parent.to_folder()?;

        if !f.is_empty() {
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
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let url = if self.if_not_exists {
            format!("{}/createfolderifnotexists", self.client.api_host)
        } else {
            format!("{}/createfolder", self.client.api_host)
        };

        let mut r = self.client.client.get(url);

        if let Some(p) = self.path {
            debug!("Creating folder {} in folder {}", self.name, p);
            r = r.query(&[("path", p)]);
        }

        if let Some(id) = self.folder_id {
            debug!("Creating folder {} in folder {}", self.name, id);
            r = r.query(&[("folderid", id)]);
        }

        r = r.query(&[("name", self.name)]);

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(stat)
    }
}

pub struct CopyFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// source file path
    from_path: Option<String>,
    /// source file id
    from_folder_id: Option<u64>,
    /// destination folder path
    to_path: Option<String>,
    /// destination folder id
    to_folder_id: Option<u64>,
    /// New file name
    to_name: Option<String>,
    /// If it is set and files with the same name already exist, overwriting will be preformed (otherwise error 2004 will be returned)
    overwrite: bool,
    /// If set will skip files that already exist
    skip_existing: bool,
    ///  If it is set only the content of source folder will be copied otherwise the folder itself is copied
    copy_content_only: bool,
}

#[allow(dead_code)]
impl CopyFolderRequestBuilder {
    /// Copies a folder identified by folderid or path to either topath or tofolderid.
    pub(crate) fn copy_folder<'a, S: FolderDescriptor, T: FolderDescriptor>(
        client: &PCloudClient,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<CopyFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        let source: PCloudFolder = folder_like.to_folder()?;
        let target: PCloudFolder = target_folder_like.to_folder()?;

        if !source.is_empty() && !target.is_empty() {
            Ok(CopyFolderRequestBuilder {
                from_path: source.path,
                from_folder_id: source.folder_id,
                to_path: target.path,
                to_folder_id: target.folder_id,
                client: client.clone(),
                to_name: None,
                overwrite: true,
                skip_existing: false,
                copy_content_only: false,
            })
        } else {
            Err(pcloud_model::PCloudResult::NoFileIdOrPathProvided)?
        }
    }

    /// If it is set (default true) and files with the same name already exist, overwriting will be preformed (otherwise error 2004 will be returned)
    pub fn overwrite(mut self, value: bool) -> CopyFolderRequestBuilder {
        self.overwrite = value;
        self
    }

    /// If set will skip files that already exist
    pub fn skip_existing(mut self, value: bool) -> CopyFolderRequestBuilder {
        self.skip_existing = value;
        self
    }

    /// If it is set only the content of source folder will be copied otherwise the folder itself is copied
    pub fn copy_content_only(mut self, value: bool) -> CopyFolderRequestBuilder {
        self.copy_content_only = value;
        self
    }

    /// Execute the copy operation
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .post(format!("{}/copyfolder", self.client.api_host));

        if let Some(v) = self.from_path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.from_folder_id {
            r = r.query(&[("folderid", v)]);
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

        if !self.overwrite {
            r = r.query(&[("noover", "1")]);
        }

        if !self.skip_existing {
            r = r.query(&[("skipexisting", "1")]);
        }

        if !self.copy_content_only {
            r = r.query(&[("copycontentonly", "1")]);
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

pub struct MoveFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// source file path
    from_path: Option<String>,
    /// source file id
    from_folder_id: Option<u64>,
    /// destination folder path
    to_path: Option<String>,
    /// destination folder id
    to_folder_id: Option<u64>,
    /// New file name
    to_name: Option<String>,
}

#[allow(dead_code)]
impl MoveFolderRequestBuilder {
    /// Renames (and/or moves) a folder identified by folderid or path to either topath (if topath is a existing folder to place source folder without new name for the folder it MUST end with slash - /newpath/) or tofolderid/toname (one or both can be provided).
    pub(crate) fn move_folder<'a, S: FolderDescriptor, T: FolderDescriptor>(
        client: &PCloudClient,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<MoveFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        let source: PCloudFolder = folder_like.to_folder()?;
        let target: PCloudFolder = target_folder_like.to_folder()?;

        if !source.is_empty() && !target.is_empty() {
            Ok(MoveFolderRequestBuilder {
                from_path: source.path,
                from_folder_id: source.folder_id,
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
    pub fn with_new_name(mut self, value: &str) -> MoveFolderRequestBuilder {
        self.to_name = Some(value.to_string());
        self
    }

    // Execute the move operation
    pub async fn execute(
        self,
    ) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .post(format!("{}/renamefolder", self.client.api_host));

        if let Some(v) = self.from_path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.from_folder_id {
            r = r.query(&[("folderid", v)]);
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

pub struct ListFolderRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// Path of the folder
    path: Option<String>,
    ///  id of the folder
    folder_id: Option<u64>,
    /// If is set full directory tree will be returned, which means that all directories will have contents filed.
    recursive: bool,
    /// If is set, deleted files and folders that can be undeleted will be displayed.
    show_deleted: bool,
    ///  If is set, only the folder (sub)structure will be returned.
    no_files: bool,
    /// If is set, only user's own folders and files will be displayed.
    no_shares: bool,
}

#[allow(dead_code)]
impl ListFolderRequestBuilder {
    pub(crate) fn for_folder<'a, T: FolderDescriptor>(
        client: &PCloudClient,
        folder_like: T,
    ) -> Result<ListFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        let f = folder_like.to_folder()?;

        if !f.is_empty() {
            Ok(ListFolderRequestBuilder {
                folder_id: f.folder_id,
                path: f.path,
                client: client.clone(),
                recursive: false,
                show_deleted: false,
                no_files: false,
                no_shares: false,
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
        self.show_deleted = value;
        self
    }

    ///  If is set, only the folder (sub)structure will be returned.
    pub fn nofiles(mut self, value: bool) -> ListFolderRequestBuilder {
        self.no_files = value;
        self
    }

    /// If is set, only user's own folders and files will be displayed.
    pub fn noshares(mut self, value: bool) -> ListFolderRequestBuilder {
        self.no_shares = value;
        self
    }

    /// Execute list operation
    pub async fn get(self) -> Result<pcloud_model::FileOrFolderStat, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/listfolder", self.client.api_host));

        if let Some(v) = self.path {
            debug!("List folder {}", v);
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.folder_id {
            debug!("List folder {}", v);
            r = r.query(&[("folderid", v)]);
        }

        if self.recursive {
            r = r.query(&[("recursive", "1")]);
        }

        if self.show_deleted {
            r = r.query(&[("showdeleted", "1")]);
        }

        if self.no_files {
            r = r.query(&[("nofiles", "1")]);
        }

        if self.no_shares {
            r = r.query(&[("noshares", "1")]);
        }

        r = self.client.add_token(r);

        let stat = r
            .send()
            .await?
            .json::<pcloud_model::FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(stat)
    }
}

#[allow(dead_code)]
impl PCloudClient {
    /// Lists the content of a folder. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn list_folder<'a, T: FolderDescriptor>(
        &self,
        folder_like: T,
    ) -> Result<ListFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        ListFolderRequestBuilder::for_folder(self, folder_like)
    }

    /// Creates a new folder in a parent folder. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn create_folder<'a, T: FolderDescriptor>(
        &self,
        parent_folder_like: T,
        name: &str,
    ) -> Result<CreateFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        CreateFolderRequestBuilder::for_folder(self, parent_folder_like, name)
    }

    /// Deletes a folder. Either only if empty or recursively. Accepts either a folder id (u64), a folder path (String) or any other pCloud object describing a folder (like Metadata)
    pub fn delete_folder<'a, T: FolderDescriptor>(
        &self,
        folder_like: T,
    ) -> Result<DeleteFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        DeleteFolderRequestBuilder::for_folder(self, folder_like)
    }

    /// Copies a folder identified by folderid or path to either topath or tofolderid.
    pub fn copy_folder<'a, S: FolderDescriptor, T: FolderDescriptor>(
        &self,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<CopyFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        CopyFolderRequestBuilder::copy_folder(self, folder_like, target_folder_like)
    }

    /// Renames (and/or moves) a folder identified by folderid or path to either topath (if topath is a existing folder to place source folder without new name for the folder it MUST end with slash - /newpath/) or tofolderid/toname (one or both can be provided).
    pub fn move_folder<'a, S: FolderDescriptor, T: FolderDescriptor>(
        &self,
        folder_like: S,
        target_folder_like: T,
    ) -> Result<MoveFolderRequestBuilder, Box<dyn 'a + std::error::Error>> {
        MoveFolderRequestBuilder::move_folder(self, folder_like, target_folder_like)
    }

    /// Returns the folder id of a PCloudFolder. If the folder_id is given, just return it. If a path is given, fetch the metadata with the folder id.
    pub(crate) async fn get_folder_id<T: FolderDescriptor>(
        &self,
        folder_like: T,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let folder = folder_like.to_folder()?;

        if let Some(folder_id) = folder.folder_id {
            Ok(folder_id)
        } else {
            let metadata = self
                .list_folder(folder)?
                .recursive(false)
                .nofiles(true)
                .get()
                .await?
                .metadata
                .unwrap();

            if !metadata.isfolder {
                Err(PCloudResult::InvalidFolderId)?
            }

            if let Some(folder_id) = metadata.folderid {
                Ok(folder_id)
            } else {
                Err(PCloudResult::InvalidFolderId)?
            }
        }
    }
}

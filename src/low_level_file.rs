#![cfg(feature = "low_level_file_ops")]
use std::collections::HashSet;

use log::{debug, warn};
use reqwest::Body;

use crate::{
    file_ops::{FileDescriptor, PCloudFile},
    folder_ops::FolderDescriptor,
    pcloud_client::PCloudClient,
    pcloud_model::{FileCloseResponse, FileOpenResponse, FileWriteResponse, WithPCloudResult},
};

impl PCloudClient {
    /// Opens a file for low-level file operations.
    pub fn open_file(&self) -> InitiatePCloudFileOpenRequest {
        InitiatePCloudFileOpenRequest::initiate(self)
    }
}

#[derive(Eq, Hash, PartialEq)]
pub enum PCloudFileFlag {
    /// You do not need to specify O_WRITE even if you intend to write to the file. However that will preform write access control and quota checking and you will get possible errors during open, not at the first write.
    WRITE = 0x0002,
    /// If O_CREAT is set, file_open will create the file. In this case full "path" or "folderid" and "name" MUST be provided for the new file. If the file already exists the old file will be open unless O_EXCL is set, in which case open will fail.
    CREATE = 0x0040,
    EXCL = 0x0080,
    /// O_TRUNC will truncate files when opening existing files.
    TRUNCATE = 0x0200,
    /// Files opened with O_APPEND will always write to the end of file (unless you use pwrite). That is the only reliable method without race conditions for writing in the end of file when there are multiple writers.
    APPEND = 0x0400,
}

impl PCloudFileFlag {
    fn to_number(&self) -> u16 {
        match self {
            PCloudFileFlag::WRITE => 0x0002,
            PCloudFileFlag::CREATE => 0x0040,
            PCloudFileFlag::EXCL => 0x0080,
            PCloudFileFlag::TRUNCATE => 0x0200,
            PCloudFileFlag::APPEND => 0x0400,
        }
    }
}

pub struct InitiatePCloudFileOpenRequest {
    /// Client to actually perform the request
    client: PCloudClient,
}

impl InitiatePCloudFileOpenRequest {
    /// Creates a InitiatePCloudFileOpenRequest
    pub(crate) fn initiate(client: &PCloudClient) -> InitiatePCloudFileOpenRequest {
        InitiatePCloudFileOpenRequest {
            client: client.clone(),
        }
    }

    /// Opens the file by its file id
    pub async fn by_file_id<'a, T: FileDescriptor>(
        self,
        file_like: T,
    ) -> Result<PCloudFileOpenRequest, Box<dyn 'a + std::error::Error>> {
        let file = file_like.to_file()?;
        let file_id = self.client.get_file_id(file).await?;

        Ok(PCloudFileOpenRequest {
            client: self.client,
            flags: HashSet::default(),
            path: None,
            file_id: Some(file_id),
            folder_id: None,
            name: None,
        })
    }

    /// Full path of the file to create / open
    pub fn by_file_path(self, path: &str) -> PCloudFileOpenRequest {
        PCloudFileOpenRequest {
            client: self.client,
            flags: HashSet::default(),
            path: Some(path.to_string()),
            file_id: None,
            folder_id: None,
            name: None,
        }
    }

    /// Target folder and file name of the target  file
    pub async fn by_file_in_folder<'a, T: FolderDescriptor>(
        self,
        folder_like: T,
        file_name: &str,
    ) -> Result<PCloudFileOpenRequest, Box<dyn 'a + std::error::Error>> {
        let folder = folder_like.to_folder()?;
        let folder_id = self.client.get_folder_id(folder).await?;

        Ok(PCloudFileOpenRequest {
            client: self.client,
            flags: HashSet::default(),
            path: None,
            file_id: None,
            folder_id: Some(folder_id),
            name: Some(file_name.to_string()),
        })
    }
}

pub struct PCloudFileOpenRequest {
    /// Client to actually perform the request
    client: PCloudClient,
    /// which can be a combination of the file_open flags.
    flags: HashSet<PCloudFileFlag>,
    ///  path to the file, for which the file descriptor is created.
    path: Option<String>,
    /// id of the folder, for which the file descriptor is created.
    file_id: Option<u64>,
    ///  id of the folder, in which new file is created and file descriptor is returned.
    folder_id: Option<u64>,
    ///  name of the file, in which new file is created and file descriptor is returned.
    name: Option<String>,
}

impl PCloudFileOpenRequest {
    /// Adds a flag to the list of flags
    pub fn with_flag(mut self, flag: PCloudFileFlag) -> PCloudFileOpenRequest {
        self.flags.insert(flag);
        self
    }

    /// Performs the request to open the file
    pub async fn open(self) -> Result<OpenPCloudFile, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .get(format!("{}/file_open", self.client.api_host));

        let flags: u16 = self.flags.iter().map(|f| f.to_number()).sum();

        r = r.query(&[("flags", flags)]);

        if let Some(v) = self.path {
            r = r.query(&[("path", v)]);
        }

        if let Some(v) = self.file_id {
            r = r.query(&[("fileid", v)]);
        }

        if let Some(v) = self.folder_id {
            r = r.query(&[("folderid", v)]);
        }

        if let Some(v) = self.name {
            r = r.query(&[("name", v)]);
        }

        r = self.client.add_token(r);

        let response = r
            .send()
            .await?
            .json::<FileOpenResponse>()
            .await?
            .assert_ok()?;

        let result = OpenPCloudFile {
            client: self.client,
            fd: response.fd,
            file_id: response.fileid,
            open: true,
        };

        Ok(result)
    }
}

/// Currently does not work. Most probably because of connection pooling and: A descriptor is only valid for the same connection. If a connection closes, all the files are also closed. You can open the same file in multiple connections.
pub struct OpenPCloudFile {
    /// Client to actually perform the request
    client: PCloudClient,
    /// File descriptor
    fd: u64,
    /// File id
    file_id: u64,
    /// Is open
    open: bool,
}

#[allow(dead_code)]
impl OpenPCloudFile {
    /// Close the given file
    async fn close_file(
        client: &PCloudClient,
        fd: u64,
    ) -> Result<FileCloseResponse, Box<dyn std::error::Error>> {
        let mut r = client.client.get(format!("{}/file_close", client.api_host));

        r = r.query(&[("fd", fd)]);

        r = client.add_token(r);

        let result = r
            .send()
            .await?
            .json::<FileCloseResponse>()
            .await?
            .assert_ok()?;

        Ok(result)
    }

    /// Close this file (Called by drop)
    async fn close(mut self) -> Result<FileCloseResponse, Box<dyn std::error::Error>> {
        let result = Self::close_file(&self.client, self.fd).await?;
        self.open = false;
        Ok(result)
    }

    /// Write content to file
    pub async fn write<T: Into<Body>>(
        &self,
        body: T,
    ) -> Result<FileWriteResponse, Box<dyn std::error::Error>> {
        let mut r = self
            .client
            .client
            .post(format!("{}/file_write", self.client.api_host));
        r = r.query(&[("fd", self.fd)]);

        r = self.client.add_token(r);

        let part = reqwest::multipart::Part::stream(body);
        let form = reqwest::multipart::Form::new().part("files", part);

        let result = r
            .multipart(form)
            .send()
            .await?
            .json::<FileWriteResponse>()
            .await?
            .assert_ok()?;

        Ok(result)
    }
}

impl Drop for OpenPCloudFile {
    fn drop(&mut self) {
        if self.open {
            let client = self.client.clone();
            let fd = self.fd.clone();
            let file_id = self.file_id.clone();

            let op = tokio::spawn(async move {
                match Self::close_file(&client, fd).await {
                    Ok(_) => {
                        debug!("Successfully closed file with id {}", file_id);
                    }
                    Err(e) => {
                        warn!("Failed to close file with id {}: {:?}", file_id, e);
                    }
                };
            });
            // Wait until the lockout thread finished
            futures::executor::block_on(op).unwrap();
        }
    }
}

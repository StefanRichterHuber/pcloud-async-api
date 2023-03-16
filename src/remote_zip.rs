#![cfg(feature = "remote_zip")]
use std::time::Duration;

use log::warn;
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    time::sleep,
};
use uuid::Uuid;

use crate::{
    file_ops::Tree,
    folder_ops::PCloudFolder,
    pcloud_client::PCloudClient,
    pcloud_model::{FileOrFolderStat, SaveZipProgressResponse, WithPCloudResult},
};

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
    ) -> Result<(FileOrFolderStat, Receiver<SaveZipProgressResponse>), Box<dyn std::error::Error>>
    {
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
    pub async fn execute(self) -> Result<FileOrFolderStat, Box<dyn std::error::Error>> {
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
            .json::<FileOrFolderStat>()
            .await?
            .assert_ok()?;
        Ok(result)
    }
}

impl PCloudClient {
    /// Creates a zip file on the remote file system with the content specified by the given Tree
    pub fn create_zip(&self, tree: Tree) -> InitiateSavezipRequestBuilder {
        InitiateSavezipRequestBuilder::zip(self, tree)
    }
}

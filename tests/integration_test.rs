use chrono::DateTime;
use log::info;
use pcloud_async_api::{
    self,
    pcloud_model::{DiffEntry, DiffEvent, PCloudResult},
};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

async fn get_client(
) -> Result<pcloud_async_api::pcloud_client::PCloudClient, Box<dyn std::error::Error>> {
    let host = std::env::var("PCLOUD_HOST")?;
    let user = std::env::var("PCLOUD_USER")?;
    let pw = std::env::var("PCLOUD_PASSWORD")?;

    let pcloud = pcloud_async_api::pcloud_client::PCloudClient::with_username_and_password(
        &host, &user, &pw,
    )
    .await?;

    Ok(pcloud)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_event_stream() -> Result<(), Box<dyn std::error::Error>> {
    // Lets wait some time to avoid previous events to be shown (due to times not in sync between client and server)
    sleep(Duration::from_millis(5000)).await;

    let now = chrono::offset::Local::now();

    let pcloud = get_client().await?;

    let mut events = pcloud
        .diff()
        .limit(32)
        .after(&now)
        .block_timeout(Duration::from_secs(1))
        .stream();

    // Lets wait some time to avoid missed events due to times not in sync between client and server
    sleep(Duration::from_millis(500)).await;

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<DiffEntry>>();

    tokio::spawn(async move {
        let mut result = Vec::default();

        while let Some(event) = events.recv().await {
            result.push(event);
            // 4 events: folder created, file created, file deleted, folder deleted
            if result.len() == 4 {
                break;
            }
        }
        tx.send(result).unwrap();
        events.close();
    });

    let folder_name = Uuid::new_v4().to_string();
    // Create test folder
    let createfolder_result = pcloud.create_folder("/", &folder_name)?.execute().await?;

    assert_eq!(PCloudResult::Ok, createfolder_result.result);
    assert_eq!(
        folder_name,
        createfolder_result.metadata.as_ref().unwrap().name
    );
    info!("Created test folder {}", folder_name);

    // Upload file content
    let upload_result = pcloud
        .upload_file_into_folder(format!("/{}", folder_name))?
        .with_file("test.txt", "This is nice test content")
        .upload()
        .await?;

    assert_eq!(PCloudResult::Ok, upload_result.result);
    assert_eq!("test.txt", upload_result.metadata.get(0).unwrap().name);

    // Delete test folder
    let deletefolder_result = pcloud
        .delete_folder(&createfolder_result.metadata.unwrap())?
        .delete_recursive()
        .await?;
    assert_eq!(PCloudResult::Ok, deletefolder_result.result);
    info!("Deleted folder {}", folder_name);

    // After closing the receiving event channel on has to wait long enough to run in to a timeout configured previously, otherwise there will be runtime errors droping the connection
    sleep(Duration::from_millis(2000)).await;

    // Check if the correct events have arrived
    let result = rx.await?;
    assert_eq!(DiffEvent::CreateFolder, result.get(0).unwrap().event);
    assert_eq!(
        folder_name,
        result.get(0).unwrap().metadata.as_ref().unwrap().name
    );
    assert_eq!(DiffEvent::CreateFile, result.get(1).unwrap().event);
    assert_eq!(
        "test.txt",
        result.get(1).unwrap().metadata.as_ref().unwrap().name
    );
    assert_eq!(DiffEvent::DeleteFile, result.get(2).unwrap().event);
    assert_eq!(
        "test.txt",
        result.get(2).unwrap().metadata.as_ref().unwrap().name
    );
    assert_eq!(DiffEvent::DeleteFolder, result.get(3).unwrap().event);
    assert_eq!(
        folder_name,
        result.get(3).unwrap().metadata.as_ref().unwrap().name
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_file_revisions() -> Result<(), Box<dyn std::error::Error>> {
    let folder_name = Uuid::new_v4().to_string();

    let pcloud = get_client().await?;

    // Create test folder
    let createfolder_result = pcloud.create_folder("/", &folder_name)?.execute().await?;

    assert_eq!(PCloudResult::Ok, createfolder_result.result);
    assert_eq!(
        folder_name,
        createfolder_result.metadata.as_ref().unwrap().name
    );
    info!("Created test folder {}", folder_name);

    // Upload file content
    let upload_result = pcloud
        .upload_file_into_folder(format!("/{}", folder_name))?
        .with_file("test.txt", "This is nice test content")
        .upload()
        .await?;

    assert_eq!(PCloudResult::Ok, upload_result.result);
    assert_eq!("test.txt", upload_result.metadata.get(0).unwrap().name);

    // Overwrite file content
    let upload_result1 = pcloud
        .upload_file_into_folder(format!("/{}", folder_name))?
        .with_file(
            "test.txt",
            "This is nice test content: We are experts in that!",
        )
        .upload()
        .await?;

    assert_eq!(PCloudResult::Ok, upload_result1.result);
    assert_eq!("test.txt", upload_result1.metadata.get(0).unwrap().name);

    sleep(Duration::from_millis(200)).await;

    // Check file rev
    let file_rev = pcloud
        .list_file_revisions(format!("/{}/{}", folder_name, "test.txt"))
        .await?;

    assert_eq!(PCloudResult::Ok, file_rev.result);
    assert_eq!(1, file_rev.revisions.len());

    // Download old rev
    let link = pcloud
        .get_download_link_for_file(format!("/{}/{}", folder_name, "test.txt"))?
        .with_revision(file_rev.revisions.get(0).unwrap().revisionid)
        .get()
        .await?;
    let old_content = pcloud.download_link(&link).await?.text().await?;
    assert_eq!("This is nice test content", old_content);

    // Delete test folder
    let deletefolder_result = pcloud
        .delete_folder(&createfolder_result.metadata.unwrap())?
        .delete_recursive()
        .await?;
    assert_eq!(PCloudResult::Ok, deletefolder_result.result);
    info!("Deleted folder {}", folder_name);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_file_operations() -> Result<(), Box<dyn std::error::Error>> {
    let folder_name = Uuid::new_v4().to_string();

    let pcloud = get_client().await?;

    let date =
        DateTime::parse_from_str("2023 Jan 25 12:09:14.000 +0000", "%Y %b %d %H:%M:%S%.3f %z")
            .unwrap();

    // Create test folder
    let createfolder_result = pcloud.create_folder("/", &folder_name)?.execute().await?;

    assert_eq!(PCloudResult::Ok, createfolder_result.result);
    assert_eq!(
        folder_name,
        createfolder_result.metadata.as_ref().unwrap().name
    );
    info!("Created test folder {}", folder_name);

    // Upload two files
    let upload_result = pcloud
        .upload_file_into_folder(format!("/{}", folder_name))?
        .rename_if_exists(false)
        .mtime(&date)
        .with_file("test.txt", "This is nice test content")
        .with_file("second test.txt", "This is another nice test content")
        .upload()
        .await?;

    assert_eq!(PCloudResult::Ok, upload_result.result);
    assert_eq!(2, upload_result.fileids.len());
    assert_eq!(2, upload_result.metadata.len());
    assert_eq!("test.txt", upload_result.metadata.get(0).unwrap().name);
    assert_eq!(
        "second test.txt",
        upload_result.metadata.get(1).unwrap().name
    );
    assert_eq!(date, upload_result.metadata.get(0).unwrap().modified);
    assert_eq!(date, upload_result.metadata.get(1).unwrap().modified);
    info!("Created test files: {:?}", upload_result.fileids);

    let file_id = upload_result.fileids.get(0).unwrap();
    let file_id2 = upload_result.fileids.get(1).unwrap();

    // Download file
    let download_result = pcloud.download_file(file_id).await?.text().await?;
    assert_eq!("This is nice test content", download_result);

    let download_result2 = pcloud.download_file(file_id2).await?.text().await?;
    assert_eq!("This is another nice test content", download_result2);
    info!("Downloaded files");

    // Get file metadata
    let metadata = pcloud.get_file_metadata(file_id).await?;
    assert_eq!(PCloudResult::Ok, metadata.result);
    assert_eq!("test.txt", metadata.metadata.as_ref().unwrap().name);
    info!(
        "Downloaded file metadata of {}",
        metadata.metadata.as_ref().unwrap().name
    );

    // Copy one file
    let copy_result = pcloud
        .copy_file(file_id, format!("/{}/anothertext.txt", folder_name))?
        .overwrite(true)
        .execute()
        .await?;
    assert_eq!(PCloudResult::Ok, copy_result.result);
    assert_eq!(
        "anothertext.txt",
        copy_result.metadata.as_ref().unwrap().name
    );

    let d1 = pcloud
        .download_file(copy_result.metadata.as_ref().unwrap())
        .await?
        .text()
        .await?;
    assert_eq!("This is nice test content", d1);
    info!(
        "Copied file {} -> {}",
        file_id,
        copy_result.metadata.as_ref().unwrap().name
    );

    // Move one file
    let move_result = pcloud
        .move_file(file_id2, format!("/{}/third test.txt", folder_name))?
        .execute()
        .await?;
    assert_eq!(PCloudResult::Ok, move_result.result);
    assert_eq!(
        "third test.txt",
        move_result.metadata.as_ref().unwrap().name
    );

    let d2 = pcloud
        .download_file(move_result.metadata.as_ref().unwrap())
        .await?
        .text()
        .await?;
    assert_eq!("This is another nice test content", d2);
    info!(
        "Moved file {} -> {}",
        file_id2,
        move_result.metadata.as_ref().unwrap().name
    );

    // Fetch checksums
    let checksum_result = pcloud.checksum_file(file_id)?.get().await?;
    assert_eq!(PCloudResult::Ok, checksum_result.result);
    info!("Fetched checksums");

    sleep(Duration::from_millis(500)).await;

    // List folder content
    let folder_content = pcloud
        .list_folder(createfolder_result.metadata.as_ref().unwrap())?
        .recursive(true)
        .get()
        .await?;
    assert_eq!(PCloudResult::Ok, folder_content.result);
    let files: Vec<String> = folder_content
        .metadata
        .as_ref()
        .unwrap()
        .contents
        .iter()
        .map(|m| m.name.clone())
        .collect();

    assert_eq!(3, files.len());
    assert_eq!(true, files.contains(&String::from("test.txt")));
    assert_eq!(true, files.contains(&String::from("third test.txt")));
    assert_eq!(true, files.contains(&String::from("anothertext.txt")));
    // File was moved, so it should no long exist
    assert_eq!(false, files.contains(&String::from("second test.txt")));
    info!("Listed folder content: {:?}", files);

    // Delete test file
    let delete_result = pcloud.delete_file(file_id).await?;
    assert_eq!(PCloudResult::Ok, delete_result.result);
    info!("Deleted file {}", delete_result.metadata.unwrap().name);

    // Delete test folder
    let deletefolder_result = pcloud
        .delete_folder(&createfolder_result.metadata.unwrap())?
        .delete_recursive()
        .await?;
    assert_eq!(PCloudResult::Ok, deletefolder_result.result);
    info!("Deleted folder {}", folder_name);

    Ok(())
}

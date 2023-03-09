use chrono::DateTime;
use log::info;
use pcloud_async_api::{self, pcloud_model::PCloudResult};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_file_operations() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let folder_name = Uuid::new_v4().to_string();

    let pcloud = get_client().await?;

    let date =
        DateTime::parse_from_str("2023 Jan 25 12:09:14.274 +0000", "%Y %b %d %H:%M:%S%.3f %z")
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
    // Does not work because of small rounding errors
    //assert_eq!(date, upload_result.metadata.get(0).unwrap().modified);
    //assert_eq!(date, upload_result.metadata.get(1).unwrap().modified);
    info!("Created test files");

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
    assert_eq!("test.txt", metadata.metadata.unwrap().name);
    info!("Downloaded file metadata files");

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
        .download_file(&copy_result.metadata.unwrap())
        .await?
        .text()
        .await?;
    assert_eq!("This is nice test content", d1);
    info!("Copied file");

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
        .download_file(&move_result.metadata.unwrap())
        .await?
        .text()
        .await?;
    assert_eq!("This is another nice test content", d2);
    info!("Moved file");

    // Fetch checksums
    let checksum_result = pcloud.checksum_file(file_id).await?;
    assert_eq!(PCloudResult::Ok, checksum_result.result);
    info!("Fetched checksums");

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
    info!("Listed folder content");

    // Delete test file
    let delete_result = pcloud.delete_file(file_id).await?;
    assert_eq!(PCloudResult::Ok, delete_result.result);
    info!("Deleted file");

    // Delete test folder
    let deletefolder_result = pcloud
        .delete_folder(&createfolder_result.metadata.unwrap())?
        .delete_recursive()
        .await?;
    assert_eq!(PCloudResult::Ok, deletefolder_result.result);
    info!("Deleted folder");

    Ok(())
}

use chrono::DateTime;
use pcloud_async_api::{self, pcloud_model::PCloudResult};

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

    let pcloud = get_client().await?;

    let date =
        DateTime::parse_from_str("2023 Jan 25 12:09:14.274 +0000", "%Y %b %d %H:%M:%S%.3f %z")
            .unwrap();

    // Create test folder
    let createfolder_result = pcloud.create_folder("/", "test-folder")?.execute().await?;

    assert_eq!(PCloudResult::Ok, createfolder_result.result);
    assert_eq!(
        "test-folder",
        createfolder_result.metadata.as_ref().unwrap().name
    );

    // Upload two files
    let upload_result = pcloud
        .upload_file_into_folder("/test-folder")?
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

    let file_id = upload_result.fileids.get(0).unwrap();

    // Download one file
    let download_result = pcloud.download_file(file_id).await?.text().await?;
    assert_eq!("This is nice test content", download_result);

    // Copy one file
    let copy_result = pcloud
        .copy_file(file_id, "/test-folder/anothertext.txt")?
        .overwrite(true)
        .execute()
        .await?;
    assert_eq!(PCloudResult::Ok, copy_result.result);
    assert_eq!(
        "anothertext.txt",
        copy_result.metadata.as_ref().unwrap().name
    );

    // Fetch checksums
    let checksum_result = pcloud.checksum_file(file_id).await?;
    assert_eq!(PCloudResult::Ok, checksum_result.result);

    // Delete test file
    let delete_result = pcloud.delete_file(file_id).await?;
    assert_eq!(PCloudResult::Ok, delete_result.result);

    // Delete test folder
    let deletefolder_result = pcloud
        .delete_folder(&createfolder_result.metadata.unwrap())?
        .delete_recursive()
        .await?;
    assert_eq!(PCloudResult::Ok, deletefolder_result.result);

    Ok(())
}

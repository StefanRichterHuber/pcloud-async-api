[![Rust](https://github.com/StefanRichterHuber/pcloud-async-api/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/StefanRichterHuber/pcloud-async-api/actions/workflows/rust.yml)

# Rust client for pCloud

This is Rust client for the pCloud rest api as described in the [pCloud API documention](https://docs.pcloud.com/). It uses async reqwest.

## Entry point

> In some details pCloud treats european customers and international customers differently, especially on checksums. While european customers get SHA-1 and SHA-256 checksums for their files, international customers only get MD5 and SHA-1 checksums. Keep this mind when designing code using checksums. See [pCloud checksums](https://docs.pcloud.com/methods/file/checksumfile.html) for more details.

Main entry point is the `PCloudClient` struct. It has to factory methods. The first one `PCloudClient::with_oauth()` takes a host name (either `https://api.pcloud.com` or `https://eapi.pcloud.com` for european customers) and a ready-to-use OAuth2 token. See documentation for details.

```rust

     let pcloud = PCloudClient::with_oauth(
         "https://eapi.pcloud.com",
         "[OAUTH2_TOKEN]",
     )
     .await?;
```

The second entry point is the `PCloudClient::with_username_and_password()` function, which takes a host name (see above) and the pCloud username and password. It creates a temporary session authentication token, which is shared within all clones of the `PCloudClient` and dropped after the last copy of the `PCloudClient` instances was dropped.

```rust

   let pcloud = PCloudClient::with_username_and_password(
        "https://eapi.pcloud.com",
        "[EMAIL_OF_USER]",
        "[PASSWORD_OF_USER]",
    )
    .await?;
```

After creating a `PCloudClient` instance one, could all methods to creates folders and files, get metadata, move and copy folders and files and so on. If optional parameters are possible builder pattern is used to supply the parameters.
Since pCloud accepts both a full path (`String` starting with `/`) or a unique id (`u64`, preferred) to identify its files or folders, all methods accepts both.

```rust
    let upload_result = pcloud
        .upload_file_into_folder("/test-folder")?
        .rename_if_exists(false)
        // Supports uploading multiple files at once!
        .with_file("test.txt", "This is nice test content") 
        .with_file("second test.txt", "This is another nice test content")
        .upload()
        .await?;
    println!("Files uploaded: {:?}", upload_result);

```

```rust
    let download_result = pcloud
        .download_file("/test-folder/test.txt")
        .await?
        .text() // Build-in response conversion of reqwest.
        .await?;
    assert_eq!("This is nice test content", download_result);
```

## Tests

There is an integration test in place to test (almost) all provided functionality. Prior to running the tests it is necessary to provide some environment variables containing the necessary authentication.

| Variable          | Description |
|-------------------| ------------|
| `PCLOUD_HOST`     | API Host. Either `https://api.pcloud.com` for international customers or `https://eapi.pcloud.com` for european customers. |
| `PCLOUD_USER`     | pCloud username. Usually the mail address of the user. |
| `PCLOUD_PASSWORD` | pCloud password. |

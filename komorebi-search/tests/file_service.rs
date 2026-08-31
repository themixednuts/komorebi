use komorebi_search::FileSearchLimit;
use komorebi_search::FileSearchQueueCapacity;
use komorebi_search::FileSearchRequestError;
use komorebi_search::FileSearchService;

#[tokio::test]
async fn owned_service_searches_resolves_and_stops_external_clients()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("command-palette.rs");
    std::fs::write(&path, b"fn palette() {}")?;
    let service = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(8).ok_or("eight is a valid queue capacity")?,
    )
    .await?;
    let client = service.client();

    let matches = client
        .search(
            "palete",
            FileSearchLimit::new(10).ok_or("ten is a valid result limit")?,
        )
        .await?;
    let selected = matches.first().ok_or("typo should match indexed file")?;
    assert_eq!(selected.display_path(), "command-palette.rs");
    assert_eq!(client.resolve(selected.id().clone()).await?, Some(path));

    service.shutdown().await?;
    assert!(matches!(
        client
            .search(
                "palette",
                FileSearchLimit::new(1).ok_or("one is a valid result limit")?,
            )
            .await,
        Err(FileSearchRequestError::Stopped)
    ));
    Ok(())
}

#[tokio::test]
async fn cancelling_one_caller_does_not_cancel_or_poison_the_index()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("survivor.txt"), b"still indexed")?;
    let service = FileSearchService::start(
        directory.path().to_path_buf(),
        FileSearchQueueCapacity::new(1).ok_or("one is a valid queue capacity")?,
    )
    .await?;
    let client = service.client();

    let abandoned_client = client.clone();
    let abandoned_limit = FileSearchLimit::new(1).ok_or("one is a valid result limit")?;
    let abandoned = tokio::spawn(async move {
        abandoned_client
            .search("never-observed", abandoned_limit)
            .await
    });
    tokio::task::yield_now().await;
    abandoned.abort();
    let _ = abandoned.await;

    let matches = client
        .search(
            "survivor",
            FileSearchLimit::new(1).ok_or("one is a valid result limit")?,
        )
        .await?;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].display_path(), "survivor.txt");
    service.shutdown().await?;
    Ok(())
}

use komorebi_shell::WebSearchEndpoint;
use komorebi_shell::WebSearchEndpointError;
use komorebi_shell::WebSearchRequest;
#[cfg(windows)]
use komorebi_shell::WebUriSupport;
#[cfg(windows)]
use komorebi_shell::WindowsWebLauncher;

#[test]
fn configured_https_endpoint_builds_one_encoded_search_target()
-> Result<(), Box<dyn std::error::Error>> {
    let endpoint = WebSearchEndpoint::new("https://search.example/results?source=komorebi", "q")?;
    let request = WebSearchRequest::new("rust windows & gpu")
        .ok_or("nonempty terms should produce a search request")?;

    let target = endpoint.target(&request);

    assert_eq!(target.scheme(), "https");
    assert_eq!(target.host(), Some("search.example"));
    assert_eq!(
        target.as_str(),
        "https://search.example/results?source=komorebi&q=rust+windows+%26+gpu"
    );
    Ok(())
}

#[test]
fn endpoint_rejects_ambient_authority_and_ambiguous_query_configuration() {
    assert!(matches!(
        WebSearchEndpoint::new("http://search.example/", "q"),
        Err(WebSearchEndpointError::HttpsRequired)
    ));
    assert!(matches!(
        WebSearchEndpoint::new("https://user:secret@search.example/", "q"),
        Err(WebSearchEndpointError::CredentialsForbidden)
    ));
    assert!(matches!(
        WebSearchEndpoint::new("https://search.example/#fragment", "q"),
        Err(WebSearchEndpointError::FragmentForbidden)
    ));
    assert!(matches!(
        WebSearchEndpoint::new("https://search.example/?q=existing", "q"),
        Err(WebSearchEndpointError::DuplicateQueryParameter)
    ));
    assert!(matches!(
        WebSearchEndpoint::new("https://search.example/", ""),
        Err(WebSearchEndpointError::InvalidQueryParameter)
    ));
}

#[cfg(windows)]
#[tokio::test]
async fn windows_reports_a_registered_https_handler_without_launching_it()
-> Result<(), Box<dyn std::error::Error>> {
    let endpoint = WebSearchEndpoint::new("https://example.com/search", "q")?;
    let request = WebSearchRequest::new("native shell").ok_or("terms should be nonempty")?;
    let target = endpoint.target(&request);

    let support = WindowsWebLauncher::query_support(&target).await?;

    assert_eq!(support, WebUriSupport::Available);
    Ok(())
}

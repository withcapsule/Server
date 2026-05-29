// Requires the server to be running on localhost:9001

const BASE_URL: &str = "http://localhost:9001";

#[tokio::test]
async fn test_download_nonexistent_id_returns_404() {
    let client = reqwest::Client::new();

    let res = client
        .get( format!( "{}/download/00000", BASE_URL ) )
        .send()
        .await
        .expect( "Request failed" );

    assert_eq!( res.status(), 404 );
}

#[tokio::test]
async fn test_upload_no_file_returns_400() {
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new();

    let res = client
        .post( format!( "{}/curlup", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .expect( "Request failed" );

    assert_eq!( res.status(), 400 );
}

#[tokio::test]
async fn test_search_nonexistent_id_returns_error() {
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text( "file_download_field", "00000" );

    let res = client
        .post( format!( "{}/html_download_processor", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .expect( "Request failed" );

    assert!(
        res.status() == 404 || res.status() == 500,
        "Expected 404 or 500, got {}",
        res.status()
    );
}

#[tokio::test]
async fn test_ping() {
    let client = reqwest::Client::new();

    let res = client
        .get( format!( "{}/ping", BASE_URL ) )
        .send()
        .await
        .expect( "Request failed" );

    assert_eq!( res.status(), 200 );

    let body = res.text().await.unwrap();
    assert!( body.contains( "pong" ), "Expected pong in response, got: {}", body );
}

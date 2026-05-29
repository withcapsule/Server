// Requires the server to be running on localhost:9001

const BASE_URL: &str = "http://localhost:9001";

#[tokio::test]
async fn test_download_nonexistent_id_returns_404() {
    println!( "-- GET /download/notanid (expect 404) --" );
    let client = reqwest::Client::new();

    let res = client
        .get( format!( "{}/download/notanid", BASE_URL ) )
        .send()
        .await
        .expect( "Request failed" );

    println!( "  status: {}", res.status() );
    assert_eq!( res.status(), 404 );
    println!( "  PASS" );
}

#[tokio::test]
async fn test_upload_no_file_returns_400() {
    println!( "-- POST /curlup with empty form (expect 400) --" );
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new();

    let res = client
        .post( format!( "{}/curlup", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .expect( "Request failed" );

    println!( "  status: {}", res.status() );
    assert_eq!( res.status(), 400 );
    println!( "  PASS" );
}

#[tokio::test]
async fn test_search_nonexistent_id_returns_error() {
    println!( "-- POST /html_download_processor with bad ID (expect 404 or 500) --" );
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text( "file_download_field", "notanid" );

    let res = client
        .post( format!( "{}/html_download_processor", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .expect( "Request failed" );

    println!( "  status: {}", res.status() );
    assert!(
        res.status() == 404 || res.status() == 500,
        "Expected 404 or 500, got {}",
        res.status()
    );
    println!( "  PASS" );
}

#[tokio::test]
async fn test_ping() {
    println!( "-- GET /ping (expect 200 + pong) --" );
    let client = reqwest::Client::new();

    let res = client
        .get( format!( "{}/ping", BASE_URL ) )
        .send()
        .await
        .expect( "Request failed" );

    println!( "  status: {}", res.status() );
    assert_eq!( res.status(), 200 );

    let body = res.text().await.unwrap();
    println!( "  body: {}", body.trim() );
    assert!( body.contains( "pong" ), "Expected pong in response, got: {}", body );
    println!( "  PASS" );
}

// Requires the server to be running on localhost:9001
// Run in isolation — rate limiter state is shared with the live server

const BASE_URL: &str = "http://localhost:9001";

#[tokio::test]
async fn test_upload_route_rate_limited() {
    let client = reqwest::Client::new();

    let mut rate_limited = 0u32;

    for _ in 0..10 {
        let form = reqwest::multipart::Form::new().part(
            "f",
            reqwest::multipart::Part::bytes( vec![ 0u8; 64 ] ).file_name( "ratelimitfile.bin" ),
        );

        let res = client
            .post( format!( "{}/curlup", BASE_URL ) )
            .multipart( form )
            .send()
            .await
            .expect( "Request failed" );

        if res.status() == 429 {
            rate_limited += 1;
        }
    }

    assert!(
        rate_limited > 0,
        "Expected some requests to be rate limited (429), but none were"
    );
}

#[tokio::test]
async fn test_global_rate_limit() {
    let client = reqwest::Client::new();

    let mut rate_limited = 0u32;

    for _ in 0..30 {
        let res = client
            .get( format!( "{}/ping", BASE_URL ) )
            .send()
            .await
            .expect( "Request failed" );

        if res.status() == 429 {
            rate_limited += 1;
        }
    }

    assert!(
        rate_limited > 0,
        "Expected some requests to be rate limited (429), but none were"
    );
}

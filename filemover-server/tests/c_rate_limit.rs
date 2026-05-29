// Requires the server to be running on localhost:9001
// Run in isolation — rate limiter state is per-IP and shared with the live server

const BASE_URL: &str = "http://localhost:9001";

#[tokio::test]
async fn test_upload_route_rate_limited() {
    println!( "-- Upload route per-IP rate limit test (limit: 2 req/s, sending 10) --" );
    let client = reqwest::Client::new();

    let mut allowed = 0u32;
    let mut rate_limited = 0u32;

    for i in 1..=10 {
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

        let status = res.status();
        println!( "  request {}: HTTP {}", i, status );

        if status == 429 {
            rate_limited += 1;
        } else {
            allowed += 1;
        }
    }

    println!( "  allowed: {}  |  rate limited: {}", allowed, rate_limited );
    assert!(
        rate_limited > 0,
        "Expected some per-IP requests to be rate limited (429), but none were"
    );
    println!( "  PASS" );
}

#[tokio::test]
async fn test_per_ip_default_rate_limit() {
    println!( "-- Per-IP default rate limit test (limit: 20 req/s, sending 30) --" );
    let client = reqwest::Client::new();

    let mut allowed = 0u32;
    let mut rate_limited = 0u32;

    for i in 1..=30 {
        let res = client
            .get( format!( "{}/ping", BASE_URL ) )
            .send()
            .await
            .expect( "Request failed" );

        let status = res.status();
        println!( "  request {}: HTTP {}", i, status );

        if status == 429 {
            rate_limited += 1;
        } else {
            allowed += 1;
        }
    }

    println!( "  allowed: {}  |  rate limited: {}", allowed, rate_limited );
    assert!(
        rate_limited > 0,
        "Expected some per-IP requests to be rate limited (429), but none were"
    );
    println!( "  PASS" );
}

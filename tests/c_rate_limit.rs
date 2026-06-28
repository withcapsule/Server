mod common;

use common::{ client, init_limiter, spawn_server };
use reqwest::multipart::{ Form, Part };

const RATE_LIMIT_PER_SEC: u32 = 1;

#[tokio::test]
async fn test_upload_route_rate_limited() {
	init_limiter( RATE_LIMIT_PER_SEC ).await;
	let base = spawn_server().await;
	let client = client();

	let mut rate_limited = 0;
	for _ in 0..10 {
		let form = Form::new().part( "f", Part::bytes( vec![ 0u8; 64 ] ).file_name( "rl.bin" ) );
		let res = client
			.post( format!( "{}/upload", base ) )
			.multipart( form )
			.send()
			.await
			.expect( "request failed" );
		if res.status() == 429 { rate_limited += 1; }
	}

	assert!( rate_limited > 0, "expected some requests to be rate limited (429)" );
}

#[tokio::test]
async fn test_default_route_rate_limited() {
	init_limiter( RATE_LIMIT_PER_SEC ).await;
	let base = spawn_server().await;
	let client = client();

	let mut rate_limited = 0;
	for _ in 0..20 {
		let res = client
			.get( format!( "{}/ping", base ) )
			.send()
			.await
			.expect( "request failed" );
		if res.status() == 429 { rate_limited += 1; }
	}

	assert!( rate_limited > 0, "expected some requests to be rate limited (429)" );
}

#[tokio::test]
async fn test_status_route_rate_limited() {
	init_limiter( RATE_LIMIT_PER_SEC ).await;
	let base = spawn_server().await;
	let client = client();

	let mut rate_limited = 0;
	for _ in 0..10 {
		let res = client
			.get( format!( "{}/status/someid", base ) )
			.send()
			.await
			.expect( "request failed" );
		if res.status() == 429 { rate_limited += 1; }
	}

	assert!( rate_limited > 0, "expected some requests to be rate limited (429)" );
}

#[tokio::test]
async fn test_retry_after_header_on_429() {
	init_limiter( RATE_LIMIT_PER_SEC ).await;
	let base = spawn_server().await;
	let client = client();

	let mut checked = false;
	for _ in 0..10 {
		let res = client
			.get( format!( "{}/download/retrycheck", base ) )
			.send()
			.await
			.expect( "request failed" );

		if res.status() == 429 {
			let retry_after = res
				.headers()
				.get( "retry-after" )
				.expect( "429 response missing Retry-After header" )
				.to_str()
				.expect( "Retry-After not valid UTF-8" );
			assert_eq!( retry_after, "1", "expected Retry-After: 1, got: {}", retry_after );
			checked = true;
			break;
		}
	}

	assert!( checked, "never received a 429 to check Retry-After on" );
}

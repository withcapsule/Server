// Requires the server to be running on localhost:9001
// Run in isolation — rate limiter state is per-IP and shared with the live server

use std::{
	time::{
		Duration
	}
};

use tokio::{
	time::{
		sleep
	}
};

use reqwest::{
	Client,
	multipart::{
		Form,
		Part
	}
};

const BASE_URL: &str = "http://localhost:9001";

#[tokio::test]
async fn test_upload_route_rate_limited() {
	println!( "-- Upload route per-IP rate limit test (limit: 2 req/s, sending 10) --" );
	let client = Client::new();

	let mut allowed = 0u32;
	let mut rate_limited = 0u32;

	for i in 1..=10 {
		let form = Form::new().part(
			"f",
			Part::bytes( vec![ 0u8; 64 ] ).file_name( "ratelimitfile.bin" ),
		);

		let res = client
			.post( format!( "{}/curlup", BASE_URL ) )
			.multipart( form )
			.send()
			.await
			.expect( "Request failed" );

		let status = res.status();
		println!( "  request {}: HTTP {}", i, status );

		if status == 429 { rate_limited += 1; } else { allowed += 1; };
	}

	println!( "  allowed: {}  |  rate limited: {}", allowed, rate_limited );
	assert!( rate_limited > 0, "Expected some per-IP requests to be rate limited (429), but none were" );
	println!( "  PASS" );
}

#[tokio::test]
async fn test_per_ip_default_rate_limit() {
	println!( "-- Per-IP default rate limit test (limit: 20 req/s, sending 30) --" );
	let client = Client::new();

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

		if status == 429 { rate_limited += 1; } else { allowed += 1; };
	}

	println!( "  allowed: {}  |  rate limited: {}", allowed, rate_limited );
	assert!( rate_limited > 0, "Expected some per-IP requests to be rate limited (429), but none were" );
	println!( "  PASS" );
}

#[tokio::test]
async fn test_download_processor_rate_limited() {
	sleep( Duration::from_secs( 1 ) ).await;
	println!( "-- Download processor per-IP rate limit test (limit: 2 req/s, sending 10) --" );
	let client = Client::new();

	let mut allowed = 0u32;
	let mut rate_limited = 0u32;

	for i in 1..=10 {
		let form = Form::new()
			.text( "file_download_field", "nonexistent_id" );

		let res = client
			.post( format!( "{}/html_download_processor", BASE_URL ) )
			.multipart( form )
			.send()
			.await
			.expect( "Request failed" );

		let status = res.status();
		println!( "  request {}: HTTP {}", i, status );

		if status == 429 { rate_limited += 1; } else { allowed += 1; };
	}

	println!( "  allowed: {}  |  rate limited: {}", allowed, rate_limited );
	assert!( rate_limited > 0, "Expected some requests to be rate limited (429), but none were" );
	println!( "  PASS" );
}

#[tokio::test]
async fn test_retry_after_header_on_429() {
	sleep( Duration::from_secs( 1 ) ).await;
	println!( "-- Retry-After header present on 429 responses --" );
	let client = Client::new();

	let mut got_429_with_header = false;

	for _ in 1..=10 {
		let form = Form::new().text( "file_download_field", "nonexistent_id" );

		let res = client
			.post( format!( "{}/html_download_processor", BASE_URL ) )
			.multipart( form )
			.send()
			.await
			.expect( "Request failed" );

		if res.status() == 429 {
			let retry_after = res.headers().get( "retry-after" );
			assert!( retry_after.is_some(), "429 response missing Retry-After header" );

			let value = retry_after.unwrap().to_str().expect( "Retry-After value not valid UTF-8" );
			println!( "  Retry-After: {}", value );
			assert_eq!( value, "1", "Expected Retry-After: 1, got: {}", value );

			got_429_with_header = true;

			break;
		}
	}

	assert!( got_429_with_header, "Never received a 429 response to check Retry-After on" );
	println!( "  PASS" );
}

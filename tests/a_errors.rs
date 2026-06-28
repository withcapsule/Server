mod common;

use common::{ client, init_limiter, spawn_server, UNLIMITED };
use reqwest::multipart::{ Form, Part };

#[tokio::test]
async fn test_download_nonexistent_id_returns_404() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let res = client()
		.get( format!( "{}/download/notanid", base ) )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 404 );
}

#[tokio::test]
async fn test_status_nonexistent_id_returns_404() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let res = client()
		.get( format!( "{}/status/notanid", base ) )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 404 );
}

#[tokio::test]
async fn test_upload_no_file_returns_400() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let res = client()
		.post( format!( "{}/upload", base ) )
		.multipart( Form::new() )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 400 );
}


#[tokio::test]
async fn test_upload_unsafe_filename_returns_400() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let form = Form::new().part( "f", Part::bytes( vec![ 0u8; 16 ] ).file_name( ".." ) );

	let res = client()
		.post( format!( "{}/upload", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 400 );
}


#[tokio::test]
async fn test_html_download_processor_gated_returns_404() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let form = Form::new().text( "file_download_field", "notanid" );

	let res = client()
		.post( format!( "{}/html_download_processor", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 404 );
}

#[tokio::test]
async fn test_ping() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;

	let res = client()
		.get( format!( "{}/ping", base ) )
		.send()
		.await
		.expect( "request failed" );

	assert_eq!( res.status(), 200 );
	let body = res.text().await.unwrap();
	assert!( body.contains( "pong" ), "expected pong, got: {}", body );
}

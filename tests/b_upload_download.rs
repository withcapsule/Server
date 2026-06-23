// Requires the server to be running on localhost:9001

use std::{
	sync::{
		OnceLock
	},
	time::{
		Duration
	}
};

use tokio::{
	sync::{
		Mutex
	},
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
static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn test_lock() -> &'static Mutex<()> {
	TEST_LOCK.get_or_init( || Mutex::new(()) )
}

fn make_test_data( size: usize ) -> Vec<u8> { (0..size).map( |i| (i % 256) as u8 ).collect() }

fn parse_file_id( response_body: &str ) -> Option<String> {
	response_body
		.split( "File ID for downloading is " )
		.nth( 1 )
		.map( |s| s.trim().trim_end_matches( '.' ).trim_end_matches( '\n' ).to_string() )
}

#[tokio::test]
async fn test_upload_and_download_integrity() {
	let _guard = test_lock().lock().await;
	sleep( Duration::from_secs( 2 ) ).await;

	let test_data = make_test_data( 5 * 1024 * 1024 );
	println!( "-- Upload/download integrity test ({} bytes) --", test_data.len() );

	let client = Client::new();

	let form = Form::new().part( "f", Part::bytes( test_data.clone() ).file_name( "rusttest.bin" ) );

	println!( "  uploading rusttest.bin..." );
	let upload_res = client
		.post( format!( "{}/upload", BASE_URL ) )
		.multipart( form )
		.send()
		.await
		.expect( "Upload request failed" );

	println!( "  upload status: {}", upload_res.status() );
	assert_eq!( upload_res.status(), 200, "Upload did not return 200" );

	let body = upload_res.text().await.unwrap();
	println!( "  server response: {}", body.trim() );
	let file_id = parse_file_id( &body ).expect( "Could not parse file ID from upload response" );
	println!( "  parsed file ID: {}", file_id );

	println!( "  downloading file ID {}...", file_id );
	let dl_res = client
		.get( format!( "{}/download/{}", BASE_URL, file_id ) )
		.send()
		.await
		.expect( "Download request failed" );

	println!( "  download status: {}", dl_res.status() );
	assert_eq!( dl_res.status(), 200, "Download did not return 200" );

	let downloaded = dl_res.bytes().await.unwrap();
	println!( "  downloaded {} bytes", downloaded.len() );

	assert_eq!(
		downloaded.as_ref(),
		test_data.as_slice(),
		"Downloaded data does not match uploaded data"
	);
	println!( "  integrity check passed" );
	println!( "  PASS" );
}

#[tokio::test]
async fn test_content_disposition_filename() {
	let _guard = test_lock().lock().await;
	sleep( Duration::from_secs( 2 ) ).await;

	println!( "-- Content-Disposition filename test --" );
	let client = Client::new();

	let form = Form::new().part(
		"f",
		Part::bytes( vec![ 0u8; 64 ] ).file_name( "myfile.txt" ),
	);

	println!( "  uploading myfile.txt..." );
	let upload_res = client
		.post( format!( "{}/upload", BASE_URL ) )
		.multipart( form )
		.send()
		.await
		.unwrap();

	println!( "  upload status: {}", upload_res.status() );
	assert_eq!( upload_res.status(), 200, "Upload did not return 200" );

	let body = upload_res.text().await.unwrap();
	let file_id = parse_file_id( &body ).expect( "Could not parse file ID" );
	println!( "  parsed file ID: {}", file_id );

	println!( "  downloading file ID {}...", file_id );
	let dl_res = client
		.get( format!( "{}/download/{}", BASE_URL, file_id ) )
		.send()
		.await
		.unwrap();

	println!( "  download status: {}", dl_res.status() );
	let disposition = dl_res
		.headers()
		.get( "content-disposition" )
		.expect( "No Content-Disposition header" )
		.to_str()
		.unwrap();

	println!( "  Content-Disposition: {}", disposition );
	assert!( disposition.contains( "myfile.txt" ), "Content-Disposition does not contain original filename, got: {}", disposition );
	println!( "  PASS" );
}

#[tokio::test]
async fn test_delete_uploaded_file() {
	let _guard = test_lock().lock().await;
	sleep( Duration::from_secs( 2 ) ).await;

	println!( "-- Delete uploaded file test --" );
	let client = Client::new();

	let form = Form::new().part(
		"f",
		Part::bytes( vec![ 1u8; 64 ] ).file_name( "delete_me.txt" ),
	);

	println!( "  uploading delete_me.txt..." );
	let upload_res = client
		.post( format!( "{}/upload", BASE_URL ) )
		.multipart( form )
		.send()
		.await
		.expect( "Upload request failed" );

	println!( "  upload status: {}", upload_res.status() );
	assert_eq!( upload_res.status(), 200, "Upload did not return 200" );

	let body = upload_res.text().await.unwrap();
	let file_id = parse_file_id( &body ).expect( "Could not parse file ID" );
	println!( "  parsed file ID: {}", file_id );

	println!( "  deleting file ID {}...", file_id );
	let delete_res = client
		.delete( format!( "{}/delete/{}", BASE_URL, file_id ) )
		.send()
		.await
		.expect( "Delete request failed" );

	println!( "  delete status: {}", delete_res.status() );
	assert_eq!( delete_res.status(), 200, "Delete did not return 200" );

	println!( "  verifying file ID {} is gone...", file_id );
	let dl_res = client
		.get( format!( "{}/download/{}", BASE_URL, file_id ) )
		.send()
		.await
		.expect( "Download verification request failed" );

	println!( "  post-delete download status: {}", dl_res.status() );
	assert_eq!( dl_res.status(), 404, "Expected deleted file to return 404" );
	println!( "  PASS" );
}

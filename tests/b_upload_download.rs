mod common;

use common::{ client, init_limiter, make_test_data, parse_file_id, spawn_server, UNLIMITED };
use reqwest::multipart::{ Form, Part };

#[tokio::test]
async fn test_upload_and_download_integrity() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;
	let client = client();

	let data = make_test_data( 5 * 1024 * 1024 );
	let form = Form::new().part( "f", Part::bytes( data.clone() ).file_name( "rusttest.bin" ) );

	let upload = client
		.post( format!( "{}/upload", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "upload failed" );
	assert_eq!( upload.status(), 200, "upload did not return 200" );

	let body = upload.text().await.unwrap();
	let file_id = parse_file_id( &body ).expect( "could not parse file ID" );

	let dl = client
		.get( format!( "{}/download/{}", base, file_id ) )
		.send()
		.await
		.expect( "download failed" );
	assert_eq!( dl.status(), 200, "download did not return 200" );

	let downloaded = dl.bytes().await.unwrap();
	assert_eq!( downloaded.as_ref(), data.as_slice(), "downloaded data does not match upload" );
}

#[tokio::test]
async fn test_content_disposition_filename() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;
	let client = client();

	let form = Form::new().part( "f", Part::bytes( vec![ 0u8; 64 ] ).file_name( "myfile.txt" ) );

	let upload = client
		.post( format!( "{}/upload", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "upload failed" );
	assert_eq!( upload.status(), 200 );

	let file_id = parse_file_id( &upload.text().await.unwrap() ).expect( "could not parse file ID" );

	let dl = client
		.get( format!( "{}/download/{}", base, file_id ) )
		.send()
		.await
		.expect( "download failed" );

	let disposition = dl
		.headers()
		.get( "content-disposition" )
		.expect( "no Content-Disposition header" )
		.to_str()
		.unwrap();
	assert!( disposition.contains( "myfile.txt" ), "unexpected disposition: {}", disposition );
}



#[tokio::test]
async fn test_traversal_filename_is_cleaned_to_basename() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;
	let client = client();

	let form = Form::new().part(
		"f",
		Part::bytes( vec![ 7u8; 32 ] ).file_name( "../../../../etc/evil.txt" ),
	);

	let upload = client
		.post( format!( "{}/upload", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "upload failed" );
	assert_eq!( upload.status(), 200 );

	let body = upload.text().await.unwrap();
	let file_id = parse_file_id( &body ).expect( "could not parse file ID" );

	let dl = client
		.get( format!( "{}/download/{}", base, file_id ) )
		.send()
		.await
		.expect( "download failed" );
	let disposition = dl
		.headers()
		.get( "content-disposition" )
		.expect( "no Content-Disposition header" )
		.to_str()
		.unwrap();

	assert!( disposition.contains( "evil.txt" ), "expected cleaned basename, got: {}", disposition );
	assert!( !disposition.contains( ".." ), "filename still contains traversal: {}", disposition );
}

#[tokio::test]
async fn test_delete_uploaded_file() {
	init_limiter( UNLIMITED ).await;
	let base = spawn_server().await;
	let client = client();

	let form = Form::new().part( "f", Part::bytes( vec![ 1u8; 64 ] ).file_name( "delete_me.txt" ) );

	let upload = client
		.post( format!( "{}/upload", base ) )
		.multipart( form )
		.send()
		.await
		.expect( "upload failed" );
	assert_eq!( upload.status(), 200 );

	let file_id = parse_file_id( &upload.text().await.unwrap() ).expect( "could not parse file ID" );

	let delete = client
		.delete( format!( "{}/delete/{}", base, file_id ) )
		.send()
		.await
		.expect( "delete failed" );
	assert_eq!( delete.status(), 200, "delete did not return 200" );

	let dl = client
		.get( format!( "{}/download/{}", base, file_id ) )
		.send()
		.await
		.expect( "verification request failed" );
	assert_eq!( dl.status(), 404, "expected deleted file to be gone" );
}

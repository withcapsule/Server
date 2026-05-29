// Requires the server to be running on localhost:9001

const BASE_URL: &str = "http://localhost:9001";

fn make_test_data( size: usize ) -> Vec<u8> {
    (0..size).map( |i| (i % 256) as u8 ).collect()
}

fn parse_file_id( response_body: &str ) -> Option<String> {
    response_body
        .split( "File ID for downloading is " )
        .nth( 1 )
        .map( |s| s.trim().trim_end_matches( '.' ).trim_end_matches( '\n' ).to_string() )
}

#[tokio::test]
async fn test_upload_and_download_integrity() {
    tokio::time::sleep( std::time::Duration::from_secs( 2 ) ).await;

    let test_data = make_test_data( 5 * 1024 * 1024 );

    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new().part(
        "f",
        reqwest::multipart::Part::bytes( test_data.clone() ).file_name( "rusttest.bin" ),
    );

    let upload_res = client
        .post( format!( "{}/curlup", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .expect( "Upload request failed" );

    assert_eq!( upload_res.status(), 200, "Upload did not return 200" );

    let body = upload_res.text().await.unwrap();
    let file_id = parse_file_id( &body ).expect( "Could not parse file ID from upload response" );

    let dl_res = client
        .get( format!( "{}/download/{}", BASE_URL, file_id ) )
        .send()
        .await
        .expect( "Download request failed" );

    assert_eq!( dl_res.status(), 200, "Download did not return 200" );

    let downloaded = dl_res.bytes().await.unwrap();
    assert_eq!(
        downloaded.as_ref(),
        test_data.as_slice(),
        "Downloaded data does not match uploaded data"
    );
}

#[tokio::test]
async fn test_content_disposition_filename() {
    tokio::time::sleep( std::time::Duration::from_secs( 2 ) ).await;

    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new().part(
        "f",
        reqwest::multipart::Part::bytes( vec![ 0u8; 64 ] ).file_name( "myfile.txt" ),
    );

    let upload_res = client
        .post( format!( "{}/curlup", BASE_URL ) )
        .multipart( form )
        .send()
        .await
        .unwrap();

    let body = upload_res.text().await.unwrap();
    let file_id = parse_file_id( &body ).expect( "Could not parse file ID" );

    let dl_res = client
        .get( format!( "{}/download/{}", BASE_URL, file_id ) )
        .send()
        .await
        .unwrap();

    let disposition = dl_res
        .headers()
        .get( "content-disposition" )
        .expect( "No Content-Disposition header" )
        .to_str()
        .unwrap();

    assert!(
        disposition.contains( "myfile.txt" ),
        "Content-Disposition does not contain original filename, got: {}",
        disposition
    );
}

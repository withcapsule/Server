use std::{
	fmt::format, process::exit, str::FromStr, time::SystemTime
};

use axum::{
    Router,
    http::{
   		HeaderValue,
     	Method,
      	StatusCode
    },
    routing::{
    	get,
    	post
    },
    response::{
    	Html,
     	Json
    },
    extract::{
    	State,
    	DefaultBodyLimit,
    	Multipart,
     	multipart::{
      		Field
      	}
    }
};

use rand::RngExt;

use tokio::{
    io::{
        AsyncWriteExt
    },
	fs::{
		File,
		try_exists,
		create_dir_all,
	},
	net::{
		TcpListener
	}
};

use serde_json::{
	json,
	Value
};

use tower_http::{
	cors::{
		CorsLayer
	}
};

use sqlx::{
	Row, SqlitePool, sqlite::{
		SqliteConnectOptions, SqliteJournalMode::Wal, SqliteRow
	}
};

#[derive(Clone)]
struct AppState {
	database: SqlitePool
}

async fn pong() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}

async fn main_menu() -> Html<&'static str> {
	Html( r#"
        <!doctype html>
        <html>
            <body>
            	<div>
               		<button onclick="location.href='/html_uploader_form'" type="button">Upload File</button>
               		<button onclick="location.href='/html_downloader_form'" type="button">Download File</button>
             	</div>
            </body>
        </html>
    "#
	)
}

async fn html_uploader_form() -> Html<&'static str> {
	Html( r#"
        <!doctype html>
        <html>
            <body>
                <button onclick="location.href='/'" type="button">Home</button>
                <form id="upload_form" enctype="multipart/form-data">
                    <label>
                        Choose file to upload:
                        <input type="file" name="file_upload_field">
                    </label>
                    <button type="submit">Upload</button>
                </form>
                <progress id="progress" value="0" max="100"></progress>
                <p id="status"></p>
                <p id="statistics"></p>
                <div id="statistics2" style="white-space: pre-line;"></div>
                <script>
                    document.getElementById('upload_form').addEventListener('submit', function(e) {
                        e.preventDefault();

                        const xhr = new XMLHttpRequest();

                        let totalBytes = 0;
                        let maxMbps = 0;
                        let minMbps = Infinity;

                        xhr.upload.onprogress = function(e) {
                            if (e.lengthComputable) {
                                totalBytes = e.total;
                                const percent = Math.round((e.loaded / e.total) * 100);
                                const elapsed = (Date.now() - startTime) / 1000;
                                const mbps = e.loaded / elapsed / (1024 * 1024);
                                if (mbps > maxMbps) maxMbps = mbps;
                                if (mbps < minMbps) minMbps = mbps;
                                document.getElementById('progress').value = percent;
                                document.getElementById('status').textContent = percent + '% — ' + mbps.toFixed(2) + ' MB/s';
                            }
                        };

                        xhr.onload = function() {
                            const elapsed = (Date.now() - startTime) / 1000;
                            const avgMbps = (totalBytes / elapsed / (1024 * 1024)).toFixed(2);
                            document.getElementById( 'statistics' ).textContent = xhr.responseText;
                            document.getElementById( 'statistics2' ).textContent =
                            'Transfer Statistics: \n' +
                            '- Avg: ' + avgMbps + ' MB/s \n' +
                            '- Max: ' + maxMbps.toFixed(2) + ' MB/s \n' +
                            '- Min: ' + minMbps.toFixed(2) + ' MB/s' ;
                        };

                        xhr.open('POST', '/html_upload_processor');
                        const startTime = Date.now();
                        xhr.send(new FormData(this));
                    });
                </script>
            </body>
        </html>
    "#
	)
}

async fn html_downloader_form() -> Html<&'static str> {
	Html( r#"
        <!doctype html>
        <html>
            <body>
            	<button onclick="location.href='/'" type="button">Home</button>
                <form id="download_form" enctype="multipart/form-data">
                    <label>
                        Enter file download key or file link:
                        <input type="text" name="file_download_field">
                    </label>
                    <button type="submit">Download</button>
                </form>
                <p id="status"></p>
                <script>
                    document.getElementById('download_form').addEventListener('submit', function(e) {
                        e.preventDefault();

                        const xhr = new XMLHttpRequest();

                        xhr.onload = function() {
                            if (xhr.status === 200) {
                                document.getElementById('status').textContent = 'File found: ' + xhr.responseText;
                            } else {
                                document.getElementById('status').textContent = 'Error: ' + xhr.responseText;
                            }
                        };

                        xhr.open('POST', '/html_download_processor');
                        xhr.send(new FormData(this));
                    });
                </script>
            </body>
        </html>
    "#
	)
}

async fn upload_file( State( state ): State<AppState>, mut parsed_field: Field<'_> ) -> Result<String, ( StatusCode, String )> {
	// DB contains ID, FileName, UploadTime
	let file_id: i32 = rand::rng().random_range( 0..=99999 );

	// this is now an Option<&str>
	// this handles the `self` parameter and handles the success branch
	// the failure branch results in the file_name becoming the set string
	// don't forget to call to_string() at the end
	let file_name = parsed_field
							.file_name()
							.unwrap_or( "__failure_upload_file()__" )
							.to_string();

	if file_name == "" || file_name == "__failure_upload_file()__" {
		return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
	}

	create_dir_all( format!( "./uploads/temp/{}", file_id ) ).await.unwrap();
	let path = format!( "./uploads/temp/{}/{}", file_id, file_name );

	let file = File::create( path ).await; // io::Result<File>

	// This function can be used to pass through a successful result while handling an error. - rust docs
	// this is exactly what was needed; a way to handle errors without stopping on Ok()
	// let _ = file.map_err( |error_message| {
	// 	return ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 1 error: {}", error_message ) );
	// } );

	let mut file_fd = match file {
		Err( error_msg ) => {
			return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 1 error: {}", error_msg ) ) )
		}
		Ok( filefd ) => filefd
	};

	// at this point, a file has been created, and it needs to be written to
	// ideally, chunking is the best method
	//
	// explanantion:
	// 100 MB file, 1 MB/s connection
	// -> wait for entire 100 MB to reach server, 100 seconds, and only written when fully received
	// OR
	// -> for each megabyte streamed in, just write it, then discard those MB from system RAM since they're on disk


	let mut chunk_loops: u16 = 0;
    let mut total_bytes_written: usize = 0;

    // TODO: ADD ERROR HANDLING
    // sqlx::query( format!( "INSERT INTO filetable({})", file_id ) ).execute( &state.database );

    // from rust docs
    let upload_time: u64 = match SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ) {
        Ok( time ) => time.as_secs(),
        Err( error_message ) => panic!( "Critical system time issue, possibly before UNIX_EPOCH. Details: {}", error_message ),
    };

    // println!( "1970-01-01 00:00:00 UTC was {} seconds ago!", upload_time );

    let _= sqlx::query( "INSERT INTO filetable(ID, FileName, UploadTime) VALUES(?, ?, ?)" )
        .bind( file_id )
        .bind( &file_name )
        .bind( upload_time as i64 )
        .execute( &state.database )
        .await
        .map_err( |error_message| {
        	( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to add to db, error: {}", error_message ) )
        } );



	loop {
		let chunk_piece = parsed_field.chunk().await;      // Result<Option<Bytes>, MultipartError>
		chunk_loops += 1;

		// chunk_piece is a Result<Option<Bytes>, MultipartError>
		let chunk = match chunk_piece {
			Err( error ) => {
				return Err(
					( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 2 error: {}", error ) )
			 	);
			},
			Ok( inner_option_and_bytes ) => {
				inner_option_and_bytes
			}
		};

		// chunk is now Option<Bytes>
		let bytes = match chunk {
			Some( bytes ) => bytes,
			None => break
		};

        file_fd.write_all( &bytes ).await.map_err( |error_message| {
            return ( StatusCode::INTERNAL_SERVER_ERROR, format!( "writing error: {}", error_message ) );
        } )?;

		total_bytes_written += bytes.len();
	}

	println!( "{} bytes received over {} chunks", total_bytes_written, chunk_loops - 1 );

	return Ok( format!( "Success, uploaded {} of {} bytes. File ID for downloading is {}.\n", file_name, total_bytes_written, file_id ) )
}


async fn download_file( state: State<AppState>, parsed_field: Field<'_> ) -> Result<String, ( StatusCode, String )> {
	let id = match parsed_field.text().await {
		Err( error_msg ) => {
			return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Could not read the file ID you entered, error: {}", error_msg ) ) );
		}
		Ok( id ) => id
	};


	let res: SqliteRow = match sqlx::query( "SELECT * FROM filetable WHERE ID LIKE ?" ).bind( id ).fetch_optional( &state.database ).await.map_err( |e| { ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to search db, error: {}", e ) ) } ) {
        Err( error_message ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_message ) ) ); }
        Ok( res ) => match res {
	        Some( found_res ) => found_res,
	        None => { return Err( ( StatusCode::NOT_FOUND, format!( "No file with that ID exists." ) ) ); }
        }
    };

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );
	let upload_time: i64 = res.get( "UploadTime" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => {
			return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) );
		}
		Ok( file ) => file
	};

	println!( "Received download request for ID# {}, which is `{}` and it was uploaded at: {}.", file_id, file_name, upload_time );

	if file_exists {
		return Ok( format!( "File {} found!", file_name  ) );
	} else {
		return Err( ( StatusCode::NOT_FOUND, format!( "File record exists in database but the file is missing on disk." ) ) );
	}
}

async fn curl_upload_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	// example:
	// curl -X POST http://localhost:9001/curlup -F 'f=@mydocument.txt'

	loop {
		let parts_of_curl = part.next_field().await;  // Result<Option<Field<'_>>, MultipartError>

		// now becomes Option<Field<'_>>
		let current_part = match parts_of_curl {
			Err( error ) => {
				return Err( ( StatusCode::BAD_REQUEST, format!( "curl error location 1: {}\n", error ) ) );
			}

			Ok( part_found ) => part_found
		};

		let field = match current_part {
			Some( inner_field ) => inner_field,
			None => break
		};

		let field_name = field.name().unwrap_or( "unknown" ).to_string();

		if field_name == "f" {
			match upload_file( state, field ).await {
				Err( error_message ) => {
					return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "curl error location 2: {:?}\n", error_message ) ) );
				}
				Ok( message_from_uploader ) => {
					return Ok( message_from_uploader )
				}
			}
		}
	}

	return Err(( StatusCode::BAD_REQUEST, "No file found in request".to_string() ));
}

async fn html_upload_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	loop {
		// begin looking at the next part of an HTML form that was submitted
		let parts_of_html_form = part.next_field().await;  // returns Result<Option<Field>>

		// this looks through Result<Option<Field>>
		// and unwraps it to just Option<Field>
		// Result<> here means that the form was either successfully moved ahead or it failed due to some error
		let current_part = match parts_of_html_form {
			// error in the form, so return an error
			Err( error ) => {
				return Err(
					( StatusCode::BAD_REQUEST, format!( "Multipart Error: {}", error ) )
			 	);
			}

			// the form's next field was successfully found and it is stored as found_next_form_part
			// this section evaluates to found_next_form_part
			// so the value of current_part becomes found_next_form_part
			Ok( found_next_form_part ) => {
				// ok no semicolon here because that apparently discards the value of the evaluated line
				// expected (), found Option<{unknown}> (rust-analyzer E0308)
				// fixed ^^ error on the next few lines
				found_next_form_part
			}
		};

		// now since HTML can have many parts in forms, this checks if the part is a field
		// current_part is currently Option<Field>, so it cannot be worked with directly yet
		// it needs to be unwrapped. Either it exists as Some() or doesn't exist, which means None
		let current_field = match current_part {
			// if any HTML field was found, unwrap Option<Field> to Field
			Some( found_a_field ) => found_a_field,
			// otherwise break out, because if we're here, that means that next_field from above
			// returned a None, indicating that the HTML form has nothing left in it and the end has been hit
			None => break,
		};

		// get the name of the current field to check if it's the right one
		// unwrap is dangerous as it can panic crash, so unwrap_or is safer
		// .name() returns Option<&str>, so either it comes back as Some or None
		let current_field_name = current_field.name().unwrap_or( "unknown" ).to_string();

		// parse the html form until a field named "file" is found as that's what the name is set to in HTML
		// in that will be the file that the user is uploading
		if current_field_name == "file_upload_field" {
			match upload_file( state, current_field ).await {
				// upload file returns Result<String, (StatusCode, String)>
				// so Ok() is literally just returning a formatted String
				Ok( message_from_uploader ) => {
					// returning Result<String>
					return Ok( message_from_uploader );
				}

				// and error is returning a tuple which puts StatusCode and String together
				Err( error_msg ) => {
					// returning Result<(StatusCode, String)>
					return Err(
						( StatusCode::INTERNAL_SERVER_ERROR, format!( "File upload failed. Error: {:?}", error_msg ) )
					);
				}
			}
		}
	}

	return Err(( StatusCode::BAD_REQUEST, "No file found in request".to_string() ));
}

async fn curl_download_processor( state: State<AppState>, mut part: Multipart ) {

}

async fn html_download_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	loop {
		let html_parts = part.next_field().await;

		let current_html_part = match html_parts {
			Err( error_message ) => {
				return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "issue with download site, error: {}", error_message ) ) );
			}
			Ok( found ) => found
		};

		let current_field = match current_html_part {
			Some( field ) => field,
			None => break
		};

		let field_name = current_field.name().unwrap_or( "massive_issue_please_fix" ).to_string();

		if field_name == "file_download_field" {
			match download_file( state, current_field ).await {
				Err( error_message ) => {
					return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Error with download form. Details: {:?}", error_message ) ) )
				}
				Ok( message_from_uploader ) => {
					return Ok( message_from_uploader )
				}
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, format!( "No file ID field found in the download form." ) ) );
}


#[tokio::main]
async fn main() {
	let options = SqliteConnectOptions::from_str( "sqlite:filemover.db" )
    	.expect( "Expected to create db, failed" )
     	.create_if_missing( true )
      	.journal_mode( Wal )
       	.read_only( false );


	let sqlite_db = SqlitePool::connect_with( options ).await;
	let state = AppState { database: match sqlite_db {
		Err( error_message ) => {
			println!( "Failed to create database. Error: {}", error_message );
			exit( 1 );
		}
		Ok( db ) => db
	} };

	sqlx::query(
		"CREATE TABLE IF NOT EXISTS filetable (
			ID VARCHAR(16) PRIMARY KEY,
			FileName VARCHAR(64) NOT NULL,
			UploadTime INTEGER NOT NULL
		)"
 	).execute( &state.database ).await.expect( "Failed to create table since it didn't exist." );

	create_dir_all( "./uploads/temp" ).await.unwrap();

    let app: Router<> = Router::new()
        .route( "/ping", get( pong ) )

        .route( "/curlup", post( curl_upload_processor ) )

        .route( "/html_uploader_form", get( html_uploader_form ) )
        .route( "/html_upload_processor", post( html_upload_processor ) )

        .route( "/html_downloader_form", get( html_downloader_form ) )
        .route( "/html_download_processor", post( html_download_processor ) )

        .route("/", get( main_menu ) )

        .with_state( state )

        // 1 byte * 1024 = 1 KiB
        // 1 KiB * 1024 = 1 MiB
        // 1 MiB * 32 = 32 MiB
        .layer( DefaultBodyLimit::max( 1 * 1024 * 1024 * 256 ) )
        .layer(
        	CorsLayer::new()
         		.allow_origin( "http://localhost:3000".parse::<HeaderValue>().unwrap() )
           		.allow_methods( [ Method::GET, Method::POST ] )
        );

    let listener = TcpListener::bind( "0.0.0.0:9001" ).await.unwrap();
    axum::serve( listener, app ).await.unwrap();
}

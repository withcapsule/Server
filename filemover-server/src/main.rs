use std::{
	process::{
		exit
	},
	str::{
		FromStr
	}
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

use tokio::{
    io::{
        AsyncWriteExt
    },
	fs::{
		File,
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
	SqlitePool,
	sqlite::{
		SqliteConnectOptions,
		SqliteJournalMode::{
			Wal
		}
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
                            document.getElementById('status').textContent =
                                xhr.responseText +
                                ' | avg: ' + avgMbps + ' MB/s' +
                                ' | max: ' + maxMbps.toFixed(2) + ' MB/s' +
                                ' | min: ' + minMbps.toFixed(2) + ' MB/s';
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
                <form action="/html_download_processor" method="get" enctype="multipart/form-data">
                    <label>
                        Enter file download key or file link:
                        <input type="text" name="file_download_field">
                    </label>
                    <button type="submit">Download</button>
                </form>
            </body>
        </html>
    "#
	)
}

async fn upload_file( mut parsed_field: Field<'_>) -> Result<String, ( StatusCode, String )> {
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

	let path = format!( "./uploads/temp/{}", file_name );

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

	return Ok( format!( "Success, uploaded {} of {} bytes.\n", file_name, total_bytes_written ) )
}




async fn download_file() {

}

async fn curl_upload_processor( mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
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
			match upload_file( field ).await {
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

async fn html_upload_processor( mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
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
			match upload_file( current_field ).await {
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

async fn html_download_processor() {

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

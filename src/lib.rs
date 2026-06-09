use std::{
	time::{
		Duration,
		SystemTime
	}
};

use axum::{
	Router,
	body::{
		Body
	},
	extract::{
		DefaultBodyLimit,
		Multipart,
		Path,
		Query,
		Request,
		State,
		multipart::{
			Field
		},
	},
	http::{
		HeaderValue,
		Method,
		StatusCode,
		header,
	},
	middleware::{
		Next
	},
	response::{
		Html,
		Json,Response
	},
	routing::{
		get,
		post
	},
};

use rand::{
	RngExt,
	distr::{
		Alphanumeric
	}
};

use tokio::{
	io::AsyncWriteExt,
	fs::{
		File,
		ReadDir,
		DirEntry,
		try_exists,
		read_dir,
		remove_dir,
		remove_file,
		create_dir_all,
		remove_dir_all,
	},
};

use tokio_util::{
	io::{
		ReaderStream
	}
};

use serde_json::{
	json,
	Value
};

use tower_http::{
	cors::{
		CorsLayer
	},
	trace::{
		DefaultMakeSpan,
		DefaultOnResponse,
		TraceLayer
	}
};

use tracing::{
	info
};

use sqlx::{
	Row,
	SqlitePool,
	sqlite::{
		SqliteRow
	},
};

#[derive(Clone)]
pub struct AppState {
	pub database: SqlitePool,
}

#[derive(serde::Deserialize)]
struct UploadQuery {
	encrypted: Option<bool>,
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
					var lastUploadSentAt = 0;
					document.getElementById('upload_form').addEventListener('submit', function(e) {
						e.preventDefault();

						const now = Date.now();
						if (now - lastUploadSentAt < 1000) return;
						lastUploadSentAt = now;

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
					<button type="submit">Search</button>
				</form>
				<p id="status"></p>
				<button id="download_btn" type="button" style="display:none;">Download File</button>
				<script>
					var file_id = '';
					var lastSearchSentAt = 0;
					document.getElementById('download_form').addEventListener('submit', function(e) {
						e.preventDefault();

						const now = Date.now();
						if (now - lastSearchSentAt < 1000) return;
						lastSearchSentAt = now;

						const searchBtn = this.querySelector('button[type="submit"]');
						searchBtn.disabled = true;
						setTimeout(function() { searchBtn.disabled = false; }, 1000);

						const xhr = new XMLHttpRequest();

						xhr.onload = function() {
							if (xhr.status === 200) {
						 		file_id = document.querySelector('[name="file_download_field"]').value.trim();
								document.getElementById('status').textContent = xhr.responseText;
								document.getElementById('download_btn').style.display = 'inline';
							} else {
								document.getElementById('status').textContent = 'Error: ' + xhr.responseText;
								document.getElementById('download_btn').style.display = 'none';
							}
						};

						xhr.open('POST', '/html_download_processor');
						xhr.send(new FormData(this));
					});

					document.getElementById( 'download_btn' ).addEventListener( 'click', function( e ) {
						e.preventDefault();

					 	const xhr_dl = new XMLHttpRequest();
					  	xhr_dl.open( 'GET', '/download/' + file_id, true );
						xhr_dl.responseType = 'blob';

					  	xhr_dl.onload = function() {
							if( xhr_dl.status === 200 ) {
								const disposition = xhr_dl.getResponseHeader( 'Content-Disposition' );
								 let filename = 'download';
								  if( disposition ) {
									  const match = disposition.match( /filename="?([^";\n]+)"?/ );
									  if( match ) filename = match[ 1 ].trim();
								  }

								  const url = URL.createObjectURL( xhr_dl.response );
								  const a = document.createElement( 'a' );
								  a.href = url;
								  a.download = filename;
								  a.click();
								  URL.revokeObjectURL( url );
							} else {
								document.getElementById( 'status' ).textContent = 'Download failed: ' + xhr_dl.status;
							}
						};

						xhr_dl.onerror = function() {
							document.getElementById( 'status' ).textContent = 'Network error during download';
						};

						xhr_dl.send();
					});
				</script>
			</body>
		</html>
	"#
	)
}

async fn upload_file( State( state ): State<AppState>, mut parsed_field: Field<'_>, is_encrypted: bool ) -> Result<String, ( StatusCode, String )> {
	let file_id: String = rand::rng().sample_iter( Alphanumeric ).take( 8 ).map( char::from ).collect();
	let file_name = parsed_field.file_name().unwrap_or( "__failure_upload_file()__" ).to_string();

	if file_name == "" || file_name == "__failure_upload_file()__" {
		return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
	}

	info!( file_name, file_id, "upload started" );
	create_dir_all( format!( "./uploads/temp/{}", file_id ) ).await.unwrap();
	let path = format!( "./uploads/temp/{}/{}", file_id, file_name );

	let file = File::create( path ).await;

	let mut file_fd = match file {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 1 error: {}", error_msg ) ) ) }
		Ok( filefd ) => filefd
	};

	let mut chunk_loops: u16 = 0;
	let mut total_bytes_written: usize = 0;

	let upload_time: u64 = match SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ) {
		Ok( time ) => time.as_secs(),
		Err( error_message ) => panic!( "Critical system time issue, possibly before UNIX_EPOCH. Details: {}", error_message ),
	};

	loop {
		let chunk_piece = parsed_field.chunk().await;
		chunk_loops += 1;

		let chunk = match chunk_piece {
			Err( error ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 2 error: {}", error ) ) ); },
			Ok( inner_option_and_bytes ) => inner_option_and_bytes
		};

		let bytes = match chunk {
			Some( bytes ) => bytes,
			None => break
		};

		file_fd.write_all( &bytes ).await.map_err( |error_message| { return ( StatusCode::INTERNAL_SERVER_ERROR, format!( "writing error: {}", error_message ) ); } )?;
		total_bytes_written += bytes.len();
	}

	info!( file_name, file_id, bytes = total_bytes_written, chunks = chunk_loops - 1, "upload complete" );

	if let Err( error_message ) = sqlx::query( "INSERT INTO filetable(ID, FileName, UploadTime, FileSize, IsEncrypted) VALUES(?, ?, ?, ?, ?)" )
		.bind( &file_id )
		.bind( &file_name )
		.bind( upload_time as i64 )
		.bind( total_bytes_written as i64 )
		.bind( is_encrypted )
		.execute( &state.database )
		.await {
			remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await.ok();
			remove_dir( format!( "./uploads/temp/{}", file_id ) ).await.ok();
			return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to add to db, error: {}", error_message ) ) );
		}

	return Ok( format!( "Success, uploaded {} of {} bytes. File ID for downloading is {}.\n", file_name, total_bytes_written, file_id ) )
}

async fn lookup_file_record( id: &str, db: &SqlitePool ) -> Result<SqliteRow, ( StatusCode, String )> {
	match sqlx::query( "SELECT ID, FileName, UploadTime, FileSize, IsEncrypted FROM filetable WHERE ID = ?" )
		.bind( id )
		.fetch_optional( db )
		.await {
			Err( e ) => Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to search db, error: {}", e ) ) ),
			Ok( None ) => Err( ( StatusCode::NOT_FOUND, "No file with that ID exists.".to_string() ) ),
			Ok( Some( row ) ) => Ok( row )
		}
}

async fn search_file( state: State<AppState>, parsed_field: Field<'_> ) -> Result<String, ( StatusCode, String )> {
	let id = match parsed_field.text().await {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Could not read the file ID you entered, error: {}", error_msg ) ) ); }
		Ok( id ) => id
	};

	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );
	let upload_time: i64 = res.get( "UploadTime" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) ); }
		Ok( file ) => file
	};

	info!( file_id, file_name, upload_time, "file lookup" );

	if file_exists {
		return Ok( format!( "File {} found!", file_name  ) );
	} else {
		return Err( ( StatusCode::NOT_FOUND, format!( "File record exists in database but the file is missing on disk." ) ) );
	}
}

async fn file_status( state: State<AppState>, Path( id ): Path<String> ) -> Result<Json<serde_json::Value>, ( StatusCode, String )> {
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_name: String = res.get( "FileName" );
	let file_size: i64    = res.get( "FileSize" );
	let upload_time: i64  = res.get( "UploadTime" );
	let is_encrypted: bool = res.get( "IsEncrypted" );

	const EXPIRY_SECS: i64 = 3600;
	let now = SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ).map( |d| d.as_secs() as i64 ).unwrap_or( 0 );
	let time_remaining = ( upload_time + EXPIRY_SECS - now ).max( 0 );

	Ok( Json( json!( {
		"file_name":      file_name,
		"file_size":      file_size,
		"upload_time":    upload_time,
		"time_remaining": time_remaining,
		"is_encrypted":   is_encrypted,
	} ) ) )
}

async fn delete_file( state: State<AppState>, Path( id ): Path<String> ) -> Result<String, ( StatusCode, String )> {
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) ); }
		Ok( file ) => file
	};

	if !file_exists {
		return Err( ( StatusCode::NOT_FOUND, "File record exists in database but the file is missing on disk.".to_string() ) );
	}

	if let Err( error ) = remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to remove file {}, error: {}", file_id, error ) ) );
	}

	if let Err( error ) = sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( &file_id ).execute( &state.database ).await {
		return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "file {} deleted but database entry still exists. details: {}", file_id, error ) ) );
	}

	info!( file_id, file_name, "file deleted" );

	match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
		Ok(()) => Ok( format!( "File {} deleted", file_name ) ),
		Err( error_msg ) => Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "File {} directory deletion failed. Details: {}", file_id, error_msg ) ) ),
	}
}

async fn download_file( state: State<AppState>, Path( id ): Path<String> ) -> Result<Response, ( StatusCode, String )> {
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );
	let file_size: i64 = res.get( "FileSize" );
	let is_encrypted: bool = res.get( "IsEncrypted" );

	info!( file_id, file_name, file_size, is_encrypted, "download started" );

	let file_to_send = match File::open( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "unable to open file, details: {}", error_msg ) ) ) }
		Ok( file ) => file
	};

	let file_stream = ReaderStream::new( file_to_send );
	let file_body = Body::from_stream( file_stream );

	let response_object = Response::builder()
		.status( StatusCode::OK )
		.header( "Content-Type", "application/octet-stream" )
		.header( "Content-Length", file_size )
		.header( "Content-Disposition", format!( "attachment; filename=\"{}\"", file_name ) )
		.header( "X-Encrypted", if is_encrypted { "true" } else { "false" } )
		.body( file_body )
		.unwrap();

	return Ok( response_object );
}

async fn curl_upload_processor( state: State<AppState>, Query( query ): Query<UploadQuery>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	let is_encrypted = query.encrypted.unwrap_or( false );

	loop {
		let parts_of_curl = part.next_field().await;

		let current_part = match parts_of_curl {
			Err( error ) => { return Err( ( StatusCode::BAD_REQUEST, format!( "curl error location 1: {}\n", error ) ) ); }
			Ok( part_found ) => part_found
		};

		let field = match current_part {
			Some( inner_field ) => inner_field,
			None => break
		};

		let field_name = field.name().unwrap_or( "unknown" ).to_string();

		if field_name == "f" {
			match upload_file( state, field, is_encrypted ).await {
				Err( error_message ) => { return Err( error_message ); }
				Ok( message_from_uploader ) => { return Ok( message_from_uploader ); }
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
}

async fn html_upload_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	loop {
		let parts_of_html_form = part.next_field().await;

		let current_part = match parts_of_html_form {
			Err( error ) => { return Err( ( StatusCode::BAD_REQUEST, format!( "Multipart Error: {}", error ) ) ); }
			Ok( found_next_form_part ) => found_next_form_part
		};

		let current_field = match current_part {
			Some( found_a_field ) => found_a_field,
			None => break,
		};

		let current_field_name = current_field.name().unwrap_or( "unknown" ).to_string();

		if current_field_name == "file_upload_field" {
			match upload_file( state, current_field, false ).await {
				Ok( message_from_uploader ) => { return Ok( message_from_uploader ); }
				Err( error_msg ) => { return Err( error_msg ); }
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
}

async fn html_download_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	loop {
		let html_parts = part.next_field().await;

		let current_html_part = match html_parts {
			Err( error_message ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "issue with download site, error: {}", error_message ) ) ); }
			Ok( found ) => found
		};

		let current_field = match current_html_part {
			Some( field ) => field,
			None => break
		};

		let field_name = current_field.name().unwrap_or( "massive_issue_please_fix" ).to_string();

		if field_name == "file_download_field" {
			match search_file( state, current_field ).await {
				Err( error_message ) => { return Err( error_message ) }
				Ok( message_from_uploader ) => { return Ok( message_from_uploader ) }
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, format!( "No file ID field found in the download form." ) ) );
}

pub async fn add_retry_after( request: Request, next: Next ) -> Response {
	let response = next.run( request ).await;
	if response.status() == StatusCode::TOO_MANY_REQUESTS {
		let ( mut parts, body ) = response.into_parts();
		parts.headers.insert( header::RETRY_AFTER, HeaderValue::from_static( "1" ) );
		Response::from_parts( parts, body )
	} else { response }
}

pub fn spawn_cleanup_task( db: SqlitePool ) {
	tokio::spawn( async move {
		loop {
			tokio::time::sleep( Duration::from_secs( 60 ) ).await;
			println!( "Cleanup function" );

			let current_time: u64 = match SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ) {
				Ok( time ) => time.as_secs(),
				Err( error_message ) => panic!( "Critical system time issue, possibly before UNIX_EPOCH. Details: {}", error_message ),
			};

			let res = match sqlx::query( "SELECT ID, FileName FROM filetable WHERE UploadTime < ?" )
				.bind( (current_time as i64) - 3600 )
				.fetch_all( &db )
				.await
				.map_err( |e| ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to search db, error: {}", e ) ) ) {
					Err( error_message ) => { println!( "error: {:?}", error_message ); continue; }
					Ok( res ) => res
				};

			for row in res {
				let file_id: String = row.get( "ID" );
				let file_name: String = row.get( "FileName" );

				match remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
					Err( error ) => {
						println!( "Error in cleanup with file id {} | more info: {}", file_id, error );
						match sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( file_id.to_string() ).execute( &db ).await {
							Ok(..) => {
								println!( "File {} removed and database updated", file_id );
								match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
									Ok(()) => { println!( "File {} directory deleted", file_id ); }
									Err( error_msg ) => { println!( "File {} directory deletion failed. Details: {}", file_id, error_msg ); }
								}
							}
							Err( error ) => { println!( "file {} deleted but database entry still exists. details: {}", file_id, error ) }
						};
					},
					Ok(()) => {
						match sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( file_id.to_string() ).execute( &db ).await {
							Ok(..) => {
								println!( "File {} removed and database updated", file_id );
								match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
									Ok(()) => { println!( "File {} directory deleted", file_id ); }
									Err( error_msg ) => { println!( "File {} directory deletion failed. Details: {}", file_id, error_msg ); }
								}
							}
							Err( error ) => { println!( "file {} deleted but database entry still exists. details: {}", file_id, error ) }
						};
					}
				};
			}

			let mut directories: ReadDir = match read_dir( "./uploads/temp" ).await {
				Err( error_msg ) => { println!( "failed to read dirs, error: {}", error_msg ); continue; }
				Ok( dir ) => dir
			};

			loop {
				let entry: DirEntry = match directories.next_entry().await {
					Err( error_msg ) => { println!( "directory read error: {}", error_msg ); break; }
					Ok( dir ) => match dir {
						Some( dir2 ) => dir2,
						None => break
					}
				};

				match sqlx::query( "SELECT ID FROM filetable WHERE ID = ?" )
					.bind( entry.file_name().to_string_lossy().to_string() )
					.fetch_optional( &db )
					.await {
						Err( error_msg ) => { println!( "error in deleting orphaned dir: {}", error_msg ) }
						Ok( None ) => {
							match remove_dir_all( format!( "./uploads/temp/{}", entry.file_name().to_string_lossy().to_string() ) ).await {
								Err( error_msg ) => { println!( "failed to delete orphaned dir {}, error: {}", entry.file_name().to_string_lossy().to_string(), error_msg ); }
								Ok(()) => { println!( "orphaned directory {} deleted", entry.file_name().to_string_lossy().to_string() ) }
							}
						}
						_ => {}
					}
			}
		}
	});
}


pub fn build_router( state: AppState ) -> Router {
	Router::new()
		.route( "/ping", get( pong ) )
		.route( "/status/{file_id}", get( file_status ) )
		.route( "/delete/{file_id}", get( delete_file ) )
		.route( "/download/{file_id}", get( download_file ) )
		.route( "/curlup", post( curl_upload_processor ) )
		.route( "/html_uploader_form", get( html_uploader_form ) )
		.route( "/html_upload_processor", post( html_upload_processor ) )
		.route( "/html_downloader_form", get( html_downloader_form ) )
		.route( "/html_download_processor", post( html_download_processor ) )
		.route( "/", get( main_menu ) )
		.with_state( state )
		.layer( DefaultBodyLimit::max( 1 * 1024 * 1024 * 256 ) )
		.layer(
			TraceLayer::new_for_http()
				.make_span_with( DefaultMakeSpan::new().level( tracing::Level::INFO ) )
				.on_response( DefaultOnResponse::new().level( tracing::Level::INFO ) )
		)
		.layer(
			CorsLayer::new()
				.allow_origin( "http://localhost:3000".parse::<HeaderValue>().unwrap() )
				.allow_origin( "https://seanathan10.github.io".parse::<HeaderValue>().unwrap() )
				.allow_methods( [ Method::GET, Method::POST ] )
				.expose_headers( [ header::CONTENT_DISPOSITION, axum::http::HeaderName::from_static( "x-encrypted" ) ] )
		)
		.layer(
			CorsLayer::new()
				.allow_origin( "https://send.withcapsule.dev".parse::<HeaderValue>().unwrap() )
				.allow_methods( [ Method::GET, Method::POST ] )
				.expose_headers( [ header::CONTENT_DISPOSITION, axum::http::HeaderName::from_static( "x-encrypted" ) ] )
		)
		.layer(
			CorsLayer::new()
				.allow_origin( "https://withcapsule.dev".parse::<HeaderValue>().unwrap() )
				.allow_methods( [ Method::GET, Method::POST ] )
				.expose_headers( [ header::CONTENT_DISPOSITION, axum::http::HeaderName::from_static( "x-encrypted" ) ] )
		)
		.layer(
			CorsLayer::new()
				.allow_origin( "https://withcapsule.dev/".parse::<HeaderValue>().unwrap() )
				.allow_methods( [ Method::GET, Method::POST ] )
				.expose_headers( [ header::CONTENT_DISPOSITION, axum::http::HeaderName::from_static( "x-encrypted" ) ] )
		)


}



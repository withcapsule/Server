use std::{
	net::{
		SocketAddr
	}
};

use axum::{
	extract::{
		ConnectInfo,
		Multipart,
		State,
	},
	http::{
		StatusCode,
		header,
		HeaderMap,
	},
	response::{
		Html,
		IntoResponse,
		Redirect,
		Response,
	},
};

use crate::{
	handlers::{
		upload_file,
		search_file,
	},
	state::{
		AppState
	},
};

// Set to true to enable the built-in HTML upload/download pages (highly useful for running the server locally)
const LOCAL_HTML: bool = false;

pub async fn main_menu() -> Response {
	if !LOCAL_HTML {
		return Redirect::to( "https://withcapsule.dev" ).into_response();
	}
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
	).into_response()
}

pub async fn html_uploader_form() -> Response {
	if !LOCAL_HTML {
		return StatusCode::NOT_FOUND.into_response();
	}
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
	).into_response()
}

pub async fn html_downloader_form() -> Response {
	if !LOCAL_HTML {
		return StatusCode::NOT_FOUND.into_response();
	}
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
	).into_response()
}

pub async fn html_upload_processor( State( state ): State<AppState>, ConnectInfo( addr ): ConnectInfo<SocketAddr>, headers: HeaderMap, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	let ip = addr.ip();

	if let Some( bytes ) = headers.get( header::CONTENT_LENGTH )
		.and_then( |v| v.to_str().ok() )
		.and_then( |s| s.parse::<u64>().ok() )
	{
		if state.bandwidth.would_exceed( ip, bytes ) {
			return Err( ( StatusCode::TOO_MANY_REQUESTS, "Bandwidth limit exceeded. Try again later.".to_string() ) );
		}
	}

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
			match upload_file( State( state.clone() ), ip, current_field, false ).await {
				Ok( message_from_uploader ) => { return Ok( message_from_uploader ); }
				Err( error_msg ) => { return Err( error_msg ); }
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
}

pub async fn html_download_processor( state: State<AppState>, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
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

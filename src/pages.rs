use axum::{
	http::StatusCode,
	response::{
		Html,
		IntoResponse,
		Json,
		Redirect,
		Response,
	},
};

// Set to true to enable the built-in HTML upload/download pages (highly useful for running the server locally)
const LOCAL_HTML: bool = false;

use serde_json::{
	json,
	Value,
};

pub async fn pong() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}

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

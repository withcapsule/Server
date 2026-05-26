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
    	Multipart,
     	multipart::{
      		Field
      	}
    }
};

use std::{
	fs::{
		self,
		OpenOptions,
	},
	io::{
		Read,
		Seek,
		SeekFrom,
		Write
	}
};

use tokio::{
	fs::{
		File,
		create_dir_all,
	},
	io::AsyncWriteExt,
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
                <form action="/html_upload_processor" method="post" enctype="multipart/form-data">
                    <label>
                        Choose file to upload:
                        <input type="file" name="file_upload_field">
                    </label>
                    <button type="submit">Upload</button>
                </form>
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

async fn pong() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}


async fn upload_file_2( file_name: String ) -> Result<String, ( StatusCode, String )> {
	let path = format!( "./uploads/temp/{}", file_name );

	let file = File::create( path ).await;  // -> io::Result<File>
	match file {
		Err( error_msg ) => (StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to write: {}", error_msg )),
		Ok( _ ) => (StatusCode::CREATED, format!( "File created {}", file_name ) ),
	};

	return Ok( file_name )
}

async fn upload_file( mut field: Field<'_> ) -> Result<String, ( StatusCode, String )> {
	let file_name = field.file_name().unwrap_or( "upload" ).to_string();
	let path = format!( "./uploads/temp/{}", file_name );

	let mut file = File::create( &path ).await.map_err( |e| {
		( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to create file: {}", e ) )
	})?;

	while let Some( chunk ) = field.chunk().await.map_err( |e| {
		( StatusCode::BAD_REQUEST, format!( "Failed to read chunk: {}", e ) )
	})? {
		file.write_all( &chunk ).await.map_err( |e| {
			( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to write chunk: {}", e ) )
		})?;
	}

	return Ok( file_name )
}

async fn download_file() {

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
			// run the upload_file function and await the result
			// field.file_name().unwrap_or( "upload" ).to_string();


			let file_name = match current_field.file_name() {
				Some( file ) => file.to_string(),
				None => "__failure".to_string()
			};


			match upload_file_2( file_name ).await {
				// upload file returns Result<String, (StatusCode, String)>
				// so Ok() is literally just returning a formatted String
				Ok( file_saved_as ) => {
					// returning Result<String>
					return Ok( format!( "File uploaded and saved as: {}", file_saved_as ) );
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

	Err(
		( StatusCode::BAD_REQUEST, "No file found in request".to_string() )
 	)
}

async fn html_download_processor() {

}


#[tokio::main]
async fn main() {
	create_dir_all( "./uploads/temp" ).await.unwrap();

	let CORS = CorsLayer::new()
		.allow_origin( "http://localhost:3000".parse::<HeaderValue>().unwrap() )
		.allow_methods( [ Method::GET, Method::POST ] );

    let app: Router<> = Router::new()
        .route( "/ping", get( pong ) )

        // .route( "/upload", post( upload_file )  )
        .route( "/download", get( download_file ) )

        .route( "/html_uploader_form", get( html_uploader_form ) )
        .route( "/html_upload_processor", post( html_upload_processor ) )

        .route( "/html_downloader_form", get( html_downloader_form ) )
        .route( "/html_download_processor", post( html_download_processor ) )

        .route("/", get( main_menu ) )

        .layer( CORS );

    let listener = TcpListener::bind( "0.0.0.0:9001" ).await.unwrap();
    axum::serve( listener, app ).await.unwrap();
}

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
    	Multipart
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
	net::{
		TcpListener
	}
};

use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

async fn ping() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}

async fn upload_file( mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	loop {
		let stream_result = part.next_field().await;

		let opt_field = match stream_result {
			Err( error ) => {
				return Err(
					( StatusCode::BAD_REQUEST, format!( "Multipart Error: {}", error ) )
			 	);
			}

			Ok( option ) => {
				option;
			}
		};

		let field = match opt_field {
			Some( found_field ) => found_field,
			None => break,
		};

		let field_name = field.name().unwrap_or( "unknown" ).to_string();

		if field_name == "file" {
			match upload_file( field ).await {
				Ok( saved_file_as ) => {
					return Ok( format!( "File uploaded and saved as: {}", saved_file_as ) );
				}

				Err( error_msg ) => {
					return Err(
						( StatusCode::INTERNAL_SERVER_ERROR, format!( "File upload failed. Error: {}", error_msg ) )
					);
				}
			}
		}
	}

	Err(
		( StatusCode::BAD_REQUEST, "No file found in request".to_string() )
 	)
}

async fn download_file() {

}

async fn upload_gui() -> Html<&'static str> {
	Html( r#"
        <!doctype html>
        <html>
            <body>
                <form action="/upload" method="post" enctype="multipart/form-data">
                    <label>
                        Choose file to upload:
                        <input type="file" name="file">
                    </label>
                    <button type="submit">Upload</button>
                </form>
            </body>
        </html>
    "#
	)
}


#[tokio::main]
async fn main() {
	create_dir_all( "./uploads/temp" ).await.unwrap();

	let CORS = CorsLayer::new()
		.allow_origin( "http://localhost:3000".parse::<HeaderValue>().unwrap() )
		.allow_methods( [ Method::GET, Method::POST ] );

    let app: Router<> = Router::new()
        .route( "/upload", post( upload_file )  )
        .route( "/download", get( download_file ) )
        .route( "/upload_gui", get( upload_gui ) )
    	.route("/", get( ping ) )
     	.layer( CORS );

    let listener = TcpListener::bind( "0.0.0.0:9001" ).await.unwrap();
    axum::serve( listener, app ).await.unwrap();
}

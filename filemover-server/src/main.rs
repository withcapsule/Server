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
	while let Some( field ) = part.next_field().await.map_err( | err | {
		(StatusCode::BAD_REQUEST, format!( "Multipart Error: {}", err ) )
	} )
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

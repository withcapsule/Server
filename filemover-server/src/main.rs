use axum::{
    Router,
    Json,
    http::{
   		HeaderValue,
     	Method
    },
    routing::{
    	get,
    	post
    }
};

use std::{
	fs::{
		self,
		File,
		OpenOptions,
		create_dir_all
	},
	io::{
		Read,
		Seek,
		SeekFrom,
		Write
	}
};

use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

async fn ping() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}

async fn upload_file() {

}

async fn download_file() {

}

#[tokio::main]
async fn main() {
	create_dir_all( "./uploads/temp" ).unwrap();

	let CORS = CorsLayer::new();
	CORS.allow_origin( "http://localhost:3000".parse::<HeaderValue>().unwrap() );
	CORS.allow_methods( [ Method::GET, Method::POST ] );

    let app: Router<> = Router::new()
        .route( "/upload", post( upload_file() )  )
        .route( "/download", get( download_file() ) )
    	.route("/", get( ping ) )
     	.layer( CORS );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9001").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

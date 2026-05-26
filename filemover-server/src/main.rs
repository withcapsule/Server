use axum::{
    routing::get,
    Router,
    Json
};

use serde_json::{json, Value};

async fn ping() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}


#[tokio::main]
async fn main() {
    let app: Router<> = Router::new()
        .route("/", get( ping ) );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:9001").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

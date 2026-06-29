use std::{
	time::{
		Duration
	}
};

use axum::{
	Router,
	error_handling::{
		HandleErrorLayer
	},
	extract::{
		DefaultBodyLimit,
		Request,
	},
	http::{
		HeaderValue,
		Method,
		StatusCode,
		header,
	},
	middleware::{
		from_fn,
		Next
	},
	response::{
		Json,
		Response
	},
	routing::{
		delete,
		get,
		post
	},
};

use axum_governor::{
	GovernorLayer
};

use tower::{
	BoxError,
	ServiceBuilder,
	limit::{
		GlobalConcurrencyLimitLayer
	},
	load_shed::{
		LoadShedLayer
	},
};

use real::{
	IpExtractor,
	RealIpLayer,
};

use tower_http::{
	cors::{
		CorsLayer
	},
	timeout::{
		TimeoutLayer
	},
	trace::{
		DefaultMakeSpan,
		DefaultOnResponse,
		TraceLayer
	},
};

use crate::{
	state::{
		AppState
	}
};

use serde_json::{
	json,
	Value,
};

use crate::{
	pages::{
		main_menu,
		html_uploader_form,
		html_downloader_form,
		html_upload_processor,
		html_download_processor,
	}
};

use crate::{
	handlers::{
		file_status,
		delete_file,
		download_file,
		curl_upload_processor,
	}
};

const ANDROID_APP_LINKS_SHA256: &str = "40:28:8B:97:8A:02:82:BC:85:CC:EA:A6:4F:36:E2:FA:09:B3:62:F7:FA:38:F3:60:54:A8:69:9E:BC:2C:B3:D5";

const MAX_CONCURRENT_REQUESTS: usize = 256;
const REQUEST_TIMEOUT_SECS: u64 = 600;

pub async fn add_retry_after( request: Request, next: Next ) -> Response {
	let response = next.run( request ).await;
	if response.status() == StatusCode::TOO_MANY_REQUESTS {
		let ( mut parts, body ) = response.into_parts();
		parts.headers.insert( header::RETRY_AFTER, HeaderValue::from_static( "1" ) );
		Response::from_parts( parts, body )
	} else { response }
}

pub async fn pong() -> Json<Value> {
	return Json( json!( { "message": "pong" } ) )
}

pub async fn assetlinks_json() -> axum::response::Response {
	let body = format!(
		r#"[{{"relation":["delegate_permission/common.handle_all_urls"],"target":{{"namespace":"android_app","package_name":"dev.withcapsule.android","sha256_cert_fingerprints":["{}"]}}}}]"#,
		ANDROID_APP_LINKS_SHA256
	);
	axum::response::Response::builder()
		.status( 200 )
		.header( "Content-Type", "application/json" )
		.body( axum::body::Body::from( body ) )
		.unwrap()
}

pub fn build_router( state: AppState ) -> Router {
	Router::new()
		.route( "/ping", get( pong ) )
		.route( "/.well-known/assetlinks.json", get( assetlinks_json ) )
		.route( "/status/{file_id}", get( file_status ) )
		.route( "/delete/{file_id}", delete( delete_file ) )
		.route( "/download/{file_id}", get( download_file ) )
		.route( "/upload", post( curl_upload_processor ) )
		.route( "/html_uploader_form", get( html_uploader_form ) )
		.route( "/html_upload_processor", post( html_upload_processor ) )
		.route( "/html_downloader_form", get( html_downloader_form ) )
		.route( "/html_download_processor", post( html_download_processor ) )
		.route( "/", get( main_menu ) )
		.with_state( state )
		.layer( DefaultBodyLimit::max( 100 * 1024 * 1024 ) )
		.layer(
			TraceLayer::new_for_http()
				.make_span_with( DefaultMakeSpan::new().level( tracing::Level::INFO ) )
				.on_response( DefaultOnResponse::new().level( tracing::Level::INFO ) )
		)
		.layer( GovernorLayer::default() )
		.layer( from_fn( add_retry_after ) )


		.layer( RealIpLayer::with_extractor(
			IpExtractor::default()
				.with_headers( vec![ "CF-Connecting-IP".to_string() ] )
				.trust_private_ips( true )
		) )


		.layer( TimeoutLayer::with_status_code( StatusCode::REQUEST_TIMEOUT, Duration::from_secs( REQUEST_TIMEOUT_SECS ) ) )
		.layer(
			ServiceBuilder::new()
				.layer( HandleErrorLayer::new( |_: BoxError| async {
					StatusCode::SERVICE_UNAVAILABLE
				} ) )
				.layer( LoadShedLayer::new() )
				.layer( GlobalConcurrencyLimitLayer::new( MAX_CONCURRENT_REQUESTS ) )
		)
		.layer(
			CorsLayer::new()
				.allow_origin(
					[
						"http://localhost:3000".parse::<HeaderValue>().unwrap(),
						"https://seanathan10.github.io".parse::<HeaderValue>().unwrap(),
						"https://send.withcapsule.dev".parse::<HeaderValue>().unwrap(),
						"https://withcapsule.dev".parse::<HeaderValue>().unwrap(),
					]
				)
				.allow_methods( [ Method::GET, Method::POST, Method::DELETE ] )
				.expose_headers( [ header::CONTENT_DISPOSITION, axum::http::HeaderName::from_static( "x-encrypted" ) ] )
		)
}

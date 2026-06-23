use axum::{
	Router,
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

use real::{
	RealIpLayer
};

use tower_http::{
	cors::{
		CorsLayer
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

use crate::{
	pages::{
		pong,
		main_menu,
		html_uploader_form,
		html_downloader_form,
	}
};

use crate::{
	handlers::{
		file_status,
		delete_file,
		download_file,
		curl_upload_processor,
		html_upload_processor,
		html_download_processor,
	}
};

pub async fn add_retry_after( request: Request, next: Next ) -> Response {
	let response = next.run( request ).await;
	if response.status() == StatusCode::TOO_MANY_REQUESTS {
		let ( mut parts, body ) = response.into_parts();
		parts.headers.insert( header::RETRY_AFTER, HeaderValue::from_static( "1" ) );
		Response::from_parts( parts, body )
	} else { response }
}

pub fn build_router( state: AppState ) -> Router {
	Router::new()
		.route( "/ping", get( pong ) )
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
		.layer( DefaultBodyLimit::max( 1 * 1024 * 1024 * 256 ) )
		.layer(
			TraceLayer::new_for_http()
				.make_span_with( DefaultMakeSpan::new().level( tracing::Level::INFO ) )
				.on_response( DefaultOnResponse::new().level( tracing::Level::INFO ) )
		)
		.layer( GovernorLayer::default() )
		.layer( from_fn( add_retry_after ) )
		.layer( RealIpLayer::default() )
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

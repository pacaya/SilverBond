use std::borrow::Cow;

use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
    response::IntoResponse,
};
use include_dir::{Dir, include_dir};

static PUBLIC_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/public");

pub async fn serve(path: String) -> impl IntoResponse {
    let clean = match path.as_str() {
        "" | "/" => Cow::Borrowed("index.html"),
        value => Cow::Owned(value.trim_start_matches('/').to_string()),
    };

    let file = PUBLIC_DIR
        .get_file(clean.as_ref())
        .or_else(|| PUBLIC_DIR.get_file("index.html"));

    match file {
        Some(file) => {
            let mut response = Response::new(Body::from(file.contents().to_vec()));
            *response.status_mut() = StatusCode::OK;
            let mime = mime_guess::from_path(file.path())
                .first_raw()
                .unwrap_or("application/octet-stream");
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime)
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
            );
            response
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

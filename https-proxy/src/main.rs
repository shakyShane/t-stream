use axum::extract::State;
use axum::http::{HeaderValue, Uri};
use axum::routing::any;
use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};

use hyper_tls::HttpsConnector;
use hyper_util::{
    client::legacy::connect::HttpConnector, client::legacy::Client, rt::TokioExecutor,
};

#[tokio::main]
async fn main() {
    let https = HttpsConnector::new();
    let client: Client<HttpsConnector<HttpConnector>, Body> =
        Client::builder(TokioExecutor::new()).build(https);

    let app = Router::new().nest_service("/", any(handler).with_state(client));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(client): State<Client<HttpsConnector<HttpConnector>, Body>>,
    req: Request,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);

    let target = "browsersync.io";
    let uri = format!("https://{}{}", target, path_query);
    let parsed = Uri::try_from(uri).map_err(|_| StatusCode::BAD_REQUEST)?;

    let (parts, body) = req.into_parts();
    let mut req = Request::from_parts(parts, body);

    *req.uri_mut() = parsed;

    let host_header_value = HeaderValue::from_str(target).unwrap();
    req.headers_mut().insert("host", host_header_value);

    Ok(client
        .request(req)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .into_response())
}

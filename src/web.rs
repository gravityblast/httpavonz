use crate::HttpStatusResponse;
use axum::{
    extract::{Path, Query},
    headers,
    http::{
        header::{self},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{de, Deserialize, Deserializer};
use serde_json::json;
use std::{env, fmt, net::SocketAddr, str::FromStr, time::Duration};
use tower_http::{
    cors::{Any, CorsLayer},
    set_header::SetResponseHeaderLayer,
    trace::{DefaultOnResponse, TraceLayer},
    LatencyUnit,
};

pub fn build_sock_addr() -> SocketAddr {
    // Use "[::]" to listen on both IPv4 (0.0.0.0) and IPv6
    let srv_host = env::var("HTTPAVONZ_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let srv_port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    format!("{}:{}", srv_host, srv_port)
        .parse::<SocketAddr>()
        .unwrap()
}

pub fn build_app_router() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/:code", get(status_code_handler).post(status_code_handler))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::SERVER,
            HeaderValue::from_static("httpavonz"),
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
                .expose_headers([
                    header::LINK,               // 103
                    header::CONTENT_RANGE,      // 206
                    header::LOCATION,           // 301, 302, 303, 305, 307, 308
                    header::WWW_AUTHENTICATE,   // 401
                    header::PROXY_AUTHENTICATE, // 407
                    header::RETRY_AFTER,        // 429
                ]),
        )
        .layer(
            TraceLayer::new_for_http().on_response(
                DefaultOnResponse::new()
                    .include_headers(true)
                    .latency_unit(LatencyUnit::Micros),
            ),
        )
}

// Include utf-8 file at **compile** time.
async fn index_handler() -> Html<&'static str> {
    Html(std::include_str!("../index.html"))
}

async fn status_code_handler(
    Path(code): Path<u16>,
    req_headers: HeaderMap,
    Query(params): Query<Params>,
) -> Response {
    let resp = HttpStatusResponse::new(code);
    let status_code = StatusCode::from_u16(resp.code).unwrap();
    let mut body = "".to_string();
    let mut headers = resp.headers.unwrap_or_else(HeaderMap::new);

    if let Some(sleep) = params.sleep {
        let time = if sleep > 0 && sleep < 300000 {
            sleep
        } else {
            300000
        };

        tokio::time::sleep(Duration::from_millis(time.into())).await;
    }

    if resp.exclude_body {
        headers.insert(
            header::CONNECTION,
            headers::HeaderValue::from_str("close").unwrap(),
        );
        return (status_code, headers, body).into_response();
    }

    match req_headers.get(header::ACCEPT).map(|hv| hv.as_bytes()) {
        Some(b"application/json") => {
            headers.insert(
                header::CONTENT_TYPE,
                headers::HeaderValue::from_str("application/json").unwrap(),
            );

            body = json!({"code": resp.code, "description": resp.description}).to_string();
        }
        _ => {
            body = if let Some(b) = resp.body {
                b.to_string()
            } else {
                format!("{} {}", resp.code, resp.description)
            };
        }
    }
    (status_code, headers, body).into_response()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Params {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    sleep: Option<u32>,
}

// Serde deserialization decorator to map empty Strings to None,
fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
    }
}

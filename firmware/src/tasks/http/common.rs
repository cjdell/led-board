use alloc::string::ToString;
use embedded_io_async::{Read, Write};
use picoserve::{
    ResponseSent,
    extract::FromRequest,
    request::{Path, Request, RequestBody, RequestParts},
    response::{Content, File, IntoResponse, Response, ResponseWriter, StatusCode, ws::WebSocketUpgradeRejection},
    routing::PathRouterService,
};

#[unsafe(link_section = ".rodata.mydata")]
#[used]
static HTML_DATA: &[u8] = include_bytes!("../../../../web/bundle/index.html.gz");

pub struct CustomNotFound;

impl PathRouterService<()> for CustomNotFound {
    async fn call_path_router_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _state: &(),
        _path_parameters: (),
        path: Path<'_>,
        request: Request<'_, R>,
        response_writer: W,
    ) -> Result<ResponseSent, W::Error> {
        if request.parts.method() == "OPTIONS" {
            cors_options_response()
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await
        } else {
            (
                StatusCode::NOT_FOUND,
                format_args!("Path \"{:?}\" not found!\n", path.encoded()),
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await
        }
    }
}

pub fn redirect_home_response() -> impl IntoResponse {
    Response::new(StatusCode::TEMPORARY_REDIRECT, "").with_headers([("Location", "/")])
}

pub fn html_app_response() -> impl IntoResponse {
    Response::new(StatusCode::OK, HtmlApp).with_headers([("Content-Encoding", "gzip")])
}

pub fn cors_options_response<'a>() -> impl IntoResponse + use<'a> {
    Response::empty(StatusCode::OK).with_headers([
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "*"),
        ("Access-Control-Allow-Headers", "*"),
    ])
}

pub fn json_response(json: &str) -> impl IntoResponse + use<'_> {
    Response::ok(json)
        .with_headers([
            ("Access-Control-Allow-Origin", "*"),
            ("Content-Type", "application/json"),
        ])
        .with_headers([("Content-Length", json.len())])
}

// pub fn text_response(json: &str) -> impl IntoResponse + use<'_> {
//     Response::new(StatusCode::OK, json)
//         .with_headers([("Access-Control-Allow-Origin", "*"), ("Content-Type", "text/plain")])
// }

pub struct HtmlApp;

impl Content for HtmlApp {
    fn content_type(&self) -> &'static str {
        File::MIME_HTML
    }

    fn content_length(&self) -> usize {
        HTML_DATA.len()
    }

    async fn write_content<W: Write>(self, writer: W) -> Result<(), W::Error> {
        HTML_DATA.write_content(writer).await
    }
}

pub struct RequestInfo {}

impl<'r, State> FromRequest<'r, State> for RequestInfo {
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request<R: Read>(
        _state: &'r State,
        request_parts: RequestParts<'r>,
        _request_body: RequestBody<'r, R>,
    ) -> Result<Self, Self::Rejection> {
        for header in request_parts.headers() {
            if header.0.as_str().unwrap_or_default().to_lowercase() == "user-agent" {
                // info!("User Agent: {:?}", header.1);
            }
        }

        Ok(Self {})
    }
}

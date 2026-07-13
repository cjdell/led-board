use crate::{config::DEFAULT_PLAYLIST_FILENAME, types::*};
use animations::Playlist;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};
use utils::local_fs::LocalFsTrait as _;

pub struct GetAnimationsHandler {}

impl GetAnimationsHandler {
    pub fn new() -> Self {
        Self {}
    }
}

impl RequestHandlerService<()> for GetAnimationsHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let animations = Playlist::get_available();

        let json = serde_json::to_string(&animations).expect("Could not serialise animations");

        json_response!(req, writer, &json)
    }
}

pub struct GetPlaylistHandler {
    local_fs: LocalFs,
}

impl GetPlaylistHandler {
    pub fn new(local_fs: LocalFs) -> Self {
        Self { local_fs }
    }
}

impl RequestHandlerService<()> for GetPlaylistHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let json = match self.local_fs.read_text_file(DEFAULT_PLAYLIST_FILENAME).await {
            Ok(json) => json,
            Err(err) => {
                // return format_response!(req, writer, "No playlist file");
                return json_response!(req, writer, "[]");
            }
        };

        json_response!(req, writer, &json)
    }
}

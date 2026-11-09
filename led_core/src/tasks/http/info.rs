use crate::config::DEFAULT_PLAYLIST_FILENAME;
use animations::Playlist;
use embedded_tools::local_fs::LocalFsTrait;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};

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

pub struct GetPlaylistHandler<FS: LocalFsTrait + Clone> {
    local_fs: FS,
}

impl<FS: LocalFsTrait + Clone> GetPlaylistHandler<FS> {
    pub fn new(local_fs: FS) -> Self {
        Self { local_fs }
    }
}

impl<FS: LocalFsTrait + Clone> RequestHandlerService<()> for GetPlaylistHandler<FS> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let json = match self.local_fs.read_text_file(DEFAULT_PLAYLIST_FILENAME).await {
            Ok(json) => json,
            Err(_err) => {
                return json_response!(req, writer, "[]");
            }
        };

        json_response!(req, writer, &json)
    }
}

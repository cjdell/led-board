use crate::types::{PlaylistWatch, StatusWatch};
use animations::Playlist;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter, StatusCode},
    routing::RequestHandlerService,
};

/// Shown until the display worker's first publish (it publishes right at
/// boot, so this only covers the very first moments after power-up).
const STATUS_PLACEHOLDER: &str = concat!(
    r#"{"now_playing":{"kind":"Blank","title":"Booting…","animation":"Blank","elapsed_ms":0,"duration_ms":0},"#,
    r#""playlist_index":0,"playlist_len":0,"paused":false,"streaming":false,"status_screen":false,"#,
    r#""power_limit_pct":0,"uptime_s":0}"#
);

const PLAYLIST_PLACEHOLDER: &str = r#"{"playlist":[],"source":"Default","index":0}"#;

/// `GET /api/animations` — the animation names a playlist may reference.
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

/// `GET /api/version` — identity of the compiled-in firmware image
/// (`{git_sha, dirty, commit_time, runtime}`). The live status carries the
/// sha too; this answers even before the display worker's first publish.
pub struct GetVersionHandler {}

impl RequestHandlerService<()> for GetVersionHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let json = serde_json::to_string(&crate::types::FirmwareVersion::current()).expect("Could not serialise version");

        json_response!(req, writer, &json)
    }
}

/// `GET /api/info/playlist` — the playlist the display worker is *actually*
/// running (`{playlist, source, index}`), not just the file on flash: a
/// live-pushed playlist and the built-in default are reported too.
pub struct GetPlaylistHandler {
    watch: &'static PlaylistWatch,
}

impl GetPlaylistHandler {
    pub fn new(watch: &'static PlaylistWatch) -> Self {
        Self { watch }
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
        let mut receiver = match self.watch.receiver() {
            Some(receiver) => receiver,
            None => {
                return (StatusCode::SERVICE_UNAVAILABLE, "Too many watchers")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await
            }
        };

        let json = receiver.try_get().unwrap_or_else(|| PLAYLIST_PLACEHOLDER.into());

        json_response!(req, writer, &json)
    }
}

/// `GET /api/status` — the same JSON the WebSocket pushes: now playing,
/// progress, playlist position, pause/streaming/power/uptime.
pub struct GetStatusHandler {
    watch: &'static StatusWatch,
}

impl GetStatusHandler {
    pub fn new(watch: &'static StatusWatch) -> Self {
        Self { watch }
    }
}

impl RequestHandlerService<()> for GetStatusHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let mut receiver = match self.watch.receiver() {
            Some(receiver) => receiver,
            None => {
                return (StatusCode::SERVICE_UNAVAILABLE, "Too many watchers")
                    .write_to(req.body_connection.finalize().await?, writer)
                    .await
            }
        };

        let json = receiver.try_get().unwrap_or_else(|| STATUS_PLACEHOLDER.into());

        json_response!(req, writer, &json)
    }
}

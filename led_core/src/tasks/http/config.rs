use crate::config::LedBoardConfig;
use embedded_tools::config::ConfigFileTrait;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};

pub struct GetConfigHandler<CF> {
    config_file: CF,
}

impl<CF> GetConfigHandler<CF> {
    pub fn new(config_file: CF) -> Self {
        Self { config_file }
    }
}

impl<CF: ConfigFileTrait<LedBoardConfig>> RequestHandlerService<()> for GetConfigHandler<CF> {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let json = match self.config_file.get_json().await {
            Ok(json) => json,
            Err(err) => {
                return format_response!(req, writer, "Error reading JSON: {err:?}");
            }
        };

        json_response!(req, writer, json.as_str())
    }
}

pub struct SaveConfigHandler<CF, B: crate::board::Board> {
    config_file: CF,
    board: B,
}

impl<CF, B: crate::board::Board> SaveConfigHandler<CF, B> {
    pub fn new(config_file: CF, board: B) -> Self {
        Self { config_file, board }
    }
}

impl<CF: ConfigFileTrait<LedBoardConfig>, B: crate::board::Board> RequestHandlerService<()>
    for SaveConfigHandler<CF, B>
{
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let buffer = read_request_to_buffer!(req, writer);

        if let Err(err) = self.config_file.set_json(&buffer).await {
            return format_response!(req, writer, "Error applying JSON: {err:?}");
        }

        if let Err(err) = self.config_file.save().await {
            return format_response!(req, writer, "Error save JSON: {err:?}");
        }

        // Mirror the updated WiFi config to the ESP32-C3 slave radio (no-op without the radio build).
        self.board.config_saved().await;

        format_response!(req, writer, "Done")
    }
}

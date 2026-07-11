use crate::types::*;
use picoserve::{
    ResponseSent,
    io::Read,
    request::Request,
    response::{IntoResponse, ResponseWriter},
    routing::RequestHandlerService,
};

pub struct GetConfigHandler {
    config_file: LedBoardConfigFile,
}

impl GetConfigHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
    }
}

impl RequestHandlerService<()> for GetConfigHandler {
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

pub struct SaveConfigHandler {
    config_file: LedBoardConfigFile,
}

impl SaveConfigHandler {
    pub fn new(config_file: LedBoardConfigFile) -> Self {
        Self { config_file }
    }
}

impl RequestHandlerService<()> for SaveConfigHandler {
    async fn call_request_handler_service<R: Read, W: ResponseWriter<Error = R::Error>>(
        &self,
        _: &(),
        _: (),
        mut req: Request<'_, R>,
        writer: W,
    ) -> Result<ResponseSent, W::Error> {
        let buffer = read_request_to_buffer!(req, writer);

        // let buffer = match request.body_connection.body().read_all().await {
        //     Ok(buffer) => buffer,
        //     Err(err) => return format_response!(request, response_writer, "Error reading request: {err:?}"),
        // };

        if let Err(err) = self.config_file.set_json(&buffer).await {
            return format_response!(req, writer, "Error applying JSON: {err:?}");
        }

        if let Err(err) = self.config_file.save().await {
            return format_response!(req, writer, "Error save JSON: {err:?}");
        }

        format_response!(req, writer, "Done")
    }
}

macro_rules! format_response {
  ($request:expr, $response_writer:expr, $($arg:tt)*) => {
    alloc::format!($($arg)*)
      .write_to($request.body_connection.finalize().await?, $response_writer)
      .await
  };
}

macro_rules! json_response {
    ($request:expr, $response_writer:expr, $json:expr) => {
        crate::tasks::http::common::json_response($json)
            .write_to($request.body_connection.finalize().await?, $response_writer)
            .await
    };
}

macro_rules! read_request_to_buffer {
    ($request:expr, $response_writer:expr) => {{
        let file_size = $request.body_connection.body().content_length();
        let mut buffer = alloc::vec::Vec::new();
        buffer.resize(file_size, 0u8);
        let mut reader = $request.body_connection.body().reader();
        match reader.read_exact(&mut buffer).await {
            Ok(()) => Ok(()),
            Err(err) => match err {
                embedded_io::ReadExactError::UnexpectedEof => {
                    return format_response!($request, $response_writer, "UnexpectedEof: Expected {file_size} bytes");
                }
                embedded_io::ReadExactError::Other(err) => Err(err),
            },
        }?;
        buffer
    }};
}

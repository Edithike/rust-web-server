use crate::common::FileManager;
use crate::http::{HttpHeader, HttpStatus, RequestBody, Response, ResponseBody};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Contains all logic to handle each valid request
pub(crate) struct RequestHandler;

impl RequestHandler {
    /// Lists files in the upload folder
    ///
    /// The `index.html` template file is opened, and read into a string.
    /// The files in the upload folder are fetched, and then an HTML string og lists is generated with
    /// each file path as the `href`, and the filename as the display. This string is interpolated into
    /// the template file, and the resulting string returned in the response.
    pub(crate) fn list_files() -> Result<Response, Response> {
        let mut html_file =
            File::open("templates/index.html").map_err(|_| ErrorHandler::handle_server_error())?;
        let mut template = String::new();
        html_file
            .read_to_string(&mut template)
            .map_err(|_| ErrorHandler::handle_server_error())?;

        let files = FileManager::list_files_with_paths("uploads")
            .map_err(|_| ErrorHandler::handle_server_error())?;

        let file_links: String = files
            .iter()
            .map(|(name, path)| format!(r#"<li><a href="{}">{}</a></li>"#, path, name))
            .collect::<Vec<String>>()
            .join("\n");

        let html_output = template.replace("{{FILES_LIST}}", &file_links);

        Ok(Response::builder()
            .body(ResponseBody::Text(html_output))
            .build())
    }

    /// Returns an uploaded file in the response to be viewed in the browser
    ///
    /// Arguments:
    /// - **filename**: The name of the file to be viewed, can possibly include a directory
    ///
    /// "/uploads/" is trimmed from the start of the file name, and then joined with the uploads `Path`.
    /// The joined path is then canonicalized (transformed to its absolute path, removing all traversals)
    /// to protect from a possible traversal attack from a malicious client.
    /// The absolute path of the requested file is then compared with the absolute path of the uploads
    /// directory, to assert that the requested file exists within the uploads directory, if so, the
    /// response is built, if any condition fails along the way, an error response is sent.
    pub(crate) fn view_file(filename: String) -> Result<Response, Response> {
        let filename = filename
            .trim_start_matches('/')
            .trim_start_matches("uploads/");

        let base_path = Path::new("uploads");
        let requested_path = base_path.join(filename);

        // Get the absolute path, removing all traversals, this protects from traversal attacks
        match requested_path.canonicalize() {
            Ok(resolved_path) => {
                let canonicalized_base_path = base_path
                    .canonicalize()
                    .map_err(|_| ErrorHandler::handle_server_error())?;

                // Assert that the path is still within the uploads directory
                if resolved_path.starts_with(canonicalized_base_path) {
                    Ok(Response::builder()
                        .body(ResponseBody::File(
                            resolved_path.to_string_lossy().to_string(),
                        ))
                        .build())
                } else {
                    Err(ErrorHandler::handle_access_denied())
                }
            }
            Err(_) => {
                eprintln!("Canonicalized file not found: {}", requested_path.display());
                Err(ErrorHandler::handle_invalid_file_request())
            }
        }
    }

    /// Returns the view of the template to upload a new file
    pub(crate) fn view_to_upload_files() -> Result<Response, Response> {
        Ok(Response::builder()
            .body(ResponseBody::File("templates/upload.html".to_string()))
            .build())
    }

    /// Uploads a file from a request
    ///
    /// Arguments:
    /// - **request_body**: The `RequestBody` to be used to get the file from
    ///
    /// The `RequestBody` must be of the `Multipart` variant or an error is returned.
    /// A limited number of file extensions are allowed.
    /// The file name is constructed into a `Path`, from which the file name is extracted and converted
    /// to UTF-8, this asserts that the file name passed is truly a file and contains only valid UTF-8,
    /// to protect against unforeseen behavior.
    /// If all conditions pass, the file gets saved and a response with an empty body gets returned.
    pub(crate) fn upload_file(request_body: RequestBody) -> Result<Response, Response> {
        // Ensure that the `RequestBody` is a `Multipart` type, as that is the only supported type
        // for file uploads on this server
        let uploaded_file = match request_body {
            RequestBody::Multipart(uploaded_file) => uploaded_file,
            _ => return Err(ErrorHandler::handle_bad_request()),
        };
        // A list of allowed extensions to limit the supported file types
        let allowed_extensions = ["txt", "png", "jpg", "pdf"];

        // Sanitize file name in case it contains unanticipated characters
        let sanitized_filename = Path::new(&uploaded_file.name)
            .file_name() // Extracts only the base file name, removing paths
            .and_then(|name| name.to_str()) // Convert to &str
            .ok_or(ErrorHandler::handle_bad_request())? // Fallback in case of invalid Unicode
            .to_string();

        if sanitized_filename.is_empty() || sanitized_filename != uploaded_file.name {
            return Err(ErrorHandler::handle_bad_request());
        }

        // Assert that file is an allowed type
        Path::new(&uploaded_file.name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| allowed_extensions.contains(&ext))
            .ok_or(ErrorHandler::handle_bad_request())?;

        FileManager::save_file("uploads", uploaded_file)
            .map_err(|_| ErrorHandler::handle_server_error())?;

        Ok(Response::builder()
            .status(HttpStatus::SeeOther)
            .header(HttpHeader::LOCATION, "/")
            .body(ResponseBody::Empty)
            .build())
    }
}

/// Handles all error cases
pub(crate) struct ErrorHandler;

impl ErrorHandler {
    /// Handles cases where the client requests for a page that does not exist.
    /// A 404 status code is returned, along with an HTML template for the error case.
    pub(crate) fn handle_invalid_page_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/page-not-found.html".to_string(),
            ))
            .build()
    }

    /// Handles cases where the client does not send a valid request body.
    /// A 400 status code is returned, along with an HTML template that shows the error.
    pub(crate) fn handle_bad_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File("bad-request".to_string()))
            .build()
    }

    /// Handles cases where the client requests a file that is outside the designated uploads folder.
    /// A 403 status code is returned, along with an HTML template that says access denied.
    pub(crate) fn handle_access_denied() -> Response {
        Response::builder()
            .status(HttpStatus::Forbidden)
            .body(ResponseBody::File(
                "templates/access-denied.html".to_string(),
            ))
            .build()
    }

    /// Handles cases where the client requests a file that doesn't exist in the uploads folder.
    /// A 404 status code is returned, along with an HTML template that explains this.
    pub(crate) fn handle_invalid_file_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/file-not-found.html".to_string(),
            ))
            .build()
    }

    /// Handles cases where an unknown or unrecoverable error occurs during the lifetime of the request.
    /// A 500 status code is returned, along with an appropriate HTML template.
    pub(crate) fn handle_server_error() -> Response {
        Response::builder()
            .status(HttpStatus::ServerError)
            .body(ResponseBody::File(
                "templates/server-error.html".to_string(),
            ))
            .build()
    }
}

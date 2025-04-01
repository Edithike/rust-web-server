use crate::common::{AppError, FileManager};
use crate::http::{
    HttpHeader, HttpMethod, HttpStatus, Request, RequestBody, Response, ResponseBody,
};
use crate::warn;
use crate::{get_current_military_time, log_error};
use std::path::{Path, PathBuf};

/// This stores the HTML templates as strings in the binary during compile time, reducing the 
/// dependency on a templates folder's existence
struct Templates;

impl Templates {
    const ACCESS_DENIED: &'static str = include_str!("../templates/access-denied.html");
    const BAD_REQUEST: &'static str = include_str!("../templates/bad-request.html");
    const FILE_NOT_FOUND: &'static str = include_str!("../templates/file-not-found.html");
    const INDEX: &'static str = include_str!("../templates/index.html");
    const PAGE_NOT_FOUND: &'static str = include_str!("../templates/page-not-found.html");
    const SERVER_ERROR: &'static str = include_str!("../templates/server-error.html");
    const UPLOAD : &'static str = include_str!("../templates/upload.html");
}

/// Contains all logic to handle each valid request
pub(crate) struct RequestHandler;

impl RequestHandler {
    /// Lists files in the upload folder
    ///
    /// The `index.html` template file is opened, and read into a string.
    /// The files in the upload folder are fetched, and then an HTML string og lists is generated with
    /// each file path as the `href`, and the filename as the display. This string is interpolated into
    /// the template file, and the resulting string returned in the response.
    pub(crate) fn list_files() -> Result<Response, AppError> {
        let template = Templates::INDEX;

        let files = FileManager::list_files_with_paths("uploads")?;

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
    /// "/uploads/" is trimmed from the start of the file name, and then the file path is validated
    /// to assert that it meets all requirements, then the file name is joined with the uploads 
    /// directory and an assert is done to ensure the file is inside the directory, to protect against
    /// possible traversal attacks.  
    /// If the validation or canonicalization fails, an error is returned.
    pub(crate) fn view_file(filename: String) -> Result<Response, AppError> {
        let filename = filename
            .trim_start_matches('/')
            .trim_start_matches("uploads/");

        Self::validate_filename(filename)?;

        let base_path = Path::new("uploads");
        let requested_path = base_path.join(filename);
        
        // Get the absolute path, removing all traversals, this protects from traversal attacks
        match requested_path.canonicalize() {
            Ok(resolved_path) => {
                let canonicalized_base_path = base_path.canonicalize().map_err(|_| {
                    AppError::Unknown("Failed to canonicalize base path".to_string())
                })?;

                // Assert that the path is still within the uploads directory
                if resolved_path.starts_with(canonicalized_base_path) {
                    Ok(Response::builder()
                        .body(ResponseBody::File(
                            resolved_path.to_string_lossy().to_string(),
                        ))
                        .build())
                } else {
                    Err(AppError::NotPermitted(format!(
                        "Client attempted to access a path outside the uploads directory: {}",
                        resolved_path.display()
                    )))
                }
            }
            Err(_) => {
                Err(AppError::NotFound(format!(
                    "Client attempted to access a file that does not exist: {}",
                    requested_path.display()
                )))
            }
        }
    }

    /// Returns the view of the template to upload a new file
    pub(crate) fn get_file_upload_view() -> Result<Response, AppError> {
        Ok(Response::builder()
            .body(ResponseBody::Text(Templates::UPLOAD.to_string()))
            .build())
    }

    /// Uploads a file from a request
    ///
    /// Arguments:
    /// - **request_body**: The `RequestBody` to be used to get the file from
    ///
    /// The `RequestBody` must be of the `Multipart` variant or an error is returned.  
    /// The file path is then validated to assert that it meets all requirements.
    /// The file path is again resolved to assert that it is inside the 'uploads' directory.    
    /// If all conditions pass, the file gets saved and a response with an empty body gets returned.
    pub(crate) fn upload_file(request_body: RequestBody) -> Result<Response, AppError> {
        // Ensure that the `RequestBody` is a `Multipart` type, as that is the only supported type
        // for file uploads on this server
        let uploaded_file = match request_body {
            RequestBody::Multipart(uploaded_file) => uploaded_file,
            _ => {
                return Err(AppError::Invalid(format!(
                    "Request body is not multipart: {request_body}"
                )));
            }
        };
        
        Self::validate_filename(&uploaded_file.name)?;
        
        let path = Path::new("uploads").join(&uploaded_file.name);
        let resolved_path = Self::resolve_traversals(&path);
        if !resolved_path.starts_with("uploads/") {
            return Err(AppError::NotPermitted("Client attempted to access a path outside the uploads directory".to_string()));
        }

        FileManager::save_file("uploads", uploaded_file)?;

        Ok(Response::builder()
            .status(HttpStatus::SeeOther)
            .header(HttpHeader::LOCATION, "/")
            .body(ResponseBody::Empty)
            .build())
    }

    /// Validates a file name
    ///
    /// Arguments:
    /// - **path**: The file path to validate
    ///
    /// This method ensures a file path to be accessed meets all our requirements:
    /// 1. Its file name that is valid UTF-8
    /// 2. Its file name is not an empty string
    /// 3. The file to be accessed is of a supported extension
    ///
    /// These checks protect the server from a number of unpredictable behavior and vulnerabilities
    /// that could come from the client trying to access or upload a script or a file type the server
    /// can't serve
    fn validate_filename(path: &str) -> Result<(), AppError> {
        // A list of allowed extensions to limit the supported file types
        let allowed_extensions = ["txt", "png", "jpg", "pdf"];

        // Sanitize file name in case it contains unanticipated characters
        let sanitized_filename = Path::new(path)
            .file_name() // Extracts only the base file name, removing paths
            .and_then(|name| name.to_str()) // Convert to &str
            .ok_or(AppError::Invalid(format!(
                "Filename is not a valid UTF-8 file name: {path}",
            )))? // Fallback in case of invalid Unicode
            .to_string();

        if sanitized_filename.is_empty() {
            return Err(AppError::Invalid(format!(
                "Filename failed sanitization: {path}"
            )));
        }

        // Assert that file is an allowed type
        Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                if allowed_extensions.contains(&ext) {
                    Some(ext)
                } else {
                    None
                }
            })
            .ok_or(AppError::Invalid(format!(
                "Filename has an unsupported extension: {path}"
            )))?;

        Ok(())
    }

    /// Resolves the traversal of a file path
    /// 
    /// Arguments: 
    /// - **path**: The path to resolve
    /// 
    /// This resolves all traversals in a file path and returns the actual file path. This undoes
    /// all traversals in a path and can save from a possible traversal attack where the client
    /// uses traversals to access files outside the allowed uploads directory
    fn resolve_traversals(path: &PathBuf) -> PathBuf {
        let mut normalized = PathBuf::new();

        for component in Path::new(path).components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop(); // Remove the last component (if exists)
                }
                std::path::Component::CurDir => {
                    // Skip `.`, as it means "current directory"
                }
                _ => {
                    normalized.push(component);
                }
            }
        }

        normalized
    }
}

/// Abstracts routing from a request method and path to a handler
pub(crate) struct Router;

impl Router {
    /// Routes a request to its appropriate handler
    /// 
    /// Arguments:
    /// - **request**: A `Request` to route to a possible handler
    /// 
    /// The method and path are matched against, and if a supported handler exists, it is called and 
    /// the response is returned
    pub(crate) fn route_request(request: Request) -> Result<Response, AppError> {
        match (&request.method, request.path.as_str()) {
            (HttpMethod::Get, "/") => RequestHandler::list_files(),
            (HttpMethod::Get, file_path) if file_path.starts_with("/uploads") => {
                RequestHandler::view_file(file_path.to_string())
            }
            (HttpMethod::Get, "/upload") => RequestHandler::get_file_upload_view(),
            (HttpMethod::Post, "/upload") => RequestHandler::upload_file(request.body),
            _ => Ok(ErrorHandler::handle_invalid_page_request(
                request.method,
                request.path,
            )),
        }
    }
}

/// Handles all error cases
pub(crate) struct ErrorHandler;

impl ErrorHandler {
    /// Handles cases where the client requests for a page that does not exist.
    /// A 404 status code is returned, along with an HTML template for the error case.
    pub(crate) fn handle_invalid_page_request(http_method: HttpMethod, path: String) -> Response {
        warn!("Invalid page request: {} {}", http_method, path);
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::Text(Templates::PAGE_NOT_FOUND.to_string()))
            .build()
    }

    /// Handles cases where the client does not send a valid request body.
    /// A 400 status code is returned, along with an HTML template that shows the error.
    pub(crate) fn handle_bad_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::Text(Templates::BAD_REQUEST.to_string()))
            .build()
    }

    /// Handles cases where the client requests a file that is outside the designated uploads folder.
    /// A 403 status code is returned, along with an HTML template that says access denied.
    pub(crate) fn handle_access_denied() -> Response {
        Response::builder()
            .status(HttpStatus::Forbidden)
            .body(ResponseBody::Text(Templates::ACCESS_DENIED.to_string()))
            .build()
    }

    /// Handles cases where the client requests a file that doesn't exist in the uploads folder.
    /// A 404 status code is returned, along with an HTML template that explains this.
    pub(crate) fn handle_invalid_file_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::Text(Templates::FILE_NOT_FOUND.to_string()))
            .build()
    }

    /// Handles cases where an unknown or unrecoverable error occurs during the lifetime of the request.
    /// A 500 status code is returned, along with an appropriate HTML template.
    pub(crate) fn handle_server_error() -> Response {
        Response::builder()
            .status(HttpStatus::ServerError)
            .body(ResponseBody::Text(Templates::SERVER_ERROR.to_string()))
            .build()
    }

    /// Maps an `AppError` to a handler
    /// 
    /// Arguments:
    /// - **app_error**: The `AppError` to be to a handler
    /// 
    /// The given `AppError` is matched against, and routed to an appropriate error handler after the
    /// error is logged.  
    /// All errors propagate to this function and so it is the best and only place that logs errors.
    pub(crate) fn map_error_to_handler(app_error: AppError) -> Response {
        match app_error {
            AppError::Invalid(error) => {
                warn!("{}", error);
                Self::handle_bad_request()
            }
            AppError::NotFound(error) => {
                warn!("{}", error);
                Self::handle_invalid_file_request()
            }
            AppError::NotPermitted(error) => {
                warn!("{}", error);
                Self::handle_access_denied()
            }
            AppError::IO(error) => {
                log_error!("{}", error);
                Self::handle_server_error()
            }
            AppError::Unknown(error) => {
                log_error!("{}", error);
                Self::handle_server_error()
            }
        }
    }
}

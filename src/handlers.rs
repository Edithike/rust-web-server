use std::fs::File;
use std::io::Read;
use std::path::Path;
use crate::http::{HttpStatus, Response, ResponseBody};

mod helpers {
    use std::fs;

    pub(super) fn list_files_with_paths(dir: &str) -> std::io::Result<Vec<(String, String)>> {
        let mut files = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name().into_string().unwrap_or_default();
            let full_path = path.to_string_lossy().into_owned();
            files.push((file_name, full_path));
        }

        Ok(files)
    }
}

pub(crate) struct RequestHandler;

impl RequestHandler {
    pub(crate) fn list_files() -> Result<Response, Response> {
        let mut html_file = File::open("templates/index.html").map_err(|_| ErrorHandler::handle_server_error())?;
        let mut template = String::new();
        html_file
            .read_to_string(&mut template)
            .map_err(|_| ErrorHandler::handle_server_error())?;

        let files = helpers::list_files_with_paths("uploads").map_err(|_| ErrorHandler::handle_server_error())?;

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

    pub(crate) fn view_to_upload_files() -> Result<Response, Response> {
        Ok(Response::builder()
            .body(ResponseBody::File("templates/upload.html".to_string()))
            .build())
    }

}

pub(crate) struct ErrorHandler;

impl ErrorHandler {
    pub(crate) fn handle_invalid_page_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/page-not-found.html".to_string(),
            ))
            .build()
    }

    pub(crate) fn handle_invalid_file_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/file-not-found.html".to_string(),
            ))
            .build()
    }

    pub(crate) fn handle_access_denied() -> Response {
        Response::builder()
            .status(HttpStatus::Forbidden)
            .body(ResponseBody::File(
                "templates/access-denied.html".to_string(),
            ))
            .build()
    }

    pub(crate) fn handle_server_error() -> Response {
        Response::builder()
            .status(HttpStatus::ServerError)
            .body(ResponseBody::File(
                "templates/server-error.html".to_string(),
            ))
            .build()
    }
}
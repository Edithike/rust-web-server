use std::collections::HashMap;
use std::fmt::{Display, Formatter, format};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::{fmt, fs, thread};

/// A `Job` is a type alias for any function that runs once and implements `Send` and `static`
type Job = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;

/// A `Worker` is a type that handles a single thread and runs a job received
struct Worker {
    _id: usize,
    _thread: thread::JoinHandle<Arc<Mutex<mpsc::Receiver<Job>>>>,
}

impl Worker {
    /// Creates a new `Worker`
    ///
    /// Arguments:  
    /// - **id**: a usize to uniquely identify the worker  
    /// - **receiver**: a channel receiver wrapped in a Mutex wrapped in an Arc
    ///
    /// This method creates a new thread and passes a closure containing an infinite loop of waiting
    /// for the mutex to be free, acquiring the lock, getting the available job in the channel, freeing
    /// the lock and then executing the job.
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let job = receiver
                    .lock()
                    .expect(format!("Worker {id} unable to acquire mutex lock").as_str())
                    .recv()
                    .expect(format!("Worker {id} failed to receive job from channel").as_str());

                match job() {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
        });
        Worker {
            _id: id,
            _thread: thread,
        }
    }
}

/// A `ThreadPool` is a struct that handles multiple threads using workers, and communicates with
/// them by sending `Job`s through a channel, the first available worker picks up the job and executes it
struct ThreadPool {
    _workers: Vec<Worker>,
    sender: mpsc::Sender<Job>,
}

impl ThreadPool {
    /// Creates a new `ThreadPool`
    ///
    /// Arguments:
    /// - **size**: the number of workers in the `ThreadPool`
    ///
    /// This method creates a channel and holds onto the sender, passing the receiver to each new
    /// `Worker` created.
    /// An Arc<Mutex> is used so that the channel can be passed between threads and so that only
    /// one worker has access to the mutex of the receiver at a time
    fn new(size: usize) -> ThreadPool {
        assert!(size > 0);
        let mut workers = Vec::with_capacity(size);

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        for id in 0..size {
            let worker = Worker::new(id, Arc::clone(&receiver));
            workers.push(worker);
        }
        ThreadPool {
            _workers: workers,
            sender,
        }
    }

    /// Executes a job in a thread
    ///
    /// Arguments:
    /// - **f**: any object that implements `FnOnce()` + `Send` + `'static`
    ///
    /// This method creates a new job and sends it to a channel from the sender, to be consumed by
    /// the first available receiver, which will be a thread in one of the workers
    fn execute<F>(&self, f: F)
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        let job = Box::new(f);
        self.sender
            .send(job)
            .expect("Failed to send job to worker through channel");
    }
}

#[derive(PartialEq, Debug)]
enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Trace,
    Connect,
}

impl TryFrom<String> for Method {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let method = match value.as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            "TRACE" => Method::Trace,
            "CONNECT" => Method::Connect,
            _ => {
                return Err(format!("Unknown method: {}", value));
            }
        };
        Ok(method)
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Method::Get => write!(f, "GET"),
            Method::Post => write!(f, "POST"),
            Method::Put => write!(f, "PUT"),
            Method::Patch => write!(f, "PATCH"),
            Method::Delete => write!(f, "DELETE"),
            Method::Head => write!(f, "HEAD"),
            Method::Options => write!(f, "OPTIONS"),
            Method::Trace => write!(f, "TRACE"),
            Method::Connect => write!(f, "CONNECT"),
        }
    }
}

/// A Request is an abstraction of an HTTP Request and its contents
struct Request {
    path: String,
    method: Method,
    http_version: String,
    headers: HashMap<String, String>,
    body: RequestBody,
}

enum RequestBody {
    Multipart(UploadedFile),
    Empty,
}

impl Display for RequestBody {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RequestBody::Multipart(uploaded_file) => write!(f, "{}", uploaded_file),
            RequestBody::Empty => write!(f, "Empty"),
        }
    }
}

#[derive(Debug)]
enum ResponseBody {
    File(String),
    Text(String),
    Empty,
}

impl TryFrom<ResponseBody> for Option<UploadedFile> {
    type Error = String;

    fn try_from(value: ResponseBody) -> Result<Self, Self::Error> {
        match value {
            ResponseBody::File(filename) => {
                let path = Path::new(&filename);
                let uploaded_file = UploadedFile::try_from(path)?;
                Ok(Some(uploaded_file))
            }
            ResponseBody::Text(text) => {
                let uploaded_file = UploadedFile {
                    name: "response.html".to_string(),
                    content: text.as_bytes().to_vec(),
                };
                Ok(Some(uploaded_file))
            }
            ResponseBody::Empty => Ok(None),
        }
    }
}

impl TryFrom<&Path> for UploadedFile {
    type Error = String;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if !path.exists() {
            return Err(format!("File does not exist: {}", path.display()));
        }
        if path.is_dir() {
            return Err(format!("Path is a directory: {}", path.display()));
        }
        let file_name = path
            .file_name()
            .ok_or("Error reading file name")?
            .to_str()
            .ok_or("Error reading file name")?;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                return Err("File failed to open".to_string());
            }
        };

        let mut file_buffer = Vec::new();
        file.read_to_end(&mut file_buffer).unwrap();

        Ok(UploadedFile {
            name: file_name.to_string(),
            content: file_buffer,
        })
    }
}

struct UploadedFile {
    name: String,
    content: Vec<u8>,
}

impl Display for UploadedFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let content = String::from_utf8(self.content.clone())
            .unwrap_or_else(|_| "File is not text based".to_string());
        write!(f, "Filename: {}\nFile content: {}", self.name, content)
    }
}

impl Request {
    /// Tries to create a new HTTP request
    ///
    /// Arguments:
    /// - **buf_reader**: a `BufReader` of a `TcpStream`
    ///
    /// The `BufReader` is iterated into lines, the first line being the request line, the next couple
    /// being the headers, and then a possible body. If any of the extractions of the lines fail, an
    /// error is returned.
    fn try_new(mut buf_reader: BufReader<&mut TcpStream>) -> Result<Request, String> {
        let mut line = String::new();

        buf_reader
            .read_line(&mut line)
            .map_err(|_| "Error reading request".to_string())?;
        let (method, path, http_version) = Self::extract_request_line(line)?;
        let headers = Self::extract_headers(&mut buf_reader)?;
        let body = Self::extract_body(&mut buf_reader, &headers)?;

        Ok(Request {
            path,
            method,
            http_version,
            headers,
            body,
        })
    }

    /// Extracts the method path and HTTP version from the request line.
    ///
    /// Arguments:
    /// - **request_line**: a `String` which is typically the first line of an HTTP request
    ///
    /// The `request_line` is split into at most three parts based on whitespace, and each part is
    /// parsed and extracted, returning a tuple of three. If any of the parsings fail, an error is
    /// returned.
    fn extract_request_line(request_line: String) -> Result<(Method, String, String), String> {
        let mut parts = request_line.splitn(3, " ");

        let method: Method = parts
            .next()
            .ok_or("Could not find method")?
            .to_string()
            .try_into()?;
        let path = parts.next().ok_or("Could not find path")?.to_string();
        let http_version = parts
            .next()
            .ok_or("Could not find http_version")?
            .to_string();

        Ok((method, path, http_version))
    }

    // TODO: Update this
    /// Extracts headers from HTTP request lines.
    ///
    /// Arguments:
    /// - **lines**: a bunch of `Lines` from a `BufReader` of a `TcpStream`, typically all lines in a
    /// HTTP request besides the first.
    ///
    /// HTTP request lines are looped through, and each line is split on the first colon from the left,
    /// for example "Host: localhost" would be split into ["Host", " localhost"], the first is the
    /// key and the second is the value of the header, stored in a `HashMap`.  
    /// If the current line is an empty line, that signifies that there are no more headers, and the
    /// loop is broken.  
    /// An error is returned if reading or splitting the current line fails.
    fn extract_headers(
        buf_reader: &mut BufReader<&mut TcpStream>,
    ) -> Result<HashMap<String, String>, String> {
        let mut headers = HashMap::new();

        loop {
            let mut line = String::new();
            buf_reader
                .read_line(&mut line)
                .map_err(|e| format!("Error reading headers: {}", e))?;

            if line == "\r\n" {
                // End of headers
                break;
            }

            let Some((key, value)) = line.split_once(":") else {
                let error_msg = "Error parsing headers".to_string();

                eprintln!("{}", error_msg);
                return Err(error_msg);
            };
            headers.insert(key.trim().to_string(), value.trim().to_string());
        }
        Ok(headers)
    }

    fn extract_body(
        buf_reader: &mut BufReader<&mut TcpStream>,
        headers: &HashMap<String, String>,
    ) -> Result<RequestBody, String> {
        // TODO: make headers work with any case
        let content_length = headers
            .get(HttpHeader::CONTENT_LENGTH)
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| "Content-Length was not a number".to_string())?;
        let content_type_header = headers
            .get(HttpHeader::CONTENT_TYPE)
            .and_then(|content_type| Some(content_type.to_string()));

        if content_length.is_none()
            || content_length.is_some_and(|len| len == 0)
            || content_type_header.is_none()
        {
            return Ok(RequestBody::Empty);
        }

        let content_length_header = content_length.unwrap();
        let content_type_header = content_type_header.unwrap();

        match content_type_header {
            content_type if content_type.starts_with("multipart/form-data") => {
                MultiPartForm::extract(buf_reader, content_type, content_length_header)
                    .map(|uploaded_file| RequestBody::Multipart(uploaded_file))
            }
            _ => Err("Unsupported content type".to_string()),
        }
    }
}

trait BodyExtractor {
    type Body;
    fn extract(
        buf_reader: &mut BufReader<&mut TcpStream>,
        content_type: String,
        content_length: usize,
    ) -> Result<Self::Body, String>;
}

struct MultiPartForm;

impl BodyExtractor for MultiPartForm {
    type Body = UploadedFile;

    fn extract(
        buf_reader: &mut BufReader<&mut TcpStream>,
        content_type: String,
        _: usize,
    ) -> Result<UploadedFile, String> {
        let (_, boundary) = content_type
            .split_once("boundary=")
            .ok_or("boundary missing in Content-Type header".to_string())?;
        let boundary = boundary.trim();

        let mut form_body = String::new();
        buf_reader
            .read_to_string(&mut form_body)
            .map_err(|_| "Failed to read form body")?;

        let form_body = form_body
            .trim()
            .strip_prefix(format!("--{boundary}").as_str())
            .and_then(|body| body.strip_suffix(format!("--{boundary}--").as_str()))
            .ok_or("Form body not surrounded with boundary".to_string())?
            .trim()
            .to_string();

        let mut parts = form_body.splitn(3, "\n");
        let filename = parts
            .next()
            .and_then(|content_disposition| {
                let (_, filename_part) = content_disposition.rsplit_once(';')?;
                let (_, filename) = filename_part.split_once("=")?;
                let filename = filename.trim().trim_matches('"').to_string();

                Some(filename)
            })
            .ok_or("Invalid content disposition".to_string())?
            .to_string();
        parts
            .next()
            .ok_or("Content type missing from form body".to_string())?;
        let data = parts
            .next()
            .ok_or("file data missing from form body".to_string())?
            .to_string()
            .trim()
            .as_bytes()
            .to_vec();

        Ok(UploadedFile {
            name: filename,
            content: data,
        })
    }
}

impl Display for Request {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut headers_string = String::new();
        for (key, value) in &self.headers {
            headers_string.push_str(&format!("{}: {}\r\n", key, value));
        }

        write!(
            f,
            "{} {} {}\r\n{}",
            self.method, self.path, self.http_version, headers_string
        )
    }
}

#[derive(Debug)]
enum HttpStatus {
    Ok,
    Forbidden,
    NotFound,
    ServerError,
}

impl HttpStatus {
    fn get_status_code(&self) -> u16 {
        match self {
            HttpStatus::Ok => 200,
            HttpStatus::Forbidden => 403,
            HttpStatus::NotFound => 404,
            HttpStatus::ServerError => 500,
        }
    }

    fn get_reason_phrase(&self) -> String {
        match self {
            HttpStatus::Ok => "OK".to_string(),
            HttpStatus::Forbidden => "FORBIDDEN".to_string(),
            HttpStatus::NotFound => "NOT FOUND".to_string(),
            HttpStatus::ServerError => "SERVER ERROR".to_string(),
        }
    }
}

struct HttpHeader;

impl HttpHeader {
    const CONTENT_LENGTH: &'static str = "Content-Length";
    const CONTENT_TYPE: &'static str = "Content-Type";
    const CONTENT_DISPOSITION: &'static str = "Content-Disposition";
}

#[derive(Default)]
struct ResponseBuilder {
    status: Option<HttpStatus>,
    headers: HashMap<String, String>,
    body: Option<ResponseBody>,
}

impl ResponseBuilder {
    fn new() -> Self {
        ResponseBuilder::default()
    }

    fn status(mut self, status: HttpStatus) -> Self {
        self.status = Some(status);
        self
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }
    fn body(mut self, body: ResponseBody) -> Self {
        self.body = Some(body);
        self
    }

    fn build(self) -> Response {
        let status = self.status.unwrap_or(HttpStatus::Ok);
        let body = self.body.unwrap_or(ResponseBody::Empty);

        Response::new(status, self.headers, body)
    }
}

#[derive(Debug)]
struct Response {
    http_version: String,
    status: HttpStatus,
    headers: HashMap<String, String>,
    body: ResponseBody,
}

impl Response {
    fn builder() -> ResponseBuilder {
        ResponseBuilder::new()
    }

    fn new(status: HttpStatus, headers: HashMap<String, String>, body: ResponseBody) -> Self {
        Response {
            http_version: "HTTP/1.1".to_string(),
            status,
            headers,
            body,
        }
    }

    fn to_http_response(mut self) -> Result<(Vec<u8>, Option<Vec<u8>>), Response> {
        //TODO: remove all unwraps
        let mut headers_buffer = Vec::new();

        let status_code = self.status.get_status_code();
        let reason_phrase = self.status.get_reason_phrase();
        write!(
            headers_buffer,
            "{} {} {}\r\n",
            self.http_version, status_code, reason_phrase
        )
        .unwrap();

        let file: Option<UploadedFile> = self.body.try_into().map_err(|e| {
            println!("Failed to convert body to uploaded file: {:?}", e);
            ErrorHandler::handle_invalid_file_request()
        })?;

        let body_buffer = match file {
            Some(file) => {
                let content_type = get_content_type(&file.name);

                self.headers.insert(
                    HttpHeader::CONTENT_LENGTH.to_string(),
                    file.content.len().to_string(),
                );
                self.headers.insert(
                    HttpHeader::CONTENT_TYPE.to_string(),
                    content_type.to_string(),
                );
                if !content_type.starts_with("text/html") {
                    let content_disposition = format!(r#"inline; filename="{}""#, file.name);
                    self.headers.insert(
                        HttpHeader::CONTENT_DISPOSITION.to_string(),
                        content_disposition,
                    );
                }

                Some(file.content)
            }
            None => {
                self.headers
                    .insert(HttpHeader::CONTENT_LENGTH.to_string(), "0".to_string());
                None
            }
        };

        for (key, value) in &self.headers {
            write!(headers_buffer, "{}: {}\r\n", key, value).unwrap();
        }
        write!(headers_buffer, "\r\n").unwrap();

        Ok((headers_buffer, body_buffer))
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Start with the status line
        let status_code = self.status.get_status_code();
        let reason_phrase = self.status.get_reason_phrase();
        write!(
            f,
            "{} {} {}\r\n",
            self.http_version, status_code, reason_phrase
        )?;

        // Add headers
        for (key, value) in &self.headers {
            write!(f, "{}: {}\r\n", key, value)?;
        }

        // Add a blank line to separate headers from body
        write!(f, "\r\n")
    }
}

fn list_files_with_paths(dir: &str) -> std::io::Result<Vec<(String, String)>> {
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

struct RequestHandler;

impl RequestHandler {
    fn list_files() -> Result<Response, Response> {
        let mut html_file = File::open("templates/index.html").map_err(|_| ErrorHandler::handle_server_error())?;
        let mut template = String::new();
        html_file
            .read_to_string(&mut template)
            .map_err(|_| ErrorHandler::handle_server_error())?;

        let files = list_files_with_paths("uploads").map_err(|_| ErrorHandler::handle_server_error())?;

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

    fn view_file(filename: String) -> Result<Response, Response> {
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

    fn view_to_upload_files() -> Result<Response, Response> {
        Ok(Response::builder()
            .body(ResponseBody::File("templates/upload.html".to_string()))
            .build())
    }

}

/// Handles an HTTP connection
///
/// Arguments:
/// - **stream**: a mutable TcpStream that represents a single TCP connection or HTTP request
///
/// This method reads the stream using a BufReader and uses the request line to identify what path
/// was called and how to handle each one.
fn handle_connection(mut stream: TcpStream) -> Result<(), String> {
    let buf_reader = BufReader::new(&mut stream);
    let request = Request::try_new(buf_reader)?;

    let response: Result<Response, Response> = match (request.method, request.path.as_str()) {
        (Method::Get, "/") => RequestHandler::list_files(),
        (Method::Get, file_path) if file_path.starts_with("/uploads") => {
            RequestHandler::view_file(file_path.to_string())
        }
        (Method::Get, "/upload") => RequestHandler::view_to_upload_files(),
        _ => Err(ErrorHandler::handle_invalid_page_request()),
    };

    let response = response.unwrap_or_else(|response| response);

    let (response_headers, response_body) = match response.to_http_response() {
        Ok((response_headers, response_body)) => (response_headers, response_body),
        Err(error) => error
            .to_http_response()
            .expect("Failed to convert response to http headers"),
    };

    stream
        .write_all(&response_headers)
        .map_err(|e| format!("Error writing response to stream: {}", e))?;
    if let Some(body) = response_body {
        stream
            .write_all(&body)
            .map_err(|e| format!("Error writing file to stream: {}", e))?;
    }

    stream
        .flush()
        .map_err(|e| format!("Error flushing stream: {}", e))?;
    Ok(())
}

fn get_content_type(file_path: &str) -> &str {
    match Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
    {
        Some("html") => "text/html; charset=UTF-8", // ✅ Ensure HTML is rendered
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

struct ErrorHandler;

impl ErrorHandler {
    fn handle_invalid_page_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/page-not-found.html".to_string(),
            ))
            .build()
    }

    fn handle_invalid_file_request() -> Response {
        Response::builder()
            .status(HttpStatus::NotFound)
            .body(ResponseBody::File(
                "templates/file-not-found.html".to_string(),
            ))
            .build()
    }

    fn handle_access_denied() -> Response {
        Response::builder()
            .status(HttpStatus::Forbidden)
            .body(ResponseBody::File(
                "templates/access-denied.html".to_string(),
            ))
            .build()
    }

    fn handle_server_error() -> Response {
        Response::builder()
            .status(HttpStatus::ServerError)
            .body(ResponseBody::File(
                "templates/server-error.html".to_string(),
            ))
            .build()
    }
}

fn main() {
    let listener = TcpListener::bind("localhost:7878").expect("Could not bind to localhost:7878");

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("Encountered error getting stream {}", e);
                continue;
            }
        };

        pool.execute(move || handle_connection(stream));
    }
}

#[cfg(test)]
mod tests {
    use crate::{Method, Request};
    use std::io::{BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn try_new_request() {
        let listener = TcpListener::bind("localhost:7878").expect("Could not bind localhost:7878");
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();

                let buf_reader = BufReader::new(&mut stream);
                let request = Request::try_new(buf_reader).expect("Could not parse request");

                assert_eq!(request.method, Method::Get);
                assert_eq!(request.path, String::from("/home"));
                assert_eq!(request.http_version, "HTTP/1.1");
                assert_eq!(request.headers.len(), 3);
                assert_eq!(request.headers.get("Host").unwrap(), "localhost");
                assert_eq!(request.headers.get("Accept").unwrap(), "text/html");
                break;
            }
        });

        // Wait for server to start
        thread::sleep(std::time::Duration::from_millis(100));

        // Create a mock HTTP client request
        let mut stream = TcpStream::connect("localhost:7878").expect("Failed to connect");

        let request = "GET /home HTTP/1.1\r\n\
               Host: localhost\r\n\
               User-Agent: MyTestClient/1.0\r\n\
               Accept: text/html\r\n\
               \r\n";
        stream
            .write_all(request.as_bytes())
            .expect("Failed to send request");

        handle.join().expect("Failed to join thread");
    }
}

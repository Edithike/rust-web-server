use crate::common::{AppError, BufferedFile};
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// Represents all HTTP methods
#[derive(PartialEq, Debug)]
pub(crate) enum HttpMethod {
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

impl TryFrom<String> for HttpMethod {
    type Error = AppError;

    /// Tries to get an HTTP method from a string
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let method = match value.to_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            "HEAD" => HttpMethod::Head,
            "OPTIONS" => HttpMethod::Options,
            "TRACE" => HttpMethod::Trace,
            "CONNECT" => HttpMethod::Connect,
            _ => {
                return Err(AppError::Invalid(format!("Unknown method: {}", value)));
            }
        };
        Ok(method)
    }
}

impl Display for HttpMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
            HttpMethod::Trace => write!(f, "TRACE"),
            HttpMethod::Connect => write!(f, "CONNECT"),
        }
    }
}

/// A `Request` is an abstraction of an HTTP Request and its contents
pub(crate) struct Request {
    pub(crate) path: String,
    pub(crate) method: HttpMethod,
    http_version: String,
    headers: HashMap<String, String>,
    pub(crate) body: RequestBody,
}

/// A `RequestBody` is an abstraction of an HTTP request body
pub(crate) enum RequestBody {
    Multipart(BufferedFile),
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

/// A `ResponseBody` is an abstraction of an HTTP response body
#[derive(Debug)]
pub(crate) enum ResponseBody {
    File(String),
    Text(String),
    Empty,
}

impl Request {
    /// Tries to create a new HTTP request
    ///
    /// Arguments:
    /// - **buf_reader**: a `BufReader` of a `TcpStream` containing the current HTTP request
    ///
    /// The `BufReader`'s first line is read into a string, and the request line is extracted from that.
    /// It is then used to extract the headers from the next couple of lines.
    /// And finally, used to extract the request body.
    pub(crate) fn try_new(mut buf_reader: BufReader<&mut TcpStream>) -> Result<Request, AppError> {
        let mut line = String::new();

        buf_reader
            .read_line(&mut line)
            .map_err(|_| AppError::IO("Error reading request".to_string()))?;
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
    /// The `request_line` is split on whitespace, the first three parts parsed accordingly and
    /// returned as a tuple of 3 if all parsings succeed. Otherwise, an error is returned
    fn extract_request_line(
        request_line: String,
    ) -> Result<(HttpMethod, String, String), AppError> {
        let mut parts = request_line.split_whitespace();

        let method: HttpMethod = parts
            .next()
            .ok_or(AppError::Invalid("Could not find method".to_string()))?
            .to_string()
            .try_into()?;
        let path = parts
            .next()
            .ok_or(AppError::Invalid("Could not find path".to_string()))?
            .to_string();
        let http_version = parts
            .next()
            .ok_or(AppError::Invalid("Could not find http_version".to_string()))?
            .to_string();

        Ok((method, path, http_version))
    }

    /// Extracts headers from a `TcpStream`
    ///
    /// Arguments:
    /// - **buf_reader**: A mutable reference to a `BufReader` of a mutable reference to a `TcpStream`
    ///
    /// Starts a loop of reading a line from the `BufReader` to a string, and then splitting the string
    /// on a colon to get the key and value of each header. The loop breaks when we reach an empty line,
    /// marking the end of the headers in the HTTP request.
    fn extract_headers(
        buf_reader: &mut BufReader<&mut TcpStream>,
    ) -> Result<HashMap<String, String>, AppError> {
        let mut headers = HashMap::new();

        loop {
            let mut line = String::new();
            buf_reader
                .read_line(&mut line)
                .map_err(|e| AppError::Invalid(format!("Error reading headers: {}", e)))?;

            if line == "\r\n" {
                // End of headers
                break;
            }

            let Some((key, value)) = line.split_once(":") else {
                return Err(AppError::Invalid("Error parsing headers".to_string()));
            };
            // Transform the case of the header to ensure we always store them in header case
            let key = Self::to_header_case(key.trim());
            headers.insert(key, value.trim().to_string());
        }
        Ok(headers)
    }

    /// Extracts a body from a `TcpStream`
    ///
    /// Arguments:
    /// - **buf_reader**: A mutable reference to a `BufReader` of a mutable reference to a `TcpStream`
    /// - **headers**: A reference to a `HashMap` containing HTTP request headers.
    ///
    /// Gets the content length and content type headers to know how to read the body.
    /// If the content length is 0 or either is not set, the request has no body.
    /// Otherwise, the content type is matched against and determines the extractor to call.
    fn extract_body(
        buf_reader: &mut BufReader<&mut TcpStream>,
        headers: &HashMap<String, String>,
    ) -> Result<RequestBody, AppError> {
        let content_length = headers
            .get(HttpHeader::CONTENT_LENGTH)
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| {
                AppError::Invalid(format!(
                    "{} request header is not a number",
                    HttpHeader::CONTENT_LENGTH
                ))
            })?;
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
                MultiPartFormExtractor::extract(buf_reader, content_type, content_length_header)
                    .map(|uploaded_file| RequestBody::Multipart(uploaded_file))
            }
            _ => Err(AppError::Invalid(format!(
                "Unsupported content type: {content_type_header}"
            ))),
        }
    }

    fn to_header_case(s: &str) -> String {
        s.split('-')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("-")
    }
}

/// This represents a contract that all body extractors should fulfill
trait BodyExtractor {
    type Body;

    /// Extracts a body from a TCP stream
    ///
    /// Arguments:
    /// - **buf_reader**: A mutable reference to a `BufReader` of a mutable reference to a `TcpStream`
    /// - **content_type**: The content type of the body to determine how to read it
    /// - **content_length**: The length of the body, to determine how much to read from the stream
    ///
    /// This method must be implemented by any struct that implements this trait
    fn extract(
        buf_reader: &mut BufReader<&mut TcpStream>,
        content_type: String,
        content_length: usize,
    ) -> Result<Self::Body, AppError>;
}

/// A type that helps extract a body from a multipart/form
struct MultiPartFormExtractor;

impl MultiPartFormExtractor {
    /// Limits the file size possible to upload to 50MB, to avoid very large files
    const MAX_FILE_SIZE: usize = 50 * 1024 * 1024;
}

impl BodyExtractor for MultiPartFormExtractor {
    type Body = BufferedFile;

    /// Extracts a body from a multipart form request
    ///
    /// Arguments:
    /// - **buf_reader**: A mutable reference to a `BufReader` of a mutable reference to a `TcpStream`
    /// - **content_type**: *Content-Type* header value
    /// - **content_length**: *Content-Length* header value
    ///
    /// The *Content-Type* header is checked to determine if the file is larger the allowed size, if
    /// so, an error is returned.  
    /// The boundary is gotten from the *Content-Type* header value, and then the `TcpStream` is read
    /// into a byte buffer of the size determined by the *Content-Length* header, which is the exact
    /// size of the body.  
    /// The body is then stripped of the boundaries and split on newlines, extracting the first 3
    /// parts; the content disposition which contains the file name, the content type and the file.  
    /// The file name and file data are parsed and used to construct a `BufferedFile` which gets returned.
    fn extract(
        buf_reader: &mut BufReader<&mut TcpStream>,
        content_type: String,
        content_length: usize,
    ) -> Result<Self::Body, AppError> {
        if content_length > Self::MAX_FILE_SIZE {
            return Err(AppError::Invalid(
                "File size exceeds 50MB limit".to_string(),
            ));
        }

        let (_, boundary) = content_type
            .split_once("boundary=")
            .ok_or(AppError::Invalid(
                "Boundary missing in Content-Type header".to_string(),
            ))?;
        let boundary = boundary.trim();

        let mut form_body_buffer = vec![0; content_length];
        buf_reader
            .read_exact(&mut form_body_buffer)
            .map_err(|_| AppError::Invalid("Failed to read form data".to_string()))?;
        let form_body = String::from_utf8(form_body_buffer)
            .map_err(|_| AppError::Invalid("Failed to parse form data".to_string()))?;

        let form_body = form_body
            .trim()
            .strip_prefix(format!("--{boundary}").as_str())
            .and_then(|body| body.strip_suffix(format!("--{boundary}--").as_str()))
            .ok_or(AppError::Invalid(
                "Form body not surrounded with boundary".to_string(),
            ))?
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
            .ok_or(AppError::Invalid("Invalid content disposition".to_string()))?
            .to_string();
        parts.next().ok_or(AppError::Invalid(
            "Content type missing from form body".to_string(),
        ))?;
        let data = parts
            .next()
            .ok_or(AppError::Invalid(
                "file data missing from form body".to_string(),
            ))?
            .to_string()
            .trim()
            .as_bytes()
            .to_vec();

        Ok(BufferedFile {
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

/// A `HttpStatus` is an abstraction of an HTTP status code and a reason phrase
#[derive(Debug)]
pub(crate) enum HttpStatus {
    Ok,
    SeeOther,
    Forbidden,
    NotFound,
    ServerError,
}

impl HttpStatus {
    /// Gets the status code used in an HTTP response from a `HttpStatus`
    fn get_status_code(&self) -> u16 {
        match self {
            HttpStatus::Ok => 200,
            HttpStatus::SeeOther => 303,
            HttpStatus::Forbidden => 403,
            HttpStatus::NotFound => 404,
            HttpStatus::ServerError => 500,
        }
    }

    /// Gets the reason phrase used in an HTTP response from a `HttpStatus`
    fn get_reason_phrase(&self) -> String {
        match self {
            HttpStatus::Ok => "OK".to_string(),
            HttpStatus::SeeOther => "SEE OTHER".to_string(),
            HttpStatus::Forbidden => "FORBIDDEN".to_string(),
            HttpStatus::NotFound => "NOT FOUND".to_string(),
            HttpStatus::ServerError => "SERVER ERROR".to_string(),
        }
    }
}

/// Contains constants for HTTP headers
pub(crate) struct HttpHeader;

impl HttpHeader {
    pub(crate) const CONTENT_LENGTH: &'static str = "Content-Length";
    pub(crate) const CONTENT_TYPE: &'static str = "Content-Type";
    pub(crate) const CONTENT_DISPOSITION: &'static str = "Content-Disposition";
    pub(crate) const LOCATION: &'static str = "Location";
}

/// Holds data to create a `Response` using the builder pattern
#[derive(Default)]
pub(crate) struct ResponseBuilder {
    status: Option<HttpStatus>,
    headers: HashMap<String, String>,
    body: Option<ResponseBody>,
}

impl ResponseBuilder {
    /// Creates a new `ResponseBuilder` using the default values of each field
    fn new() -> Self {
        ResponseBuilder::default()
    }

    /// Updates the status of the `ResponseBuilder`
    ///
    /// Arguments:
    /// - **mut self**: A mutable capture of self
    /// - **status**: A `HttpStatus`
    pub(crate) fn status(mut self, status: HttpStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Updates the headers of the `ResponseBuilder`
    ///
    /// Arguments:
    /// - **mut self**: A mutable capture of self
    /// - **name**: The name of a single header to update
    /// - **value**: The value of the header being added
    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    /// Updates the body of the `ResponseBuilder`
    ///
    /// Arguments:
    /// - **mut self**: A mutable capture of self
    /// - **body**: The `ResponseBody` to update the `ResponseBuilder` with
    pub(crate) fn body(mut self, body: ResponseBody) -> Self {
        self.body = Some(body);
        self
    }

    /// Builds a `Response` from a `ResponseBuilder`
    /// Sets useful defaults for status and body if not present, and passes the headers alongside
    pub(crate) fn build(self) -> Response {
        let status = self.status.unwrap_or(HttpStatus::Ok);
        let body = self.body.unwrap_or(ResponseBody::Empty);

        Response::new(status, self.headers, body)
    }
}

/// A `Response` is an abstraction of an HTTP response and its contents
#[derive(Debug)]
pub(crate) struct Response {
    http_version: String,
    status: HttpStatus,
    headers: HashMap<String, String>,
    body: ResponseBody,
}

impl Response {
    /// Returns a `ResponseBuilder` to build a `Response` easily
    pub(crate) fn builder() -> ResponseBuilder {
        ResponseBuilder::new()
    }

    /// Creates a new `Response`
    ///
    /// Arguments:
    /// - **status**: The `HttpStatus` of the `Response`
    /// - **headers**: A `HashMap` of all HTTP headers of the `Response`
    /// - **body**: the body of the `Response`
    fn new(status: HttpStatus, headers: HashMap<String, String>, body: ResponseBody) -> Self {
        Response {
            http_version: "HTTP/1.1".to_string(),
            status,
            headers,
            body,
        }
    }

    /// Tries to cast a `Response` into bytes that get written to the TCP stream as a response
    ///
    /// Writes each field of the `Response` to a byte buffer.  
    /// First the status line is written, then an attempt is made to get an `Option<BufferedFile>` for
    /// cases where the response has or doesn't have a body.  
    /// Content-Type and Content-Length headers are overridden, depending on whether there is
    /// a body and how long it is.  
    /// The headers are written next, then an empty line, then the response body, if any.
    pub(crate) fn to_bytes(mut self) -> Result<Vec<u8>, AppError> {
        let mut buffer = Vec::new();

        let status_code = self.status.get_status_code();
        let reason_phrase = self.status.get_reason_phrase();
        write!(
            buffer,
            "{} {} {}\r\n",
            self.http_version, status_code, reason_phrase
        )
        .map_err(|_| AppError::IO("Error writing HTTP response to buffer".to_string()))?;

        let file: Option<BufferedFile> = self.body.try_into()?;

        let body_buffer = match file {
            Some(file) => {
                let content_type = Self::get_content_type(&file.name);

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
            write!(buffer, "{}: {}\r\n", key, value)
                .map_err(|_| AppError::IO("Error writing HTTP response to buffer".to_string()))?;
        }
        write!(buffer, "\r\n")
            .map_err(|_| AppError::IO("Error writing HTTP response to buffer".to_string()))?;

        if let Some(mut body) = body_buffer {
            buffer.append(&mut body)
        }

        Ok(buffer)
    }

    /// Gets the HTTP content type based on the extension of a file
    pub(crate) fn get_content_type(file_path: &str) -> &str {
        match Path::new(file_path)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some("html") => "text/html; charset=UTF-8",
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

#[cfg(test)]
mod tests {
    use crate::Request;
    use crate::http::HttpMethod;
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

                assert_eq!(request.method, HttpMethod::Get);
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

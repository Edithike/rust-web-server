use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader, Lines, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::{fs, thread};

/// A `Job` is a type alias for any function that runs once and implements `Send` and `static`
type Job = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;

/// A `Worker` is a type that handles a single thread and runs a job received
struct Worker {
    id: usize,
    thread: thread::JoinHandle<Arc<Mutex<mpsc::Receiver<Job>>>>,
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

                println!("Worker {} got a job; executing.", id);

                match job() {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
        });
        Worker { id, thread }
    }
}

/// A `ThreadPool` is a struct that handles multiple threads using workers, and communicates with
/// them by sending `Job`s through a channel, the first available worker picks up the job and executes it
struct ThreadPool {
    workers: Vec<Worker>,
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
        ThreadPool { workers, sender }
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
    fn try_new(buf_reader: BufReader<&mut TcpStream>) -> Result<Request, String> {
        let mut lines = buf_reader.lines();

        if let Some(Ok(first_line)) = lines.next() {
            let (method, path, http_version) = Self::extract_request_line(first_line)?;
            let headers = Self::extract_headers(lines)?;
            
            Ok(Request {
                path,
                method,
                http_version,
                headers,
            })
        } else {
            Err("No request line found".to_string())
        }
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
    fn extract_headers(lines: Lines<BufReader<&mut TcpStream>>) -> Result<HashMap<String, String>, String> {
        let mut headers = HashMap::new();
        
        for line in lines {
            match line {
                Ok(line) => {
                    if line.is_empty() {
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
                Err(e) => {
                    let error_msg = "Error reading request line".to_string();

                    eprintln!("{}: {}", error_msg, e);
                    return Err(error_msg);
                }
            }
        }
        Ok(headers)
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
    
    let (status_line, filename) = match (request.method, request.path.as_str()) {
        (Method::Get, "/") => ("HTTP/1.1 200 OK", "home.html"),
        (Method::Get, "/sleep") => {
            thread::sleep(std::time::Duration::from_secs(10));
            ("HTTP/1.1 200 OK", "home.html")
        },
        _ => ("HTTP/1.1 404 NOT FOUND", "404.html"),
    };

    let contents =
        fs::read_to_string(filename).map_err(|e| format!("Failed to read file {filename}: {e}"))?;
    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("Error writing response to stream: {}", e))?;
    Ok(())
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

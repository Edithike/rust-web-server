mod common;
mod handlers;
mod http;

use crate::handlers::{ErrorHandler, RequestHandler};
use crate::http::{HttpMethod, Request, Response};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

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

/// A `Server` is an abstraction of some of the logic that runs a web server and handles each TCP stream
/// It holds the listener that listens for each HTTP request and the thread pool that assigns each
/// request to an available thread.
struct Server {
    listener: TcpListener,
    thread_pool: ThreadPool,
}

impl Server {
    /// Creates a new `Server`
    ///
    /// Arguments:
    /// - **server_address**: The host and port the server will run on
    /// - **number_of_workers**: The number of threads that the server will have
    fn new(server_address: &str, number_of_workers: usize) -> Server {
        let listener = TcpListener::bind(&server_address).expect("Could not bind to address");
        let thread_pool = ThreadPool::new(number_of_workers);

        Server {
            listener,
            thread_pool,
        }
    }

    /// Handles an HTTP connection
    ///
    /// Arguments:
    /// - **stream**: a mutable TcpStream that represents a single HTTP request
    ///
    /// This method reads the stream using a BufReader and uses that to construct a new `Request`, the
    /// `Request`'s `method` and `path` determine what handler gets invoked. The handler returns a `Response`,
    /// which is then cast into a byte buffer that gets written to the stream, ending the HTTP request.
    fn handle_connection(mut stream: TcpStream) -> Result<(), String> {
        let buf_reader = BufReader::new(&mut stream);
        let request = Request::try_new(buf_reader)?;

        let response: Result<Response, Response> = match (request.method, request.path.as_str()) {
            (HttpMethod::Get, "/") => RequestHandler::list_files(),
            (HttpMethod::Get, file_path) if file_path.starts_with("/uploads") => {
                RequestHandler::view_file(file_path.to_string())
            }
            (HttpMethod::Get, "/upload") => RequestHandler::view_to_upload_files(),
            (HttpMethod::Post, "/upload") => RequestHandler::upload_file(request.body),
            _ => Err(ErrorHandler::handle_invalid_page_request()),
        };

        let response = response.unwrap_or_else(|response| response);

        let response_bytes = response.to_bytes().unwrap_or_else(|error| error
            .to_bytes()
            .expect("Failed to convert response to http headers"));

        stream
            .write_all(&response_bytes)
            .map_err(|e| format!("Error writing response to stream: {}", e))?;

        stream
            .flush()
            .map_err(|e| format!("Error flushing stream: {}", e))?;
        Ok(())
    }
}

fn main() {
    let server = Server::new("localhost:7878", 4);

    for stream in server.listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("Encountered error getting stream {}", e);
                continue;
            }
        };

        server.thread_pool.execute(move || Server::handle_connection(stream));
    }
}
mod common;
mod handlers;
mod http;

use crate::common::{AppError, Time};
use crate::handlers::{ErrorHandler, Router};
use crate::http::{Request, Response};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc, LazyLock};
use std::{fs, thread};

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
                        warn!("{}", e);
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
    /// An `Arc<Mutex>` is used so that the channel can be passed between threads and so that only
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
    /// `Request`'s gets passed to `Router` which handles routing and returns a `Response`.  
    /// The `Response` is converted to bytes and any errors are passed to the `ErrorHandler` which
    /// handles them and produces an appropriate response and logs errors.  
    /// The response bytes are then written to the `TcpStream`, ending the request. The `TcpStream`
    /// is then flushed, to ensure the connection is closed, in the case of unexpected behavior.
    fn handle_connection(mut stream: TcpStream, state: Arc<AppState>) -> Result<(), String> {
        let buf_reader = BufReader::new(&mut stream);

        let map_error_to_response_bytes = |error| {
            let error_response = ErrorHandler::map_error_to_handler(error);

            error_response
                .to_bytes()
                .expect("Failed to convert response to http headers")
        };

        let response_bytes = match Request::try_new(buf_reader) {
            Ok(request) => {
                log!("{} {}", request.method, request.path);
                let response: Result<Response, AppError> = Router::route_request(request, state);
                match response {
                    Ok(response) => response
                        .to_bytes()
                        .unwrap_or_else(map_error_to_response_bytes),
                    Err(e) => map_error_to_response_bytes(e),
                }
            }
            Err(app_error) => map_error_to_response_bytes(app_error),
        };

        stream
            .write_all(&response_bytes)
            .map_err(|e| format!("Error writing response to stream: {}", e))?;

        stream
            .flush()
            .map_err(|e| format!("Error flushing stream: {}", e))?;
        Ok(())
    }
}

/// This ensures that the uploads directory always exists.  
/// A `Path` is created with the uploads directory path, and if it does not exist, it is created
/// before the server starts listening.  
/// 
/// This is required for the binary to be self-sufficient.
fn ensure_uploads_dir() {
    let uploads_path = Path::new("uploads");
    if !uploads_path.exists() {
        log!("Uploads directory does not exist, and is being created");
        fs::create_dir(uploads_path).expect("Failed to create uploads directory");
    }
}

struct AppState {
    file_lock: Arc<Mutex<()>>,
}

static LOCKS: LazyLock<AppState> = LazyLock::new(|| {
    AppState{
        file_lock: Arc::new(Mutex::new(())),
    }
});

fn main() {
    let server = Server::new("localhost:7878", 4);
    log!("Server started and running on port 7878");
    ensure_uploads_dir();
    let app_state = Arc::new(AppState{file_lock: Arc::new(Mutex::new(()))});

    for stream in server.listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                log_error!("Encountered error getting stream {}", e);
                continue;
            }
        };
        let state = app_state.clone();
        server
            .thread_pool
            .execute(move || Server::handle_connection(stream, state));
    }
}

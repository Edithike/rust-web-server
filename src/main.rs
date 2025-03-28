use std::{fs, thread};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};

/// A `Worker` is a type that handles a single thread and runs a job received
struct Worker {
    id: usize,
    thread: thread::JoinHandle<Arc<Mutex<mpsc::Receiver<Job>>>>
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
        let thread = thread::spawn(move || loop {
            let job = receiver.lock().unwrap().recv().unwrap();
            
            println!("Worker {} got a job; executing.", id);
            
            job();
        });
        Worker { id, thread }
    }
}

/// A `Job` is a type alias for any function that runs once and implements `Send` and `static`
type Job = Box<dyn FnOnce() + Send + 'static>;


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
    fn execute<F>(&self, f: F) where F: FnOnce() + Send + 'static {
        let job = Box::new(f);
        self.sender.send(job).unwrap();
    }
}

/// Handles an HTTP connection 
/// 
/// Arguments:
/// - **stream**: a mutable TcpStream that represents a single TCP connection or HTTP request
/// 
/// This method reads the stream using a BufReader and uses the request line to identify what path
/// was called and how to handle each one.
fn handle_connection(mut stream: TcpStream) {
    let buf_reader = BufReader::new(&mut stream);
    let request_line = buf_reader.lines().next().unwrap().unwrap();

    let (status_line, filename) = match request_line.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "home.html"),
        "GET /sleep HTTP/1.1" => {
            std::thread::sleep(std::time::Duration::from_secs(5));
            ("HTTP/1.1 200 OK", "home.html")
        }
        _ => ("HTTP/1.1 404 NOT FOUND", "404.html"),
    };

    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();

    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
    stream.write_all(response.as_bytes()).unwrap();
}

fn main() {
    let listener = TcpListener::bind("localhost:7878").unwrap();

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let stream = stream.unwrap();

        pool.execute(move || handle_connection(stream));
    }
}

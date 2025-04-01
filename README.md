# rust-web-server

This is a simple Rust web server that allows users upload
files and view uploaded files.

## Prerequisites

Ensure you have the following installed before proceeding:

- **Git**: [Download & Install Git](https://git-scm.com/downloads)
- **Rust & Cargo**: [Install Rust](https://www.rust-lang.org/tools/install) using **rustup** (recommended)

## Installation

### 1. Clone the Repository

```sh
git clone https://github.com/Edithike/rust-web-server
```

### 2. Install Rust (if not installed)

If Rust is not installed, run:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the on-screen instructions, then restart your 
terminal and verify the installation:
```sh
rustc --version
cargo --version
```

### 3. Compile the binary

To build the project:
```shell
cargo build --release
```
This creates an optimized executable in target/release/web-server.

### 4. Run the binary

To run the program:
```shell
cargo run
```

Or if built with `--release`
```shell
./target/release/web-server
```

### 5. Open in browser

The app should be running locally and can be accessed on
[localhost:7878](http://localhost:7878)

### 6. Documentation

The documentation for this Rust crate can be viewed with this command
```shell
cargo doc --open
```
This will open up the documentation in your browser.
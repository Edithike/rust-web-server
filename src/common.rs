use crate::http::ResponseBody;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

/// Logs info to the standard output, adding the current date and time, and using colors to 
/// indicate it is an info log
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        {
            // Get the current time in military time
            let military_time = get_current_military_time();

            // Print the log message
            println!("\x1b[32mINFO [{}]\x1b[0m {}", military_time, format!($($arg)*));
        }
    };
}

/// Logs a warning to the standard error output, adding the current date and time, using colors
/// to indicate it's a warning log
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        {
            // Get the current time in military time
            let military_time = get_current_military_time();

            // Print the log message
            eprintln!("\x1b[33mWARNING [{}]\x1b[0m {}", military_time, format!($($arg)*));
        }
    };
}

/// Logs an error to the standard error output, adding the current date and time, using colors 
/// to indicate that it is an error and might be critical
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        {
            // Get the current time in military time
            let military_time = get_current_military_time();

            // Print the log message
            eprintln!("\x1b[31mWARNING [{}]\x1b[0m {}", military_time, format!($($arg)*));
        }
    };
}

/// The error type of the application, representing all known variants of errors that might occur
/// during the lifetime of the application
/// - **IO**: This represents errors that have to do with reading or writing to files or streams, they
/// are critical and typically mean something is wrong with the application or configuration
/// - **Invalid**: This represents errors from a malformed request or invalid body, they are usually 
/// not critical
/// - **NotFound**: This represents errors that come from the client requesting a file or page that
/// can't be found on the server
/// - **NotPermitted**: This represents errors that emanate from the client attempting to access a
/// resource outside the permission, usually a file outside the uploads directory.
/// - **Unknown**: This represents all errors of unknown reason or origin.
#[derive(Debug)]
pub(crate) enum AppError {
    IO(String),
    Invalid(String),
    NotFound(String),
    NotPermitted(String),
    Unknown(String),
}

/// Checks if a year is a leap year using the Gregorian calendar's definition of a leap year.  
/// A year is a leap year if it is divisible by 4 but not by 100, or it is divisible by 400
fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Gets the current timestamp by taking the current `SystemTime` and finding the duration from the
/// UNIX epoch till now, and then parsing that into seconds
fn get_current_timestamp() -> u64 {
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_epoch.as_secs()
}

/// Gets the current date and time in military time (YYYY-MM-DDThh:mm:ss).  
/// The current timestamp is gotten, and then divided by the number of seconds in a day to derive
/// how many days have passed since the UNIX epoch. The number of days is then used to figure out
/// how many years have passed since the then.  
/// The remainder of the division is used to find the time of the current day
pub(crate) fn get_current_military_time() -> String {
    let timestamp = get_current_timestamp();
    let seconds_per_minute = 60u32;
    let seconds_per_hour = 3_600u32;
    let seconds_per_day = 86_400u32;

    let mut days_since_epoch = timestamp / seconds_per_day as u64;
    let mut current_year = 1970u16;
    let seconds_in_current_day: u32 = (timestamp % seconds_per_day as u64) as u32;

    // Continuously subtracts the number days of each year from the total days since the UNIX epoch, 
    // until the days left can't make up a year
    while days_since_epoch >= if is_leap_year(current_year) { 366 } else { 365 } {
        current_year += 1;
        days_since_epoch -= if is_leap_year(current_year) { 366 } else { 365 }
    }

    let days_in_months: [u8; 12] = [
        31,
        if is_leap_year(current_year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut current_month = 1;
    // Repeatedly subtracts the number of days of each month from the days left, until the subtraction
    // amounts in a negative number
    for days in days_in_months {
        days_since_epoch = match days_since_epoch.checked_sub(days as u64) {
            Some(d) => d,
            None => break,
        };
        current_month += 1;
    }
    let current_day: u8 = days_since_epoch as u8 + 1;
    let hour = seconds_in_current_day / seconds_per_hour;
    let minute = (seconds_in_current_day % seconds_per_hour) / seconds_per_minute;
    let seconds = (seconds_in_current_day % seconds_per_minute) % seconds_per_minute;

    format!("{current_year}-{current_month:02}-{current_day:02}T{hour:02}:{minute:02}:{seconds:02}")
}

/// Handles all file reading and writing logic besides the basics
pub(crate) struct FileManager;

impl FileManager {
    /// Saves a file to a specified directory
    ///
    /// Arguments:
    /// - **dir**: The directory to save the file in
    /// - **buffered_file**: An `BufferedFile` to be saved
    ///
    /// A `Path` is made out of the directory, and then joined with the file name to create the file path.  
    /// A `File` is then created in that `Path`, and the contents of the `BufferedFile` are written into it.  
    /// This will override any previous file saved in the same path with the same name.
    pub(crate) fn save_file(dir: &str, buffered_file: BufferedFile) -> Result<(), AppError> {
        let path = Path::new(dir).join(buffered_file.name);

        let mut file = File::create(&path) // Create or overwrite the file
            .map_err(|_| AppError::IO("Failed to create file".to_string()))?;
        file.write_all(&*buffered_file.content) // Write the bytes to the file
            .map_err(|_| AppError::IO("Failed to read file".to_string()))?;

        Ok(())
    }

    /// Returns a list of file names and their paths
    ///
    /// Arguments:
    /// - **dir**: The directory to search in for files
    ///
    /// It uses the `traverse_dir()` helper method to traverse each subdirectory to ensure all files within
    /// them are returned in a flattened list.
    pub(crate) fn list_files_with_paths(dir: &str) -> Result<Vec<(String, String)>, AppError> {
        let mut files = Vec::new();

        Self::traverse_dir(Path::new(dir), &mut files, "".to_string())?;

        Ok(files)
    }

    /// Traverses through a directory and its subdirectories recursively.
    ///
    /// Arguments:
    /// - **dir**: The current directory being traversed
    /// - **files**: The list of files that have been added to be returned
    /// - **relative_path**: The current path relative to the first directory
    ///
    /// The current directory's entries are looped through, with each one being added to the list if
    /// they are a file, or going one step deeper into the next recursion if they are a directory.
    fn traverse_dir(
        dir: &Path,
        files: &mut Vec<(String, String)>,
        relative_path: String,
    ) -> Result<(), AppError> {
        for entry in fs::read_dir(dir)
            .map_err(|_| AppError::IO(format!("Failed to read directory: {}", dir.display())))?
        {
            let entry = entry
                .map_err(|entry| AppError::IO(format!("Failed to read entry: {:?}", entry)))?;
            let path = entry.path();
            let file_name = entry.file_name().into_string().map_err(|_| {
                AppError::IO(format!(
                    "Failed to parse entry filename to string: {:?}",
                    entry
                ))
            })?;

            // Construct the relative file path
            let full_path = path.to_string_lossy().into_owned();
            let relative_file_path = if relative_path.is_empty() {
                file_name.clone()
            } else {
                format!("{}/{}", relative_path, file_name)
            };

            if path.is_dir() {
                // Recursively process subdirectories
                Self::traverse_dir(&path, files, relative_file_path)?;
            } else {
                // Add file to list
                files.push((relative_file_path, full_path));
            }
        }
        Ok(())
    }
}

/// A `BufferedFile` is an abstraction of a file in memory.  
/// It can be any file, be it one gotten from a form, an upload being served or an HTML template    
/// It contains a name and the contents of the file in a byte buffer
pub(crate) struct BufferedFile {
    pub(crate) name: String,
    pub(crate) content: Vec<u8>,
}

impl Display for BufferedFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let content = String::from_utf8(self.content.clone())
            .unwrap_or_else(|_| "File is not text based".to_string());
        write!(f, "Filename: {}\nFile content: {}", self.name, content)
    }
}

impl TryFrom<ResponseBody> for Option<BufferedFile> {
    type Error = AppError;

    /// Tries to cast a `ResponseBody` into an `Option<BufferedFile>`.  
    /// The resultant `BufferedFile` is optional because not all `ResponseBody`s contain a file
    fn try_from(value: ResponseBody) -> Result<Self, Self::Error> {
        match value {
            ResponseBody::File(filename) => {
                let path = Path::new(&filename);
                let uploaded_file = BufferedFile::try_from(path)?;
                Ok(Some(uploaded_file))
            }
            ResponseBody::Text(text) => {
                let uploaded_file = BufferedFile {
                    name: "response.html".to_string(),
                    content: text.as_bytes().to_vec(),
                };
                Ok(Some(uploaded_file))
            }
            ResponseBody::Empty => Ok(None),
        }
    }
}

impl TryFrom<&Path> for BufferedFile {
    type Error = AppError;

    /// Tries to cast a `&Path` into an `BufferedFile`.  
    /// This is for scenarios where the path of a file is known, and it needs to be constructed into
    /// an `BufferedFile` to be served to the client.
    ///
    /// The path is first validated to ensure it exists and isn't a directory, then an attempt to open
    /// the file is made, if successful, the contents are read into a buffer, which becomes the `content`
    /// of the `BufferedFile`, while the filename is gotten from the `Path` and validated to be UTF-8
    /// before being passed into the `BufferedFile` constructor as the `name` field.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if !path.exists() {
            return Err(AppError::NotFound(format!(
                "File does not exist: {}",
                path.display()
            )));
        }
        if path.is_dir() {
            return Err(AppError::NotFound(format!(
                "Path for file is a directory: {}",
                path.display()
            )));
        }
        let file_name = path
            .file_name()
            .ok_or(AppError::Invalid(format!(
                "Invalid file name: {}",
                path.display()
            )))?
            .to_str()
            .ok_or(AppError::Invalid(format!(
                "File name is not valid UTF-8: {}",
                path.display()
            )))?;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                return Err(AppError::NotFound(format!(
                    "File failed to open: {file_name}"
                )));
            }
        };

        let mut file_buffer = Vec::new();
        file.read_to_end(&mut file_buffer)
            .map_err(|_| AppError::IO(format!("Error reading file into buffer: {file_name}")))?;

        Ok(BufferedFile {
            name: file_name.to_string(),
            content: file_buffer,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::common::get_current_military_time;

    #[test]
    fn test_get_military_time() {
        let military_time = get_current_military_time();

        println!("military time: {}", military_time);
    }
}

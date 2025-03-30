use crate::http::ResponseBody;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::{fmt, fs};

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
    pub(crate) fn save_file(dir: &str, buffered_file: BufferedFile) -> std::io::Result<()> {
        let path = Path::new(dir).join(buffered_file.name);

        let mut file = File::create(&path)?; // Create or overwrite the file
        file.write_all(&*buffered_file.content)?; // Write the bytes to the file

        Ok(())
    }

    /// Returns a list of file names and their paths
    /// 
    /// Arguments:
    /// - **dir**: The directory to search in for files
    /// 
    /// It uses the `traverse_dir()` helper method to traverse each subdirectory to ensure all files within
    /// them are returned in a flattened list.
    pub(crate) fn list_files_with_paths(dir: &str) -> std::io::Result<Vec<(String, String)>> {
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
    fn traverse_dir(dir: &Path, files: &mut Vec<(String, String)>, relative_path: String) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name().into_string().unwrap_or_default();

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
    type Error = String;

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
    type Error = String;

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
        file.read_to_end(&mut file_buffer).map_err(|_| "Error reading file into buffer")?;

        Ok(BufferedFile {
            name: file_name.to_string(),
            content: file_buffer,
        })
    }
}
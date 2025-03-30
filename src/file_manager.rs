use crate::http::ResponseBody;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::{fmt, fs};

pub(crate) fn save_file(dir: &str, uploaded_file: UploadedFile) -> std::io::Result<()> {
    let path = Path::new(dir).join(uploaded_file.name);

    let mut file = File::create(&path)?; // Create or overwrite the file
    file.write_all(&*uploaded_file.content)?; // Write the bytes to the file

    Ok(())
}

pub(crate) fn list_files_with_paths(dir: &str) -> std::io::Result<Vec<(String, String)>> {
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

pub(crate) struct UploadedFile {
    pub(crate) name: String,
    pub(crate) content: Vec<u8>,
}

impl Display for UploadedFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let content = String::from_utf8(self.content.clone())
            .unwrap_or_else(|_| "File is not text based".to_string());
        write!(f, "Filename: {}\nFile content: {}", self.name, content)
    }
}

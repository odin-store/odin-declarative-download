use std::cmp::PartialEq;
use std::path::Path;
use std::sync::Arc;
use std::{fs};
use std::fs::File;
use std::io::Write;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use futures_util::StreamExt;
use queues::{IsQueue, Queue, queue};
use reqwest::Client;
use tokio::sync::Mutex;
use tokio::time::Instant;
use json::{JsonValue, object};
use tokio::spawn;

#[derive(Debug, Clone)]
enum DownloadStatus {
    Pending,
    Downloading,
    Extracting,
    Paused,
    Error
}

/// Wrapper for downloader state and queue management
#[doc(hidden)]
pub struct DownloaderWrapper {
    pub status: DownloadStatus,
    pub queue: Queue<String>,
    pub current_speed: f64,
    pub current_target: String,
    pub current_percent: f64
}

impl DownloaderWrapper {
    /// Creates a new instance of DownloaderWrapper
    fn new() -> DownloaderWrapper {
        DownloaderWrapper {
            status: DownloadStatus::Pending,
            queue: queue![],
            current_speed: 0.0,
            current_target: String::new(),
            current_percent: 0.0
        }
    }
}

impl PartialEq for DownloadStatus {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DownloadStatus::Pending, DownloadStatus::Pending) => true,
            (DownloadStatus::Downloading, DownloadStatus::Downloading) => true,
            (DownloadStatus::Paused, DownloadStatus::Paused) => true,
            _ => false,
        }
    }
}

pub struct DownloaderClient(pub Arc<Mutex<DownloaderWrapper>>);

impl DownloaderClient {
    /// Creates a new DownloaderClient and starts a background thread to manage downloads
    pub fn new() -> DownloaderClient {
        let client = DownloaderClient(
            Arc::new(
                Mutex::new(
                    DownloaderWrapper::new()
                )
            )
        );

        client
    }

    pub async fn register(&self,id:String, url: String, file_path: String) {
        let mut lock = self.0.lock().await;
        println!("url: {}, file_path: {}", url, file_path);

        let target = Self::encode_target(id, url, file_path);
        
        if lock.status == DownloadStatus::Pending {
            self.start_download(target);
        }
        else {
            lock.queue.add(target).expect("Cannot add target to download queue.");
        }
    }

    pub fn start_download(&self, target: String) {
        let cloned_wrapper = Arc::clone(&self.0);
        let target = Self::decode_target(target);

        spawn(async move {
            let client = DownloaderClient(cloned_wrapper);

            println!("url: {}, path: {}",target["url"],target["file_path"] );

            client.download(target["url"].to_string(),target["file_path"].to_string()).await.expect("Error");
        });
    }

    /// Retrieves current download information
    pub async fn get_info(&self) -> String {
        let lock = self.0.lock().await;

        let info_object = object! {
            id: lock.current_target.to_string(),
            speed: lock.current_speed.to_string(),
            percent: lock.current_percent.to_string()
        };

        json::stringify(info_object)
    }

    pub async fn pause_download(&self) {
        let lock = self.0.lock().await;

        lock.status = DownloadStatus::Paused;
    }

    pub async fn resume_download(&self) {
        let lock = self.0.lock().await;
        let target = Self::decode_target(lock.current_target.to_string());
        let cloned_wrapper = Arc::clone(&self.0);

        spawn(async move {
            let client = DownloaderClient(cloned_wrapper);

            println!("url: {}, path: {}",target["url"],target["file_path"] );

            client.download(target["url"].to_string(),target["file_path"].to_string()).await.expect("Error");
        });
    }

    fn decode_target(target: String)-> JsonValue {
        let decoded_bytes = BASE64_STANDARD.decode(target).unwrap();
        let decoded_str = String::from_utf8(decoded_bytes).unwrap();
        println!("decoded : {}",decoded_str);
        json::parse(&decoded_str).unwrap()
    }

    fn encode_target(id: String, url:String, file_path:String)-> String {
        let stringified_target = json::stringify(object! {
            id: id,
            url: url,
            file_path: file_path
        });
        println!("stringified : {}",stringified_target);
        BASE64_STANDARD.encode(stringified_target)
    }

    /// Handles the actual file download
    async fn download(&self, url: String, file_path: String) -> Result<(), String> {
        println!("Starting download");

        let client = Client::new();

        let mut lock = self.0.lock().await;

        let progress_path = format!("{}.progress", file_path);
        let mut start_byte = 0;

        let total_size = 5000;

        println!("got client total size");

        if Path::new(&progress_path).exists() {
            start_byte = fs::read_to_string(&progress_path).map_err(|e| e.to_string())?.parse::<u64>().map_err(|e| e.to_string())?;
        }
        println!("Downloading game to {}.. Starting byte: {}", file_path, start_byte);
        let response = client.get(&url)
            .header("Range", format!("bytes={}-", start_byte))
            .send().await.map_err(|e| e.to_string())?;

        println!("Fetching file from server");

        let mut file = if start_byte > 0 {
            File::open(&file_path).map_err(|e| e.to_string())?
        } else {
            File::create(&format!("{}/.progress",file_path)).map_err(|e| e.to_string())?
        };

        println!("File created. Overwriting..");

        let mut stream = response.bytes_stream();
        let mut last_time = Instant::now();
        let mut downloaded_bytes = 0u64;

        while let Some(item) = stream.next().await {
            if lock.status == DownloadStatus::Paused {
                break;
            }

            let chunk = item.map_err(|e| e.to_string())?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            start_byte += chunk.len() as u64;
            downloaded_bytes += chunk.len() as u64;

            let elapsed_time = last_time.elapsed().as_secs_f64();

            if elapsed_time > 1.0 {
                let speed = downloaded_bytes as f64 / elapsed_time;
                let percent = (start_byte as f64 / total_size as f64) * 100.0;
                println!("Download speed: {:.2} KB/s, Progress: {:.2}%", speed / 1024.0, percent);

                lock.current_percent = percent;
                lock.current_speed = (speed / 1024.0).floor();

                downloaded_bytes = 0;
                last_time = Instant::now();
            }

            downloaded_bytes = 0;
            last_time = Instant::now();

            fs::write(&progress_path, start_byte.to_string()).map_err(|e| e.to_string())?;
        }

        fs::remove_file(&progress_path).map_err(|e| e.to_string())?;


        let cloned_wrapper = Arc::clone(&self.0);

        spawn(async move {
            let client = DownloaderClient(cloned_wrapper);
            client.extract(Path::new(&file_path)).await;
        });

        Ok(())
    }

    /// Extracts a downloaded file
    async fn extract(&self, path: &Path) {
        let mut lock = self.0.lock().await;

        lock.status = DownloadStatus::Extracting;

        println!("current path : {}/.progress",path.to_string_lossy());

        fs::rename(format!("{}/.progress",path.to_string_lossy()), format!("{}/downloaded.zip",path.to_string_lossy())).expect("error occurred during renamed file.");

        let file = File::open(format!("{}/downloaded.zip",path.to_string_lossy())).expect("Error opening file");
        if let Err(e) = zip_extract::extract(file, path, true) {
            println!("Error during extraction: {}", e);
            lock.status = DownloadStatus::Error;
            return;
        }
        self.delete(path).await;
    }

    async fn delete(&self, path: &Path) {
        fs::remove_file(format!("{}/downloaded.zip",path.to_string_lossy())).expect("error occurred during renamed file.");
    }
}
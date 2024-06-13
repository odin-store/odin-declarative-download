use std::cmp::PartialEq;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fs, thread};
use std::fs::File;
use std::io::Write;
use futures_util::StreamExt;
use queues::{IsQueue, Queue, queue};
use reqwest::Client;
use tokio::time::Instant;
use json::{object};

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

        client.start_loop();

        client
    }

    /// Starts a loop in a new thread to handle download tasks
    fn start_loop(&self) {
        let clone = Arc::clone(&self.0);

        thread::spawn(move || {
            loop {
                let lock = clone.lock().unwrap();

                if lock.status == DownloadStatus::Pending && !lock.queue.size()==0 {
                    if let Ok(download_info) = json::parse(&lock.current_target) {
                    } else {
                        eprintln!("Error parsing JSON: {}", lock.current_target);
                    }
                };
            };
        });
    }

    /// Processes the next item in the queue
    async fn next(&self) {
        let mut lock = self.0.lock().unwrap();
        if lock.queue.size()==0 {
            lock.status = DownloadStatus::Pending;
        } else {
            if let Ok(download_info) = json::parse(&lock.current_target) {
                lock.status = DownloadStatus::Downloading;
                if let Err(e) = self.download(download_info["url"].to_string(), download_info["target"].to_string()).await {
                    eprintln!("Error during download: {}", e);
                }
            } else {
                eprintln!("Error parsing JSON: {}", lock.current_target);
            }
        }
    }

    /// Registers a new download task
    pub fn register(&self, id: String, url: String, file_path: String) {
        let mut lock = self.0.lock().unwrap();
        let string = json::stringify(object! {
            id: id,
            url: url,
            target: file_path
        });

        lock.current_target = string;
        lock.status = DownloadStatus::Downloading;
    }

    /// Retrieves current download information
    pub fn get_info(&self) -> String {
        let lock = self.0.lock().unwrap();

        let info_object = object! {
            id: lock.current_target.to_string(),
            speed: lock.current_speed.to_string(),
            percent: lock.current_percent.to_string()
        };

        json::stringify(info_object)
    }

    /// Handles the actual file download
    async fn download(&self, url: String, file_path: String) -> Result<(), String> {
        println!("Starting download");

        let mut lock = self.0.lock().unwrap();

        lock.status = DownloadStatus::Downloading;

        let progress_path = format!("{}.progress", file_path);
        let mut start_byte = 0;
        let total_size = reqwest::Client::new()
            .head(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .content_length()
            .ok_or("Could not retrieve total file size.")?;

        if Path::new(&progress_path).exists() {
            start_byte = fs::read_to_string(&progress_path).map_err(|e| e.to_string())?.parse::<u64>().map_err(|e| e.to_string())?;
        }
        println!("Downloading game to {}.. Starting byte: {}", file_path, start_byte);

        let client = Client::new();
        let response = client.get(&url)
            .header("Range", format!("bytes={}-", start_byte))
            .send().await.map_err(|e| e.to_string())?;

        println!("Fetching file from server");

        let mut file = if start_byte > 0 {
            File::open(&file_path).map_err(|e| e.to_string())?
        } else {
            File::create(&file_path).map_err(|e| e.to_string())?
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
                lock.current_speed = speed / 1024.0;

                downloaded_bytes = 0;
                last_time = Instant::now();
            }

            fs::write(&progress_path, start_byte.to_string()).map_err(|e| e.to_string())?;
        }

        fs::remove_file(&progress_path).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Extracts a downloaded file
    async fn extract(&self, file: File, path: &Path) {
        let mut lock = self.0.lock().unwrap();

        lock.status = DownloadStatus::Extracting;
        if let Err(e) = zip_extract::extract(file, path, true) {
            println!("Error during extraction: {}", e);
            lock.status = DownloadStatus::Error;
            return;
        }
    }
}
use std::cmp::PartialEq;
use std::path::Path;
use std::sync::Arc;
use std::fs::{self, File};
use std::io::Write;
use base64::prelude::BASE64_STANDARD;
use futures_util::{StreamExt, future::FutureExt};
use queues::{IsQueue, Queue, queue};
use reqwest::Client;
use tokio::sync::Mutex;
use tokio::time::Instant;
use json::{JsonValue, object};
use tokio::spawn;

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Extracting,
    Paused,
    Error
}

pub struct DownloaderWrapper {
    pub status: DownloadStatus,
    pub queue: Queue<String>,
    pub current_speed: f64,
    pub next_target: String,
    pub current_target: String,
    pub current_percent: f64
}

impl DownloaderWrapper {
    fn new() -> Self {
        Self {
            status: DownloadStatus::Pending,
            queue: queue![],
            current_speed: 0.0,
            next_target: String::new(),
            current_target: String::new(),
            current_percent: 0.0
        }
    }
}

pub struct DownloaderClient(pub Arc<Mutex<DownloaderWrapper>>);

impl DownloaderClient {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(DownloaderWrapper::new())))
    }

    pub async fn register(&self, id: String, url: String, total_size: f64, file_path: String) {
        let cloned_wrapper = Arc::downgrade(&self.0);
        let mut lock = self.0.lock().await;
        println!("url: {}, file_path: {}", url, file_path);

        let target = Self::encode_target(id, url, total_size, file_path);

        if lock.status == DownloadStatus::Pending {
            spawn(async move {
                if let Some(wrapper) = cloned_wrapper.upgrade() {
                    let client = DownloaderClient(wrapper);
                    client.start_download(target).await;
                }
            }.boxed());
        } else {
            lock.queue.add(target).expect("Cannot add target to download queue.");
        }
    }

    pub async fn start_download(&self, target: String) {
        let cloned_wrapper = Arc::clone(&self.0);
        let target = Self::decode_target(target);

        let url = target["url"].to_string();
        let total_size = target["total_size"].as_f64().unwrap();
        let file_path = target["file_path"].to_string();

        let mut lock = self.0.lock().await;
        lock.current_target = format!("{{\"url\":\"{}\",\"total_size\":{},\"file_path\":\"{}\"}}", url, total_size, file_path);

        println!("current target : {}", lock.current_target);
        print!("download starting..");

        let cloned_wrapper_inner = Arc::downgrade(&cloned_wrapper);
        spawn(async move {
            if let Some(wrapper) = cloned_wrapper_inner.upgrade() {
                let client = DownloaderClient(wrapper);
                println!("url: {}, path: {}", url, file_path);
                client.download(url, total_size, file_path).await.expect("Error");
            }
        });
    }

    pub async fn get_info(&self) -> String {
        println!("decode started.");
        let lock = self.0.lock().await;

        println!("target: {}", lock.current_target);

        if lock.current_target.is_empty() {
            return "No current target".to_string();
        }

        let decoded = match json::parse(&lock.current_target) {
            Ok(value) => value,
            Err(e) => {
                println!("Error parsing JSON: {}", e);
                return "Error parsing current target".to_string();
            }
        };

        println!("decoded: {}", json::stringify(decoded.clone()));

        let info_object = object! {
            id: decoded["id"].to_string(),
            speed: lock.current_speed.to_string(),
            percent: lock.current_percent.to_string()
        };
        json::stringify(info_object)
    }

    pub async fn pause_download(&self) {
        let mut lock = self.0.lock().await;
        lock.status = DownloadStatus::Paused;
    }

    pub async fn resume_download(&self) {
        let lock = self.0.lock().await;
        let target = Self::decode_target(lock.next_target.to_string());
        let cloned_wrapper = Arc::clone(&self.0);

        spawn(async move {
            let client = DownloaderClient(cloned_wrapper);
            println!("url: {}, path: {}", target["url"], target["file_path"]);
            client.download(target["url"].to_string(), target["total_size"].as_f64().unwrap(), target["file_path"].to_string()).await.expect("Error");
        });
    }

    pub fn decode_target(target: String) -> JsonValue {
        match base64::Engine::decode(&BASE64_STANDARD, &target) {
            Ok(decoded_bytes) => match String::from_utf8(decoded_bytes) {
                Ok(decoded_str) => {
                    println!("decoded : {}", decoded_str);
                    match json::parse(&decoded_str) {
                        Ok(parsed_json) => parsed_json,
                        Err(e) => {
                            println!("Error parsing JSON: {}", e);
                            JsonValue::Null
                        }
                    }
                }
                Err(e) => {
                    println!("Error converting bytes to string: {}", e);
                    JsonValue::Null
                }
            },
            Err(e) => {
                println!("Error decoding base64: {}", e);
                JsonValue::Null
            }
        }
    }

    fn encode_target(id: String, url: String, total_size: f64, file_path: String) -> String {
        let stringified_target = json::stringify(object! {
            id: id,
            url: url,
            total_size: total_size,
            file_path: file_path
        });
        println!("stringified : {}", stringified_target);
        base64::Engine::encode(&BASE64_STANDARD, stringified_target)
    }

    async fn download(&self, url: String, total_size: f64, file_path: String) -> Result<(), String> {
        println!("Starting download to {}/,progress", file_path);

        let client = Client::new();

        let mut lock = self.0.lock().await;

        let progress_path = format!("{}/.progress", file_path);
        let mut start_byte = 0;

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
        let last_time = Instant::now();
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
            if elapsed_time > 0.1 {
                let speed = downloaded_bytes as f64 / elapsed_time * 10.0;
                let percent: f64 = (downloaded_bytes as f64 / total_size) * 100.0;
                println!("Download speed: {:.2} KB/s, Progress: {:.0}%", speed / 1024.0, percent);
                println!("Downloaded: {}b, Total: {}b", downloaded_bytes, total_size);
                lock.current_percent = percent;
                lock.current_speed = (speed / 1024.0).floor();
            }
        }

        let cloned_wrapper = Arc::downgrade(&self.0);

        spawn(async move {
            if let Some(wrapper) = cloned_wrapper.upgrade() {
                let client = DownloaderClient(wrapper);
                client.extract(Path::new(&file_path)).await;
            }
        });

        Ok(())
    
    }

    async fn extract(&self, path: &Path) {
        let mut lock = self.0.lock().await;
        lock.status = DownloadStatus::Extracting;

        println!("extracting.. : {}/.progress", path.to_string_lossy());

        fs::rename(format!("{}/.progress", path.to_string_lossy()), format!("{}/downloaded.zip", path.to_string_lossy())).expect("error occurred during renamed file.");

        let file = File::open(format!("{}/downloaded.zip", path.to_string_lossy())).expect("Error opening file");
        if let Err(e) = zip_extract::extract(file, path, true) {
            println!("Error during extraction: {}", e);
            lock.status = DownloadStatus::Error;
            return;
        }

        self.delete(path).await;
        lock.current_target = String::new();
        lock.status = DownloadStatus::Pending;
    }

    async fn delete(&self, path: &Path) {
        println!("deleting {}/downloaded.zip", path.to_string_lossy());
        let remove_result = fs::remove_file(format!("{}/downloaded.zip", path.to_string_lossy()));
        match remove_result {
            Ok(_) => println!("File deleted successfully."),
            Err(e) => println!("Failed to delete file: {}", e),
        }
    }

    pub async fn next(&self) {
        println!("calling next download..");
        let mut lock = self.0.lock().await;
        if let Ok(next_target) = lock.queue.remove() {
            lock.next_target = next_target.clone();
            let cloned_self = Arc::clone(&self.0);
            tokio::spawn(async move {
                let client = DownloaderClient(cloned_self);
                client.start_download(next_target).await;
            });
        } else {
            lock.status = DownloadStatus::Pending;
        }
    }
}
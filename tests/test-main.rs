use std::time::Duration;
use futures_util::StreamExt;
use declartive_download::DownloaderClient;

#[tokio::test]
async fn test() -> Result<(), Box<dyn std::error::Error>> {
    let client = DownloaderClient::new();

    println!("Registering new download..");
    client.register("1".to_string(), "http://localhost:8080/games/download/odin-1711812665673".to_string(),"D:/Programming/Odin/test".to_string()).await;

    tokio::time::sleep(Duration::new(0, 15)).await;

    println!("Getting info..");
    println!("Got status: {}",client.get_info().await);

    tokio::time::sleep(Duration::new(10, 0)).await;

    println!("Getting info..");
    println!("Got status: {}",client.get_info().await);

    tokio::time::sleep(Duration::new(10, 0)).await;

    println!("Getting info..");
    println!("Got status: {}",client.get_info().await);

    Ok(())
}
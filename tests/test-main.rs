use std::time::Duration;
use declartive_download::DownloaderClient;

#[tokio::test]
async fn test() -> Result<(), Box<dyn std::error::Error>> {
    let client = DownloaderClient::new();

    println!("Registering new download..");
    client.register("1".to_string(), "http://localhost:8080/games/download/odin-1711812665673".to_string(), 2928778.0, "D:/Programming/Odin/test".to_string()).await;
            let info = client.get_info().await;
        println!("Got status: {}", info);
        tokio::time::sleep(Duration::from_millis(30)).await;
    client.register("1".to_string(), "http://localhost:8080/games/download/odin-1719408830366".to_string(), 57886032.0, "D:/Programming/Odin/test".to_string()).await;
    for _ in 0..10 {
        println!("Getting info..");
        let info = client.get_info().await;
        if info == "No current target" {
            client.next().await;
        }
        println!("Got status: {}", info);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}

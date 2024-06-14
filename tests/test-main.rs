use declartive_download::DownloaderClient;

#[test]
fn test() -> Result<(), Box<dyn std::error::Error>> {
    let client = DownloaderClient::new();

    println!("Registering new downlaod..");
    client.register("1".to_string(), "http://localhost:8080/games/download/odin-1711812665673".to_string(), "C:/Users/userse/Downloads".to_string());


    println!("Getting info..");
    println!("Got status: {}",client.get_info());


    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("Getting info..");
    println!("Got status: {}",client.get_info());

    std::thread::sleep(std::time::Duration::from_secs(20));

    println!("Getting info..");
    println!("Got status: {}",client.get_info());

    Ok(())
}
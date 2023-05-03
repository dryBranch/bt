use bt::{torrent::Torrent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bf = None;
    let mut queue = None;
    while let Some((_bf, _queue)) = Torrent::download("t3.torrent", bf, queue).await? {
        // 如果返回了 Some 说明没完
        bf = Some(_bf);
        queue = Some(_queue);
        println!("retry");
    }
    Ok(())
}

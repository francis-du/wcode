#[tokio::main]
async fn main() -> anyhow::Result<()> {
    wcode::run().await
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    wayfinder_palette_search_benchmark::entry().await
}

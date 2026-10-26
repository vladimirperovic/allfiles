mod cli;
mod crypto;
mod demo;
mod encrypted_vfs;
mod extract;
mod import;
mod mcp;
mod scanner;
mod server;
mod store;
#[cfg(test)]
mod tests;
fn main() {
    if let Err(e) = cli::run() {
        eprintln!("AllFiles: {e:#}");
        std::process::exit(1);
    }
}
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

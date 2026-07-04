use std::io::{self, BufRead, Write};
use std::thread;

use night24_agent_core::AgentCore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("night24-agent-core panic: {panic_info}");
    }));

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_ansi(false)
        .without_time()
        .init();

    let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let writer = thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        while let Some(message) = output_rx.blocking_recv() {
            if writeln!(stdout, "{message}").is_err() {
                break;
            }
            if stdout.flush().is_err() {
                break;
            }
        }
    });

    let stdin = io::stdin();
    let mut core = AgentCore::with_output(output_tx.clone());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                eprintln!("failed to read stdin: {err}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let messages = core.handle_line(&line).await;
        for message in messages {
            output_tx.send(message)?;
        }

        if core.should_exit() {
            break;
        }
    }

    drop(core);
    drop(output_tx);
    let _ = writer.join();

    Ok(())
}

use ring::digest::{Context, SHA256};
use std::cell::RefCell;
use std::io::Read;

const STREAM_BUF_SIZE: usize = 1024 * 1024;

std::thread_local! {
    static SHA256_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn sha256_of_file(path: &std::path::Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut ctx = Context::new(&SHA256);

    let mut buffer = SHA256_BUF.take();
    if buffer.len() < STREAM_BUF_SIZE {
        buffer.resize(STREAM_BUF_SIZE, 0);
    }

    let result = (|| -> anyhow::Result<String> {
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            ctx.update(&buffer[..bytes_read]);
        }
        Ok(sha256_hex(ctx.finish()))
    })();

    SHA256_BUF.with(|buf| {
        let _ = buf.replace(buffer);
    });

    result
}

pub fn sha256_of_bytes(data: &[u8]) -> String {
    let mut ctx = Context::new(&SHA256);
    ctx.update(data);
    sha256_hex(ctx.finish())
}

fn sha256_hex(digest: ring::digest::Digest) -> String {
    hex::encode(digest.as_ref())
}

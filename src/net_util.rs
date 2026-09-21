use anyhow::{Context, Result, bail};
use std::net::{SocketAddr, TcpListener};

/// Try preferred port first, then scan upward (and wrap) within 1024..=65535.
pub fn find_free_port(preferred: u16) -> Result<u16> {
    let preferred = preferred.clamp(1024, 65535);
    if port_is_free(preferred) {
        return Ok(preferred);
    }
    for offset in 1u32..60_000 {
        let candidate = preferred as u32 + offset;
        let port = if candidate > 65535 {
            1024 + (candidate - 65536)
        } else {
            candidate
        } as u16;
        if port_is_free(port) {
            return Ok(port);
        }
    }
    bail!("هیچ پورت آزادی بین 1024 تا 65535 پیدا نشد.")
}

pub fn port_is_free(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpListener::bind(addr).is_ok()
}

pub fn bind_or_explain(port: u16) -> Result<TcpListener> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpListener::bind(addr).with_context(|| {
        format!("پورت {port} اشغال است یا در دسترس نیست. پورت دیگری انتخاب کنید.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_free_port_when_preferred_busy() {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let busy = listener.local_addr().unwrap().port();
        let free = find_free_port(busy).unwrap();
        assert_ne!(free, busy);
        assert!(port_is_free(free));
    }
}

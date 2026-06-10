//! wisp-core::framing — protocol I/O over quinn streams. Pairs with `transport` (which
//! builds the QUIC pipe). Three flavors:
//! - media frames  (host -> client): length is in the fixed `FrameHeader`.
//! - input events  (client -> host): u16 length prefix + encoded `InputEvent`.
//! - spike hello   (client -> host): u16 length prefix + the pre-shared token.
//! - generic msgs  (either direction): u32 length prefix + opaque bytes (Noise handshake).
//!
//! The decode path is a designated cargo-fuzz target. `wire::FrameHeader::decode` caps
//! the payload length before `read_frame` allocates.

use anyhow::{Context, Result};
use quinn::{RecvStream, SendStream};

use crate::wire::{FrameHeader, InputEvent, WireError, MAX_FRAME_PAYLOAD};

// ---------------------------------------------------------------------------
// Media-frame stream helpers (host -> client, ordered uni stream)
// ---------------------------------------------------------------------------

pub async fn write_frame(
    send: &mut SendStream,
    header: &FrameHeader,
    payload: &[u8],
) -> Result<()> {
    send.write_all(&header.encode())
        .await
        .context("write frame header")?;
    send.write_all(payload)
        .await
        .context("write frame payload")?;
    Ok(())
}

pub async fn read_frame(recv: &mut RecvStream) -> Result<(FrameHeader, Vec<u8>)> {
    let mut hbuf = [0u8; FrameHeader::ENCODED_LEN];
    recv.read_exact(&mut hbuf)
        .await
        .context("read frame header")?;
    let header = FrameHeader::decode(&hbuf).map_err(wire_err)?;
    let mut payload = vec![0u8; header.payload_len as usize];
    recv.read_exact(&mut payload)
        .await
        .context("read frame payload")?;
    Ok((header, payload))
}

// ---------------------------------------------------------------------------
// Input-event stream helpers (client -> host, reliable uni stream)
// ---------------------------------------------------------------------------

pub async fn write_input(send: &mut SendStream, ev: &InputEvent) -> Result<()> {
    let body = ev.encode();
    send.write_all(&(body.len() as u16).to_be_bytes())
        .await
        .context("write input len")?;
    send.write_all(&body).await.context("write input body")?;
    Ok(())
}

pub async fn read_input(recv: &mut RecvStream) -> Result<InputEvent> {
    let mut lb = [0u8; 2];
    recv.read_exact(&mut lb).await.context("read input len")?;
    let len = u16::from_be_bytes(lb) as usize;
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .context("read input body")?;
    let (ev, _) = InputEvent::decode(&body).map_err(wire_err)?;
    Ok(ev)
}

// ---------------------------------------------------------------------------
// SPIKE auth handshake: a pre-shared token sent on the control stream BEFORE any
// input is accepted or any frame is sent. NOTE: it crosses the cert-unverified
// channel in cleartext, so it guards against casual/opportunistic LAN access,
// NOT an active MITM. Real authentication is the Phase-1 Noise + SAS pairing.
// ---------------------------------------------------------------------------

pub async fn write_hello(send: &mut SendStream, token: &str) -> Result<()> {
    let body = token.as_bytes();
    send.write_all(&(body.len() as u16).to_be_bytes())
        .await
        .context("write hello len")?;
    send.write_all(body).await.context("write hello body")?;
    Ok(())
}

pub async fn read_hello(recv: &mut RecvStream) -> Result<String> {
    let mut lb = [0u8; 2];
    recv.read_exact(&mut lb).await.context("read hello len")?;
    let len = u16::from_be_bytes(lb) as usize;
    anyhow::ensure!(len <= 256, "hello token too long: {len}");
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .context("read hello body")?;
    String::from_utf8(body).context("hello token utf8")
}

// ---------------------------------------------------------------------------
// Generic length-prefixed (u32) byte messages — used for Noise handshake messages
// (core::channel) and, later, Noise-wrapped frames. Capped like a media frame.
// ---------------------------------------------------------------------------

pub async fn write_msg(send: &mut SendStream, msg: &[u8]) -> Result<()> {
    send.write_all(&(msg.len() as u32).to_be_bytes())
        .await
        .context("write msg len")?;
    send.write_all(msg).await.context("write msg body")?;
    Ok(())
}

pub async fn read_msg(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut lb = [0u8; 4];
    recv.read_exact(&mut lb).await.context("read msg len")?;
    let len = u32::from_be_bytes(lb);
    anyhow::ensure!(
        len <= MAX_FRAME_PAYLOAD,
        "msg length {len} exceeds cap {MAX_FRAME_PAYLOAD}"
    );
    let mut body = vec![0u8; len as usize];
    recv.read_exact(&mut body).await.context("read msg body")?;
    Ok(body)
}

fn wire_err(e: WireError) -> anyhow::Error {
    anyhow::anyhow!("wire decode: {e}")
}
